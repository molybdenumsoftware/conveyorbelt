use std::process::Stdio;

use anyhow::Context as _;
use droppy_child::DroppyChild;
use for_stdoutput_line::ForStdoutputLine as _;
use tempfile::TempDir;
use tracing::info;
use tracing_subscriber::fmt::time::UtcTime;

#[derive(Debug)]
pub struct TestEnvironment {
    weston: Weston,
    _dbus: DBusSession,
    _home: TempDir,
}

impl TestEnvironment {
    pub fn init() -> anyhow::Result<Self> {
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_ansi_sanitization(false)
            .with_target(false)
            .with_level(false)
            .with_timer(UtcTime::new(time::macros::format_description!(
                "[hour]:[minute]:[second].[subsecond digits:3]Z"
            )))
            .init();

        let home = TempDir::new().context("create temp home")?;

        let shared_environment = Self {
            weston: Weston::spawn(&home)?,
            _dbus: DBusSession::spawn()?,
            _home: home,
        };

        info!("shared environment initialized");
        Ok(shared_environment)
    }
    pub fn weston(&self) -> &Weston {
        &self.weston
    }
}

#[derive(Debug)]
pub struct Weston {
    _socket_dir: TempDir,
    socket: String,
    _child: DroppyChild,
}

impl Weston {
    const WIDTH: u16 = 1920;
    const HEIGHT: u16 = 1080;

    pub(crate) fn spawn(home: &TempDir) -> anyhow::Result<Self> {
        let socket_dir = TempDir::new().context("create temp dir")?;
        let socket_dir_path = socket_dir.path();
        info!("weston socket dir {socket_dir_path:?}");

        let socket = socket_dir_path
            .join("socket")
            .to_str()
            .context("socket path to str")?
            .to_string();

        let mut command = std::process::Command::new(env!("WESTON_EXECUTABLE"));

        command
            .env_clear()
            .args([
                "--backend=headless",
                &format!("--socket={}", socket),
                &format!("--height={}", Self::HEIGHT),
                &format!("--width={}", Self::WIDTH),
            ])
            .env("HOME", home.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("{command:?}");
        let mut process = command.spawn().context("spawn weston")?;
        info!("weston spawned {}", process.id());

        process.for_stdout_line(|line| {
            info!("weston stdout: {line}");
        });

        process.for_stderr_line(|line| {
            info!("weston stderr: {line}");
        });

        Ok(Self {
            _socket_dir: socket_dir,
            socket,
            _child: DroppyChild::new(process),
        })
    }

    pub fn socket(&self) -> &str {
        &self.socket
    }
}

#[derive(Debug)]
struct DBusSession(#[allow(dead_code)] DroppyChild);

impl DBusSession {
    pub(crate) fn spawn() -> anyhow::Result<DBusSession> {
        let mut command = std::process::Command::new(env!("DBUS_DAEMON_EXECUTABLE"));

        command
            .env_clear()
            .args([
                "--nopidfile",
                "--nofork",
                "--config-file",
                env!("DBUS_SESSION_CONFIG_FILE"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("{command:?}");
        let mut process = command.spawn().context("spawn dbus-daemon")?;
        info!("dbus-daemon spawned {}", process.id());

        process.for_stderr_line(|line| {
            info!("dbus-daemon stderr: {line}");
        });

        process.for_stdout_line(|line| {
            info!("dbus-daemon stdout: {line}");
        });

        Ok(Self(DroppyChild::new(process)))
    }
}
