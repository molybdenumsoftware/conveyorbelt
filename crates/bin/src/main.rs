mod app;
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
    app::{App, Command, Control, Event},
    cli::Args,
    driver::{
        browser::BrowserDriver,
        build::BuildDriver,
        fswatch::FsWatchInit,
        server::{ServeDir, ServerDriver},
        signal::InstallSignal,
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

    // TODO driver?
    let serve_dir = ServeDir::obtain()?;
    // TODO driver?
    let project_root = crate::project_path::resolve(&std::env::current_dir()?)?;

    // let (signal_events, signal_driver) = SignalDriver::new();
    // let (server_events, server_driver) = ServerDriver::new();
    let (build_events, build_driver) = BuildDriver::new();
    let (browser_events, browser_driver) = BrowserDriver::new();
    let (fs_watch_events, fs_watch_driver) = FsWatchInit::new();

    let app = App {
        project_root,
        serve_dir: Arc::new(serve_dir),
        build_command_path: build_command,
    };

    // TODO try to avoid having any `unreachable!` invocations

    let input_events = Shared::merge_observables([
        signal_events.map(Event::Signal).box_it(),
        server_events.map(Event::Server).box_it(),
        build_events.map(Event::Build).box_it(),
        browser_events.map(Event::Browser).box_it(),
        fs_watch_events.map(Event::Fs).box_it(),
    ])
    .box_it();

    let exit_code = app
        .run(input_events)
        .map(move |control| match control {
            Control::Command(command) => {
                let future = match command {
                    Command::Build(build_command) => build_driver.effect(build_command).boxed(),
                    Command::Server(server_command) => server_driver.effect(server_command).boxed(),
                    Command::Fs(fs_command) => fs_watch_driver.effect(fs_command).boxed(),
                    Command::Browser(browser_command) => {
                        browser_driver.effect(browser_command).boxed()
                    }
                    Command::Signal(signal_command) => signal_driver.effect(signal_command).boxed(),
                };
                async move {
                    future.await;
                    None
                }
                .boxed()
            }
            Control::Exit(code) => async move { Some(code) }.boxed(),
        })
        .flat_map(Shared::from_future)
        .filter_map(|exit_code| exit_code)
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
