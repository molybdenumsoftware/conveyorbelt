mod app;
mod browser;
mod cli;
#[path = "../common.rs"]
mod common;
mod driver;
mod logging;
mod project_path;

use std::sync::Arc;

use futures::{FutureExt as _, StreamExt};
use rxrust::prelude::*;

use crate::{
    app::{App, Command, Event},
    cli::Args,
    driver::{
        browser::BrowserDriver,
        build::BuildDriver,
        fswatch::FsWatchDriver,
        server::{ServeDir, ServerDriver},
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

    let serve_dir = ServeDir::obtain()?;
    let project_root = crate::project_path::resolve(&std::env::current_dir()?)?;

    let (server_events, server_driver) = ServerDriver::new();
    let (build_events, build_driver) = BuildDriver::new();
    let (browser_events, browser_driver) = BrowserDriver::new();
    let (fs_watch_events, fs_watch_driver) = FsWatchDriver::new();

    let app = App {
        project_root,
        serve_dir: Arc::new(serve_dir),
        build_command_path: build_command,
    };

    let input_events = Shared::merge_observables([
        server_events.map(Event::Server).box_it(),
        build_events.map(Event::Build).box_it(),
        browser_events.map(Event::Browser).box_it(),
        fs_watch_events.map(Event::Fs).box_it(),
    ])
    .box_it();

    app.run(input_events)
        .map(move |command| match command {
            Command::Server(command) => Some(server_driver.effect(command).boxed()),
            Command::Build(command) => Some(build_driver.effect(command).boxed()),
            Command::Browser(command) => Some(browser_driver.effect(command).boxed()),
            Command::FsWatch(command) => Some(fs_watch_driver.effect(command).boxed()),
            Command::Println(string) => Some(
                async move {
                    println!("{string}");
                }
                .boxed(),
            ),
            Command::Eprintln(string) => Some(
                async move {
                    eprintln!("{string}");
                }
                .boxed(),
            ),
            Command::Terminate => None,
        })
        .take_while(Option::is_some)
        .map(Option::unwrap)
        .concat_map(Shared::from_future)
        .delay(Duration::ZERO)
        .into_stream()
        .for_each(|_| async {})
        .await;

    // TODO shut down gracefully
    std::process::exit(1);
}
