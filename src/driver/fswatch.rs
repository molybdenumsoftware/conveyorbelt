use notify::{INotifyWatcher, RecursiveMode, Watcher as _};
use rxrust::prelude::*;

use std::{convert::Infallible, path::PathBuf, rc::Rc};

#[derive(Debug, Clone)]
pub(crate) enum FsWatchCommand {
    Init(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) enum FsWatchEvent {
    WatcherCreationError(Rc<notify::Error>),
    Watching(Rc<INotifyWatcher>),
    WatcherWatchError(Rc<notify::Error>),
    EventError(Rc<notify::Error>),
    Event(notify::Event),
}

pub(crate) struct FsWatchDriver {
    event_sender: LocalSubject<'static, FsWatchEvent, Infallible>,
}

impl FsWatchDriver {
    pub(crate) fn new() -> (
        LocalBoxedObservable<'static, FsWatchEvent, Infallible>,
        Self,
    ) {
        let event_sender = Local::subject();
        let event_stream = event_sender.clone().box_it();
        let driver = Self { event_sender };
        (event_stream, driver)
    }

    pub(crate) fn effect(&self, command: FsWatchCommand) -> impl Future<Output = ()> + 'static {
        let mut event_sender = self.event_sender.clone();
        match command {
            FsWatchCommand::Init(path_buf) => {
                async move {
                    let event_handler = |event| {
                        let fs_watch_event = match event {
                            Ok(event) => FsWatchEvent::Event(event),
                            Err(error) => FsWatchEvent::EventError(Rc::new(error)),
                        };
                        event_sender.next(fs_watch_event)
                    };
                    let mut watcher = match notify::recommended_watcher(event_handler) {
                        Ok(watcher) => watcher,
                        Err(error) => {
                            event_sender.next(FsWatchEvent::WatcherCreationError(Rc::new(error)));
                            return;
                        }
                    };
                    if let Err(error) = watcher.watch(&path_buf, RecursiveMode::Recursive) {
                        event_sender.next(FsWatchEvent::WatcherWatchError(Rc::new(error)));
                        return;
                    }
                    event_sender.next(FsWatchEvent::Watching(Rc::new(watcher)))
                }
            }
        }
    }
}
