use std::convert::Infallible;

use rxrust::{Observable as _, ObservableFactory as _, Shared, SharedBoxedObservable};
use tokio::{signal, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

#[derive(derive_more::Display)]
pub(crate) enum SignalInstallEvent {
    #[display("handler installed")]
    HandlerInstalled(SharedBoxedObservable<'static, SignalKind, Infallible>),
    #[display("handler install fail")]
    HandlerInstallFail(std::io::Error),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum SignalKind {
    #[display("SIGINT")]
    Sigint,
    #[display("SIGTERM")]
    Sigterm,
}

pub(crate) struct InstallSignal;

impl InstallSignal {
    pub(crate) fn effect(self) -> SharedBoxedObservable<'static, SignalInstallEvent, Infallible> {
        let (event_sender, event_receiver) = mpsc::channel(1);
        tokio::spawn(async move {
            let mut sigint = match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
                Ok(signal) => signal,
                Err(error) => {
                    event_sender
                        .send(SignalEvent::HandlerInstallFail(error))
                        .await
                        .unwrap();
                    return;
                }
            };

            let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    event_sender
                        .send(SignalEvent::HandlerInstallFail(error))
                        .await
                        .unwrap();
                    return;
                }
            };

            tokio::spawn(async move {
                let event = tokio::select! {
                    _ = sigint.recv() => {
                        SignalEvent::Received(SignalKind::Sigint)
                    },
                    _ = sigterm.recv() => {
                        SignalEvent::Received(SignalKind::Sigterm)
                    }
                };

                event_sender_clone.send(event).await.unwrap();
            });

            event_sender
                .send(SignalEvent::HandlerInstalled)
                .await
                .unwrap();
        });
        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}
