use anyhow::Context as _;
use futures::{FutureExt, future::BoxFuture};
use tokio::{select, signal::unix::signal};
use tracing::{debug, info};

#[derive(Debug, derive_more::Display)]
pub(crate) enum SignalKind {
    #[display("SIGINT")]
    Int,
    #[display("SIGTERM")]
    Term,
}

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
pub(crate) struct SignalListener(BoxFuture<'static, SignalKind>);

impl SignalListener {
    pub fn create() -> anyhow::Result<Self> {
        debug!("install signal listener");
        let mut sigint = signal(SignalKind::interrupt()).context("create sigint listener")?;
        let mut sigterm = signal(SignalKind::terminate()).context("create sigterm listener")?;

        let f = async {
            select! {
                _ = sigint.recv() => SignalKind::Int,
                _ = sigterm.recv() => SignalKind::Term,
            }
        }
        .boxed();

        info!("listening on sigint and sigterm");
        Ok(SignalListener(f))
    }
}
