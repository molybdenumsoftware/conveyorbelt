use std::{
    ffi::OsStr,
    fs::Permissions,
    io::Write,
    net::Ipv4Addr,
    os::unix::fs::PermissionsExt,
    process::{ExitStatus, Stdio},
};

use anyhow::Context;
use chromiumoxide::{
    Browser, BrowserConfig, cdp::browser_protocol::network::EventResponseReceived,
};
use conveyorbelt::{CaptureStdoutsLines as _, StateForTesting};
use futures::StreamExt as _;
use indoc::formatdoc;
use maud::{DOCTYPE, html};
use nix::{sys::signal::Signal, unistd::Pid};
use sysinfo::{ProcessRefreshKind, RefreshKind};
use tempfile::{TempDir, TempPath, tempdir};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    task::JoinHandle,
};

#[derive(Debug)]
struct Subject {
    process: tokio::process::Child,
    state_for_testing: StateForTesting,
    _git_toplevel: TempDir,
}

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
struct DroppyChild(std::process::Child);

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
    fn spawn() -> anyhow::Result<Self> {
        let mut process = std::process::Command::new(env!("XVFB_EXECUTABLE"))
            .arg(Self::DISPLAY)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn weston")?;

        process.capture_stdouts_lines(|stdoutput, line| {
            eprintln!("Xvfb {stdoutput}: {line}");
        })?;

        Ok(Self(DroppyChild(process)))
    }
}

impl EnvProvider for Xvfb {
    fn envs(&self) -> impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)> {
        let envs: [(&str, &OsStr); _] = [
            //("XDG_RUNTIME_DIR", self.xdg_runtime_dir.path().as_os_str()),
            ("DISPLAY", Self::DISPLAY.as_ref()),
        ];
        envs
    }
}

trait Signalable {
    fn signal(&self, signal: Signal) -> anyhow::Result<()>;
    async fn kill_wait(&mut self, signal: Signal) -> anyhow::Result<ExitStatus>;
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
        if let Err(e) = self.signal(Signal::SIGTERM) {
            eprintln!("Failed to signal dropped child: {e}");
            return;
        }
        let Ok(status) = self.wait() else { return };
        if status.success() {
            return;
        }
        eprintln!("Dropped child terminated with {status}")
    }
}

#[static_init::dynamic(drop)]
static mut DISPLAY_SERVER: Xvfb = Xvfb::spawn().unwrap();

#[derive(Debug)]
struct DBusSession {
    _process: DroppyChild,
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
                env!("DBUS_SESSION_CONFIG_FILE"),
                "--address",
                &server_address,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        process.capture_stdouts_lines(|stdoutput, line| {
            eprintln!("dbus-daemon {stdoutput}: {line}");
        })?;

        Ok(Self {
            _process: DroppyChild(process),
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

    async fn spawn(build_command: impl AsRef<OsStr>) -> anyhow::Result<Self> {
        let git_toplevel = TempDir::with_prefix(
            // https://github.com/static-web-server/static-web-server/pull/606
            "not-hidden",
        )?;

        let git_repo_initialized = std::process::Command::new("git")
            .current_dir(&git_toplevel)
            .args(["init", "--quiet"])
            .status()
            .context("failed to spawn `git init`")?
            .success();

        assert!(git_repo_initialized);

        const SUBJECT: &str = env!("CARGO_BIN_EXE_conveyorbelt");

        let mut process = tokio::process::Command::new(SUBJECT)
            .kill_on_drop(true)
            .current_dir(&git_toplevel)
            .env(StateForTesting::ENV_VAR, "true")
            .envs(DISPLAY_SERVER.read().envs())
            .envs(DBUS_SESSION.read().envs())
            .stdout(Stdio::piped())
            .arg(build_command)
            .spawn()
            .context("failed to spawn subject")?;

        let mut stdout = process
            .stdout
            .as_mut()
            .context("failed to obtain subject stdout mutable reference")?;

        let mut stdout_lines = BufReader::new(&mut stdout).lines();

        let line = stdout_lines
            .next_line()
            .await
            .context("failed to read subject's first stdout line")?
            .context("subject stdout ended before reading first line")?;

        let state_for_testing =
            serde_json::from_str(&line).context("failed to parse state for testing")?;

        //process.pipe_stdouts_prefixed("subject").await?;

        Ok(Self {
            process,
            state_for_testing,
            _git_toplevel: git_toplevel,
        })
    }
}

#[derive(Debug)]
struct FreshBrowser {
    instance: Browser,
    _handler_task: Option<JoinHandle<()>>,
    _data_dir: TempDir,
}

async fn fresh_browser() -> anyhow::Result<FreshBrowser> {
    let data_dir = tempdir()?;
    let (browser, handler) = Browser::launch(
        BrowserConfig::builder()
            .user_data_dir(data_dir.path())
            .envs(DBUS_SESSION.read().envs_owned())
            .build()
            .map_err(|e| anyhow::anyhow!(e))?,
    )
    .await?;

    let handler_task = tokio::spawn(async move {
        handler.for_each(async |_| {}).await;
    });

    Ok(FreshBrowser {
        _data_dir: data_dir,
        instance: browser,
        _handler_task: Some(handler_task),
    })
}

#[derive(Clone, Copy, Debug)]
struct TestPage {
    path: &'static str,
    title: &'static str,
}

fn escape_nu_string(s: &str) -> String {
    assert!(!s.contains("'##"));
    format!("r##'{s}'##")
}

async fn build_command_with(
    pages: impl IntoIterator<Item = TestPage>,
    post_pages: &str,
) -> anyhow::Result<TempPath> {
    let mut command = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o755))
        .suffix(".nu")
        .tempfile()?;

    let page_lines = pages
        .into_iter()
        .map(|page| {
            let html = html! {
                (DOCTYPE)
                html {
                    head {
                        link rel="icon" href="data:," type="image/x-icon";
                        meta charset="UTF-8";
                        title { (page.title) }
                    }
                    body {}
                }
            }
            .into_string();
            let html = escape_nu_string(&html);

            format!(r#"{html} | save $"($serve_path)/{}";"#, page.path)
        })
        .collect::<Vec<String>>()
        .join("\n");

    const NU_EXECUTABLE: &str = env!("NU_EXECUTABLE");

    let script = formatdoc! {r#"
        #! {NU_EXECUTABLE}
        let serve_path: path = $env.SERVE_PATH
        if ($serve_path | path exists) {{
            rm --recursive $serve_path
        }}
        mkdir $serve_path
        {page_lines}
        {post_pages}
    "#};

    command.as_file_mut().write_all(script.as_bytes())?;
    Ok(command.into_temp_path())
}

#[tokio::test]
async fn page_content_is_served() {
    let build_command = build_command_with(
        [TestPage {
            path: "foo.html",
            title: "some page",
        }],
        "",
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with([], "").await.unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with([], "").await.unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    subject.connect_to_browser().await.unwrap();
}

#[tokio::test]
async fn browser_orphaned() {
    let build_command = build_command_with([], "").await.unwrap();

    let mut subject = Subject::spawn(&build_command).await.unwrap();
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
    let build_command = build_command_with([], "").await.unwrap();
    let subject = Subject::spawn(&build_command).await.unwrap();
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
    let build_command = build_command_with(
        [
            TestPage {
                path: "exists.html",
                title: "I'm here!",
            },
            TestPage {
                path: "404.html",
                title: "Ain't found",
            },
        ],
        "",
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
async fn dot_html_is_optional() {
    let build_command = build_command_with(
        [TestPage {
            path: "foo.html",
            title: "I can haz pretty path",
        }],
        "",
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with(
        [TestPage {
            path: "index.html",
            title: "I am root",
        }],
        "",
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with(
        [
            TestPage {
                path: "file.html",
                title: "I'm a page",
            },
            TestPage {
                path: "file.txt",
                title: "I am text",
            },
        ],
        "",
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with(
        [TestPage {
            path: ".file.html",
            title: "can't find me",
        }],
        "",
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with(
        [TestPage {
            path: "real.html",
            title: "real page",
        }],
        r#"ln -s $"($env.SERVE_PATH)/real.html" $"($env.SERVE_PATH)/symlink.html";"#,
    )
    .await
    .unwrap();

    let subject = Subject::spawn(&build_command).await.unwrap();
    let browser = fresh_browser().await.unwrap();

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
    let build_command = build_command_with([], "").await.unwrap();
    let mut subject = Subject::spawn(&build_command).await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGTERM).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn sigint() {
    let build_command = build_command_with([], "").await.unwrap();
    let mut subject = Subject::spawn(&build_command).await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGINT).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn sigquit() {
    let build_command = build_command_with([], "").await.unwrap();
    let mut subject = Subject::spawn(&build_command).await.unwrap();
    let status = subject.process.kill_wait(Signal::SIGQUIT).await.unwrap();
    assert!(status.success());
}

#[tokio::test]
#[ignore = "todo"]
async fn cannot_find_git_executable() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn not_in_a_git_worktree() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_not_found() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_stderr() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_stdout() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn build_command_failure() {
    todo!()
}

#[tokio::test]
#[ignore = "todo"]
async fn browser_viewport() {
    todo!()
}
