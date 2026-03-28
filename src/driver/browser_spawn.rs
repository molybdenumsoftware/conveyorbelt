use std::{
    convert::Infallible,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
};

use anyhow::Context as _;
use rxrust::prelude::*;
use tempfile::tempdir;
use tokio::task;

use crate::browser::Browser;

pub(crate) struct BrowserSpawnDriver {
    event_sender: LocalSubject<'static, BrowserSpawnDriverEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserSpawnDriverEvent(pub(crate) Result<Rc<Mutex<Browser>>, Rc<anyhow::Error>>);

#[derive(Debug, Clone)]
pub(crate) struct BrowserSpawnDriverCommand;

impl BrowserSpawnDriver {
    pub(crate) fn new() -> (
        LocalBoxedObservable<'static, BrowserSpawnDriverEvent, Infallible>,
        Self,
    ) {
        let event_sender = Local::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn init(
        mut self,
        commands: LocalBoxedObservable<'static, BrowserSpawnDriverCommand, Infallible>,
    ) -> BoxedSubscription {
        commands
            .delay(Duration::ZERO)
            .switch_map(|_| {
                Local::from_future(async {
                    Browser::init()
                        .await
                        .map(|browser| Rc::new(Mutex::new(browser)))
                        .map_err(Rc::new)
                })
            })
            .subscribe(move |result| {
                self.event_sender.next(BrowserSpawnDriverEvent(result));
            })
            .into_boxed()
    }
}

// use tokio::runtime::Runtime;
// use tokio::task;

// let rt  = Runtime::new().unwrap();
// let local = task::LocalSet::new();
// local.block_on(&rt, async {
//     let join = task::spawn_local(async {
//         let blocking_result = task::spawn_blocking(|| {
//             // ...
//         }).await;
//         // ...
//     });
//     join.await.unwrap();
// })
