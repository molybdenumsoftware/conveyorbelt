use std::{convert::Infallible, rc::Rc, sync::Mutex};

use rxrust::prelude::*;

use crate::browser::Browser;

pub(crate) struct BrowserSpawnDriver {
    event_sender: LocalSubject<'static, BrowserSpawnDriverEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserSpawnDriverEvent(pub(crate) Result<Rc<Mutex<Browser>>, Rc<anyhow::Error>>);

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

    pub(crate) fn effect(&self) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        async move {
            let result = Browser::init()
                .await
                .map(|browser| Rc::new(Mutex::new(browser)))
                .map_err(Rc::new);
            event_sender.next(BrowserSpawnDriverEvent(result));
        }
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
