use std::{io::BufRead as _, path::PathBuf};

use nix::{sys::signal::Signal, unistd::Pid};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt as _;

pub const SERVE_PATH: &str = env!("SERVE_PATH");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateForTesting {
    pub serve_path: PathBuf,
    pub serve_port: u16,
    pub browser_debugging_address: String,
    pub browser_pid: u32,
}

pub const TESTING_MODE: &str = "_TESTING_MODE";

pub trait ForStdoutputLine {
    fn for_stderr_line(&mut self, f: impl Fn(&str) + Send + 'static) -> Option<()>;
    fn for_stdout_line(&mut self, f: fn(line: &str)) -> Option<()>;
}

impl ForStdoutputLine for std::process::Child {
    fn for_stderr_line(&mut self, f: impl Fn(&str) + Send + 'static) -> Option<()> {
        let child_stderr = self.stderr.take()?;
        let mut child_stderr_lines = std::io::BufReader::new(child_stderr).lines();

        std::thread::spawn(move || {
            loop {
                if let Some(Ok(line)) = child_stderr_lines.next() {
                    f(&line);
                }
            }
        });

        Some(())
    }

    fn for_stdout_line(&mut self, f: fn(line: &str)) -> Option<()> {
        let child_stdout = self.stdout.take()?;
        let mut child_stdout_lines = std::io::BufReader::new(child_stdout).lines();

        std::thread::spawn(move || {
            loop {
                if let Some(Ok(line)) = child_stdout_lines.next() {
                    f(&line);
                }
            }
        });

        Some(())
    }
}

impl ForStdoutputLine for tokio::process::Child {
    fn for_stderr_line(&mut self, f: impl Fn(&str) + Send + 'static) -> Option<()> {
        let child_stderr = self.stderr.take()?;
        let mut stderr_lines = tokio::io::BufReader::new(child_stderr).lines();

        tokio::spawn(async move {
            loop {
                if let Ok(Some(line)) = stderr_lines.next_line().await {
                    f(&line);
                };
            }
        });

        Some(())
    }

    fn for_stdout_line(&mut self, f: fn(&str)) -> Option<()> {
        let child_stdout = self.stdout.take()?;
        let mut stdout_lines = tokio::io::BufReader::new(child_stdout).lines();

        tokio::spawn(async move {
            loop {
                if let Ok(Some(line)) = stdout_lines.next_line().await {
                    f(&line);
                };
            }
        });

        Some(())
    }
}

#[derive(Debug)]
pub struct DroppyChild(Option<std::process::Child>);

impl DroppyChild {
    pub fn new(child: std::process::Child) -> Self {
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

pub trait Signalable {
    fn signal(&self, signal: Signal) -> anyhow::Result<()>;
}

impl Signalable for std::process::Child {
    fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let pid = Pid::from_raw(self.id().try_into()?);
        nix::sys::signal::kill(pid, signal)?;
        Ok(())
    }
}
