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

use std::{convert::Infallible, rc::Rc};

use rxrust::prelude::*;

use crate::{
    app::{DriverCommand, Inputs},
    driver::{
        browser_spawn::BrowserSpawnDriver, process_spawn::ProcessSpawnDriver,
        process_wait::ProcessWaitDriver, stdout::StdoutDriver,
    },
    server::{Server, ServerPort},
};

#[tokio::main(flavor = "local")]
async fn main_() {
    let mut subject = Local::subject::<i32, Infallible>();
    let subject_clone = subject.clone();
    let mut emitter = subject_clone.clone();
    subject
        .clone()
        .delay(Duration::ZERO) // Delays the callback itself
        .subscribe(move |n| {
            eprintln!("count: {n}");
            if n < 3 {
                emitter.next(n + 1);
            }
        });
    subject.next(0);
    std::future::pending::<()>().await;
}

#[tokio::main(flavor = "local")]
async fn main() -> anyhow::Result<()> {
    logging::init();
    let args = crate::cli::parse();

    let serve_dir = Rc::new(crate::serve_dir::obtain()?);
    let project_root = crate::project_path::resolve(&std::env::current_dir()?)?;
    let build_command_path = args.build_command;

    let server_port = Server::init(serve_dir.path().to_path_buf())?;

    let (process_spawn_driver_events, process_spawn_driver) = ProcessSpawnDriver::new();
    let (browser_spawn_driver_events, browser_spawn_driver) = BrowserSpawnDriver::new();
    let (process_wait_driver_events, process_wait_driver) = ProcessWaitDriver::new();
    let stdout_driver = StdoutDriver::new();

    let command_stream = app::run(Inputs {
        project_root,
        serve_dir: serve_dir.clone(),
        build_command_path,
        server_port,
        process_spawn_driver_events,
        process_wait_driver_events,
        browser_spawn_driver_events,
    });

    // extract all this stream splitting stuff into a method on the app output
    let subject = Local::subject();
    let command_stream = command_stream
        // .delay(Duration::ZERO)
        .multicast(subject.into_inner());

    let process_spawn_driver_commands = command_stream
        .fork()
        .filter_map(|command| {
            if let DriverCommand::ProcessSpawn(command) = command {
                Some(command)
            } else {
                None
            }
        })
        .box_it();

    let process_wait_driver_commands = command_stream
        .fork()
        .filter_map(|command| {
            if let DriverCommand::ProcessWait(command) = command {
                Some(command)
            } else {
                None
            }
        })
        .box_it();

    let browser_spawn_driver_commands = command_stream
        .fork()
        .filter_map(|command| {
            if let DriverCommand::BrowserSpawn(command) = command {
                Some(command)
            } else {
                None
            }
        })
        .box_it();

    let stdout_driver_commands = command_stream
        .fork()
        .filter_map(|command| {
            if let DriverCommand::Stdout(command) = command {
                Some(command)
            } else {
                None
            }
        })
        .box_it();

    // let browser_spawn_driver

    let _subscription = process_spawn_driver.init(process_spawn_driver_commands);
    let _subscription = process_wait_driver.init(process_wait_driver_commands);
    let _subscription = browser_spawn_driver.init(browser_spawn_driver_commands);
    let _subscription = stdout_driver.init(stdout_driver_commands);

    let _subscription = command_stream.connect();
    std::future::pending::<()>().await;
    Ok(())
}
