mod app;
mod browser;
mod cli;
#[path = "../common.rs"]
mod common;
mod driver;
mod logging;
mod project_path;
mod serve_dir;
mod server;

use std::rc::Rc;

use futures::FutureExt as _;
use rxrust::prelude::*;

use crate::{
    app::{App, Command},
    cli::Args,
    driver::{
        browser_spawn::BrowserSpawnDriver, process_spawn::ProcessSpawnDriver,
        process_wait::ProcessWaitDriver,
    },
    server::Server,
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

    let serve_dir = Rc::new(crate::serve_dir::obtain()?);
    let project_root = crate::project_path::resolve(&std::env::current_dir()?)?;

    let server_port = Server::init(serve_dir.path().to_path_buf())?;

    let (process_spawn_driver_events, process_spawn_driver) = ProcessSpawnDriver::new();
    let (browser_spawn_driver_events, browser_spawn_driver) = BrowserSpawnDriver::new();
    let (process_wait_driver_events, process_wait_driver) = ProcessWaitDriver::new();

    let app = App {
        project_root,
        serve_dir: serve_dir.clone(),
        build_command,
        server_port,
        process_spawn_driver_events,
        process_wait_driver_events,
        browser_spawn_driver_events,
    };

    app.run()
        .switch_map(move |command| {
            let effect = match command {
                Command::ProcessSpawn(command) => {
                    process_spawn_driver.effect(command).boxed_local()
                }
                Command::ProcessWait(command) => process_wait_driver.effect(command).boxed_local(),
                Command::BrowserSpawn => browser_spawn_driver.effect().boxed_local(),
                Command::Stdout(string) => async move { print!("{string}") }.boxed_local(),
            };
            Local::from_future(effect)
        })
        .delay(Duration::ZERO)
        .subscribe(|_| {});

    std::future::pending::<()>().await;
    Ok(())
}
