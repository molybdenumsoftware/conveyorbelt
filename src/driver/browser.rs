use std::convert::Infallible;

use anyhow::{Context as _, anyhow, bail};
use chromiumoxide::{
    BrowserConfig,
    cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams},
    error::CdpError,
};
use rxrust::prelude::*;
use tempfile::tempdir;
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};
use tracing::debug;

use crate::common::TESTING_MODE;

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
                BrowserCommand::Spawn { url: address } => {
                    match Browser::spawn(address, event_sender.clone()).await {
                        Ok(browser) => BrowserEvent::SpawnSuccess(browser),
                        Err(error) => BrowserEvent::SpawnError(error),
                    }
                }
                BrowserCommand::Reload(browser) => match browser.reload().await {
                    Ok(_) => BrowserEvent::ReloadSuccess(browser),
                    Err(error) => BrowserEvent::ReloadError(browser, error),
                },
            };
            event_sender.send(event).await.unwrap();
        }
    }
}

#[derive(Debug)]
//pub(crate) struct Browser(&'static mut chromiumoxide::Browser);
pub(crate) struct Browser {
    handle: &'static chromiumoxide::Browser,
    pid: u32,
    page: chromiumoxide::Page,
}

impl Browser {
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn debugging_address(&self) -> String {
        self.handle.websocket_address().clone()
    }

    pub(crate) async fn spawn(
        url: String,
        event_sender: tokio::sync::mpsc::Sender<BrowserEvent>,
    ) -> anyhow::Result<Self> {
        let browser_data_dir = tempdir().context("failed to create temporary browser data dir")?;

        // #TODO do not trace anywhere?
        debug!("browser data dir: {browser_data_dir:?}");

        let mut browser_config_builder = BrowserConfig::builder()
            .with_head()
            .viewport(None)
            .user_data_dir(browser_data_dir.path())
            .port(0);

        if std::env::var(TESTING_MODE).is_ok() {
            browser_config_builder = browser_config_builder.launch_timeout(Duration::from_mins(15));
        }

        let browser_config = browser_config_builder
            .build()
            .map_err(|e| anyhow!("failed to build browser config: {e}"))?;

        debug!("browser config: {browser_config:?}");

        let (mut browser, mut handler) = chromiumoxide::Browser::launch(browser_config)
            .await
            .context("failed to launch browser")?;

        let pid = browser
            .get_mut_child()
            .context("failed to obtain mutable reference to browser Child")?
            .as_mut_inner()
            .id()
            .context("failed to obtain browser pid")?;

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

        let targets = browser
            .execute(GetTargetsParams { filter: None })
            .await
            .context("get targets")?;

        let targets = targets.target_infos.as_slice();

        let [target] = &targets else {
            bail!("number of browser pages is not 1: {targets:?}");
        };

        if target.url != "chrome://newtab/" {
            bail!("unexpected browser page: {target:?}");
        }

        // TODO phrase anyhow context method strings

        browser
            .execute(CloseTargetParams {
                target_id: target.target_id.clone(),
            })
            .await
            .context("close newtab page")?;

        let page = browser.new_page(url).await.context("creating page")?;

        Ok(Self {
            handle: Box::leak(Box::new(browser)),
            pid,
            page,
        })
    }
    pub(crate) async fn reload(&self) -> anyhow::Result<()> {
        self.page.reload().await.context("reloading")?;
        Ok(())
    }
}
