// TODO can this file be loaded as two distinct modules, one in the program and another in the tests?
use std::io::BufRead as _;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt as _;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateForTesting {
    pub serve_port: u16,
    pub browser_debugging_address: String,
    pub browser_pid: u32,
}

impl StateForTesting {
    pub const ENV_VAR: &str = "_PRINT_STATE_FOR_TESTING";
}

pub trait ForStdoutputLine {
    fn for_stderr_line(&mut self, f: fn(line: &str)) -> anyhow::Result<()>;
    fn for_stdout_line(&mut self, f: fn(line: &str)) -> anyhow::Result<()>;
}

impl ForStdoutputLine for std::process::Child {
    fn for_stderr_line(&mut self, f: fn(line: &str)) -> anyhow::Result<()> {
        let child_stderr = self.stderr.take().context("Child stderr missing")?;
        let mut child_stderr_lines = std::io::BufReader::new(child_stderr).lines();
        std::thread::spawn(move || {
            loop {
                if let Some(Ok(line)) = child_stderr_lines.next() {
                    f(&line);
                }
            }
        });
        Ok(())
    }

    fn for_stdout_line(&mut self, f: fn(line: &str)) -> anyhow::Result<()> {
        let child_stdout = self.stdout.take().context("Child stdout missing")?;
        let mut child_stdout_lines = std::io::BufReader::new(child_stdout).lines();
        std::thread::spawn(move || {
            loop {
                if let Some(Ok(line)) = child_stdout_lines.next() {
                    f(&line);
                }
            }
        });
        Ok(())
    }
}

impl ForStdoutputLine for tokio::process::Child {
    fn for_stderr_line(&mut self, f: fn(&str)) -> anyhow::Result<()> {
        let child_stderr = self.stderr.take().context("Child stderr missing")?;
        let mut stderr_lines = tokio::io::BufReader::new(child_stderr).lines();

        tokio::spawn(async move {
            loop {
                if let Ok(Some(line)) = stderr_lines.next_line().await {
                    f(&line);
                };
            }
        });

        Ok(())
    }

    fn for_stdout_line(&mut self, f: fn(&str)) -> anyhow::Result<()> {
        let child_stdout = self.stdout.take().context("Child stdout missing")?;
        let mut stdout_lines = tokio::io::BufReader::new(child_stdout).lines();

        tokio::spawn(async move {
            loop {
                if let Ok(Some(line)) = stdout_lines.next_line().await {
                    f(&line);
                };
            }
        });

        Ok(())
    }
}
