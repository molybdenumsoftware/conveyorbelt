use anyhow::{Context, Ok};
use git2::Repository;
use notify::{RecursiveMode, Watcher as _};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use std::path::{Path, PathBuf};

// TODO logging
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

#[derive(Debug)]
pub(crate) struct FsWatch(pub ReceiverStream<anyhow::Result<Vec<FsChange>>>);

impl FsWatch {
    pub fn init(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let repository = Repository::open_from_env().context("open git repo")?;
        let (event_sender, event_receiver) = mpsc::channel(1);

        let event_handler = move |result| -> anyhow::Result<Vec<FsChange>> {
            let event = result.context("fs watch error")?;
            let changes = Vec::new();

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
                _ => return Ok(changes),
            };

            for path in event.paths {
                let is_ignored = repository
                    .is_path_ignored(&path)
                    .context("ask wether path is ignored")?;

                changes.push(FsChange {
                    path,
                    kind,
                    is_ignored,
                });
            }

            Ok(changes)
        };

        let mut watcher = notify::recommended_watcher(|event| {
            event_sender.blocking_send(event_handler(event)).unwrap();
        })
        .context("create watcher")?;

        watcher
            .watch(path.as_ref(), RecursiveMode::Recursive)
            .context("begin watching")?;

        Ok(FsWatch(ReceiverStream::new(event_receiver)))
    }
}
