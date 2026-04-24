use notify::{INotifyWatcher, RecursiveMode, Watcher as _};
use rxrust::prelude::*;
use tokio_stream::wrappers::ReceiverStream;

use std::{convert::Infallible, path::PathBuf};

#[derive(Debug)]
pub(crate) enum FsWatchCommand {
    Init(PathBuf),
}

#[derive(Debug)]
pub(crate) enum FsEvent {
    WatcherCreationError(notify::Error),
    Watching(INotifyWatcher),
    WatcherWatchError(notify::Error),
    EventError(notify::Error),
    Event(notify::Event),
}

pub(crate) struct FsWatchDriver {
    event_sender: tokio::sync::mpsc::Sender<FsEvent>,
}

impl FsWatchDriver {
    pub(crate) fn new() -> (SharedBoxedObservable<'static, FsEvent, Infallible>, Self) {
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(0);
        let driver = Self { event_sender };
        (
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
            driver,
        )
    }

    pub(crate) fn effect(&self, command: FsWatchCommand) -> impl Future<Output = ()> + 'static {
        let event_sender = self.event_sender.clone();
        async move {
            match command {
                FsWatchCommand::Init(path_buf) => {
                    let event_sender_clone = event_sender.clone();
                    let event_handler = move |event| {
                        let fs_watch_event = match event {
                            Ok(event) => FsEvent::Event(event),
                            Err(error) => FsEvent::EventError(error),
                        };
                        event_sender_clone.blocking_send(fs_watch_event).unwrap();
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
