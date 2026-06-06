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
                        .send(SignalInstallEvent::HandlerInstallFail(error))
                        .await
                        .unwrap();
                    return;
                }
            };

            let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    event_sender
                        .send(SignalInstallEvent::HandlerInstallFail(error))
                        .await
                        .unwrap();
                    return;
                }
            };

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
            event_sender
                .send(SignalInstallEvent::HandlerInstalled(signal_events))
                .await
                .unwrap();
        });
        Shared::from_stream(ReceiverStream::new(event_receiver)).box_it()
    }
}
