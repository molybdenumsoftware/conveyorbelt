use std::{
    borrow::BorrowMut as _,
    cell::RefCell,
    convert::Infallible,
    ops::DerefMut as _,
    path::PathBuf,
    process::{Child, Command, ExitStatus},
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
};

use rxrust::prelude::*;
use tracing::info;

use crate::common::{ForStdoutputLine as _, SERVE_PATH};

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
pub(crate) struct ProcessWaitDriverCommand(pub(crate) Rc<Mutex<Child>>);

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

    pub(crate) fn init(
        mut self,
        commands: LocalBoxedObservable<'static, ProcessWaitDriverCommand, Infallible>,
    ) -> BoxedSubscription {
        commands
            //TODO see whether the delay can be in one place
            .delay(Duration::ZERO)
            .subscribe(move |ProcessWaitDriverCommand(child)| {
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
                self.event_sender.next(event);
            })
            .into_boxed()
    }
}
