use std::{convert::Infallible, net::SocketAddr};

use rxrust::prelude::*;
use tokio_stream::wrappers::ReceiverStream;

use crate::browser::Browser;

pub(crate) struct BrowserDriver {
    event_sender: tokio::sync::mpsc::Sender<BrowserEvent>,
}

#[derive(Debug)]
pub(crate) enum BrowserCommand {
    Spawn(SocketAddr),
    Reload(Browser),
}

#[derive(Debug)]
pub(crate) enum BrowserEvent {
    SpawnSuccess(Browser),
    SpawnError(anyhow::Error),
    ReloadSuccess(Browser),
    ReloadError(Browser, anyhow::Error),
}

impl BrowserDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, BrowserEvent, Infallible>,
        Self,
    ) {
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(1);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self, command: BrowserCommand) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            let event = match command {
                BrowserCommand::Spawn(port) => match Browser::init(port).await {
                    Ok(browser) => BrowserEvent::SpawnSuccess(browser),
                    Err(error) => BrowserEvent::SpawnError(error),
                },
                BrowserCommand::Reload(browser) => match browser.reload().await {
                    Ok(_) => BrowserEvent::ReloadSuccess(browser),
                    Err(error) => BrowserEvent::ReloadError(browser, error),
                },
            };
            event_sender.send(event).await.unwrap();
        }
    }
}
