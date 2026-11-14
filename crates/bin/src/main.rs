mod browser;
mod build;
mod cli;
#[path = "../common.rs"]
mod common;
mod fswatch;
mod git;
mod logging;
mod server;
mod signal;

use anyhow::Context;
use futures::{
    FutureExt as _, StreamExt,
    future::{
        Either::{Left, Right},
        join, select,
    },
};
use nix::sys::signal::{Signal::SIGTERM, kill};
use tokio::select;
use tracing::{info, warn};

use crate::{
    browser::Browser,
    build::Build,
    cli::Args,
    common::{StateForTesting, TESTING_MODE},
    fswatch::FsWatch,
    git::GitToplevel,
    server::{ServeDir, Server},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    let Args { build_command } = crate::cli::parse();
    let toplevel = GitToplevel::resolve().context("get git toplevel")?;
    let mut signal = signal::listen().context("listen on signals")?;
    let serve_dir = ServeDir::create()?;
    let fs_watch = FsWatch::init(&toplevel).context("init fs watch")?;

    // TODO nix::sys::signal::kill(pid, signal).with_context(|| format!("send {signal} to {pid}"))?;

    let (server, build) = match select(
        &mut signal,
        select(
            Box::pin(Server::start(serve_dir.clone())),
            Box::pin(Build::spawn(serve_dir.clone(), build_command)),
        ),
    )
    .await
    {
        Left((_signal, _)) => return Ok(()),
        Right((Left((Err(server), _)), _)) => return Err(server),
        Right((Left((Ok(server), build_spawn)), signal)) => {
            select! {
                _ = signal => {
                    server.shutdown().await?;
                    return Ok(());
                },
                    // TODO: is build_spawn cancellation safe?
                result = build_spawn => (server, result?)
            }
        }
        Right((Right((Err(build_error), _)), _)) => return Err(build_error),
        Right((Right((Ok(build), server)), signal)) => {
            select! {
                _ = signal => return Ok(()),
                result = server => (result?, build)
            }
        }
    };

    let pid = build.pid()?;

    match select(&mut signal, Box::pin(build.wait())).await {
        Left((_signal, build_wait)) => {
            kill(pid, SIGTERM);
            join(build_wait, server.shutdown()).await;
            return Ok(());
        }
        Right((Err(e), _)) => {
            if let Err(e) = server.shutdown().await {
                warn!("{e}");
            }
            return Err(e);
        }
        Right((Ok(_build_succeeded), _)) => {}
    }

    let browser = match Browser::launch(&server.address()).await {
        Ok(browser) => browser,
        Err(e) => {
            if let Err(e) = server.shutdown().await {
                warn!("{e}");
            }
            return Err(e);
        }
    };

    if std::env::var(TESTING_MODE).is_ok() {
        let state_for_testing = StateForTesting {
            serve_path: serve_dir.path().to_path_buf(),
            serve_port: server.port(),
            browser_debugging_address: browser.websocket_address().to_owned(),
            browser_pid: browser.pid(),
        };
        println!("{state_for_testing}");
    }

    // Instead of sharing memory keep an Option of a task and share browser handle with it
    let mut task = None;
    loop {
        let changes = match fs_watch.recv().await {
            Err(e) => {
                warn!("{e}");
                continue;
            }
            Ok(changes) => changes,
        };

        let all_ignored = true;
        for change in &changes {
            info!("{change}");
            all_ignored = all_ignored && change.is_ignored;
        }

        if all_ignored {
            continue;
        }

        match task.take() {
            None => {
                let build = Build::spawn(serve_dir.clone(), build_command.clone());
                task = Some(tokio::spawn(async move {
                    let spawn = build.await.context("spawn build")?;

                    let code = spawn.wait().await.context("wait build")?;

                    Ok(code)
                }));
            }
            Some(_) => todo!(),
        }
    }

    signal.await;
    Ok(())
}
