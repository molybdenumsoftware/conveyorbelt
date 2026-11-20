mod browser;
mod build;
mod cli;
#[path = "../common.rs"]
mod common;
mod fswatch;
mod git;
mod serve_dir;
mod server;
mod signal;
#[path = "tracing.rs"]
mod tracing_;

use anyhow::Context;
use futures::{
    FutureExt,
    future::{
        BoxFuture,
        Either::{Left, Right},
        select,
    },
};
use nix::unistd::Pid;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::{
    browser::Browser,
    build::{Build, BuildCommandPath},
    cli::Args,
    common::{StateForTesting, TESTING_MODE},
    fswatch::FsWatch,
    git::GitRepo,
    serve_dir::ServeDir,
    server::Server,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_::init();
    let Args { build_command } = crate::cli::parse();
    let signal = signal::listen().context("listen on signals")?;
    let serve_dir = ServeDir::create()?;

    let server = Server::start(serve_dir.clone())
        .await
        .context("start server")?;

    let main_loop = main_loop(
        serve_dir.clone(),
        server.port(),
        server.address(),
        build_command,
    );

    let result = match select(Box::pin(signal), Box::pin(main_loop)).await {
        Left((_signal, _)) => Ok(()),
        Right((result, _)) => result,
    };

    if let Err(e) = server.shutdown().await.context("server shutdown") {
        warn!("{e}");
    }

    result
}

async fn main_loop(
    serve_dir: ServeDir,
    serve_port: u16,
    serve_address: String,
    build_command: BuildCommandPath,
) -> anyhow::Result<()> {
    let git_repo = GitRepo::resolve()?;
    let mut fs_watch = FsWatch::init(git_repo).context("init fs watch")?;

    let mut browser = Browser::launch(&serve_address)
        .await
        .context("launch browser")?;

    if std::env::var(TESTING_MODE).is_ok() {
        let state_for_testing = StateForTesting {
            serve_path: serve_dir.to_path_buf(),
            serve_port,
            browser_debugging_address: browser.websocket_address().to_owned(),
            browser_pid: browser.pid(),
        };
        println!("{state_for_testing}");
    }
    enum State {
        Idle,
        Building(Pid, JoinHandle<anyhow::Result<bool>>),
        Terminating(BoxFuture<'static, anyhow::Result<anyhow::Result<bool>>>),
    }
    use State::*;

    let build = Build::spawn(serve_dir.clone(), build_command.clone())
        .await
        .context("spawn build")?;

    let mut state = State::Building(
        build.id,
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
                    build.id,
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
                        Build::terminate(pid);
                        Terminating(build_task.boxed())
                    }
                    Right((join_result, _)) => {
                        match join_result {
                            Err(join_error) => warn!("{join_error}"),
                            Ok(Err(wait_error)) => warn!("{wait_error}"),
                            Ok(Ok(false)) => {}
                            Ok(Ok(true)) => {
                                browser.reload_page().await.context("reload page")?;
                            }
                        }
                        Idle
                    }
                }
            }
            Terminating(build_task) => match select(Box::pin(fs_watch.recv()), build_task).await {
                Left((_changes, build_task)) => Terminating(build_task),
                Right((join_result, _)) => {
                    match join_result {
                        Err(join_error) => warn!("{join_error}"),
                        Ok(Err(wait_error)) => warn!("{wait_error}"),
                        Ok(Ok(false | true)) => {}
                    }

                    let build = Build::spawn(serve_dir.clone(), build_command.clone())
                        .await
                        .context("build spawn")?;

                    Building(
                        build.id,
                        tokio::spawn(async move { build.wait().await.context("build wait") }),
                    )
                }
            },
        };
    }
}
