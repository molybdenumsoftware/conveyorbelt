use anyhow::{Context as _, anyhow};
use chromiumoxide::{
    BrowserConfig,
    cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams},
};
use rxrust::prelude::*;
use tempfile::tempdir;
use tokio_stream::StreamExt as _;
use tracing::debug;

use crate::{common::TESTING_MODE, effects::Effect};

#[derive(Debug, derive_more::Display)]
// TODO using observable that is known to have at most a single emit is suboptimal.
// What's the alternative?
pub(crate) enum BrowserCommand {
    #[display("spawn and go to {url}")]
    Spawn { url: String },
    #[display("reload")]
    Reload(PageReload),
}

#[derive(Debug, derive_more::Display)]
#[display("browser; pid: {pid}, websocket address: {websocket_address}")]
pub(crate) struct Browser {
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
}

// TODO should we be using the Observable types' error type argument?

#[derive(Debug, derive_more::Display)]
#[display("spawn browser and go to: {url}")]
pub(crate) struct BrowserSpawn {
    pub(crate) url: String,
}

#[derive(thiserror::Error, Debug)]
#[error("browser spawn: {_0}")]
pub(crate) struct BrowserSpawnError(#[from] anyhow::Error);

#[derive(Debug, derive_more::Display)]
#[display("spawned: {browser}")]
pub(crate) struct BrowserSpawnSuccess {
    pub(crate) browser: Browser,
    pub(crate) page_reload: PageReload,
}

impl Effect<BrowserSpawnSuccess, BrowserSpawnError> for BrowserSpawn {
    async fn effect(self) -> Result<BrowserSpawnSuccess, BrowserSpawnError> {
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
            return Err(anyhow!("number of pages is not 1: {targets:?}").into());
        };

        if target.url != "chrome://newtab/" {
            return Err(anyhow!("unexpected page: {target:?}").into());
        }

        browser
            .execute(CloseTargetParams {
                target_id: target.target_id.clone(),
            })
            .await
            .context("close newtab page")?;

        let websocket_address = browser.websocket_address().clone();
        let page = browser.new_page(self.url).await.context("create page")?;
        let page_reload = PageReload { page };

        Box::leak(Box::new(browser));
        Ok(BrowserSpawnSuccess {
            browser: Browser {
                pid,
                websocket_address,
            },
            page_reload,
        })
    }
}

#[derive(Debug)]
pub(crate) struct PageReload {
    page: chromiumoxide::Page,
}

#[derive(Debug, derive_more::Display)]
#[display("browser reloaded")]
pub(crate) struct BrowserReloaded(PageReload);

#[derive(Debug, thiserror::Error)]
#[error("page reload error: {0}")]
pub(crate) struct PageReloadError(#[from] anyhow::Error);

impl Effect<Self, (PageReloadError, Self)> for PageReload {
    async fn effect(self) -> Result<Self, (PageReloadError, Self)> {
        match self.page.reload().await.context("reloading") {
            Ok(_) => Ok(self),
            Err(error) => Err((error.into(), self)),
        }
    }
}
