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

pub(crate) struct ObtainServeDir;

impl ObtainServeDir {
    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, ObtainServeDirEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);

        tokio::spawn(async move {
            let event = match TempDir::new() {
                Ok(dir) => ObtainServeDirEvent::Obtain(ServeDir(dir)),
                Err(error) => ObtainServeDirEvent::Error(error),
            };

            event_sender.send(event).await.unwrap()
        });

        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}

#[derive(derive_more::Display)]
pub(crate) enum ObtainServeDirEvent {
    #[display("serve dir obtained: {_0}")]
    Obtain(ServeDir),
    #[display("error obtaining serve dir: {_0}")]
    Error(std::io::Error),
}

#[derive(Debug, derive_more::Deref)]
pub(crate) struct ServeDir(TempDir);

impl std::fmt::Display for ServeDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0.path())
    }
}

// TODO observed strange output:
// command: server: server at address 127.0.0.1:40521

// #[derive(Debug, derive_more::Display)]
// pub(crate) enum ServerCommand {
//     #[display("spawn at {_0}")]
//     Spawn(Arc<ServeDir>),
//     #[display("shutdown")]
//     Shutdown(Server),
// }

#[derive(Debug, derive_more::Display)]
pub(crate) enum ServerSpawnEvent {
    #[display("spawn error: {_0}")]
    SpawnError(anyhow::Error),
    #[display("server spawned; address: {address}")]
    Spawn {
        address: SocketAddr,
        shutdown_effect: ServerShutdown,
    },
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum ServerShutdownEvent {
    #[display("shutdown")]
    Shutdown,
    #[display("shutdown error: {_0}")]
    ShutdownError(hyper::Error),
    #[display("task join error: {_0}")]
    TaskJoinError(tokio::task::JoinError),
}

#[derive(Debug)]
pub(crate) struct ServerSpawn {
    serve_dir: PathBuf,
}

impl ServerSpawn {
    pub(crate) fn new(serve_dir: PathBuf) -> Self {
        Self { serve_dir }
    }

    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, ServerSpawnEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);

        tokio::spawn(async move {
            let result = (move || {
                let handler_opts = RequestHandlerOpts {
                    root_dir: self.serve_dir.clone(),
                    compression: false,
                    compression_static: false,
                    cors: None,
                    security_headers: false,
                    cache_control_headers: false,
                    page404: self.serve_dir.join("404.html"),
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
                let listener = TcpListener::bind(address)
                    .with_context(|| format!("failed to bind to {address}"))?;

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

                let join_handle = tokio::spawn(server_task);

                Ok(ServerSpawnEvent::Spawn {
                    address,
                    shutdown_effect: ServerShutdown {
                        shutdown_sender,
                        join_handle,
                    },
                })
            })();

            let event = match result {
                Ok(spawn) => spawn,
                Err(error) => ServerSpawnEvent::SpawnError(error),
            };

            event_sender.send(event).await.unwrap()
        });

        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}

#[derive(Debug)]
pub(crate) struct ServerShutdown {
    shutdown_sender: oneshot::Sender<()>,
    join_handle: JoinHandle<hyper::Result<()>>,
}

impl ServerShutdown {
    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, ServerShutdownEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);

        tokio::spawn(async move {
            self.shutdown_sender.send(()).unwrap();

            let event = match self.join_handle.await {
                Ok(Ok(())) => ServerShutdownEvent::Shutdown,
                Ok(Err(error)) => ServerShutdownEvent::ShutdownError(error),
                Err(join_error) => ServerShutdownEvent::TaskJoinError(join_error),
            };

            event_sender.send(event).await.unwrap();
        });

        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}
