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
use nix::sys::signal::{Signal::SIGTERM, kill};
use tokio::{select, task::JoinHandle};
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

    let pid = build.pid()?;

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

    let mut task: Option<JoinHandle<anyhow::Result<Option<i32>>>> = None;
    loop {
        let changes = match fs_watch.recv().await {
            Err(e) => {
                warn!("{e}");
                continue;
            }
            Ok(changes) => changes,
        };

        let mut all_ignored = true;
        for change in &changes {
            info!("{change}");
            all_ignored = all_ignored && change.is_ignored;
        }

        if all_ignored {
            continue;
        }

        let wait_result = match task.as_mut() {
            None => {
                let build = Build::spawn(serve_dir.clone(), build_command.clone());
                task = Some(tokio::spawn(async move {
                    let spawn = build.await.context("spawn build")?;
                    let code = spawn.wait().await.context("wait build")?;
                    Ok(code)
                }));
                continue;
            }
            Some(task) if !task.is_finished() => {
                info!("build in progress");
                continue;
            }
            Some(task) => task.await.context("join build task handle")?,
        };

        match wait_result {
            Err(e) => {
                warn!("{e}");
                continue;
            }
            Ok(None) => {
                continue;
            }
            Ok(Some(0)) => {}
            Ok(Some(code)) => {
                warn!("build exit with {code}");
                continue;
            }
        }

        info!("build success");

        browser.reload_page().await.context("reload page");
    }
}
