#[path = "../common.rs"]
mod common;

use std::{
    collections::BTreeSet,
    env,
    fs::{self, Permissions},
    io::{BufRead as _, Write},
    net::Ipv4Addr,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
    vec::Vec,
};

use anyhow::{Context, anyhow, bail};
use chromiumoxide::{
    Browser, BrowserConfig,
    cdp::browser_protocol::{
        browser::{GetWindowBoundsParams, GetWindowForTargetParams},
        network::EventResponseReceived,
        target::GetTargetsParams,
    },
};
use futures::StreamExt as _;
use indoc::{formatdoc, indoc};
use maud::{DOCTYPE, html};
use nix::{sys::signal::Signal, unistd::Pid};
use sysinfo::{ProcessRefreshKind, RefreshKind};
use tempfile::{TempDir, TempPath};
use tokio::task::JoinHandle;

use crate::common::{ForStdoutputLine as _, SERVE_PATH, StateForTesting, TESTING_MODE};

pub(crate) trait KillWait {
    fn kill_wait(&mut self, signal: Signal) -> anyhow::Result<ExitStatus>;
}

impl KillWait for std::process::Child {
    fn kill_wait(&mut self, signal: Signal) -> anyhow::Result<ExitStatus> {
        self.signal(signal)?;
        let status = self.wait()?;
        Ok(status)
    }
}

#[derive(Debug)]
pub(crate) struct DroppyChild(Option<std::process::Child>);

impl DroppyChild {
    pub(crate) fn new(child: std::process::Child) -> Self {
        Self(Some(child))
    }
}

impl std::ops::Deref for DroppyChild {
    type Target = std::process::Child;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for DroppyChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap()
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

pub(crate) trait Signalable {
    fn signal(&self, signal: Signal) -> anyhow::Result<()>;
}

impl Signalable for std::process::Child {
    fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let pid = Pid::from_raw(self.id() as i32);
        nix::sys::signal::kill(pid, signal)?;
        Ok(())
    }
}

#[derive(Debug)]
struct Subject {
    process: DroppyChild,
    state_for_testing: Option<StateForTesting>,
    stderr: Arc<Mutex<String>>,
}

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

        Ok(Self(DroppyChild::new(process)))
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

#[derive(Debug, Clone)]
struct NuScript(String);

impl NuScript {
    fn new(content: impl Into<String>) -> Self {
        const NU_EXECUTABLE: &str = env!("NU_EXECUTABLE");
        let content = content.into();

        let content = formatdoc! {r#"
            #! {NU_EXECUTABLE}
            {content}
        "#};

        Self(content)
    }

    fn into_executable(self) -> anyhow::Result<NuExecutable> {
        let mut temp_file = tempfile::Builder::new()
            .permissions(Permissions::from_mode(0o755))
            .suffix(".nu")
            .tempfile()
            .context("temporary build command file")?;

        temp_file.as_file_mut().write_all(self.0.as_bytes())?;

        Ok(NuExecutable(temp_file.into_temp_path()))
    }
}

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
struct NuExecutable(TempPath);

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

        Ok(Self(DroppyChild::new(process)))
    }
}

impl Subject {
    async fn connect_to_browser(&mut self) -> anyhow::Result<Browser> {
        let (browser, handler) =
            Browser::connect(self.state_for_testing()?.browser_debugging_address).await?;

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

    fn url(&mut self, path: &'static str) -> anyhow::Result<String> {
        Ok(format!(
            "http://{}:{}{path}",
            Ipv4Addr::LOCALHOST,
            self.state_for_testing()?.serve_port
        ))
    }

    fn wait_stderr_contains(&mut self, pat: impl AsRef<str>) -> anyhow::Result<String> {
        let pat = pat.as_ref();
        eprintln!("waiting for subject stderr line that contains: {pat}");

        loop {
            let mut stderr_lock = self.stderr.lock().map_err(|e| anyhow!("{e}"))?;

            let Some(line_feed_index) = stderr_lock
                .char_indices()
                .find_map(|(i, c)| if c == '\n' { Some(i) } else { None })
            else {
                continue;
            };

            let line = stderr_lock
                .drain(0..=line_feed_index)
                .take(line_feed_index)
                .collect::<String>();

            if line.contains(pat) {
                return Ok(line);
            }
        }
    }

    fn state_for_testing(&mut self) -> anyhow::Result<StateForTesting> {
        if let Some(state_for_testing) = &self.state_for_testing {
            return Ok(state_for_testing.clone());
        }

        let stdout = self
            .process
            .stdout
            .as_mut()
            .context("obtaining subject stdout mutable reference")?;

        let mut stdout_lines = std::io::BufReader::new(stdout).lines();

        let line = loop {
            if let Some(line) = stdout_lines.next() {
                break line?;
            }
        };

        let state_for_testing = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse state for testing: {line:?}"))?;

        let _ = self.state_for_testing.insert(state_for_testing);
        Ok(self.state_for_testing.as_ref().unwrap().clone())
    }

    fn wait_browser_spawned(&mut self) -> anyhow::Result<()> {
        self.wait_stderr_contains("event: browser: spawned")
            .context("wait browser spawn")?;

        Ok(())
    }
}

#[derive(Debug)]
struct FreshBrowser {
    instance: Browser,
    _handler_task: Option<JoinHandle<()>>,
    // If Browser still has open files then cleanup might fail.
    // Waiting on AsyncDrop
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
    fn init() -> anyhow::Result<Self> {
        let root = TempDir::new()?;

        let subject_path_env_var =
            BTreeSet::from_iter([env!("CHROMIUM_BIN_PATH"), env!("GIT_BIN_PATH")].to_vec());

        let build_command = NuScript::new(formatdoc! {r#"
            if ($env.{SERVE_PATH} | path exists) {{
                rm --recursive $env.{SERVE_PATH}
            }}
            mkdir $env.SRC_PATH
            cp --verbose --recursive --preserve [mode, link] $env.SRC_PATH $env.{SERVE_PATH}
        "#})
        .into_executable()?;

        let mut git_init_command =
            std::process::Command::new(Path::new(env!("GIT_BIN_PATH")).join("git"));

        git_init_command
            .current_dir(&root)
            .args(["init", "--quiet"]);

        let git_init_exit_status = git_init_command
            .status()
            .with_context(|| format!("failed to spawn: {git_init_command:?}"))?;

        if !git_init_exit_status.success() {
            bail!("exited with {git_init_exit_status}: {git_init_command:?}");
        }

        let fixture = Self {
            root,
            subject_path_env_var,
            build_command,
        };

        fs::create_dir(fixture.src_path()).context("creating fixture source dir")?;
        Ok(fixture)
    }

    fn replace_build_command_script(&mut self, script: impl Into<String>) -> anyhow::Result<()> {
        let path = &self.build_command.0;
        fs::write(path, NuScript::new(script.into()).0).context("write build command")?;
        Ok(())
    }

    fn write_source_file(
        &self,
        path: impl AsRef<Path>,
        content: impl ToBytes,
    ) -> std::io::Result<()> {
        let content = content.to_bytes();
        fs::write(self.src_path().join(path), content)
    }

    fn spawn_subject(&self) -> anyhow::Result<Subject> {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_conveyorbelt"));

        command
            .current_dir(&self.root)
            .env_clear()
            .env("DISPLAY", Xvfb::DISPLAY)
            .env(TESTING_MODE, "true")
            .env("LOG_FILTER_VAR_NAME", env!("LOG_FILTER_VAR_NAME"))
            .env(
                "PATH",
                std::env::join_paths(&self.subject_path_env_var).unwrap(),
            )
            .env("SRC_PATH", self.src_path())
            .arg(self.build_command.as_os_str());

        match env::var(env!("LOG_FILTER_VAR_NAME")) {
            Ok(log_filter) => {
                command.env(env!("LOG_FILTER_VAR_NAME"), log_filter);
            }
            Err(env::VarError::NotPresent) => {}
            Err(error @ env::VarError::NotUnicode(_)) => {
                bail!("read log filter env var: {error}");
            }
        }

        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut process = command.spawn().context("failed to spawn subject")?;
        let stderr = Arc::new(Mutex::new(String::new()));
        let stderr_clone = Arc::clone(&stderr);

        process
            .for_stderr_line(move |line| {
                eprintln!("subject stderr: {line}");
                let mut lock = stderr_clone.lock().unwrap();
                lock.push_str(line);
                lock.push('\n');
            })
            .context("handling subject stderr")?;

        Ok(Subject {
            process: DroppyChild::new(process),
            state_for_testing: None,
            stderr,
        })
    }

    fn src_path(&self) -> PathBuf {
        self.root.path().join("src")
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

impl ToBytes for &str {
    fn to_bytes(self) -> Vec<u8> {
        self.as_bytes().to_vec()
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
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file("foo.html", HtmlPage::new().title("some page"))
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let title = browser
        .instance
        .new_page(subject.url("/foo.html").unwrap())
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
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/nope.html").unwrap()).await.unwrap();
    let response_status = responses.next().await.unwrap().response.status;
    assert_eq!(response_status, 404);
}

#[tokio::test]
async fn browser_is_launched() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.connect_to_browser().await.unwrap();
}

#[test]
fn browser_orphaned() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    let browser_process_pid =
        sysinfo::Pid::from_u32(subject.state_for_testing().unwrap().browser_pid);

    subject.process.kill_wait(Signal::SIGTERM).unwrap();

    let sys = sysinfo::System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );

    let Some(browser_process) = sys.process(browser_process_pid) else {
        panic!("browser process not found")
    };

    let browser_parent_id = browser_process.parent().unwrap();
    assert_eq!(browser_parent_id, 1.into());
}

#[tokio::test]
async fn launched_browser_has_one_page_at_served_root() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let browser = subject.connect_to_browser().await.unwrap();

    let pages = browser
        .execute(GetTargetsParams { filter: None })
        .await
        .unwrap();

    let [page] = pages.target_infos.as_slice() else {
        panic!("pages length is not 1");
    };

    let actual = page.url.as_str();
    let expected = subject.url("/").unwrap();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn launched_browser_has_head() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
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
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file("404.html", HtmlPage::new().title("Ain't found"))
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();

    subject.wait_browser_spawned().unwrap();

    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/nope.html").unwrap()).await.unwrap();
    let response_status = responses.next().await.unwrap().response.status;
    assert_eq!(response_status, 404);
    let title = page.get_title().await.unwrap().unwrap();
    assert_eq!(title, "Ain't found");
}

#[tokio::test]
async fn html_extension_can_be_omitted() {
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file("foo.html", HtmlPage::new().title("I can haz pretty path"))
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let title = browser
        .instance
        .new_page(subject.url("/foo").unwrap())
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
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file("index.html", HtmlPage::new().title("I am root"))
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();

    let title = browser
        .instance
        .new_page(subject.url("/").unwrap())
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
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file("file.html", HtmlPage::new().title("I'm a page"))
        .unwrap();

    fixture
        .write_source_file("file.txt", HtmlPage::new().title("I am text"))
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/file.html").unwrap()).await.unwrap();
    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 200);
    assert_eq!(response.mime_type, "text/html");
    let title = page.get_title().await.unwrap().unwrap();
    assert_eq!(title, "I'm a page");
    page.goto(subject.url("/file.txt").unwrap()).await.unwrap();
    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 200);
    assert_eq!(response.mime_type, "text/plain");
    let content = page.content().await.unwrap();
    assert!(content.contains("I am text"));
}

#[tokio::test]
async fn ignore_hidden_files() {
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file(".file.html", HtmlPage::new().title("can't find me"))
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/.file.html").unwrap())
        .await
        .unwrap();

    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 404);
}

#[tokio::test]
async fn forbid_symlinks() {
    let fixture = Fixture::init().unwrap();

    fixture
        .write_source_file("real.html", HtmlPage::new().title("real page"))
        .unwrap();

    symlink("real.html", fixture.src_path().join("symlink.html")).unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let browser = FreshBrowser::spawn().await.unwrap();
    let page = browser.instance.new_page("about:blank").await.unwrap();

    let mut responses = page
        .event_listener::<EventResponseReceived>()
        .await
        .unwrap();

    page.goto(subject.url("/symlink.html").unwrap())
        .await
        .unwrap();

    let response = &responses.next().await.unwrap().response;
    assert_eq!(response.status, 403);
}

#[test]
fn sigterm_early() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let status = subject.process.kill_wait(Signal::SIGTERM).unwrap();
    assert_eq!(status.code(), None);
}

#[test]
fn sigterm() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.state_for_testing().unwrap();
    let status = subject.process.kill_wait(Signal::SIGTERM).unwrap();
    assert_eq!(status.code(), None);
}

#[test]
fn sigint_early() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let status = subject.process.kill_wait(Signal::SIGINT).unwrap();
    assert_eq!(status.code(), None);
}

#[test]
fn sigint() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.state_for_testing().unwrap();
    let status = subject.process.kill_wait(Signal::SIGINT).unwrap();
    assert_eq!(status.code(), None);
}

#[test]
fn sigquit_early() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let status = subject.process.kill_wait(Signal::SIGQUIT).unwrap();
    assert_eq!(status.code(), None);
}

#[test]
fn sigquit() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.state_for_testing().unwrap();
    let status = subject.process.kill_wait(Signal::SIGQUIT).unwrap();
    assert_eq!(status.code(), None);
}

#[test]
fn cannot_find_git_executable() {
    let mut fixture = Fixture::init().unwrap();
    fixture.subject_path_env_var.remove(env!("GIT_BIN_PATH"));
    let mut subject = fixture.spawn_subject().unwrap();
    subject.wait_stderr_contains("failed to run ").unwrap();
    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn cannot_find_browser_executable() {
    let mut fixture = Fixture::init().unwrap();

    fixture
        .subject_path_env_var
        .remove(env!("CHROMIUM_BIN_PATH"));

    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("Could not auto detect a chrome executable")
        .unwrap();

    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn not_inside_a_git_work_tree() {
    let fixture = Fixture::init().unwrap();
    fs::remove_dir_all(fixture.root.path().join(".git")).unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("not a git repository")
        .unwrap();

    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn initial_build_command_not_found() {
    let fixture = Fixture::init().unwrap();
    fs::remove_file(&*fixture.build_command).unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("build: spawn error: spawn build process: ")
        .unwrap();

    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[tokio::test]
async fn subsequent_build_failed_to_spawn() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject.wait_browser_spawned().unwrap();

    fs::set_permissions(&*fixture.build_command, Permissions::from_mode(0o644)).unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject
        .wait_stderr_contains("event: build: spawn error")
        .unwrap();

    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn subsequent_build_terminated_with_failure() {
    let mut fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject.wait_browser_spawned().unwrap();

    fixture.replace_build_command_script("exit 1").unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject
        .wait_stderr_contains("build: terminated with code Some(1)")
        .unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject
        .wait_stderr_contains("build: terminated with code Some(1)")
        .unwrap();
}

#[tokio::test]
async fn build_process_restart() {
    let mut fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject.wait_browser_spawned().unwrap();

    fixture
        .replace_build_command_script(indoc! {"
            loop {
                print -e 'looping'
                sleep 20sec
            }
        "})
        .unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject
        .wait_stderr_contains("build: stderr: looping")
        .unwrap();

    fixture
        .replace_build_command_script("print -e hello")
        .unwrap();

    fixture.write_source_file("trigger-again", "").unwrap();

    subject
        .wait_stderr_contains("event: build: sent SIGTERM to")
        .unwrap();

    subject
        .wait_stderr_contains("event: build: terminated with code None")
        .unwrap();

    subject
        .wait_stderr_contains("event: build: stderr: hello")
        .unwrap();
}

#[test]
fn initial_build_command_not_executable() {
    let fixture = Fixture::init().unwrap();
    fs::set_permissions(&*fixture.build_command, Permissions::from_mode(0o644)).unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("build: spawn error: spawn build process: ")
        .unwrap();

    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn initial_build_fail() {
    let mut fixture = Fixture::init().unwrap();

    fixture.replace_build_command_script("exit 1").unwrap();

    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("build: terminated with code Some(1)")
        .unwrap();

    let status = subject.process.wait().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn build_command_stderr() {
    let mut fixture = Fixture::init().unwrap();

    fixture
        .replace_build_command_script("print -e 'some stderr line'")
        .unwrap();

    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("build: stderr: some stderr line")
        .unwrap();
}

#[test]
fn build_command_stdout() {
    let mut fixture = Fixture::init().unwrap();
    fixture
        .replace_build_command_script("print 'some stdout line'")
        .unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject
        .wait_stderr_contains("build: stdout: some stdout line")
        .unwrap();
}

#[test]
fn build_failure_followed_by_success() {
    let mut fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject.wait_browser_spawned().unwrap();

    fixture.replace_build_command_script("exit 1").unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject
        .wait_stderr_contains("build: terminated with code Some(1)")
        .unwrap();

    fixture.replace_build_command_script("exit 0").unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject
        .wait_stderr_contains("build: TerminatedSuccessfully")
        .unwrap();

    subject.wait_stderr_contains("browser: reloaded").unwrap();
}

#[tokio::test]
async fn browser_window_not_at_default_chromiumoxide_dimensions() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
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

#[test]
fn build_not_executed_on_git_ignored_file_creation() {
    let mut fixture = Fixture::init().unwrap();
    fixture.write_source_file(".gitignore", "/foo").unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    let serve_path = &subject.state_for_testing().unwrap().serve_path;
    let serve_path_str = serve_path.to_str().unwrap();

    fixture
        .replace_build_command_script(format!("touch {serve_path_str}/foo-indicator"))
        .unwrap();

    fixture.write_source_file("foo", "no trigger").unwrap();

    subject
        .wait_stderr_contains("/foo\" (git ignored) ")
        .unwrap();

    fixture
        .replace_build_command_script(format!("touch {serve_path_str}/bar-indicator"))
        .unwrap();

    fixture.write_source_file("bar", "trigger").unwrap();
    subject.wait_stderr_contains("/bar\" create File").unwrap();

    subject
        .wait_stderr_contains("build: TerminatedSuccessfully")
        .unwrap();

    assert!(!fs::exists(serve_path.join("foo-indicator")).unwrap());
    assert!(fs::exists(serve_path.join("bar-indicator")).unwrap());
}

#[test]
fn build_not_executed_on_git_ignored_file_change() {
    let fixture = Fixture::init().unwrap();
    fixture.write_source_file("foo", "").unwrap();
    fixture.write_source_file(".gitignore", "foo\n").unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.wait_browser_spawned().unwrap();
    fixture.write_source_file("foo", "no trigger").unwrap();
    subject.wait_stderr_contains("file change ignored").unwrap();
}

#[test]
#[ignore = "TODO"]
fn build_not_executed_on_git_ignored_file_removal() {
    todo!();
}

#[test]
fn build_executed_on_file_creation() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.wait_browser_spawned().unwrap();
    fixture.write_source_file("new_file", "").unwrap();
    subject
        .wait_stderr_contains("event: build: spawned pid ")
        .unwrap();
}

#[test]
fn build_executed_on_file_change() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.wait_browser_spawned().unwrap();
    fixture.write_source_file("file", "").unwrap();
    subject
        .wait_stderr_contains("event: build: spawned pid ")
        .unwrap();
    fixture.write_source_file("file", "").unwrap();
    subject
        .wait_stderr_contains("event: build: spawned pid ")
        .unwrap();
}

#[test]
fn build_executed_on_file_removal() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();
    subject.wait_browser_spawned().unwrap();
    fixture.write_source_file("file", "").unwrap();
    subject
        .wait_stderr_contains("event: build: spawned pid ")
        .unwrap();
    fs::remove_file(fixture.src_path().join("file")).unwrap();
    subject
        .wait_stderr_contains("event: build: spawned pid ")
        .unwrap();
}

#[test]
fn browser_reloads_following_build_execution() {
    let fixture = Fixture::init().unwrap();
    let mut subject = fixture.spawn_subject().unwrap();

    subject.wait_browser_spawned().unwrap();

    fixture.write_source_file("trigger", "").unwrap();

    subject.wait_stderr_contains("browser: reloaded").unwrap();
}

// TODO make sure `.gitignore` is not the only ignore file that is used in testing
// TODO various other events do not trigger anything:
// TODO test that serve dir is cleaned up
// TODO test browser detection
// TODO launched in subdir
// TODO test that subject does not write to stdout
// TODO method for signalling Subject
// TODO test for logging of detected changes
// TODO tests return Result?
// TODO use watchexec to handle signals
// TODO loggin of termination by signal
// TODO test sub

#[test]
#[ignore = "TODO"]
fn cdp_errors_are_reported() {}
