use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
};

use rxrust::prelude::*;

use crate::browser::Browser;

pub(crate) struct BrowserSpawnDriver {
    event_sender: SharedSubject<'static, BrowserSpawnEvent, Infallible>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserSpawnEvent(pub(crate) Result<Arc<Mutex<Browser>>, Arc<anyhow::Error>>);

impl BrowserSpawnDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, BrowserSpawnEvent, Infallible>,
        Self,
    ) {
        let event_sender = Shared::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(&self) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        async move {
            let result = Browser::init()
                .await
                .map(|browser| Arc::new(Mutex::new(browser)))
                .map_err(Arc::new);
            event_sender.next(BrowserSpawnEvent(result));
        }
    }
}
