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

#[derive(Debug, Copy, Clone, derive_more::Display)]
pub enum Stdoutput {
    #[display("err")]
    Err,
    #[display("out")]
    Out,
}

pub trait CaptureStdoutsLines {
    fn capture_stdouts_lines(
        &mut self,
        f: fn(stdoutput: Stdoutput, line: &str),
    ) -> anyhow::Result<()>;
}

impl CaptureStdoutsLines for std::process::Child {
    fn capture_stdouts_lines(
        &mut self,
        f: fn(stdoutput: Stdoutput, line: &str),
    ) -> anyhow::Result<()> {
        let child_stderr = self.stderr.take().context("Child stderr missing")?;
        let mut child_stderr_lines = std::io::BufReader::new(child_stderr).lines();
        let child_stdout = self.stdout.take().context("Child stdout missing")?;
        let mut child_stdout_lines = std::io::BufReader::new(child_stdout).lines();
        std::thread::spawn(move || {
            loop {
                if let Some(Ok(line)) = child_stderr_lines.next() {
                    f(Stdoutput::Err, &line);
                }
            }
        });
        std::thread::spawn(move || {
            loop {
                if let Some(Ok(line)) = child_stdout_lines.next() {
                    f(Stdoutput::Out, &line);
                }
            }
        });
        Ok(())
    }
}

pub trait CaptureStdoutLinesAsync {
    fn capture_stdout_lines(&mut self, f: fn(Stdoutput, &str)) -> anyhow::Result<()>;
}

impl CaptureStdoutLinesAsync for tokio::process::Child {
    fn capture_stdout_lines(&mut self, f: fn(Stdoutput, &str)) -> anyhow::Result<()> {
        let child_stderr = self.stderr.take().context("Child stderr missing")?;
        let child_stdout = self.stdout.take().context("Child stdout missing")?;
        let mut stderr_lines = tokio::io::BufReader::new(child_stderr).lines();
        let mut stdout_lines = tokio::io::BufReader::new(child_stdout).lines();

        tokio::spawn(async move {
            loop {
                if let Ok(Some(line)) = stderr_lines.next_line().await {
                    f(Stdoutput::Err, &line);
                };
            }
        });

        tokio::spawn(async move {
            loop {
                if let Ok(Some(line)) = stdout_lines.next_line().await {
                    f(Stdoutput::Out, &line);
                };
            }
        });

        Ok(())
    }
}
