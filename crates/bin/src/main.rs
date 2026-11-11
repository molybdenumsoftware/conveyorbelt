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

use crate::{
    build::Build,
    cli::Args,
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
            server.shutdown().await;
            return Err(e);
        }
        Right((Ok(_build_succeeded), _)) => {}
    }

    // Spawn browser.

    let Ok(browser) = BrowserSpawn {
        url: format!("http://{}", server.address),
    }
    .do_logged()
    .await
    else {
        server.shutdown_effect.do_logged().await;
        return Err(1);
    };

    if std::env::var(TESTING_MODE).is_ok() {
        let state_for_testing = StateForTesting {
            serve_path: serve_dir.path().to_path_buf(),
            serve_port: server.address.port(),
            browser_debugging_address: browser.browser.websocket_address().to_string(),
            browser_pid: browser.browser.pid(),
        };
        println!("{state_for_testing}");
    }

    // FS watch

    // Instead of sharing memory keep an Option of a task and share browser handle with it
    let mut build = None;
    while let Some(result) = fs_watching.0.next().await {
        let event = match result {
            Err(e) => {
                warn!("{e}");
                continue;
            }
            Ok(event) => event,
        };
        if event.is_ignored {
            continue;
        }
        match build {
            None => {
                let serve_dir_clone = serve_dir.clone();
                let build_command_path_clone = self.build_command_path.clone();
                build = Some(tokio::spawn(async move {
                    let spawn_result = BuildSpawn {
                        path: build_command_path_clone.clone(),
                        serve_dir: serve_dir_clone.clone(),
                    }
                    .do_logged()
                    .await;

                    let spawn = match spawn_result {
                        Err(e) => {
                            warn!("{e}");
                            return;
                        }
                        Ok(build) => build,
                    };

                    let wait_result = spawn.wait.do_logged().await;

                    if let Err(e) = wait_result {
                        warn!("{e}");
                        return;
                    }
                }));
            }
            Some(_) => todo!(),
        }
    }

    signal.await;
    Ok(())
}
