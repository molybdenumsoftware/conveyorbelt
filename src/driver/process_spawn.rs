use std::{
    borrow::BorrowMut as _,
    cell::RefCell,
    convert::Infallible,
    ops::DerefMut as _,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
};

use rxrust::{observer::DynObserver as _, prelude::*};

use crate::common::SERVE_PATH;

pub(crate) struct ProcessSpawnDriver {
    event_sender: LocalSubject<'static, ProcessSpawnDriverEvent, Infallible>,
}

pub(crate) type ProcessSpawnDriverEvent = Result<Rc<Mutex<Child>>, Rc<std::io::Error>>;

#[derive(Debug, Clone)]
pub(crate) struct ProcessSpawnDriverCommand {
    pub(crate) path: PathBuf,
    pub(crate) envs: Vec<(String, String)>,
}

impl ProcessSpawnDriver {
    pub(crate) fn new() -> (
        LocalBoxedObservable<'static, ProcessSpawnDriverEvent, Infallible>,
        Self,
    ) {
        let event_sender = Local::subject();

        let event_stream = event_sender.clone().box_it();

        let driver = Self { event_sender };

        (event_stream, driver)
    }

    pub(crate) fn init(
        mut self,
        commands: LocalBoxedObservable<'static, ProcessSpawnDriverCommand, Infallible>,
    ) -> BoxedSubscription {
        commands
            .delay(Duration::ZERO)
            .subscribe(move |command| {
                let result = Command::new(command.path)
                    .envs(command.envs)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map(|child| Rc::new(Mutex::new(child)))
                    .map_err(Rc::new);

                self.event_sender.next(result);
            })
            .into_boxed()
    }
}
