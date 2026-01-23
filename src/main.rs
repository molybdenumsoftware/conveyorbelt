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

use std::{env::current_dir, time::Duration};

use watchexec::Watchexec;
use watchexec_events::Priority;

use crate::{
    event_filterer::EventFilterer,
    event_handler::initial_event,
};

#[tokio::main]

async fn main() -> anyhow::Result<()> {
    logging::init();
    let args = cli::parse();
    let project_root = project_path::resolve(&current_dir()?).await?;
    let serve_dir = serve_dir::obtain()?;
    let wx = Watchexec::default();
    wx.config.throttle(Duration::ZERO); // to guarantee one event at a time
    wx.config.pathset([project_root.as_path()]);
    wx.config.filterer(EventFilterer::new(project_root).await?);
    wx.config.on_action(event_handler::new(
        project_root,
        serve_dir.path().to_path_buf(),
        args.build_command,
    ));
    wx.send_event(initial_event(), Priority::Normal).await?;
    wx.main().await??;
    Ok(())
}
