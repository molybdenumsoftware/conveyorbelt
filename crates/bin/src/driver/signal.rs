use std::convert::Infallible;

use rxrust::{Observable as _, ObservableFactory as _, Shared, SharedBoxedObservable};
use tokio::{signal, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, derive_more::Display)]
pub(crate) enum SignalCommand {
    #[display("install handler")]
    InstallHandler,
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum SignalInstallEvent {
    #[display("handler installed")]
    HandlerInstalled,
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

pub(crate) struct SignalDriver {
    event_sender: mpsc::Sender<SignalEvent>,
}

impl SignalDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, SignalEvent, Infallible>,
        Self,
    ) {
        let (event_sender, event_receiver) = mpsc::channel(1);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self, command: SignalCommand) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            match command {
                SignalCommand::InstallHandler => {
                    let event_sender_clone = event_sender.clone();

                    let mut sigint =
                        match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
                            Ok(signal) => signal,
                            Err(error) => {
                                event_sender
                                    .send(SignalEvent::HandlerInstallFail(error))
                                    .await
                                    .unwrap();
                                return;
                            }
                        };

                    let mut sigterm =
                        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
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
                }
            };
        }
    }
}
