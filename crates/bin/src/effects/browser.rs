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

#[derive(Debug)]
pub(crate) struct Browser {
    page: chromiumoxide::Page,
    pid: u32,
    websocket_address: String,
}

impl Browser {
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn websocket_address(&self) -> &str {
        &self.websocket_address
    }

    pub(crate) fn reload(&self) -> BrowserReload {
        BrowserReload {
            page: self.page.Clone(),
        }
    }
}

// TODO should we be using the Observable types' error type argument?

#[derive(Debug)]
pub(crate) struct BrowserSpawn {
    pub(crate) url: String,
}

#[derive(derive_more::Display, derive_more::From)]
#[display("browser spawn: {_0}")]
pub(crate) struct BrowserSpawnError(#[from] anyhow::Error);

impl Effect<Browser, BrowserSpawnError> for BrowserSpawn {
    async fn effect(self) -> anyhow::Result<Browser> {
        let browser_data_dir = tempdir().context("create data dir")?;
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
            .map_err(|e| anyhow!("build config: {e}"))?;

        debug!("browser config: {browser_config:?}");

        let (mut browser, mut handler) = chromiumoxide::Browser::launch(browser_config)
            .await
            .context("launch")?;

        let pid = browser
            .get_mut_child()
            .context("get child")?
            .as_mut_inner()
            .id()
            .context("get pid")?;

        tokio::spawn(async move { while handler.next().await.is_some() {} });

        let targets = browser
            .execute(GetTargetsParams { filter: None })
            .await
            .context("get targets")?;

        let targets = targets.target_infos.as_slice();

        let [target] = &targets else {
            bail!("number of pages is not 1: {targets:?}");
        };

        if target.url != "chrome://newtab/" {
            bail!("unexpected page: {target:?}");
        }

        browser
            .execute(CloseTargetParams {
                target_id: target.target_id.clone(),
            })
            .await
            .context("close newtab page")?;

        let websocket_address = browser.websocket_address().clone();
        let page = browser.new_page(self.url).await.context("create page")?;
        let browser_reload = BrowserReload { page };

        Box::leak(Box::new(browser));
        Ok(Browser {
            reload: browser_reload,
            pid,
            websocket_address,
        })
    }
}

#[derive(Debug)]
pub(crate) struct BrowserReload {
    page: chromiumoxide::Page,
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum BrowserReloadEvent {
    #[display("reloaded")]
    Reload(BrowserReload),
    #[display("reload error: {_1}")]
    ReloadError(BrowserReload, anyhow::Error),
}

impl Effect<Browser, BrowserSpawnError> for BrowserReload {

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
