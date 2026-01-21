use std::time::Duration;

use anyhow::{Context as _, anyhow};
use chromiumoxide::BrowserConfig;
use tempfile::tempdir;
use tracing::debug;

use crate::common::TESTING_MODE;

#[derive(Debug)]
pub struct Browser(&'static mut chromiumoxide::Browser);

impl Browser {
    pub async fn init() -> anyhow::Result<Self> {
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

        let (browser, _handler) = chromiumoxide::Browser::launch(browser_config)
            .await
            .context("failed to launch browser")?;

        Ok(Self(Box::leak(Box::new(browser))))
    }

    pub fn pid(&mut self) -> anyhow::Result<u32> {
        self.0
            .get_mut_child()
            .context("failed to obtain mutable reference to browser Child")?
            .as_mut_inner()
            .id()
            .context("failed to obtain browser pid")
    }

    pub fn debugging_address(&self) -> String {
        self.0.websocket_address().clone()
    }
}
