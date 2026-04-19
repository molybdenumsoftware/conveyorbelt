use std::{
    convert::Infallible,
    process::Child,
    sync::{Arc, Mutex},
};

use rxrust::prelude::*;
use tracing::info;

use crate::common::ForStdoutputLine as _;

pub(crate) struct ProcessWaitDriver {
    event_sender: SharedSubject<'static, ProcessWaitEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) enum ProcessWaitEvent {
    FailedToWait(Arc<std::io::Error>),
    StderrLine(String),
    StdoutLine(String),
    TerminatedSuccessfully,
    TerminatedWithFailure,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessWaitCommand(pub(crate) Arc<Mutex<Child>>);

impl ProcessWaitDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, ProcessWaitEvent, Infallible>,
        Self,
    ) {
        let event_sender = Shared::subject();
        let event_stream = event_sender.clone().box_it();

        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(
        &self,
        ProcessWaitCommand(child): ProcessWaitCommand,
    ) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();

        async move {
            let mut child = child.lock().unwrap();

            child
                .for_stdout_line(|line| {
                    info!("build command stdout: {line}");
                })
                .unwrap();

            child
                .for_stderr_line(|line| {
                    info!("build command stderr: {line}");
                })
                .unwrap();

            let event = match child.wait() {
                Ok(exit_status) => match exit_status.success() {
                    true => ProcessWaitEvent::TerminatedSuccessfully,
                    false => ProcessWaitEvent::TerminatedWithFailure,
                },
                Err(error) => ProcessWaitEvent::FailedToWait(Arc::new(error)),
            };
            drop(child);

            event_sender.next(event);
        }
    }
}
