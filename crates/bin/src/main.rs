mod app;
mod cli;
#[path = "../common.rs"]
mod common;
mod effects;
mod logging;
mod project_path;

use std::sync::Arc;

use futures::{FutureExt as _, StreamExt};
use rxrust::prelude::*;

use crate::{
    app::App,
    cli::Args,
    effects::{fswatch::FsWatchInit, server::ServeDir, signal::InstallSignalHandler},
};

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build_local(tokio::runtime::LocalOptions::default())?
        .block_on(async_main())?;
    Ok(())
}

async fn async_main() -> anyhow::Result<()> {
    logging::init();
    let Args { build_command } = crate::cli::parse();

    // TODO effect?
    let project_root = crate::project_path::resolve(&std::env::current_dir()?)?;

    let app = App {
        project_root,
        build_command_path: build_command,
    };

    let exit_code = app
        .run()
        // TODO why doesn't this work? rxrust bug?
        // .first()
        // .into_future()
        .into_stream()
        .next()
        .await
        .unwrap()
        .unwrap();

    std::process::exit(exit_code);
}
