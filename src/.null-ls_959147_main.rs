mod build_command;
mod cli;
#[path = "../common.rs"]
mod common;
mod file_watching;
mod logging;
mod project_path;
mod server;

use std::{env::current_dir, mem, time::Duration};

use anyhow::{Context, anyhow};
use chromiumoxide::{Browser, BrowserConfig};
use static_web_server::signals;
use tempfile::{TempDir, tempdir};
use tracing::{debug, info};

use crate::{
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
    build_command.invoke().await.unwrap();
    let file_watcher = FileWatcher::new(build_command.clone(), git_toplevel).unwrap();
    file_watcher.init().await.unwrap();
    let server = Server::init(serve_dir.path().to_path_buf()).await.unwrap();

    let browser_data_dir = tempdir()
        .context("failed to create temporary browser data dir")
        .unwrap();

    debug!("browser data dir: {browser_data_dir:?}");

    let mut browser_config_builder = BrowserConfig::builder()
        .with_head()
        .viewport(None)
        .user_data_dir(browser_data_dir.path())
        .port(0);

    if std::env::var(common::TESTING_MODE).is_ok() {
        browser_config_builder = browser_config_builder.launch_timeout(Duration::from_mins(15));
    }

    let browser_config = browser_config_builder
        .build()
        .map_err(|e| anyhow!("failed to build browser config: {e}"))
        .unwrap();

    debug!("browser config: {browser_config:?}");

    let (mut browser, _handler) = Browser::launch(browser_config)
        .await
        .context("failed to launch browser")
        .unwrap();

    let browser_debugging_address = browser.websocket_address().clone();
    debug!("browser debugging address: {browser_debugging_address}");

    let browser_pid = browser
        .get_mut_child()
        .context("failed to obtain mutable reference to browser Child")
        .unwrap()
        .as_mut_inner()
        .id()
        .context("failed to obtain browser pid")
        .unwrap();

    debug!("browser pid: {browser_pid}");

    if std::env::var(TESTING_MODE).is_ok() {
        let state_for_testing = StateForTesting {
            serve_port: server.port(),
            browser_debugging_address,
            browser_pid,
            serve_path: serve_dir.path().to_path_buf(),
        };

        debug!("{state_for_testing:?}");
        let state_for_testing = serde_json::to_string(&state_for_testing)
            .context("failed to serialize state for testing")
            .unwrap();
        println!("{state_for_testing}");
    }

    // chromiumoxide sets up the browser with `kill_on_drop`.
    // This prevents that from happening.
    mem::forget(browser);

    let shutdown_signal =
        static_web_server::signals::wait_for_signals(signals, 0, Default::default());

    server.into_inner().await.context("server failed").unwrap();

    handle.close();
}
