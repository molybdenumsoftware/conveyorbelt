use std::convert::Infallible;

use anyhow::{Context as _, anyhow, bail};
use chromiumoxide::{
    BrowserConfig,
    cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams},
};
use rxrust::prelude::*;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio_stream::{StreamExt as _, wrappers::ReceiverStream};
use tracing::debug;

use crate::{common::TESTING_MODE, effects::Effect};

#[derive(Debug, derive_more::Display)]
// TODO using observable that is known to have at most a single emit is suboptimal.
// What's the alternative?
pub(crate) enum BrowserCommand {
    #[display("spawn and go to {url}")]
    Spawn { url: String },
    #[display("reload")]
    Reload(BrowserReload),
}

#[derive(derive_more::Display)]
pub(crate) enum BrowserSpawnEvent {
    #[display("spawned")]
    Spawn {
        reload: BrowserReload,
        pid: u32,
        websocket_address: String,
    },
    #[display("spawn error: {_0}")]
    SpawnError(anyhow::Error),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum BrowserReloadEvent {
    #[display("reloaded")]
    Reload(BrowserReload),
    #[display("reload error: {_1}")]
    ReloadError(BrowserReload, anyhow::Error),
}

pub (crate) struct Browser {
    pub reload: BrowserReload,
    pid: u32,
    websocket_address: String,

    
}

impl Browser {
    pub (crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub (crate) fn websocket_address(&self) -> u32 {
        self.pid
    }
}

// TODO should we be using the Observable types' error type argument?

#[derive(Debug)]
pub(crate) struct BrowserSpawn {
    pub(crate) url: String,
}

impl Effect< for BrowserSpawn {
    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, BrowserSpawnEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);
        tokio::spawn(async move {
            let result: anyhow::Result<_> = (async || {
                let browser_data_dir =
                    tempdir().context("failed to create temporary browser data dir")?;

                debug!("browser data dir: {browser_data_dir:?}");

                let mut browser_config_builder = BrowserConfig::builder()
                    .with_head()
                    .viewport(None)
                    .user_data_dir(browser_data_dir.path())
                    .port(0);

                if std::env::var(TESTING_MODE).is_ok() {
                    browser_config_builder =
                        browser_config_builder.launch_timeout(Duration::from_mins(15));
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

                tokio::spawn(async move { while handler.next().await.is_some() {} });

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
                let websocket_address = browser.websocket_address().clone();
                let page = browser.new_page(self.url).await.context("creating page")?;
                let browser_reload = BrowserReload { page };

                Box::leak(Box::new(browser));
                Ok((browser_reload, pid, websocket_address))
            })()
            .await;
            let event = match result {
                Ok((reload, pid, websocket_address)) => BrowserSpawnEvent::Spawn {
                    reload,
                    pid,
                    websocket_address,
                },
                Err(err) => BrowserSpawnEvent::SpawnError(err),
            };
            event_sender.send(event).await.unwrap();
        });
        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}

#[derive(Debug)]
pub(crate) struct BrowserReload {
    page: chromiumoxide::Page,
}

impl BrowserReload {
    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, BrowserReloadEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);
        tokio::spawn(async move {
            let event = match self.page.reload().await.context("reloading") {
                Ok(_) => BrowserReloadEvent::Reload(self),
                Err(err) => BrowserReloadEvent::ReloadError(self, err),
            };
            event_sender.send(event).await.unwrap();
        });
        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}
