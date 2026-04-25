use std::{net::SocketAddr, time::Duration};

use anyhow::{Context as _, anyhow};
use chromiumoxide::BrowserConfig;
use tempfile::tempdir;
use tracing::debug;

use crate::common::TESTING_MODE;

#[derive(Debug)]
//pub(crate) struct Browser(&'static mut chromiumoxide::Browser);
pub(crate) struct Browser {
    handle: &'static chromiumoxide::Browser,
    pid: u32,
    page: chromiumoxide::Page,
}

impl Browser {
    pub(crate) async fn init(address: SocketAddr) -> anyhow::Result<Self> {
        let browser_data_dir = tempdir().context("failed to create temporary browser data dir")?;

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

        let (mut browser, _handler) = chromiumoxide::Browser::launch(browser_config)
            .await
            .context("failed to launch browser")?;

        let pid = browser
            .get_mut_child()
            .context("failed to obtain mutable reference to browser Child")?
            .as_mut_inner()
            .id()
            .context("failed to obtain browser pid")?;

        let page = browser
            .new_page(address.to_string())
            .await
            .context("creating page")?;

        Ok(Self {
            handle: Box::leak(Box::new(browser)),
            pid,
            page,
        })
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn debugging_address(&self) -> String {
        self.handle.websocket_address().clone()
    }

    pub(crate) async fn reload(&self) -> anyhow::Result<()> {
        self.page.reload().await.context("reloading")?;
        Ok(())
    }
}
