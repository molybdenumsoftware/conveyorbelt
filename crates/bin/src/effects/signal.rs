use anyhow::Context as _;
use futures::{FutureExt, future::BoxFuture};
use tokio::signal;

use crate::effects::Effect;

#[derive(Debug, derive_more::Display)]
pub(crate) enum SignalKind {
    #[display("SIGINT")]
    Sigint,
    #[display("SIGTERM")]
    Sigterm,
}

#[derive(Debug, derive_more::Display)]
#[display("install signal handler")]
pub(crate) struct InstallSignalListener;

#[derive(derive_more::Display)]
#[display("signal installed")]
pub(crate) struct SignalListenerInstalled {
    pub receive_f: BoxFuture<'static, SignalKind>,
}

impl Effect<SignalListenerInstalled, anyhow::Error> for InstallSignalListener {
    async fn effect(self) -> Result<SignalListenerInstalled, anyhow::Error> {
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .context("create sigint listener")?;

        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context("create sigterm listener")?;

        let receive_f = async move {
            tokio::select! {
                _ = sigint.recv() => {
                    SignalKind::Sigint
                },
                _ = sigterm.recv() => {
                    SignalKind::Sigterm
                }
            }
        }
        .boxed();

        Ok(SignalListenerInstalled { receive_f })
    }
}
