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

use crate::{
    common::{ForStdoutputLine as _, SERVE_PATH},
    effects::{Effect, server::ServeDir},
};

#[derive(Debug, derive_more::Display)]
#[display("spawn {path:?} with serve dir {serve_dir:?}")]
pub(crate) struct BuildSpawn {
    pub path: PathBuf,
    pub serve_dir: ServeDir,
}

#[derive(derive_more::Debug, derive_more::Display)]
#[display("spawn pid {pid}")]
pub(crate) struct BuildSpawned {
    pub pid: Pid,
    pub wait: BuildWait,
}

impl Effect<BuildSpawned, anyhow::Error> for BuildSpawn {
    async fn effect(self) -> Result<BuildSpawned, anyhow::Error> {
        let mut child = Command::new(self.path)
            .env(SERVE_PATH, self.serve_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn build process")?;

        let pid = Pid::from_raw(child.id().context("obtain build process id")? as i32);

        let stdout_join_handle = child.for_stdout_line(move |line| {
            eprintln!("stdout: {line}");
            async {}.boxed()
        });

        let stderr_join_handle = child.for_stderr_line(move |line| {
            eprintln!("stderr: {line}");
            async {}.boxed()
        });

        let wait = BuildWait {
            child,
            stdout_join_handle,
            stderr_join_handle,
        };

        Ok(BuildSpawned { pid, wait })
    }
}

#[derive(Debug, derive_more::Display)]
#[display("wait for {child:?}")]
pub(crate) struct BuildWait {
    child: Child,
    stdout_join_handle: task::JoinHandle<()>,
    stderr_join_handle: task::JoinHandle<()>,
}

#[derive(Debug, derive_more::Display)]
#[display("build succeeded")]
pub(crate) struct BuildSucceeded;

impl Effect<BuildSucceeded, anyhow::Error> for BuildWait {
    async fn effect(self) -> Result<BuildSucceeded, anyhow::Error> {
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
    async fn effect(self) -> Result<SignalSent, anyhow::Error> {
        let Self { pid, signal } = self;

        nix::sys::signal::kill(pid, signal).with_context(|| format!("send {signal} to {pid}"))?;
        Ok(SignalSent)
    }
}
