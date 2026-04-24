use std::convert::Infallible;

use rxrust::prelude::*;
use tokio_stream::wrappers::ReceiverStream;

use crate::browser::Browser;

pub(crate) struct BrowserDriver {
    event_sender: tokio::sync::mpsc::Sender<BrowserEvent>,
}

#[derive(Debug)]
pub(crate) enum BrowserEvent {
    SpawnSuccess(Browser),
    SpawnError(anyhow::Error),
}

impl BrowserDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, BrowserEvent, Infallible>,
        Self,
    ) {
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(0);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            let result = Browser::init().await;
            let event = match result {
                Ok(browser) => BrowserEvent::SpawnSuccess(browser),
                Err(error) => BrowserEvent::SpawnError(error),
            };
            event_sender.send(event).await.unwrap();
        }
    }
}
