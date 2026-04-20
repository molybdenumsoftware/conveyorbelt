use std::{
    convert::Infallible,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
};

use rxrust::prelude::*;

use crate::common::ForStdoutputLine as _;

pub(crate) struct BuildDriver {
    event_sender: SharedSubject<'static, BuildEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) enum BuildEvent {
    SpawnError(Arc<std::io::Error>),
    Stdoutln(String),
    Stderrln(String),
    TerminatedSuccessfully,
    TerminatedWithFailure,
    WaitError(Arc<std::io::Error>),
}

#[derive(Debug, Clone)]
pub(crate) struct BuildCommand {
    pub(crate) path: PathBuf,
    pub(crate) envs: Vec<(String, String)>,
}

impl BuildDriver {
    pub(crate) fn new() -> (SharedBoxedObservable<'static, BuildEvent, Infallible>, Self) {
        let event_sender = Shared::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(&self, command: BuildCommand) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        async move {
            let spawn_result = Command::new(command.path.clone())
                .envs(command.envs.clone())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            let mut child = match spawn_result {
                Ok(child) => child,
                Err(error) => {
                    event_sender.next(BuildEvent::SpawnError(Arc::new(error)));
                    return;
                }
            };

            let mut event_sender_clone = event_sender.clone();
            child
                .for_stdout_line(move |line| {
                    event_sender_clone.next(BuildEvent::Stdoutln(format!(
                        "build command stdout: {line}"
                    )));
                })
                .unwrap();

            let mut event_sender_clone = event_sender.clone();
            child
                .for_stderr_line(move |line| {
                    event_sender_clone.next(BuildEvent::Stderrln(format!(
                        "build command stderr: {line}"
                    )));
                })
                .unwrap();

            let wait_event = match child.wait() {
                Ok(exit_status) => match exit_status.success() {
                    true => BuildEvent::TerminatedSuccessfully,
                    false => BuildEvent::TerminatedWithFailure,
                },
                Err(error) => BuildEvent::WaitError(Arc::new(error)),
            };

            event_sender.next(wait_event);
        }
    }
}
