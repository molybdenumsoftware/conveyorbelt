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

use crate::common::TESTING_MODE;

#[derive(Debug, derive_more::Display)]
pub(crate) enum BrowserCommand {
    #[display("spawn and go to {url}")]
    Spawn { url: String },
    #[display("reload")]
    Reload(Browser),
}

#[derive(derive_more::Display)]
pub(crate) enum BrowserSpawnEvent {
    #[display("spawned")]
    Spawn(Browser),
    #[display("spawn error: {_0}")]
    SpawnError(anyhow::Error),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum BrowserReloadEvent {
    #[display("reloaded")]
    Reload(Browser),
    #[display("reload error: {_1}")]
    ReloadError(Browser, anyhow::Error),
}

// TODO should we be using the Observable types' error type argument?

#[derive(Debug)]
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

    pub(crate) fn spawn(
        url: String,
    ) -> SharedBoxedObservable<'static, BrowserSpawnEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);
        tokio::spawn(async move {
            let result: anyhow::Result<Self> = (async || {
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

                let page = browser.new_page(url).await.context("creating page")?;
                Ok(Self {
                    handle: Box::leak(Box::new(browser)),
                    pid,
                    page,
                })
            })()
            .await;
            let event = match result {
                Ok(browser) => BrowserSpawnEvent::Spawn(browser),
                Err(err) => BrowserSpawnEvent::SpawnError(err),
            };
            event_sender.send(event).await.unwrap();
        });
        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
    pub(crate) fn reload(&self) -> SharedBoxedObservable<'static, BrowserReloadEvent, Infallible> {
        self.page.reload().await.context("reloading")?;
        Ok(())
    }
}
