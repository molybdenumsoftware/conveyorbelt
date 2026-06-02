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

pub(crate) struct BuildDriver {
    event_sender: mpsc::Sender<BuildEvent>,
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum BuildEvent {
    #[display("spawn pid {_0}")]
    Spawn(Pid),
    #[display("spawn error: {_0:#}")]
    SpawnError(anyhow::Error),
    // --- ?
    #[display("{output}: {line}")]
    OutputLine { output: Output, line: String },
    // ---
    #[display("exited with {_0:?}")]
    Exited(Option<i32>),
    #[display("error waiting for termination: {_0}")]
    WaitError(std::io::Error),
    #[display("error sending signal: {_0}")]
    SignalError(nix::errno::Errno),
    #[display("sent {_1} to {_0}")]
    SignalSent(Pid, Signal),
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
pub(crate) enum Output {
    #[display("stdout")]
    Out,
    #[display("stderr")]
    Err,
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

impl BuildDriver {
    pub(crate)
    // pub(crate) fn new() -> (SharedBoxedObservable<'static, BuildEvent, Infallible>, Self) {
    //     let (event_sender, event_receiver) = mpsc::channel(1);
    //     let driver = Self { event_sender };
    //     (
    //         Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
    //         driver,
    //     )
    // }

    // pub(crate) fn effect(&self, command: BuildCommand) -> impl Future<Output = ()> + 'static {
    //     let event_sender = self.event_sender.clone();
    //     async move {
    //         match command {
    //             BuildCommand::Spawn { path, envs } => {
    //                 let spawn_result = Command::new(path.clone())
    //                     .envs(envs.clone())
    //                     .stdout(Stdio::piped())
    //                     .stderr(Stdio::piped())
    //                     .spawn()
    //                     .context("spawn build process");

    //                 let mut child = match spawn_result {
    //                     Ok(child) => child,
    //                     Err(error) => {
    //                         event_sender
    //                             .send(BuildEvent::SpawnError(error))
    //                             .await
    //                             .unwrap();
    //                         return;
    //                     }
    //                 };

    //                 let event_sender_clone = event_sender.clone();
    //                 let stdout_join_handle = child
    //                     .for_stdout_line(move |line| {
    //                         let line = line.to_owned();
    //                         let event_sender = event_sender_clone.clone();
    //                         async move {
    //                             event_sender
    //                                 .send(BuildEvent::OutputLine {
    //                                     output: Output::Out,
    //                                     line,
    //                                 })
    //                                 .await
    //                                 .unwrap();
    //                         }
    //                         .boxed()
    //                     })
    //                     .unwrap();

    //                 let event_sender_clone = event_sender.clone();
    //                 let stderr_join_handle = child
    //                     .for_stderr_line(move |line| {
    //                         let line = line.to_owned();
    //                         let event_sender = event_sender_clone.clone();
    //                         async move {
    //                             event_sender
    //                                 .send(BuildEvent::OutputLine {
    //                                     output: Output::Err,
    //                                     line,
    //                                 })
    //                                 .await
    //                                 .unwrap();
    //                         }
    //                         .boxed()
    //                     })
    //                     .unwrap();

    //                 let pid = match child.id().context("obtain build process id") {
    //                     Ok(pid) => Pid::from_raw(pid as i32),
    //                     Err(error) => {
    //                         event_sender
    //                             .send(BuildEvent::SpawnError(error))
    //                             .await
    //                             .unwrap();

    //                         return;
    //                     }
    //                 };

    //                 event_sender.send(BuildEvent::Spawn(pid)).await.unwrap();

    //                 let wait_event = match child.wait().await {
    //                     Ok(exit_status) => BuildEvent::Exited(exit_status.code()),
    //                     Err(error) => BuildEvent::WaitError(error),
    //                 };

    //                 // TODO await concurrently
    //                 stderr_join_handle.await.unwrap();
    //                 stdout_join_handle.await.unwrap();

    //                 event_sender.send(wait_event).await.unwrap();
    //             }

    //             BuildCommand::Signal(pid, signal) => {
    //                 if let Err(error) = nix::sys::signal::kill(pid, signal) {
    //                     event_sender
    //                         .send(BuildEvent::SignalError(error))
    //                         .await
    //                         .unwrap();
    //                 };

    //                 event_sender
    //                     .send(BuildEvent::SignalSent(pid, SIGTERM))
    //                     .await
    //                     .unwrap();
    //             }
    //         }
    //     }
    // }
}
