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
    FutureExt,
    future::{
        Either::{Left, Right},
        join, select,
    },
};
use nix::{
    sys::signal::{Signal::SIGTERM, kill},
    unistd::Pid,
};
use tokio::{select, sync::mpsc, task::JoinHandle};
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
    let mut fs_watch = FsWatch::init(&toplevel).context("init fs watch")?;

    // TODO nix::sys::signal::kill(pid, signal).with_context(|| format!("send {signal} to {pid}"))?;

    let (server, build) = match select(
        &mut signal,
        select(
            Box::pin(Server::start(serve_dir.clone())),
            Box::pin(Build::spawn(serve_dir.clone(), build_command.clone())),
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

    let pid = build.pid;
    match select(
        &mut signal,
        Box::pin(async move { build.wait().await.context("wait build") }),
    )
    .await
    {
        Left((_signal, build_wait)) => {
            if let Err(e) = kill(pid, SIGTERM).context("terminate build") {
                warn!("{e}");
            };
            let (build, server) = join(build_wait, server.shutdown()).await;
            if let Err(e) = build {
                warn!("{e}");
            }
            if let Err(e) = server {
                warn!("{e}");
            }
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

    loop {
        fs_watch.recv().await;

        let mut build = match Build::spawn(serve_dir.clone(), build_command.clone())
            .await
            .context("spawn build")
        {
            Ok(build) => build,
            Err(e) => {
                warn!("{e}");
                continue;
            }
        };

        let mut killing_it = false;
        let mut pid = build.pid;
        loop {
            let mut build_done = Box::pin(build.wait());
            match select(Box::pin(fs_watch.recv()), &mut build_done).await {
                Left(_) => {
                    if killing_it {
                        info!("build in progress, sending SIGTERM");
                        if let Err(e) = kill(pid, SIGTERM).context("SIGTERM build") {
                            warn!("{e}");
                        };
                        killing_it = true;
                    } else {
                        info!("build already terminating");
                    };
                }
                Right(_) => {
                    if killing_it {
                        build = match Build::spawn(serve_dir.clone(), build_command.clone())
                            .await
                            .context("spawn build")
                        {
                            Ok(build) => build,
                            Err(e) => {
                                warn!("{e}");
                                break;
                            }
                        };
                        pid = build.pid;
                    } else {
                        // TODO: reload the browser page if code is 0
                        break;
                    }
                }
            }
        }
    }
}

/*


buildstate::start
|
v
browser page reload


buildstate
- nothing (not started yet)
- building
- stopping



        start v
nothing ->  building -> nothing  + send reload




      start v         start v    start v
nothing ->  building -> stopping -> building -> nothing + send reload


*/
