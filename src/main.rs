mod browser;
mod build_command;
mod cli;
#[path = "../common.rs"]
mod common;
mod config;
mod event_filterer;
mod event_handler;
mod logging;
mod project_path;
mod serve_dir;
mod server;
mod testing;

use watchexec::Watchexec;
use watchexec_events::{Event, Priority};

use crate::{config::Config, event_filterer::EventFilterer};

#[tokio::main]

async fn main() -> anyhow::Result<()> {
    logging::init();
    let config = Config::obtain()?;
    let mut wx = Watchexec::default();
    wx.config.pathset([config.project_root.as_path()]);
    wx.config
        .filterer(EventFilterer::new(config.project_root.clone()).await?);
    event_handler::set(&mut wx.config, config);
    wx.send_event(Event::default(), Priority::Normal).await?;
    wx.main().await??;
    Ok(())
}
