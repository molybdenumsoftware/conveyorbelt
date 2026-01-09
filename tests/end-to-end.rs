use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs::Permissions,
    io::Write,
    net::Ipv4Addr,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
};

use anyhow::{Context, anyhow, bail};
use chromiumoxide::{
    Browser, BrowserConfig,
    cdp::browser_protocol::{
        browser::{GetWindowBoundsParams, GetWindowForTargetParams},
        network::EventResponseReceived,
    },
};
use conveyorbelt::{ForStdoutputLine as _, StateForTesting};
use futures::StreamExt as _;
use indoc::formatdoc;
use maud::{DOCTYPE, html};
use nix::{sys::signal::Signal, unistd::Pid};
use sysinfo::{ProcessRefreshKind, RefreshKind};
use tempfile::{TempDir, TempPath, tempdir};
use tokio::{io::AsyncBufReadExt as _, task::JoinHandle};

#[derive(Debug)]
struct Subject {
    process: tokio::process::Child,
    state_for_testing: StateForTesting,
}

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
struct DroppyChild(Option<std::process::Child>);

trait EnvProvider {
    fn envs(&self) -> impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)>;
    fn envs_owned(&self) -> impl IntoIterator<Item = (String, String)> {
        self.envs().into_iter().map(|(n, v)| {
            (
                n.as_ref().to_str().unwrap().to_string(),
                v.as_ref().to_str().unwrap().to_string(),
            )
        })
    }
}

#[derive(Debug)]
struct Xvfb(#[allow(dead_code)] DroppyChild);

impl Xvfb {
    const DISPLAY: &str = ":99";
    const WIDTH: u16 = 1920;
    const HEIGHT: u16 = 1080;

    fn spawn() -> anyhow::Result<Self> {
        let mut process = std::process::Command::new(env!("XVFB_EXECUTABLE"))
            .args([
                Self::DISPLAY,
                "-screen",
                "0",
                &format!("{}x{}x24", Self::WIDTH, Self::HEIGHT),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn weston")?;

        process.for_stdout_line(|line| {
            eprintln!("Xvfb stdout: {line}");
        })?;

        process.for_stderr_line(|line| {
            eprintln!("Xvfb stderr: {line}");
        })?;

        Ok(Self(DroppyChild(Some(process))))
    }
}

impl EnvProvider for Xvfb {
    fn envs(&self) -> impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)> {
        let envs: [(&str, &OsStr); _] = [("DISPLAY", Self::DISPLAY.as_ref())];
        envs
    }
}

trait Signalable {
    fn signal(&self, signal: Signal) -> anyhow::Result<()>;
    fn kill_wait(&mut self, signal: Signal) -> impl Future<Output = anyhow::Result<ExitStatus>>;
}

impl Signalable for std::process::Child {
    fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let pid = Pid::from_raw(self.id().try_into()?);
        nix::sys::signal::kill(pid, signal)?;
        Ok(())
    }

    async fn kill_wait(&mut self, signal: Signal) -> anyhow::Result<ExitStatus> {
        self.signal(signal)?;
        let status = self.wait()?;
        Ok(status)
    }
}
impl Signalable for tokio::process::Child {
    fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let pid = Pid::from_raw(self.id().context("no pid")?.try_into()?);
        nix::sys::signal::kill(pid, signal)?;
        Ok(())
    }

    async fn kill_wait(&mut self, signal: Signal) -> anyhow::Result<ExitStatus> {
        self.signal(signal)?;
        let status = self.wait().await?;
        Ok(status)
    }
}

impl Drop for DroppyChild {
    fn drop(&mut self) {
        let Some(mut inner) = self.0.take() else {
            return;
        };
        if let Err(e) = inner.signal(Signal::SIGTERM) {
            eprintln!("Failed to signal dropped child: {e}");
            return;
        }
        let Ok(status) = inner.wait() else { return };
        if status.success() {
            return;
        }
        eprintln!("Dropped child terminated with {status}")
    }
}

#[static_init::dynamic(drop)]
static mut DISPLAY_SERVER: Xvfb = Xvfb::spawn().unwrap();

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
struct NuExecutable(TempPath);

impl NuExecutable {
    fn new(content: &str) -> anyhow::Result<Self> {
        const NU_EXECUTABLE: &str = env!("NU_EXECUTABLE");

        let mut temp_file = tempfile::Builder::new()
            .permissions(Permissions::from_mode(0o755))
            .suffix(".nu")
            .tempfile()
            .context("temporary build command file")?;

        temp_file.as_file_mut().write_all(
            formatdoc! {r#"
                #! {NU_EXECUTABLE}
                {content}
            "#}
            .as_bytes(),
        )?;

        Ok(Self(temp_file.into_temp_path()))
    }
}

#[derive(Debug)]
struct DBusSession {
    _process: DroppyChild,
    // TODO which of these are actually needed
    _socket_dir: TempDir,
    _xdg_runtime_dir: TempDir,
    _home_dir: TempDir,
    server_address: String,
}

impl DBusSession {
    fn spawn() -> anyhow::Result<DBusSession> {
        let socket_dir = tempdir()?;
        let home_dir = tempdir()?;
        let xdg_runtime_dir = tempdir()?;

        let server_address = format!("unix:path={}/session", socket_dir.path().to_str().unwrap());

        let mut process = std::process::Command::new(env!("DBUS_DAEMON_EXECUTABLE"))
            .envs([
                ("HOME", home_dir.path()),
                ("XDG_RUNTIME_DIR", xdg_runtime_dir.path()),
            ])
            .args([
                "--nopidfile",
                "--nofork",
                "--config-file",
                // TODO disable notifications service
                env!("DBUS_SESSION_CONFIG_FILE"),
                "--address",
                &server_address,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        process.for_stderr_line(|line| {
            eprintln!("dbus-daemon stderr: {line}");
        })?;

        process.for_stdout_line(|line| {
            eprintln!("dbus-daemon stdout: {line}");
        })?;

        Ok(Self {
            _process: DroppyChild(Some(process)),
            _socket_dir: socket_dir,
            _xdg_runtime_dir: xdg_runtime_dir,
            server_address,
            _home_dir: home_dir,
        })
    }
}

impl EnvProvider for DBusSession {
    fn envs(&self) -> impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)> {
        [("DBUS_SESSION_BUS_ADDRESS", &self.server_address)]
    }
}

#[static_init::dynamic(drop)]
static mut DBUS_SESSION: DBusSession = DBusSession::spawn().unwrap();

impl Subject {
    async fn connect_to_browser(&self) -> chromiumoxide::Result<Browser> {
        let (browser, handler) =
            Browser::connect(&self.state_for_testing.browser_debugging_address).await?;
        tokio::spawn(async move {
            handler.for_each(async |_| {}).await;
        });
        Ok(browser)
    }

    fn url(&self, path: &'static str) -> String {
        format!(
            "http://{}:{}{path}",
            Ipv4Addr::LOCALHOST,
            self.state_for_testing.serve_port
        )
    }
}

#[derive(Debug)]
struct FreshBrowser {
    instance: Browser,
    _handler_task: Option<JoinHandle<()>>,
    _data_dir: TempDir,
}

impl FreshBrowser {
    async fn spawn() -> anyhow::Result<Self> {
        let data_dir = tempdir()?;
        let (browser, handler) = Browser::launch(
            BrowserConfig::builder()
                .chrome_executable(Path::new(env!("CHROMIUM_BIN_PATH")).join("chromium"))
                .user_data_dir(data_dir.path())
                .envs(DBUS_SESSION.read().envs_owned())
                .build()
                .map_err(|e| anyhow!(e))?,
        )
        .await?;

        let handler_task = tokio::spawn(async move {
            handler.for_each(async |_| {}).await;
        });

        Ok(Self {
            _data_dir: data_dir,
            instance: browser,
            _handler_task: Some(handler_task),
        })
    }
}

struct Fixture {
    root: TempDir,
    // TODO which of these are actually needed?
    xdg_config_home: TempDir,
    xdg_cache_home: TempDir,
    build_command: NuExecutable,
    home: TempDir,
    subject_path_env_var: BTreeSet<&'static str>,
}

impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let root = TempDir::with_prefix(
            // https://github.com/static-web-server/static-web-server/pull/606
            "not-hidden-",
        )?;

        let xdg_config_home = tempfile::Builder::new()
            .permissions(Permissions::from_mode(0o755))
            .tempdir()?;

        let xdg_cache_home = tempfile::Builder::new()
            .permissions(Permissions::from_mode(0o755))
            .tempdir()?;

        let home = tempfile::Builder::new()
            .permissions(Permissions::from_mode(0o755))
            .tempdir()?;

        let subject_path_env_var =
            BTreeSet::from_iter([env!("GIT_BIN_PATH"), env!("CHROMIUM_BIN_PATH")].to_vec());

        let build_command = NuExecutable::new(&formatdoc! {r#"
            if ($env.SERVE_PATH | path exists) {{
                rm --recursive $env.SERVE_PATH
            }}
            mkdir $env.SRC_PATH
            cp --verbose --recursive --preserve [mode, link] $env.SRC_PATH $env.SERVE_PATH
        "#})?;

        std::fs::write(
            root.path().join(".gitignore"),
            format!("/{}", env!("SERVE_DIR")),
        )
        .unwrap();

        let fixture = Self {
            root,
            xdg_config_home,
            xdg_cache_home,
            home,
            subject_path_env_var,
            build_command,
        };

        std::fs::create_dir(fixture.src_path()).context("creating fixture source dir")?;
        Ok(fixture)
    }

    fn build_command(&mut self, build_command: NuExecutable) {
        self.build_command = build_command;
    }

    async fn git_init(&self) -> anyhow::Result<()> {
        let mut command = tokio::process::Command::new(Path::new(env!("GIT_BIN_PATH")).join("git"));
        command.current_dir(&self.root).args(["init", "--quiet"]);

        let exit_status = command
            .status()
            .await
            .with_context(|| format!("failed to spawn: {command:?}"))?;

        if !exit_status.success() {
            bail!("exited with {exit_status}: {command:?}");
        }

        Ok(())
    }

    fn write_source_file(
        &self,
        path: impl AsRef<Path>,
        content: impl ToBytes,
    ) -> std::io::Result<()> {
        let content = content.to_bytes();
        std::fs::write(self.src_path().join(path), content)
    }

    async fn spawn_subject(&self) -> anyhow::Result<Subject> {
        let mut subject_command = self.subject_command();

        subject_command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut process = subject_command.spawn().context("failed to spawn subject")?;

        let stdout = process
            .stdout
            .as_mut()
            .context("obtaining subject stdout mutable reference")?;

        let mut line = String::new();
        let mut stdout_buf = tokio::io::BufReader::new(stdout);

        stdout_buf
            .read_line(&mut line)
            .await
            .context("reading subject stdout line")?;

        let state_for_testing = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse state for testing: {line:?}"))?;

        Ok(Subject {
            process,
            state_for_testing,
        })
    }

    fn subject_command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_conveyorbelt"));

        command
            .kill_on_drop(true)
            .current_dir(&self.root)
            .env(StateForTesting::ENV_VAR, "true")
            .env(
                "PATH",
                std::env::join_paths(&self.subject_path_env_var).unwrap(),
            )
            .envs(self.envs())
            .arg(self.build_command.as_os_str());

        command
    }

    fn src_path(&self) -> PathBuf {
        self.root.path().join("src")
    }
}

impl EnvProvider for Fixture {
    fn envs(&self) -> impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)> {
        let mut envs = DISPLAY_SERVER
            .read()
            .envs_owned()
            .into_iter()
            .chain(DBUS_SESSION.read().envs_owned())
            .collect::<Vec<_>>();

        envs.push((
            "SRC_PATH".to_string(),
            self.src_path().to_str().unwrap().to_string(),
        ));

        // Browsers by default have needs
        envs.push((
            "XDG_CONFIG_HOME".to_string(),
            self.xdg_config_home.path().to_str().unwrap().to_string(),
        ));

        // Browsers by default have needs
        envs.push((
            "XDG_CACHE_HOME".to_string(),
            self.xdg_cache_home.path().to_str().unwrap().to_string(),
        ));

        // Browsers by default have needs
        envs.push((
            "HOME".to_string(),
            self.home.path().to_str().unwrap().to_string(),
        ));

        envs
    }
}

trait ToBytes {
    fn to_bytes(self) -> Vec<u8>;
}

impl ToBytes for &HtmlPage {
    fn to_bytes(self) -> Vec<u8> {
        self.to_string().into_bytes()
    }
}

impl ToBytes for HtmlPage {
    fn to_bytes(self) -> Vec<u8> {
        (&self).to_bytes()
    }
}

#[derive(Debug, Clone)]
struct HtmlPage {
    title: Option<&'static str>,
}

impl HtmlPage {
    fn new() -> Self {
        Self { title: None }
    }
    fn title(self, title: &'static str) -> Self {
        Self { title: Some(title) }
    }
}

impl std::fmt::Display for HtmlPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let html = html! {
            (DOCTYPE)
            html {
                head {
                    link rel="icon" href="data:," type="image/x-icon";
                    meta charset="UTF-8";
                    @if let Some(title) = &self.title {
                        title { (title) }
                    }
                }
                body {}
            }
        };

        write!(f, "{}", html.into_string())
    }
}

#[tokio::test]
async fn page_content_is_served() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file("foo.html", HtmlPage::new().title("some page"))
        .unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let title = browser
        .instance
        .new_page(subject.url("/foo.html"))
        .await
        .unwrap()
        .wait_for_navigation()
        .await
        .unwrap()
        .get_title()
        .await
        .unwrap()
        .unwrap();

    assert_eq!(title, "some page");
}

#[tokio::test]
async fn default_404_page() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/nope.html")).await.unwrap();
    let response_status = responses.next().await.unwrap().response.status;
    assert_eq!(response_status, 404);
}

#[tokio::test]
async fn browser_launch() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let subject = fixture.spawn_subject().await.unwrap();
    subject.connect_to_browser().await.unwrap();
}

#[tokio::test]
async fn browser_orphaned() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();

    subject.process.kill_wait(Signal::SIGTERM).await.unwrap();

    let sys = sysinfo::System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );

    let browser_process_pid = sysinfo::Pid::from_u32(subject.state_for_testing.browser_pid);

    let Some(browser_process) = sys.process(browser_process_pid) else {
        panic!("browser process not found")
    };

    let browser_parent_id = browser_process.parent().unwrap();
    assert_eq!(browser_parent_id, 1.into());
}

#[tokio::test]
async fn launched_browser_has_head() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let subject = fixture.spawn_subject().await.unwrap();
    let browser = subject.connect_to_browser().await.unwrap();
    let page = browser.new_page("about:blank").await.unwrap();
    let user_agent = page.evaluate("navigator.userAgent").await.unwrap();

    let Some(serde_json::Value::String(user_agent)) = user_agent.value() else {
        panic!();
    };

    // TODO
    // assert silimar to browser viewport dimensions test
    assert!(!user_agent.contains("HeadlessChrome"), "{user_agent}");
}

#[tokio::test]
async fn custom_404_page() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file("exists.html", HtmlPage::new().title("I'm here!"))
        .unwrap();

    fixture
        .write_source_file("404.html", HtmlPage::new().title("Ain't found"))
        .unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/nope.html")).await.unwrap();
    let response_status = responses.next().await.unwrap().response.status;
    assert_eq!(response_status, 404);
    let title = page.get_title().await.unwrap().unwrap();
    assert_eq!(title, "Ain't found");
}

#[tokio::test]
async fn html_extension_can_be_omitted() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file("foo.html", HtmlPage::new().title("I can haz pretty path"))
        .unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let title = browser
        .instance
        .new_page(subject.url("/foo"))
        .await
        .unwrap()
        .wait_for_navigation()
        .await
        .unwrap()
        .get_title()
        .await
        .unwrap()
        .unwrap();

    assert_eq!(title, "I can haz pretty path");
}

#[tokio::test]
async fn index_html() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file("index.html", HtmlPage::new().title("I am root"))
        .unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let title = browser
        .instance
        .new_page(subject.url("/"))
        .await
        .unwrap()
        .wait_for_navigation()
        .await
        .unwrap()
        .get_title()
        .await
        .unwrap()
        .unwrap();

    assert_eq!(title, "I am root");
}

#[tokio::test]
async fn mime_types() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file("file.html", HtmlPage::new().title("I'm a page"))
        .unwrap();

    fixture
        .write_source_file("file.txt", HtmlPage::new().title("I am text"))
        .unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/file.html")).await.unwrap();
    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 200);
    assert_eq!(response.mime_type, "text/html");
    let title = page.get_title().await.unwrap().unwrap();
    assert_eq!(title, "I'm a page");

    page.goto(subject.url("/file.txt")).await.unwrap();
    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 200);
    assert_eq!(response.mime_type, "text/plain");
    let content = page.content().await.unwrap();
    assert!(content.contains("I am text"));
}

#[tokio::test]
async fn ignore_hidden_files() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file(".file.html", HtmlPage::new().title("can't find me"))
        .unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/.file.html")).await.unwrap();
    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 404);
}

#[tokio::test]
async fn forbid_symlinks() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .write_source_file("real.html", HtmlPage::new().title("real page"))
        .unwrap();

    symlink("real.html", fixture.src_path().join("symlink.html")).unwrap();

    let subject = fixture.spawn_subject().await.unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/symlink.html")).await.unwrap();
    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 403);
}

#[tokio::test]
async fn sigterm() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn sigint() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGINT).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn sigquit() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGQUIT).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn cannot_find_git_executable() {
    let mut fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    fixture.subject_path_env_var.remove(env!("GIT_BIN_PATH"));
    let output = fixture.subject_command().output().await.unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to run "), "{stderr}");
}

#[tokio::test]
async fn cannot_find_browser_executable() {
    let mut fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    fixture
        .subject_path_env_var
        .remove(env!("CHROMIUM_BIN_PATH"));

    let output = fixture.subject_command().output().await.unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(
        stderr.contains("Could not auto detect a chrome executable"),
        "{stderr}"
    );
}

#[tokio::test]
async fn not_inside_a_git_work_tree() {
    let fixture = Fixture::new().unwrap();
    let output = fixture.subject_command().output().await.unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not a git repository"), "{stderr}");
}

#[tokio::test]
async fn build_command_not_found() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    std::fs::remove_file(&*fixture.build_command).unwrap();
    let output = fixture.subject_command().output().await.unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("No such file or directory"), "{stderr}");
}

#[tokio::test]
async fn build_command_not_executable() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    tokio::fs::set_permissions(&*fixture.build_command, Permissions::from_mode(0o644))
        .await
        .unwrap();

    let output = fixture.subject_command().output().await.unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Permission denied"), "{stderr}");
}

// TODO Xvfb various warnings

#[tokio::test]
async fn build_command_stderr() {
    let mut fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    fixture.build_command(NuExecutable::new("print -e 'some stderr line'").unwrap());
    let mut subject = fixture.spawn_subject().await.unwrap();

    let mut stderr_lines =
        tokio::io::BufReader::new(subject.process.stderr.take().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();
        if line.contains("build command stderr: some stderr line") {
            break;
        }
    }

    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn build_command_stdout() {
    let mut fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    fixture.build_command(NuExecutable::new("print 'some stdout line'").unwrap());
    let mut subject = fixture.spawn_subject().await.unwrap();

    let mut stderr_lines =
        tokio::io::BufReader::new(subject.process.stderr.take().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();
        if line.contains("build command stdout: some stdout line") {
            break;
        }
    }

    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn build_command_failure() {
    let mut fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    fixture.build_command(NuExecutable::new("exit 1").unwrap());

    let mut subject = fixture
        .subject_command()
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stderr_lines = tokio::io::BufReader::new(subject.stderr.take().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();
        if line.contains("build command ") && line.contains("exited with exit status: 1") {
            break;
        }
    }

    let status = subject.wait().await.unwrap();
    assert!(!status.success());
}

#[tokio::test]
async fn browser_window_not_at_default_chromiumoxide_dimensions() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();
    let subject = fixture.spawn_subject().await.unwrap();
    let browser = subject.connect_to_browser().await.unwrap();

    let page = browser.new_page("about:blank").await.unwrap();

    let window_id = browser
        .execute(
            GetWindowForTargetParams::builder()
                .target_id(page.target_id().clone())
                .build(),
        )
        .await
        .unwrap()
        .result
        .window_id;

    let window_bounds = browser
        .execute(GetWindowBoundsParams::new(window_id))
        .await
        .unwrap()
        .result
        .bounds;

    assert!(window_bounds.width.unwrap() > 800);
    assert!(window_bounds.height.unwrap() > 600);
}

#[tokio::test]
async fn serve_path_not_git_ignored() {
    let fixture = Fixture::new().unwrap();
    fixture.git_init().await.unwrap();

    tokio::fs::remove_file(fixture.root.path().join(".gitignore"))
        .await
        .unwrap();

    let mut subject = fixture
        .subject_command()
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stderr_lines = tokio::io::BufReader::new(subject.stderr.take().unwrap()).lines();

    let serve_path = fixture.root.path().join(env!("SERVE_DIR"));

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();

        let expected_line = format!(
            "serve path (`{}`) is not git ignored",
            serve_path.to_str().unwrap()
        );

        if line.contains(&expected_line) {
            break;
        }
    }

    let status = subject.wait().await.unwrap();
    assert!(!status.success());
}
