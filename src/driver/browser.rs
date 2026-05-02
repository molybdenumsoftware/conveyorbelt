use std::convert::Infallible;

use chromiumoxide::error::CdpError;
use rxrust::prelude::*;
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};

use crate::browser::Browser;

pub(crate) struct BrowserDriver {
    event_sender: tokio::sync::mpsc::Sender<BrowserEvent>,
}

#[derive(Debug)]
pub(crate) enum BrowserCommand {
    Spawn { url: String },
    Reload(Browser),
}

#[derive(Debug)]
pub(crate) enum BrowserEvent {
    SpawnSuccess(Browser),
    SpawnError(anyhow::Error),
    ReloadSuccess(Browser),
    ReloadError(Browser, anyhow::Error),
    CdpError(CdpError),
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
                BrowserCommand::Spawn { url: address } => match Browser::init(address).await {
                    Ok((browser, mut handler)) => {
                        let event_sender = event_sender.clone();
                        tokio::spawn(async move {
                            while let Some(result) = handler.next().await {
                                if let Err(error) = result {
                                    event_sender
                                        .send(BrowserEvent::CdpError(error))
                                        .await
                                        .unwrap();
                                }
                            }
                        });
                        BrowserEvent::SpawnSuccess(browser)
                    }
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
