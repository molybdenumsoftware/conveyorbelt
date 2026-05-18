use git2::Repository;
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
    WatcherCreationError(notify::Error),
    #[display("watcher created")]
    Watching(INotifyWatcher),
    #[display("watch error: {_0}")]
    WatcherWatchError(notify::Error),
    #[display("event error: {_0}")]
    EventError(notify::Error),
    #[display("change: {_0}")]
    Change(FsChange),
    #[display("git2 error: {_0}")]
    Git2Error(git2::Error),
}

#[derive(Debug, Clone)]
pub(crate) struct FsChange {
    pub(crate) path: PathBuf,
    pub(crate) kind: FsChangeKind,
    pub(crate) is_ignored: bool,
}

impl std::fmt::Display for FsChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = &self.path;
        let kind = &self.kind;
        let mut string = format!("{path:?}");

        if self.is_ignored {
            string.push_str(" (git ignored)");
        }

        string.push_str(&format!(" {kind}"));

        write!(f, "{string}")
    }
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
                    let repository = match Repository::open_from_env() {
                        Ok(repository) => repository,
                        Err(error) => {
                            event_sender_clone
                                .blocking_send(FsEvent::Git2Error(error))
                                .unwrap();
                            return;
                        }
                    };

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

                        for path in event.paths {
                            let is_ignored = match repository.is_path_ignored(&path) {
                                Ok(is_ignored) => is_ignored,
                                Err(error) => {
                                    event_sender_clone
                                        .blocking_send(FsEvent::Git2Error(error))
                                        .unwrap();
                                    return;
                                }
                            };

                            event_sender_clone
                                .blocking_send(FsEvent::Change(FsChange {
                                    path,
                                    kind,
                                    is_ignored,
                                }))
                                .unwrap();
                        }
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
