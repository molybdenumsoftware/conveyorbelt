use std::{
    convert::Infallible,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context as _;
use hyper::StatusCode;
use rxrust::prelude::*;
use static_web_server::{
    handler::{RequestHandler, RequestHandlerOpts},
    service::RouterService,
};
use tempfile::TempDir;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, derive_more::Deref)]
pub(crate) struct ServeDir(TempDir);

impl std::fmt::Display for ServeDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0.path())
    }
}

// TODO observed strange output:
// command: server: server at address 127.0.0.1:40521

impl ServeDir {
    pub(crate) fn obtain() -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;
        Ok(Self(temp_dir))
    }
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum ServerCommand {
    #[display("spawn at {_0}")]
    Spawn(Arc<ServeDir>),
    #[display("shutdown")]
    Shutdown(Server),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum ServerEvent {
    #[display("spawn error: {_0}")]
    SpawnError(anyhow::Error),
    #[display("spawn: {_0}")]
    Spawn(Server),
    #[display("shutdown")]
    Shutdown,
    #[display("shutdown error: {_0}")]
    ShutdownError(hyper::Error),
    #[display("task join error: {_0}")]
    TaskJoinError(tokio::task::JoinError),
}

pub(crate) struct ServerDriver {
    event_sender: mpsc::Sender<ServerEvent>,
}

impl ServerDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, ServerEvent, Infallible>,
        Self,
    ) {
        let (event_sender, event_receiver) = mpsc::channel(1);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self, command: ServerCommand) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            let event = match command {
                ServerCommand::Spawn(serve_dir) => {
                    match Server::spawn(serve_dir.path().to_path_buf()) {
                        Ok(server) => ServerEvent::Spawn(server),
                        Err(error) => ServerEvent::SpawnError(error),
                    }
                }
                ServerCommand::Shutdown(server) => match server.shutdown().await {
                    Ok(Ok(())) => ServerEvent::Shutdown,
                    Ok(Err(error)) => ServerEvent::ShutdownError(error),
                    Err(join_error) => ServerEvent::TaskJoinError(join_error),
                },
            };
            event_sender.send(event).await.unwrap();
        }
    }
}

#[derive(Debug)]
pub(crate) struct Server {
    address: SocketAddr,
    shutdown_sender: oneshot::Sender<()>,
    join_handle: JoinHandle<hyper::Result<()>>,
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "server at address {}", self.address)
    }
}

impl Server {
    fn spawn(path: PathBuf) -> anyhow::Result<Self> {
        let handler_opts = RequestHandlerOpts {
            root_dir: path.clone(),
            compression: false,
            compression_static: false,
            cors: None,
            security_headers: false,
            cache_control_headers: false,
            page404: path.join("404.html"),
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

        let address = SocketAddr::from((IpAddr::V4(Ipv4Addr::LOCALHOST), 0));
        let listener =
            TcpListener::bind(address).with_context(|| format!("failed to bind to {address}"))?;

        listener.set_nonblocking(true).with_context(|| {
            format!("could not set TCP stream non-blocking for listener {listener:?}")
        })?;

        let failed_to_create_server_msg =
            format!("failed to create hyper server from listener {listener:?}");

        let address = listener.local_addr()?;
        let (shutdown_sender, shutdown_signal) = oneshot::channel();
        let server_task = hyper::Server::from_tcp(listener)
            .context(failed_to_create_server_msg)?
            .tcp_nodelay(true)
            .serve(RouterService::new(RequestHandler {
                opts: Arc::from(handler_opts),
            }))
            .with_graceful_shutdown(async move {
                shutdown_signal.await.unwrap();
            });

        Ok(Self {
            join_handle: tokio::spawn(server_task),
            address,
            shutdown_sender,
        })
    }

    pub(crate) fn address(&self) -> SocketAddr {
        self.address
    }

    async fn shutdown(self) -> Result<Result<(), hyper::Error>, tokio::task::JoinError> {
        self.shutdown_sender.send(()).unwrap();
        self.join_handle.await
    }
}
