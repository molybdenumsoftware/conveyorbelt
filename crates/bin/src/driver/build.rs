use std::{convert::Infallible, path::PathBuf, process::Stdio};

use anyhow::Context;
use futures::FutureExt;
use nix::{
    sys::signal::{SIGTERM, Signal},
    unistd::Pid,
};
use rxrust::prelude::*;
use tokio::{process::Command, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::common::ForStdoutputLine as _;

#[derive(Debug)]
pub(crate) struct BuildSpawn {
    path: PathBuf,
    envs: Vec<(String, String)>,
}

#[derive(derive_more::Display)]
pub(crate) enum BuildSpawnEvent {
    #[display("spawn pid {pid}")]
    Spawn {
        pid: Pid,
        wait_events: SharedBoxedObservable<'static, BuildWaitEvent, Infallible>,
    },
    #[display("spawn error: {_0:#}")]
    Error(anyhow::Error),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum BuildWaitEvent {
    #[display("{output}: {line}")]
    OutputLine { output: Output, line: String },
    #[display("exited with {_0:?}")]
    Exited(Option<i32>),
    #[display("error waiting for termination: {_0}")]
    WaitError(std::io::Error),
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
pub(crate) enum Output {
    #[display("stdout")]
    Out,
    #[display("stderr")]
    Err,
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum BuildSignalEvent {
    #[display("error sending signal: {_0}")]
    SignalError(nix::errno::Errno),
    #[display("sent {_1} to {_0}")]
    SignalSent(Pid, Signal),
}

// #[derive(Debug, Clone, derive_more::Display)]
// pub(crate) enum BuildCommand {
//     #[display("spawn {path:?} with env {envs:?}")]
//     Spawn {
//         path: PathBuf,
//         envs: Vec<(String, String)>,
//     },
//     #[display("send {_1} to {_0}")]
//     Signal(Pid, Signal),
// }

impl BuildSpawn {
    pub(crate) fn new(path: PathBuf, envs: Vec<(String, String)>) -> Self {
        Self { path, envs }
    }

    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, BuildSpawnEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);

        tokio::spawn(async move {
            let spawn_result = Command::new(self.path)
                .envs(self.envs)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("spawn build process");

            let mut child = match spawn_result {
                Ok(child) => child,
                Err(error) => {
                    event_sender
                        .send(BuildSpawnEvent::Error(error))
                        .await
                        .unwrap();
                    return;
                }
            };

            let pid = match child.id().context("obtain build process id") {
                Ok(pid) => Pid::from_raw(pid as i32),
                Err(error) => {
                    event_sender
                        .send(BuildSpawnEvent::Error(error))
                        .await
                        .unwrap();

                    return;
                }
            };

            let (wait_event_sender, wait_event_receiver) = mpsc::channel(1);
            let wait_event_sender_clone = wait_event_sender.clone();
            let stdout_join_handle = child
                .for_stdout_line(move |line| {
                    let line = line.to_owned();
                    let wait_event_sender_clone = wait_event_sender_clone.clone();
                    async move {
                        wait_event_sender_clone
                            .send(BuildWaitEvent::OutputLine {
                                output: Output::Out,
                                line,
                            })
                            .await
                            .unwrap();
                    }
                    .boxed()
                })
                .unwrap();

            let wait_event_sender_clone = wait_event_sender.clone();
            let stderr_join_handle = child
                .for_stderr_line(move |line| {
                    let line = line.to_owned();
                    let wait_event_sender_clone = wait_event_sender_clone.clone();
                    async move {
                        wait_event_sender_clone
                            .send(BuildWaitEvent::OutputLine {
                                output: Output::Err,
                                line,
                            })
                            .await
                            .unwrap();
                    }
                    .boxed()
                })
                .unwrap();

            tokio::spawn(async move {
                let wait_event = match child.wait().await {
                    Ok(exit_status) => BuildWaitEvent::Exited(exit_status.code()),
                    Err(error) => BuildWaitEvent::WaitError(error),
                };
                wait_event_sender.send(wait_event).await.unwrap();
            });

            let wait_events =
                Shared::from_stream(ReceiverStream::new(wait_event_receiver)).box_it();

            event_sender
                .send(BuildSpawnEvent::Spawn { pid, wait_events })
                .await
                .unwrap();

            // TODO await concurrently
            stderr_join_handle.await.unwrap();
            stdout_join_handle.await.unwrap();
        });

        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}

impl BuildSignal {
    pub(crate) fn signal(pid: Pid) -> SharedBoxedObservable<'static, BuildSignalEvent, Infallible> {
        todo!()
        // if let Err(error) = nix::sys::signal::kill(pid, signal) {
        //     event_sender
        //         .send(BuildEvent::SignalError(error))
        //         .await
        //         .unwrap();
        // };

        // event_sender
        //     .send(BuildEvent::SignalSent(pid, SIGTERM))
        //     .await
        //     .unwrap();
    }
}
