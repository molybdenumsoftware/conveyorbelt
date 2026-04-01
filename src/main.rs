mod app;
mod browser;
mod cli;
#[path = "../common.rs"]
mod common;
mod driver;
mod logging;
mod project_path;
mod serve_dir;

use std::rc::Rc;

use futures::FutureExt as _;
use rxrust::prelude::*;

use crate::{
    app::{App, Command, Event},
    cli::Args,
    driver::{
        browser_spawn::BrowserSpawnDriver, process_spawn::ProcessSpawnDriver,
        process_wait::ProcessWaitDriver, server::ServerSpawnDriver,
    },
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

    let (server_spawn_events, server_spawn_driver) = ServerSpawnDriver::new();
    let (process_spawn_events, process_spawn_driver) = ProcessSpawnDriver::new();
    let (browser_spawn_events, browser_spawn_driver) = BrowserSpawnDriver::new();
    let (process_wait_events, process_wait_driver) = ProcessWaitDriver::new();

    let app = App {
        project_root,
        serve_dir: serve_dir.clone(),
        build_command,
    };

    let input_events = Local::merge_observables([
        server_spawn_events.map(Event::ServerSpawn).box_it(),
        process_spawn_events.map(Event::ProcessSpawn).box_it(),
        process_wait_events.map(Event::ProcessWait).box_it(),
        browser_spawn_events.map(Event::BrowserSpawn).box_it(),
    ])
    .box_it();

    // map(f).merge_all(usize::MAX)
    let effect_stream = app.run(input_events).flat_map(move |command| {
        let effect = match command {
            Command::ServerSpawn(command) => server_spawn_driver.effect(command).boxed_local(),
            Command::ProcessSpawn(command) => process_spawn_driver.effect(command).boxed_local(),
            Command::ProcessWait(command) => process_wait_driver.effect(command).boxed_local(),
            Command::BrowserSpawn => browser_spawn_driver.effect().boxed_local(),
            Command::Stdout(string) => async move { print!("{string}") }.boxed_local(),
        };

        Local::from_future(effect)
    });
    // .merge_all(usize::MAX);

    effect_stream.delay(Duration::ZERO).subscribe(|_| {});

    // Basic usage - flatten nested observables
    // let mut result = Vec::new();
    // Local::from_iter([Local::from_iter([1, 2]), Local::from_iter([3, 4])])
    //   .merge_all(usize::MAX)
    //   .subscribe(|v| result.push(v));
    // assert_eq!(result, vec![1, 2, 3, 4]);

    std::future::pending::<()>().await;
    Ok(())
}
