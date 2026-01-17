#[path = "../common.rs"]
mod common;

use std::{
    collections::BTreeSet,
    fs::Permissions,
    io::Write,
    net::Ipv4Addr,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    vec::Vec,
};

use anyhow::{Context, anyhow, bail};
use chromiumoxide::{
    Browser, BrowserConfig,
    cdp::browser_protocol::{
        browser::{GetWindowBoundsParams, GetWindowForTargetParams},
        network::EventResponseReceived,
    },
};
use futures::StreamExt as _;
use indoc::formatdoc;
use maud::{DOCTYPE, html};
use nix::{sys::signal::Signal, unistd::Pid};
use sysinfo::{ProcessRefreshKind, RefreshKind};
use tempfile::{TempDir, TempPath};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    task::JoinHandle,
};

use crate::common::{ForStdoutputLine as _, StateForTesting};

const SERVE_DIR: &str = env!("SERVE_DIR");

#[derive(Debug)]
struct Subject {
    process: tokio::process::Child,
    state_for_testing: StateForTesting,
}

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
struct DroppyChild(Option<std::process::Child>);

#[derive(Debug)]
struct Xvfb(#[allow(dead_code)] DroppyChild);

impl Xvfb {
    const DISPLAY: &str = ":99";
    const WIDTH: u16 = 1920;
    const HEIGHT: u16 = 1080;

    fn spawn() -> anyhow::Result<Self> {
        let mut process = std::process::Command::new(env!("XVFB_EXECUTABLE"))
            .env_clear()
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

        process
            .for_stdout_line(|line| {
                eprintln!("Xvfb stdout: {line}");
            })
            .unwrap();

        process
            .for_stderr_line(|line| {
                eprintln!("Xvfb stderr: {line}");
            })
            .unwrap();

        Ok(Self(DroppyChild(Some(process))))
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
static mut SHARED_ENVIRONMENT: SharedEnvironment = SharedEnvironment::init().unwrap();

#[derive(Debug)]
struct SharedEnvironment {
    _xvfb: Xvfb,
    _dbus: DBusSession,
}

impl SharedEnvironment {
    fn init() -> anyhow::Result<Self> {
        Ok(Self {
            _xvfb: Xvfb::spawn()?,
            _dbus: DBusSession::spawn()?,
        })
    }
}

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
struct DBusSession(#[allow(dead_code)] DroppyChild);

impl DBusSession {
    fn spawn() -> anyhow::Result<DBusSession> {
        let mut process = std::process::Command::new(env!("DBUS_DAEMON_EXECUTABLE"))
            .env_clear()
            .args([
                "--nopidfile",
                "--nofork",
                "--config-file",
                // TODO disable notifications service
                env!("DBUS_SESSION_CONFIG_FILE"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        process
            .for_stderr_line(|line| {
                eprintln!("dbus-daemon stderr: {line}");
            })
            .unwrap();

        process
            .for_stdout_line(|line| {
                eprintln!("dbus-daemon stdout: {line}");
            })
            .unwrap();

        Ok(Self(DroppyChild(Some(process))))
    }
}

impl Subject {
    async fn connect_to_browser(&self) -> chromiumoxide::Result<Browser> {
        let (browser, handler) =
            Browser::connect(&self.state_for_testing.browser_debugging_address).await?;

        tokio::spawn(async move {
            handler
                .for_each(async |v| {
                    if let Err(e) = v {
                        eprintln!("browser handler: {e}");
                    }
                })
                .await;
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
    // TODO make sure this is successfully cleaned up.
    // need to await on (async) browser close prior to dropping
    // so perhaps instead of this being a struct,
    // it'll be a function with_fresh_browser that takes a closure
    // and performs the async termination
    _data_dir: TempDir,
}

impl FreshBrowser {
    async fn spawn() -> anyhow::Result<Self> {
        let data_dir = tempfile::Builder::new()
            .prefix("chromium-data-dir-")
            .tempdir()?;

        let (browser, handler) = Browser::launch(
            BrowserConfig::builder()
                .chrome_executable(Path::new(env!("CHROMIUM_BIN_PATH")).join("chromium"))
                .user_data_dir(data_dir.path())
                .env("DISPLAY", Xvfb::DISPLAY)
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
    build_command: NuExecutable,
    subject_path_env_var: BTreeSet<&'static str>,
}

impl Fixture {
    async fn new() -> anyhow::Result<Self> {
        let root = TempDir::with_prefix(
            // https://github.com/static-web-server/static-web-server/pull/606
            "not-hidden-",
        )?;

        let subject_path_env_var =
            BTreeSet::from_iter([env!("CHROMIUM_BIN_PATH"), env!("GIT_BIN_PATH")].to_vec());

        let build_command = NuExecutable::new(&formatdoc! {r#"
            if ($env.SERVE_PATH | path exists) {{
                rm --recursive $env.SERVE_PATH
            }}
            mkdir $env.SRC_PATH
            cp --verbose --recursive --preserve [mode, link] $env.SRC_PATH $env.SERVE_PATH
        "#})?;

        std::fs::write(root.path().join(".gitignore"), format!("/{}", SERVE_DIR)).unwrap();

        let mut git_init_command =
            tokio::process::Command::new(Path::new(env!("GIT_BIN_PATH")).join("git"));

        git_init_command
            .current_dir(&root)
            .args(["init", "--quiet"]);

        let git_init_exit_status = git_init_command
            .status()
            .await
            .with_context(|| format!("failed to spawn: {git_init_command:?}"))?;

        if !git_init_exit_status.success() {
            bail!("exited with {git_init_exit_status}: {git_init_command:?}");
        }

        let fixture = Self {
            root,
            subject_path_env_var,
            build_command,
        };

        std::fs::create_dir(fixture.src_path()).context("creating fixture source dir")?;
        Ok(fixture)
    }

    fn build_command(&mut self, build_command: NuExecutable) {
        self.build_command = build_command;
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

        let mut stdout_lines = BufReader::new(stdout).lines();

        let line = stdout_lines
            .next_line()
            .await?
            .context("failed to read a single subject stdout line")?;

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
            .env_clear()
            .env("DISPLAY", Xvfb::DISPLAY)
            .env(StateForTesting::ENV_VAR, "true")
            .env(
                "PATH",
                std::env::join_paths(&self.subject_path_env_var).unwrap(),
            )
            .env("SRC_PATH", self.src_path())
            .arg(self.build_command.as_os_str());

        command
    }

    fn src_path(&self) -> PathBuf {
        self.root.path().join("src")
    }

    fn serve_path(&self) -> PathBuf {
        self.root.path().join(SERVE_DIR)
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
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();
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
    let fixture = Fixture::new().await.unwrap();
    let subject = fixture.spawn_subject().await.unwrap();
    subject.connect_to_browser().await.unwrap();
}

#[tokio::test]
async fn browser_orphaned() {
    let fixture = Fixture::new().await.unwrap();
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
    let fixture = Fixture::new().await.unwrap();
    let subject = fixture.spawn_subject().await.unwrap();
    let browser = subject.connect_to_browser().await.unwrap();
    let page = browser.new_page("about:blank").await.unwrap();
    let user_agent = page.evaluate("navigator.userAgent").await.unwrap();

    let Some(serde_json::Value::String(user_agent)) = user_agent.value() else {
        panic!();
    };

    assert!(!user_agent.contains("HeadlessChrome"), "{user_agent}");
}

#[tokio::test]
async fn custom_404_page() {
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();

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
    let fixture = Fixture::new().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert_eq!(status.code(), Some(0));
}

#[tokio::test]
async fn sigint() {
    let fixture = Fixture::new().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGINT).await.unwrap();
    assert_eq!(status.code(), Some(0));
}

#[tokio::test]
async fn sigquit() {
    let fixture = Fixture::new().await.unwrap();
    let mut subject = fixture.spawn_subject().await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGQUIT).await.unwrap();
    assert_eq!(status.code(), Some(0));
}

#[tokio::test]
async fn cannot_find_git_executable() {
    let mut fixture = Fixture::new().await.unwrap();
    fixture.subject_path_env_var.remove(env!("GIT_BIN_PATH"));
    let output = fixture.subject_command().output().await.unwrap();
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to run "), "{stderr}");
}

#[tokio::test]
async fn cannot_find_browser_executable() {
    let mut fixture = Fixture::new().await.unwrap();

    fixture
        .subject_path_env_var
        .remove(env!("CHROMIUM_BIN_PATH"));

    let output = fixture.subject_command().output().await.unwrap();
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(
        stderr.contains("Could not auto detect a chrome executable"),
        "{stderr}"
    );
}

#[tokio::test]
async fn not_inside_a_git_work_tree() {
    let fixture = Fixture::new().await.unwrap();

    tokio::fs::remove_dir_all(fixture.root.path().join(".git"))
        .await
        .unwrap();

    let output = fixture.subject_command().output().await.unwrap();
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not a git repository"), "{stderr}");
}

#[tokio::test]
async fn build_command_not_found() {
    let fixture = Fixture::new().await.unwrap();
    std::fs::remove_file(&*fixture.build_command).unwrap();
    let output = fixture.subject_command().output().await.unwrap();
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("No such file or directory"), "{stderr}");
}

#[tokio::test]
async fn build_command_not_executable() {
    let fixture = Fixture::new().await.unwrap();

    tokio::fs::set_permissions(&*fixture.build_command, Permissions::from_mode(0o644))
        .await
        .unwrap();

    let output = fixture.subject_command().output().await.unwrap();
    assert_eq!(output.status.code(), Some(101));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Permission denied"), "{stderr}");
}

// TODO Xvfb various warnings

#[tokio::test]
async fn build_command_stderr() {
    let mut fixture = Fixture::new().await.unwrap();
    fixture.build_command(NuExecutable::new("print -e 'some stderr line'").unwrap());
    let mut subject = fixture.spawn_subject().await.unwrap();

    let mut stderr_lines = BufReader::new(subject.process.stderr.as_mut().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();

        if line.contains("build command stderr: some stderr line") {
            break;
        }
    }

    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert_eq!(status.code(), Some(0));
}

#[tokio::test]
async fn build_command_stdout() {
    let mut fixture = Fixture::new().await.unwrap();
    fixture.build_command(NuExecutable::new("print 'some stdout line'").unwrap());
    let mut subject = fixture.spawn_subject().await.unwrap();

    let mut stderr_lines = BufReader::new(subject.process.stderr.take().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();
        if line.contains("build command stdout: some stdout line") {
            break;
        }
    }

    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert_eq!(status.code(), Some(0));
}

#[tokio::test]
async fn build_command_failure() {
    let mut fixture = Fixture::new().await.unwrap();
    fixture.build_command(NuExecutable::new("exit 1").unwrap());

    let mut subject = fixture
        .subject_command()
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stderr_lines = BufReader::new(subject.stderr.take().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();
        if line.contains("build command ") && line.contains("exited with exit status: 1") {
            break;
        }
    }

    let status = subject.wait().await.unwrap();
    assert_eq!(status.code(), Some(101));
}

#[tokio::test]
async fn browser_window_not_at_default_chromiumoxide_dimensions() {
    let fixture = Fixture::new().await.unwrap();
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
    let fixture = Fixture::new().await.unwrap();

    tokio::fs::remove_file(fixture.root.path().join(".gitignore"))
        .await
        .unwrap();

    let mut subject = fixture
        .subject_command()
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stderr_lines = BufReader::new(subject.stderr.take().unwrap()).lines();

    loop {
        let line = stderr_lines.next_line().await.unwrap().unwrap();

        let expected_line = format!(
            "serve path (`{}`) is not git ignored",
            fixture.serve_path().to_str().unwrap()
        );

        if line.contains(&expected_line) {
            break;
        }
    }

    let status = subject.wait().await.unwrap();
    assert_eq!(status.code(), Some(101));
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_not_executed_on_git_ignored_file_creation() {
    todo!();
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_not_executed_on_git_ignored_file_change() {
    todo!();
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_not_executed_on_git_ignored_file_removal() {
    todo!();
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_executed_on_file_creation() {
    todo!();
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_executed_on_file_change() {
    todo!();
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_executed_on_file_removal() {
    todo!();
}

#[tokio::test]
#[ignore = "todo"]
async fn browser_reloads_following_build_command_execution() {
    todo!();
}
