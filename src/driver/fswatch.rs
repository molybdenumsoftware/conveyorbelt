use notify::{INotifyWatcher, RecursiveMode, Watcher as _};
use rxrust::prelude::*;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use std::{convert::Infallible, path::PathBuf};

#[derive(Debug, derive_more::Display)]
pub(crate) enum FsCommand {
    #[display("init at {_0:?}")]
    Init(PathBuf),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum FsEvent {
    #[display("watcher creation error: {_0}")]
    WatcherCreationError(notify::Error),
    #[display("watcher created")]
    Watching(INotifyWatcher),
    #[display("watch error: {_0}")]
    WatcherWatchError(notify::Error),
    #[display("event error: {_0}")]
    EventError(notify::Error),
    #[display("change: {_0}")]
    Change(FsChange),
}

#[derive(Debug, Clone, derive_more::Display)]
#[display("{paths:?}: {kind}")]
pub(crate) struct FsChange {
    paths: Vec<PathBuf>,
    kind: FsChangeKind,
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
pub(crate) enum FsChangeKind {
    #[display("create {_0:?}")]
    Create(notify::event::CreateKind),
    #[display("modify {_0:?}")]
    Modify(notify::event::ModifyKind),
    #[display("remove {_0:?}")]
    Remove(notify::event::RemoveKind),
}

pub(crate) struct FsWatchDriver {
    event_sender: mpsc::Sender<FsEvent>,
}

impl FsWatchDriver {
    pub(crate) fn new() -> (SharedBoxedObservable<'static, FsEvent, Infallible>, Self) {
        let (event_sender, event_receiver) = mpsc::channel(1);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self, command: FsCommand) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            match command {
                FsCommand::Init(path_buf) => {
                    let event_sender_clone = event_sender.clone();
                    let event_handler = move |event| {
                        let event: notify::Event = match event {
                            Ok(event) => event,
                            Err(error) => {
                                event_sender_clone
                                    .blocking_send(FsEvent::EventError(error))
                                    .unwrap();

                                return;
                            }
                        };

                        let kind = match event.kind {
                            notify::EventKind::Create(kind) => FsChangeKind::Create(kind),
                            notify::EventKind::Modify(kind) => FsChangeKind::Modify(kind),
                            notify::EventKind::Remove(kind) => FsChangeKind::Remove(kind),
                            _ => return,
                        };

                        event_sender_clone
                            .blocking_send(FsEvent::Change(FsChange {
                                paths: event.paths,
                                kind,
                            }))
                            .unwrap();
                    };
                    let mut watcher = match notify::recommended_watcher(event_handler) {
                        Ok(watcher) => watcher,
                        Err(error) => {
                            event_sender
                                .send(FsEvent::WatcherCreationError(error))
                                .await
                                .unwrap();
                            return;
                        }
                    };

                    if let Err(error) = watcher.watch(&path_buf, RecursiveMode::Recursive) {
                        event_sender
                            .send(FsEvent::WatcherWatchError(error))
                            .await
                            .unwrap();
                    }
                    event_sender.send(FsEvent::Watching(watcher)).await.unwrap();
                }
            }
        }
    }
}
