use notify::{INotifyWatcher, RecursiveMode, Watcher as _};
use rxrust::prelude::*;

use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone)]
pub(crate) enum FsWatchCommand {
    Init(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) enum FsWatchEvent {
    WatcherCreationError(Arc<notify::Error>),
    Watching(Arc<Mutex<INotifyWatcher>>),
    WatcherWatchError(Arc<notify::Error>),
    EventError(Arc<notify::Error>),
    Event(notify::Event),
}

pub(crate) struct FsWatchDriver {
    event_sender: SharedSubject<'static, FsWatchEvent, Infallible>,
}

impl FsWatchDriver {
    pub(crate) fn new() -> (
        SharedBoxedObservable<'static, FsWatchEvent, Infallible>,
        Self,
    ) {
        let event_sender = Shared::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(&self, command: FsWatchCommand) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        let mut event_sender_clone = event_sender.clone();
        match command {
            FsWatchCommand::Init(path_buf) => {
                let event_handler = move |event| {
                    let fs_watch_event = match event {
                        Ok(event) => FsWatchEvent::Event(event),
                        Err(error) => FsWatchEvent::EventError(Arc::new(error)),
                    };
                    event_sender_clone.next(fs_watch_event)
                };
                async move {
                    let mut watcher = match notify::recommended_watcher(event_handler) {
                        Ok(watcher) => watcher,
                        Err(error) => {
                            event_sender.next(FsWatchEvent::WatcherCreationError(Arc::new(error)));
                            return;
                        }
                    };
                    let watcher = Arc::new(Mutex::new(watcher));

                    event_sender.next(FsWatchEvent::Watching(watcher.clone()));

                    if let Err(error) = watcher
                        .lock()
                        .unwrap()
                        .watch(&path_buf, RecursiveMode::Recursive)
                    {
                        event_sender.next(FsWatchEvent::WatcherWatchError(Arc::new(error)));
                        return;
                    }
                }
            }
        }
    }
}
