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

use std::pin::Pin;

use anyhow::Context;
use futures::{
    FutureExt,
    future::{
        BoxFuture,
        Either::{Left, Right},
        LocalBoxFuture, select,
    },
};
use nix::{
    sys::signal::{Signal::SIGTERM, kill},
    unistd::Pid,
};
use tokio::task::JoinHandle;
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

    let server = Server::start(serve_dir.clone())
        .await
        .context("start server")?;

    let mut browser = Browser::launch(&server.address())
        .await
        .context("launch browser")?;

    if std::env::var(TESTING_MODE).is_ok() {
        let state_for_testing = StateForTesting {
            serve_path: serve_dir.path().to_path_buf(),
            serve_port: server.port(),
            browser_debugging_address: browser.websocket_address().to_owned(),
            browser_pid: browser.pid(),
        };
        println!("{state_for_testing}");
    }

    let build = Build::spawn(serve_dir.clone(), build_command.clone())
        .await
        .context("spawn build")?;

    enum State {
        Idle,
        Building(Pid, JoinHandle<anyhow::Result<Option<i32>>>),
        //Terminating(Pin<Box<dyn Future<Output = anyhow::Result<anyhow::Result<Option<i32>>>>>>),
        Terminating(BoxFuture<'static, anyhow::Result<anyhow::Result<Option<i32>>>>),
    }
    use State::*;

    let mut state = State::Building(
        build.pid,
        tokio::spawn(async move { build.wait().await.context("wait build") }),
    );

    loop {
        state = match state {
            Idle => {
                fs_watch.recv().await;
                let build = Build::spawn(serve_dir.clone(), build_command.clone())
                    .await
                    .context("build spawn")?;

                Building(
                    build.pid,
                    tokio::spawn(async move { build.wait().await.context("build wait") }),
                )
            }
            Building(pid, build_task) => {
                match select(
                    Box::pin(fs_watch.recv()),
                    Box::pin(async move { build_task.await.context("join build task") }),
                )
                .await
                {
                    Left((_changes, build_task)) => {
                        if let Err(e) = kill(pid, SIGTERM).context("SIGTERM build") {
                            info!("{e}");
                        }
                        Terminating(build_task.boxed())
                    }
                    Right((Err(join_error), _changes)) => {
                        warn!("{join_error}");
                        Idle
                    }
                    Right((Ok(Err(wait_error)), _changes)) => {
                        warn!("{wait_error}");
                        Idle
                    }
                    Right((Ok(Ok(None)), _changes)) => {
                        warn!("build exited without code");
                        Idle
                    }
                    Right((Ok(Ok(Some(code @ (..0 | 1..)))), _changes)) => {
                        info!("build exited with {code}");
                        Idle
                    }
                    Right((Ok(Ok(Some(0))), _changes)) => {
                        info!("build success");
                        browser.reload_page().await.context("reload page")?;
                        Idle
                    }
                }
            }
            Terminating(join_handle) => todo!(),
        };
    }
}
