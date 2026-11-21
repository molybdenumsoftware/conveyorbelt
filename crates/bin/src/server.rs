use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use anyhow::{Context as _, anyhow};
use futures::future::{
    Either::{Left, Right},
    select,
};
use http::StatusCode;
use static_web_server::{
    handler::{RequestHandler, RequestHandlerOpts},
    service::RouterService,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tracing::{debug, info, warn};

use crate::serve_dir::ServeDir;

#[derive(Debug, derive_more::Display)]
#[display("server at {address}")]
pub(crate) struct Server {
    address: SocketAddr,
    shutdown_tx: oneshot::Sender<()>,
    join: JoinHandle<()>,
}

// TODO pub(crate) vs. pub visibility throughout project

impl Server {
    pub async fn start(dir: ServeDir) -> anyhow::Result<Self> {
        let handler_opts = RequestHandlerOpts {
            root_dir: dir.to_path_buf(),
            compression: false,
            compression_static: false,
            cors: None,
            security_headers: false,
            cache_control_headers: false,
            page404: dir.join("404.html"),
            page50x: PathBuf::new(),
            index_files: ["index.html"].iter().map(|s| s.to_string()).collect(),
            log_remote_address: false,
            log_x_real_ip: false,
            log_forwarded_for: false,
            trusted_proxies: Vec::new(),
            redirect_trailing_slash: false,
            accept_markdown: false,
            health: false,
            maintenance_mode: false,
            maintenance_mode_status: StatusCode::SERVICE_UNAVAILABLE,
            maintenance_mode_file: PathBuf::new(),
            advanced_opts: None,
            text_charset: true,
            use_relative_root: false, // TODO use this to support receiving serve dir as an argument
            hsts: false,
            etag: true,
            access_log_format: None,
            include_hidden: false,
            follow_symlinks: false,
            shutting_down: Arc::new(AtomicBool::new(false)),
        };

        let address = SocketAddr::from((IpAddr::V4(Ipv4Addr::LOCALHOST), 0));

        let listener = TcpListener::bind(address)
            .await
            .with_context(|| format!("bind to {address}"))?;

        let server =
            hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());

        let graceful = hyper_util::server::graceful::GracefulShutdown::new();
        let address = listener.local_addr().context("get listener address")?;
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let router_service = RouterService::new(RequestHandler {
            opts: Arc::from(handler_opts),
        });

        let server_loop = async move {
            loop {
                let accept =
                    Box::pin(async { listener.accept().await.context("accept connection") });
                let shutdown = Box::pin(&mut shutdown_rx);
                let (stream, peer_addr) = match select(accept, shutdown).await {
                    Left((Err(e), _)) => {
                        warn!("{e}");
                        continue;
                    }
                    Left((Ok(conn), _)) => conn,
                    Right((_shutdown, _)) => break,
                };

                debug!("accept from {peer_addr}");
                if let Err(e) = stream
                    .set_nodelay(true)
                    .with_context(|| format!("enable TCP_NODELAY for steam with {peer_addr}"))
                {
                    warn!("{e}");
                }
                let stream = hyper_util::rt::TokioIo::new(Box::pin(stream));
                let conn = server.serve_connection(stream, router_service.build(Some(peer_addr)));
                let conn = graceful.watch(conn.into_owned());

                tokio::spawn(async move {
                    if let Err(e) = conn.await {
                        warn!("connection error: {e}");
                    };
                    debug!("connection dropped {peer_addr}");
                });
            }
        };

        let join = tokio::spawn(async move {
            server_loop.await;
            drop(dir);
        });

        info!("listening {address}");

        Ok(Self {
            address,
            shutdown_tx,
            join,
        })
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.shutdown_tx
            .send(())
            .map_err(|()| anyhow!("send shutdown message"))?;

        self.join.await.context("server task join")?;

        info!("server shut down");
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    pub fn address(&self) -> String {
        format!("http://{}", self.address)
    }
}
