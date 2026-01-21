mod browser;
mod build_command;
mod cli;
#[path = "../common.rs"]
mod common;
mod file_watching;
mod logging;
mod project_path;
mod server;
mod testing;

use std::env::current_dir;

use anyhow::Context;
use static_web_server::signals;
use tempfile::TempDir;
use tracing::{debug, info};

use crate::{
    browser::Browser,
    build_command::BuildCommand,
    common::{StateForTesting, TESTING_MODE},
    file_watching::FileWatcher,
    server::Server,
};

#[tokio::main]
async fn main() {
    logging::init();
    info!("{} starting", env!("CARGO_PKG_NAME"));

    let signals = signals::create_signals()
        .context("failed to create signals stream")
        .unwrap();
    let handle = signals.handle();

    let args = cli::parse();

    let git_toplevel = project_path::resolve(&current_dir().unwrap())
        .await
        .unwrap();

    // https://github.com/static-web-server/static-web-server/pull/606
    let serve_dir = TempDir::with_prefix("not-hidden-").unwrap();
    debug!("serve path: {serve_dir:?}");
    let build_command = BuildCommand::new(args.build_command, serve_dir.path().to_path_buf());
    build_command.invoke_or_queue();
    let file_watcher = FileWatcher::new(build_command, git_toplevel).unwrap();
    file_watcher.init().await.unwrap();
    let server = Server::init(serve_dir.path().to_path_buf()).await.unwrap();
    let mut browser = Browser::init().await.unwrap();
    let browser_pid = browser.pid().unwrap();
    debug!("browser pid: {browser_pid}");

    if std::env::var(TESTING_MODE).is_ok() {
        StateForTesting::print(
            serve_dir.path().to_path_buf(),
            server.port(),
            browser.debugging_address(),
            browser.pid().unwrap(),
        )
        .unwrap();
    }

    let shutdown_signal =
        static_web_server::signals::wait_for_signals(signals, 0, Default::default());

    server
        .into_inner()
        .with_graceful_shutdown(shutdown_signal)
        .await
        .context("server failed")
        .unwrap();

    handle.close();
}
