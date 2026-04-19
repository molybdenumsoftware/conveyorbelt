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
use tracing::info;

#[derive(Debug, Clone)]
pub(crate) struct ServerSpawnCommand {
    pub(crate) serve_dir: Arc<TempDir>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServerSpawnEvent(pub(crate) Result<Arc<Server>, Arc<anyhow::Error>>);

pub(crate) struct ServerSpawnDriver {
    event_sender: SharedSubject<'static, ServerSpawnEvent, Infallible>,
}

impl ServerSpawnDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, ServerSpawnEvent, Infallible>,
        Self,
    ) {
        let event_sender = Shared::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(&self, command: ServerSpawnCommand) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        async move {
            let result = Server::spawn(command.serve_dir.path().to_path_buf())
                .map(Arc::new)
                .map_err(Arc::new);
            event_sender.next(ServerSpawnEvent(result));
        }
    }
}

#[derive(Debug)]
pub(crate) struct Server {
    join_handle: tokio::task::JoinHandle<hyper::Result<()>>,
    port: ServerPort,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ServerPort(pub(crate) u16);

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

        let serve_address = listener.local_addr().with_context(|| {
            format!("could not get local socket address of listener {listener:?}")
        })?;

        info!("serving address: {serve_address}");

        listener.set_nonblocking(true).with_context(|| {
            format!("could not set TCP stream non-blocking for listener {listener:?}")
        })?;

        let failed_to_create_server_msg =
            format!("failed to create hyper server from listener {listener:?}");

        let server_task = hyper::Server::from_tcp(listener)
            .context(failed_to_create_server_msg)?
            .tcp_nodelay(true)
            .serve(RouterService::new(RequestHandler {
                opts: Arc::from(handler_opts),
            }));

        Ok(Self {
            port: ServerPort(server_task.local_addr().port()),
            join_handle: tokio::spawn(server_task),
        })
    }

    pub(crate) fn port(&self) -> ServerPort {
        self.port
    }
}
