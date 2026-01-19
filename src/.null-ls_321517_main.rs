mod build_command;
#[path = "../common.rs"]
mod common;

use std::{
    env::current_dir,
    mem,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, anyhow};
use chromiumoxide::{Browser, BrowserConfig};
use clap::Parser as _;
use hyper::StatusCode;
use ignore_files::IgnoreFilter;
use static_web_server::{
    handler::{RequestHandler, RequestHandlerOpts},
    service::RouterService,
    signals,
};
use tempfile::tempdir;
use tokio::process::Command;
use tracing::{debug, info, level_filters::LevelFilter, warn};
use watchexec::Watchexec;
use watchexec_events::filekind::FileEventKind;
use watchexec_filterer_ignore::IgnoreFilterer;

use crate::{
    build_command::BuildCommand,
    common::{ForStdoutputLine as _, StateForTesting},
};

#[derive(Debug, Clone, clap::Parser)]
struct Cli {
    /// The build command
    build_command: PathBuf,
}

#[derive(Debug)]
struct EventFilter(IgnoreFilterer);

impl EventFilter {
    async fn new() -> anyhow::Result<Self> {
        let mut ignore_filter = IgnoreFilter::new(current_dir()?, &[]).await?;
        ignore_filter.finish();
        Ok(Self(IgnoreFilterer(ignore_filter)))
    }
}

impl watchexec::filter::Filterer for EventFilter {
    fn check_event(
        &self,
        event: &watchexec_events::Event,
        priority: watchexec_events::Priority,
    ) -> Result<bool, watchexec::error::RuntimeError> {
        let dot_git = current_dir()
            // TODO static to avoid this conversion
            .map_err(|err| watchexec::error::RuntimeError::IoError {
                about: "current dir",
                err,
            })?
            .join(".git");
        if let Some(path) = event.tags.iter().find_map(|tag| {
            if let watchexec_events::Tag::Path { path, .. } = tag {
                Some(path)
            } else {
                None
            }
        }) && path.starts_with(dot_git)
        {
            return Ok(false);
        };

        if let Some(kind) = event.tags.iter().find_map(|tag| {
            if let watchexec_events::Tag::FileEventKind(kind) = tag {
                Some(kind)
            } else {
                None
            }
        }) {
            let (FileEventKind::Create(_) | FileEventKind::Modify(_) | FileEventKind::Remove(_)) =
                kind
            else {
                return Ok(false);
            };
        }

        self.0.check_event(event, priority)
    }
}

#[tokio::main]
async fn main() {
    let filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .with_env_var(env!("LOG_FILTER_VAR_NAME"))
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();

    info!("{} starting", env!("CARGO_PKG_NAME"));
    let cli = Cli::parse();
    debug!("arguments parsed: {cli:?}");
    let Cli { build_command } = cli;

    let mut command = Command::new("git");
    command.args(["rev-parse", "--show-toplevel"]);

    let output = command
        .output()
        .await
        .with_context(|| format!("failed to run {command:?}"))
        .unwrap();

    if !output.status.success() {
        panic!(
            "command {:?} exited with {}. stderr: {}",
            command,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let git_toplevel: String = output
        .stdout
        .try_into()
        .with_context(|| format!("command printed non-UTF-8: {command:?}"))
        .unwrap();

    let git_toplevel = git_toplevel.trim_end().to_string();
    debug!("git toplevel obtained: {git_toplevel}");
    let mut serve_path: PathBuf = git_toplevel.into();
    serve_path.push(env!("SERVE_DIR"));
    debug!("serve path resolved: {serve_path:?}");

    let mut command = Command::new("git");

    command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .arg("check-ignore")
        .arg(&serve_path);

    let mut process = command
        .spawn()
        .with_context(|| format!("failed to run {command:?}"))
        .unwrap();

    process
        .for_stderr_line(|line| {
            warn!("`git check-ignore` stderr: {line}");
        })
        .unwrap();

    let exit_status = process
        .wait()
        .await
        .with_context(|| format!("waiting for `{command:?}` to complete"))
        .unwrap();

    if !exit_status.success() {
        panic!(
            "serve path (`{}`) is not git ignored",
            serve_path.to_str().unwrap()
        );
    }

    let build_command = BuildCommand::new(build_command, serve_path.clone());

    let wx = Watchexec::new_async(move |action| {
        Box::new({
            let build_command_clone = build_command.clone();

            async move {
                info!("change detected: {:?}", action.events);
                build_command_clone.invoke().await.unwrap();
                action
            }
        })
    })
    .unwrap();

    wx.config.throttle(Duration::ZERO);
    wx.config.pathset([current_dir().unwrap()]);
    wx.config.filterer(EventFilter::new().await.unwrap());

    wx.main();

    let address = SocketAddr::from((IpAddr::V4(Ipv4Addr::LOCALHOST), 0));

    let listener = TcpListener::bind(address)
        .with_context(|| format!("failed to bind to {address}"))
        .unwrap();

    let serve_address = listener
        .local_addr()
        .with_context(|| format!("could not get local socket address of listener {listener:?}"))
        .unwrap();

    info!("serving address: {serve_address}");

    let handler_opts = RequestHandlerOpts {
        root_dir: serve_path.clone(),
        compression: false,
        compression_static: false,
        cors: None,
        security_headers: false,
        cache_control_headers: false,
        page404: serve_path.join("404.html"),
        page50x: PathBuf::new(),
        index_files: ["index.html"].iter().map(|s| s.to_string()).collect(),
        log_remote_address: false,
        log_x_real_ip: false,
        log_forwarded_for: false,
        trusted_proxies: Vec::new(),
        redirect_trailing_slash: false,
        ignore_hidden_files: true,
        disable_symlinks: true,
        accept_markdown: false,
        health: false,
        maintenance_mode: false,
        maintenance_mode_status: StatusCode::SERVICE_UNAVAILABLE,
        maintenance_mode_file: PathBuf::new(),
        advanced_opts: None,
    };

    let router_service = RouterService::new(RequestHandler {
        opts: Arc::from(handler_opts),
    });

    let signals = signals::create_signals()
        .context("failed to create signals stream")
        .unwrap();

    let handle = signals.handle();

    listener
        .set_nonblocking(true)
        .with_context(|| format!("could not set TCP stream non-blocking for listener {listener:?}"))
        .unwrap();

    let failed_to_create_server_msg =
        format!("failed to create hyper server from listener {listener:?}");

    let server = hyper::Server::from_tcp(listener)
        .context(failed_to_create_server_msg)
        .unwrap()
        .tcp_nodelay(true)
        .serve(router_service);

    let server =
        server.with_graceful_shutdown(signals::wait_for_signals(signals, 0, Default::default()));

    let browser_data_dir = tempdir()
        .context("failed to create temporary browser data dir")
        .unwrap();

    debug!("browser data dir: {browser_data_dir:?}");

    let browser_config = BrowserConfig::builder()
        .with_head()
        .viewport(None)
        .user_data_dir(browser_data_dir.path())
        .port(0)
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

    if std::env::var(StateForTesting::ENV_VAR).is_ok() {
        let state_for_testing = StateForTesting {
            serve_port: serve_address.port(),
            browser_debugging_address,
            browser_pid,
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

    server.await.context("server failed").unwrap();
    handle.close();
}
