use std::{
    convert::Infallible,
    path::PathBuf,
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::Mutex,
};

use rxrust::prelude::*;

pub(crate) struct ProcessSpawnDriver {
    event_sender: LocalSubject<'static, ProcessSpawnEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessSpawnEvent(pub(crate) Result<Rc<Mutex<Child>>, Rc<std::io::Error>>);

#[derive(Debug, Clone)]
pub(crate) struct ProcessSpawnCommand {
    pub(crate) path: PathBuf,
    pub(crate) envs: Vec<(String, String)>,
}

impl ProcessSpawnDriver {
    pub(crate) fn new() -> (
        LocalBoxedObservable<'static, ProcessSpawnEvent, Infallible>,
        Self,
    ) {
        let event_sender = Local::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(
        &self,
        command: ProcessSpawnCommand,
    ) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        async move {
            let result = Command::new(command.path.clone())
                .envs(command.envs.clone())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map(|child| Rc::new(Mutex::new(child)))
                .map_err(Rc::new);

            event_sender.next(ProcessSpawnEvent(result));
        }
    }
}
