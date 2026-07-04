use std::convert::Infallible;

use anyhow::Context as _;
use rxrust::{Observable as _, ObservableFactory as _, Shared, SharedBoxedObservable};
use tokio::{signal, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

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
pub(crate) struct InstallSignalHandler;

#[derive(derive_more::Display)]
#[display("signal installed")]
pub(crate) struct SignalInstalled {
    pub signal_o: SharedBoxedObservable<'static, SignalKind, Infallible>,
}

impl Effect<SignalInstalled, anyhow::Error> for InstallSignalHandler {
    async fn effect(self) -> Result<SignalInstalled, anyhow::Error> {
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .context("create sigint listener")?;

        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context("create sigterm listener")?;

        let (signal_event_sender, signal_event_receiver) = mpsc::channel(1);

        tokio::spawn(async move {
            let event = tokio::select! {
                _ = sigint.recv() => {
                    SignalKind::Sigint
                },
                _ = sigterm.recv() => {
                    SignalKind::Sigterm
                }
            };

            signal_event_sender.send(event).await.unwrap();
        });

        let signal_events =
            Shared::from_stream(ReceiverStream::new(signal_event_receiver)).box_it();

        Ok(SignalInstalled {
            signal_o: signal_events,
        })
    }
}
