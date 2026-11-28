use std::time::Duration;

use anyhow::{Context as _, anyhow};
use chromiumoxide::{
    BrowserConfig, Page,
    cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams},
};
use futures::StreamExt as _;
use state_for_testing::TESTING_MODE;
use tempfile::tempdir;
use tracing::{debug, info};

#[derive(Debug, derive_more::Display)]
#[display("browser; pid: {pid}, websocket address: {websocket_address}")]
pub(crate) struct Browser {
    pid: u32,
    websocket_address: String,
    page: Page,
}

impl Browser {
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    pub(crate) fn websocket_address(&self) -> &str {
        &self.websocket_address
    }

    pub(crate) async fn launch(url: &str) -> anyhow::Result<Self> {
        let dir = tempdir().context("create data dir")?;
        debug!("data dir {:?}", dir.path());

        let mut config = BrowserConfig::builder()
            .with_head()
            .viewport(None)
            .user_data_dir(dir.path())
            .port(0);

        if std::env::var(TESTING_MODE).is_ok() {
            config = config.launch_timeout(Duration::from_mins(2)).no_sandbox();
        }

        let config = config
            .build()
            .map_err(|s| anyhow!("{s}"))
            .context("build config")?;

        debug!("launching {config:?}");

        let (mut browser, mut handler) = chromiumoxide::Browser::launch(config)
            .await
            .context("launch")?;

        let pid = browser
            .get_mut_child()
            .context("get child")?
            .as_mut_inner()
            .id()
            .context("get pid")?;

        info!("launched {pid}");

        tokio::spawn(async move { while handler.next().await.is_some() {} });

        let targets = browser
            .execute(GetTargetsParams { filter: None })
            .await
            .context("get targets")?;

        for target in &targets.target_infos {
            if target.url != "chrome://newtab/" {
                continue;
            }
            browser
                .execute(CloseTargetParams {
                    target_id: target.target_id.clone(),
                })
                .await
                .context("close newtab page")?;
        }

        let websocket_address = browser.websocket_address().clone();
        info!("navigate {url}");
        let page = browser.new_page(url).await.context("create page")?;
        Box::leak(Box::new(browser));

        Ok(Browser {
            pid,
            websocket_address,
            page,
        })
    }

    pub(crate) async fn reload_page(&mut self) -> anyhow::Result<()> {
        debug!("reload page");
        self.page.reload().await.context("reload page")?;
        info!("reloaded page");
        Ok(())
    }
}
