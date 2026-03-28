use std::{
    convert::Infallible,
    process::{Child, ExitStatus},
    rc::Rc,
    sync::Mutex,
};

use rxrust::prelude::*;
use tracing::info;

use crate::common::ForStdoutputLine as _;

pub(crate) struct ProcessWaitDriver {
    event_sender: LocalSubject<'static, ProcessWaitDriverEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) enum ProcessWaitDriverEvent {
    Terminated(ExitStatus),
    FailedToWait(Rc<std::io::Error>),
    StderrLine(String),
    StdoutLine(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessWaitCommand(pub(crate) Rc<Mutex<Child>>);

impl ProcessWaitDriver {
    pub(crate) fn new() -> (
        LocalBoxedObservable<'static, ProcessWaitDriverEvent, Infallible>,
        Self,
    ) {
        let event_sender = Local::subject();
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
                Ok(exit_status) => ProcessWaitDriverEvent::Terminated(exit_status),
                Err(error) => ProcessWaitDriverEvent::FailedToWait(Rc::new(error)),
            };

            event_sender.next(event);
        }
    }
}
