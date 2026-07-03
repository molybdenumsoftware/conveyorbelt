use std::{convert::Infallible, path::PathBuf, process::Stdio};

use anyhow::Context;
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

#[derive(Debug, derive_more::Display)]
#[display("{output}: {line}")]
pub(crate) struct OutputLine {
    output: Output,
    line: String,
}

#[derive(derive_more::Debug, derive_more::Display)]
#[display("spawn pid {pid}")]
pub(crate) struct BuildSpawned {
    pub pid: Pid,
    #[debug(skip)]
    pub output_lines: SharedBoxedObservable<'static, OutputLine, Infallible>,
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

        let (line_sender, line_receiver) = mpsc::channel(1);
        let line_sender_clone = line_sender.clone();
        let stdout_join_handle = child.for_stdout_line(move |line| {
            let line = line.to_owned();
            let line_sender_clone = line_sender_clone.clone();
            async move {
                line_sender_clone
                    .send(OutputLine {
                        output: Output::Out,
                        line,
                    })
                    .await
                    .unwrap();
            }
            .boxed()
        });

        let line_sender_clone = line_sender.clone();
        let stderr_join_handle = child.for_stderr_line(move |line| {
            let line = line.to_owned();
            let line_sender_clone = line_sender_clone.clone();
            async move {
                line_sender_clone
                    .send(OutputLine {
                        output: Output::Err,
                        line,
                    })
                    .await
                    .unwrap();
            }
            .boxed()
        });

        let wait = BuildWait {
            child,
            stdout_join_handle,
            stderr_join_handle,
        };

        let output_lines = Shared::from_stream(ReceiverStream::new(line_receiver)).box_it();

        Ok(BuildSpawned {
            pid,
            output_lines,
            wait,
        })
    }
}

#[derive(Debug, derive_more::Display)]
#[display("wait for {child:?}")]
pub(crate) struct BuildWait {
    child: Child,
    stdout_join_handle: task::JoinHandle<()>,
    stderr_join_handle: task::JoinHandle<()>,
}

impl Effect<Option<i32>, anyhow::Error> for BuildWait {
    async fn effect(self) -> Result<Option<i32>, anyhow::Error> {
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

        Ok(code)
    }
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
pub(crate) enum Output {
    #[display("stdout")]
    Out,
    #[display("stderr")]
    Err,
}

#[derive(Debug, derive_more::Display)]
#[display("send {signal} to {pid}")]
pub(crate) struct BuildSignal {
    pid: Pid,
    signal: Signal,
}

impl Effect<(), anyhow::Error> for BuildSignal {
    async fn effect(self) -> Result<(), anyhow::Error> {
        let Self { pid, signal } = self;

        nix::sys::signal::kill(pid, signal).with_context(|| format!("send {signal} to {pid}"))
    }
}
