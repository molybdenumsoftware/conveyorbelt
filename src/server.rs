use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context as _;
use hyper::{StatusCode, server::conn::AddrIncoming};
use static_web_server::{
    handler::{RequestHandler, RequestHandlerOpts},
    service::RouterService,
};
use tracing::info;

pub(crate) struct Server(hyper::Server<AddrIncoming, RouterService>);

impl Server {
    pub(crate) async fn init(path: PathBuf) -> anyhow::Result<Self> {
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

        let server = hyper::Server::from_tcp(listener)
            .context(failed_to_create_server_msg)?
            .tcp_nodelay(true)
            .serve(RouterService::new(RequestHandler {
                opts: Arc::from(handler_opts),
            }));

        Ok(Self(server))
    }

    pub(crate) fn port(&self) -> u16 {
        self.0.local_addr().port()
    }

    pub(crate) fn into_inner(self) -> hyper::Server<AddrIncoming, RouterService> {
        self.0
    }
}
