mod browser;
mod build_command;
mod cli;
#[path = "../common.rs"]
mod common;
mod event_filterer;
mod event_handler;
mod logging;
mod project_path;
mod serve_dir;
mod server;
mod testing;
mod config;

use std::time::Duration;

use watchexec::Watchexec;
use watchexec_events::Priority;

use crate::{
    config::Config, event_filterer::EventFilterer, event_handler::initial_event
};

#[tokio::main]

async fn main() -> anyhow::Result<()> {
    logging::init();
    let config = Config::obtain()?;
    let wx = Watchexec::default();
    wx.config.throttle(Duration::ZERO); // to guarantee one event at a time
    wx.config.pathset([config.project_root.as_path()]);
    wx.config.filterer(EventFilterer::new(config.project_root).await?);
    wx.config.on_action(event_handler::new(config));
    wx.send_event(initial_event(), Priority::Normal).await?;
    wx.main().await??;
    Ok(())
}
