use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::PathBuf,
    sync::Arc,
};

use anyhow::Context as _;
use hyper::StatusCode;
use static_web_server::{
    handler::{RequestHandler, RequestHandlerOpts},
    service::RouterService,
};
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{debug, info};

#[derive(Clone, Debug, derive_more::Deref, derive_more::Display)]
#[display("serve dir: {_0:?}")]
#[deref(forward)]
pub(crate) struct ServeDir(Arc<TempDir>);

impl ServeDir {
    pub fn create() -> anyhow::Result<Self> {
        debug!("create serve dir");
        let dir = TempDir::new().context("create temp dir")?;
        Ok(ServeDir(Arc::new(dir)))
    }
}

// TODO #[display("server spawned: {address}")]

#[derive(Debug, derive_more::Display)]
#[display("server at {address}")]
pub(crate) struct Server {
    address: SocketAddr,
    shutdown_sender: oneshot::Sender<()>,
    join: JoinHandle<hyper::Result<()>>,
}

impl Server {
    pub async fn start(dir: ServeDir) -> anyhow::Result<Self> {
        let handler_opts = RequestHandlerOpts {
            root_dir: dir.path().to_path_buf(),
            compression: false,
            compression_static: false,
            cors: None,
            security_headers: false,
            cache_control_headers: false,
            page404: dir.path().join("404.html"),
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
        let listener = TcpListener::bind(address).with_context(|| format!("bind to {address}"))?;

        listener
            .set_nonblocking(true)
            .with_context(|| format!("set TCP stream non-blocking for listener {listener:?}"))?;

        let failed_to_create_server_msg = format!("create hyper server from listener {listener:?}");

        let address = listener.local_addr().context("get listener address")?;
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

        let join = tokio::spawn(async move {
            let result = server_task.await;
            drop(dir);
            result
        });

        Ok(Self {
            address,
            shutdown_sender,
            join,
        })
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.shutdown_sender.send(()).unwrap();
        self.join
            .await
            .context("server task join")?
            .context("server shut down")?;

        info!("server shut down");
        Ok(())
    }
}
