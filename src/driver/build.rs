use std::{convert::Infallible, path::PathBuf, process::Stdio};

use futures::FutureExt;
use rxrust::prelude::*;
use tokio::{process::Command, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::common::ForStdoutputLine as _;

pub(crate) struct BuildDriver {
    event_sender: mpsc::Sender<BuildEvent>,
}

#[derive(Debug)]
pub(crate) enum BuildEvent {
    SpawnError(std::io::Error),
    Stdoutln(String),
    Stderrln(String),
    TerminatedSuccessfully,
    TerminatedWithFailure,
    WaitError(std::io::Error),
}

#[derive(Debug, Clone)]
pub(crate) struct BuildCommand {
    pub(crate) path: PathBuf,
    pub(crate) envs: Vec<(String, String)>,
}

impl BuildDriver {
    pub(crate) fn new() -> (SharedBoxedObservable<'static, BuildEvent, Infallible>, Self) {
        let (event_sender, event_receiver) = mpsc::channel(1);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self, command: BuildCommand) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            let spawn_result = Command::new(command.path.clone())
                .envs(command.envs.clone())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            let mut child = match spawn_result {
                Ok(child) => child,
                Err(error) => {
                    event_sender
                        .send(BuildEvent::SpawnError(error))
                        .await
                        .unwrap();
                    return;
                }
            };

            let event_sender_clone = event_sender.clone();
            let stdout_join_handle = child
                .for_stdout_line(move |line| {
                    let line = line.to_owned();
                    let event_sender = event_sender_clone.clone();
                    async move {
                        event_sender
                            .send(BuildEvent::Stdoutln(format!(
                                "build command stdout: {line}"
                            )))
                            .await
                            .unwrap();
                    }
                    .boxed()
                })
                .unwrap();

            let event_sender_clone = event_sender.clone();
            let stderr_join_handle = child
                .for_stderr_line(move |line| {
                    let line = line.to_owned();
                    let event_sender = event_sender_clone.clone();
                    async move {
                        event_sender
                            .blocking_send(BuildEvent::Stderrln(format!(
                                "build command stderr: {line}"
                            )))
                            .unwrap();
                    }
                    .boxed()
                })
                .unwrap();

            let wait_event = match child.wait().await {
                Ok(exit_status) => match exit_status.success() {
                    true => BuildEvent::TerminatedSuccessfully,
                    false => BuildEvent::TerminatedWithFailure,
                },
                Err(error) => BuildEvent::WaitError(error),
            };

            dbg!(&wait_event);

            // TODO await concurrently
            stderr_join_handle.await.unwrap();
            stdout_join_handle.await.unwrap();

            event_sender.send(wait_event).await.unwrap();
        }
    }
}
