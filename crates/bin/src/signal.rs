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

pub fn listen() -> anyhow::Result<BoxFuture<'static, SignalKind>> {
    debug!("install signal listener");
    let mut sigint =
        signal(tokio::signal::unix::SignalKind::interrupt()).context("create sigint listener")?;
    let mut sigterm =
        signal(tokio::signal::unix::SignalKind::terminate()).context("create sigterm listener")?;

    let f = async move {
        let kind = select! {
            _ = sigint.recv() => SignalKind::Int,
            _ = sigterm.recv() => SignalKind::Term,
        };
        info!("received {kind}");
        kind
    }
    .boxed();

    info!("listening on sigint and sigterm");
    Ok(f)
}
