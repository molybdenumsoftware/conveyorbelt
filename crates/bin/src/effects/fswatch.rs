use anyhow::Context;
use git2::Repository;
use notify::{RecursiveMode, Watcher as _};
use rxrust::prelude::*;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use std::{convert::Infallible, path::PathBuf};

use crate::effects::Effect;

// #[derive(derive_more::Display)]
// pub(crate) enum FsWatchInitEvent {
//     #[display("watcher creation fail: {_0}")]
//     WatcherCreationError(notify::Error),
//     #[display("watcher created")]
//     Watching(SharedBoxedObservable<'static, FsWatchWatchingEvent, Infallible>),
//     #[display("watch error: {_0}")]
//     WatcherWatchError(notify::Error),
//     #[display("git2 error: {_0}")]
//     Git2Error(git2::Error),
// }

#[derive(Debug, derive_more::Display)]
pub(crate) enum FsWatchWatchingEvent {
    #[display("event error: {_0}")]
    Error(anyhow::Error),
    #[display("change: {_0}")]
    Change(FsChange),
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
pub(crate) enum ModifyKind {
    #[display("any")]
    Any,
    #[display("data {_0:?}")]
    Data(notify::event::DataChange),
    #[display("name {_0:?}")]
    Name(notify::event::RenameMode),
    #[display("other")]
    Other,
}

#[derive(Debug, Clone, Copy, derive_more::Display)]
pub(crate) enum FsChangeKind {
    #[display("create {_0:?}")]
    Create(notify::event::CreateKind),
    #[display("modify {_0:?}")]
    Modify(ModifyKind),
    #[display("remove {_0:?}")]
    Remove(notify::event::RemoveKind),
}

#[derive(Debug, derive_more::Display)]
#[display("watch {path:?}")]
pub(crate) struct FsWatchInit {
    pub path: PathBuf,
}

#[derive(derive_more::Display)]
#[display("watching")]
pub(crate) struct FsWatching(pub SharedBoxedObservable<'static, FsWatchWatchingEvent, Infallible>);

// TODO alias the heck out of `SharedBoxedObservable<'static, FsWatchWatchingEvent, Infallible>`
impl Effect<FsWatching, anyhow::Error> for FsWatchInit {
    async fn effect(self) -> Result<FsWatching, anyhow::Error> {
        // TODO add more error context
        let repository = Repository::open_from_env().context("open git repo")?;

        let (event_sender, event_receiver) = mpsc::channel(1);

        let event_handler = move |event: Result<notify::Event, notify::Error>| {
            let event: notify::Event = match event {
                Ok(event) => event,
                Err(error) => {
                    event_sender
                        .blocking_send(FsWatchWatchingEvent::Error(error.into()))
                        .unwrap();

                    return;
                }
            };

            let kind = match event.kind {
                notify::EventKind::Create(kind) => FsChangeKind::Create(kind),
                notify::EventKind::Modify(notify::event::ModifyKind::Any) => {
                    FsChangeKind::Modify(ModifyKind::Any)
                }
                notify::EventKind::Modify(notify::event::ModifyKind::Other) => {
                    FsChangeKind::Modify(ModifyKind::Other)
                }
                notify::EventKind::Modify(notify::event::ModifyKind::Data(change)) => {
                    FsChangeKind::Modify(ModifyKind::Data(change))
                }
                notify::EventKind::Modify(notify::event::ModifyKind::Name(rename)) => {
                    FsChangeKind::Modify(ModifyKind::Name(rename))
                }
                notify::EventKind::Remove(kind) => FsChangeKind::Remove(kind),
                _ => return,
            };

            for path in event.paths {
                let is_ignored = match repository
                    .is_path_ignored(&path)
                    .context("wether path ignored")
                {
                    Ok(is_ignored) => is_ignored,
                    Err(error) => {
                        // TODO can we use try operator instead
                        event_sender
                            .blocking_send(FsWatchWatchingEvent::Error(error.into()))
                            .unwrap();
                        return;
                    }
                };

                event_sender
                    .blocking_send(FsWatchWatchingEvent::Change(FsChange {
                        path,
                        kind,
                        is_ignored,
                    }))
                    .unwrap();
            }
        };

        let mut watcher = notify::recommended_watcher(event_handler).context("create watcher")?;

        watcher
            .watch(&self.path, RecursiveMode::Recursive)
            .context("begin watching")?;

        Ok(FsWatching(
            Shared::from_stream(ReceiverStream::new(event_receiver)).box_it(),
        ))
    }
}
