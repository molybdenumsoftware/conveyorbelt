use std::{convert::Infallible, path::PathBuf, process::Stdio};

use anyhow::{Context, bail};
use futures::FutureExt;
use nix::{sys::signal::Signal, unistd::Pid};
use rxrust::prelude::*;
use tokio::{
    process::{Child, Command},
    sync::mpsc,
    task,
};
use tokio_stream::wrappers::ReceiverStream;
use tracing::info;

use crate::{
    common::{ForStdoutputLine as _, SERVE_PATH},
    server::ServeDir,
};

// TODO #[display("spawn {path:?} with serve dir {serve_dir:?}")]
// TODO #[display("spawn pid {pid}")]

#[derive(Debug)]
pub struct Build {
    child: Child,
    stdout_join_handle: task::JoinHandle<()>,
    stderr_join_handle: task::JoinHandle<()>,
}

#[derive(Debug)]
pub struct BuildCommandPath(PathBuf);

impl Build {
    pub fn pid(&self) -> anyhow::Result<Pid> {
        Ok(Pid::from_raw(
            self.child.id().context("get build pid")? as i32
        ))
    }

    async fn spawn(serve_dir: ServeDir, command: BuildCommandPath) -> anyhow::Result<Self> {
        let mut command = Command::new(command);

        command
            .env(SERVE_PATH, serve_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("spawn build {command}");
        let mut child = command.spawn().context("build spawn")?;

        let stdout_join_handle = child.for_stdout_line(move |line| {
            eprintln!("stdout: {line}");
            async {}.boxed()
        });

        let stderr_join_handle = child.for_stderr_line(move |line| {
            eprintln!("stderr: {line}");
            async {}.boxed()
        });

        Ok(Self {
            child,
            stdout_join_handle,
            stderr_join_handle,
        })
    }
}

// TODO #[display("wait for {child:?}")]

#[derive(Debug, derive_more::Display)]
#[display("build succeeded")]
pub(crate) struct BuildSucceeded;

impl Effect<BuildSucceeded, anyhow::Error> for BuildWait {
    async fn inner(self) -> Result<BuildSucceeded, anyhow::Error> {
        let Self {
            mut child,
            stdout_join_handle,
            stderr_join_handle,
        } = self;

        let code = child
            .wait()
            .await
            .with_context(|| format!("wait on {child:?}"))?
            .code();
        tokio::join!(stdout_join_handle, stderr_join_handle);

        Ok(match code {
            Some(0) => BuildSucceeded,
            Some(non_zero) => bail!("build exited with: {non_zero}"),
            None => bail!("build exited with no code"),
        })
    }
}

#[derive(Debug, derive_more::Display)]
#[display("send {signal} to {pid}")]
pub(crate) struct BuildSignal {
    pub pid: Pid,
    pub signal: Signal,
}

#[derive(Debug, derive_more::Display)]
#[display("signal sent")]
pub struct SignalSent;

impl Effect<SignalSent, anyhow::Error> for BuildSignal {
    async fn inner(self) -> Result<SignalSent, anyhow::Error> {
        let Self { pid, signal } = self;

        nix::sys::signal::kill(pid, signal).with_context(|| format!("send {signal} to {pid}"))?;
        Ok(SignalSent)
    }
}
