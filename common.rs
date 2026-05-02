use std::{io::BufRead as _, path::PathBuf};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt as _;

pub(crate) const SERVE_PATH: &str = env!("SERVE_PATH");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StateForTesting {
    pub(crate) serve_path: PathBuf,
    pub(crate) serve_port: u16,
    pub(crate) browser_debugging_address: String,
    pub(crate) browser_pid: u32,
}

impl std::fmt::Display for StateForTesting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string = serde_json::to_string(&self).unwrap();
        write!(f, "{string}")
    }
}

pub(crate) const TESTING_MODE: &str = "_TESTING_MODE";

pub(crate) trait ForStdoutputLine {
    type JoinHandle;
    type FnReturn;
    fn for_stderr_line(
        &mut self,
        f: impl (FnMut(&str) -> Self::FnReturn) + Send + 'static,
    ) -> Option<Self::JoinHandle>;
    fn for_stdout_line(
        &mut self,
        f: impl (FnMut(&str) -> Self::FnReturn) + Send + 'static,
    ) -> Option<Self::JoinHandle>;
}

impl ForStdoutputLine for std::process::Child {
    type JoinHandle = std::thread::JoinHandle<()>;
    type FnReturn = ();
    fn for_stderr_line(
        &mut self,
        mut f: impl (FnMut(&str) -> Self::FnReturn) + Send + 'static,
    ) -> Option<Self::JoinHandle> {
        let child_stderr = self.stderr.take()?;
        let mut child_stderr_lines = std::io::BufReader::new(child_stderr).lines();

        let join_handle = std::thread::spawn(move || {
            while let Some(Ok(line)) = child_stderr_lines.next() {
                f(&line);
            }
            dbg!("stderr end");
        });

        Some(join_handle)
    }

    fn for_stdout_line(
        &mut self,
        mut f: impl FnMut(&str) + Send + 'static,
    ) -> Option<Self::JoinHandle> {
        let child_stdout = self.stdout.take()?;
        let mut child_stdout_lines = std::io::BufReader::new(child_stdout).lines();

        let join_handle = std::thread::spawn(move || {
            while let Some(Ok(line)) = child_stdout_lines.next() {
                f(&line);
            }
            dbg!("stdout end");
        });

        Some(join_handle)
    }
}

impl ForStdoutputLine for tokio::process::Child {
    type JoinHandle = tokio::task::JoinHandle<()>;
    type FnReturn = BoxFuture<'static, ()>;
    fn for_stderr_line(
        &mut self,
        mut f: impl (FnMut(&str) -> Self::FnReturn) + Send + 'static,
    ) -> Option<Self::JoinHandle> {
        let child_stderr = self.stderr.take()?;
        let mut stderr_lines = tokio::io::BufReader::new(child_stderr).lines();

        let join_handle = tokio::spawn(async move {
            while let Ok(Some(line)) = stderr_lines.next_line().await {
                f(&line).await;
            }
        });

        Some(join_handle)
    }

    fn for_stdout_line(
        &mut self,
        mut f: impl (FnMut(&str) -> Self::FnReturn) + Send + 'static,
    ) -> Option<Self::JoinHandle> {
        let child_stdout = self.stdout.take()?;
        let mut stdout_lines = tokio::io::BufReader::new(child_stdout).lines();

        let join_handle = tokio::spawn(async move {
            while let Ok(Some(line)) = stdout_lines.next_line().await {
                f(&line).await;
            }
        });

        Some(join_handle)
    }
}
