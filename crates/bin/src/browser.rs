use anyhow::Context as _;
use chromiumoxide::{
    BrowserConfig, Page,
    cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams},
};
use futures::{StreamExt as _, stream};
use tempfile::tempdir;
use tracing::{debug, info};

use crate::common::TESTING_MODE;

#[derive(Debug, derive_more::Display)]
#[display("browser; pid: {pid}, websocket address: {websocket_address}")]
pub(crate) struct Browser {
    pid: u32,
    websocket_address: String,
    page: Page,
}

impl Browser {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn websocket_address(&self) -> &str {
        &self.websocket_address
    }

    pub async fn launch(url: &str) -> anyhow::Result<Self> {
        info!("launching browser");
        let dir = tempdir().context("create data dir")?;
        debug!("browser data dir: {dir:?}");

        let mut config = BrowserConfig::builder()
            .with_head()
            .viewport(None)
            .user_data_dir(dir.path())
            .port(0);

        if std::env::var(TESTING_MODE).is_ok() {
            config = config.launch_timeout(Duration::from_mins(2));
        }

        let config = config.build().context("build config")?;

        debug!("browser config: {config:?}");

        let (mut browser, mut handler) = chromiumoxide::Browser::launch(config)
            .await
            .context("launch")?;

        let pid = browser
            .get_mut_child()
            .context("get child")?
            .as_mut_inner()
            .id()
            .context("get pid")?;

        info!("browser launched: {pid}");

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
        info!("navigate to {url}");
        let page = browser.new_page(url).await.context("create page")?;
        Box::new(browser).leak();

        Ok(Browser {
            pid,
            websocket_address,
            page,
        })
    }

    async fn reload_page(self) -> anyhow::Result<()> {
        info!("reload page");
        self.handle.page.reload().await.context("reload page")
    }
}
