use anyhow::Context;
use notify::{INotifyWatcher, RecursiveMode, Watcher as _};
use tokio::sync::mpsc::{self, Receiver};
use tracing::{debug, info, warn};

use std::path::PathBuf;

use crate::git::GitRepo;

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
pub(crate) struct FsWatch {
    _watcher: INotifyWatcher,
    receiver: Receiver<anyhow::Result<Vec<FsChange>>>,
}

impl FsWatch {
    pub fn init(repo: GitRepo) -> anyhow::Result<Self> {
        debug!("fs watch init");
        let (event_sender, event_receiver) = mpsc::channel(1);

        let path = repo.workdir().to_owned();
        let event_handler =
            move |result: notify::Result<notify::Event>| -> anyhow::Result<Vec<FsChange>> {
                let event = result.context("fs watch error")?;
                let mut changes = Vec::new();

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
                    let is_ignored = repo.is_path_ignored(&path)?;

                    changes.push(FsChange {
                        path,
                        kind,
                        is_ignored,
                    });
                }

                Ok(changes)
            };

        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = event_sender.blocking_send(event_handler(event));
        })
        .context("create watcher")?;

        watcher
            .watch(&path, RecursiveMode::Recursive)
            .context("begin watching")?;

        Ok(FsWatch {
            _watcher: watcher,
            receiver: event_receiver,
        })
    }

    pub async fn recv(&mut self) {
        loop {
            let changes = match self
                .receiver
                .recv()
                .await
                .context("receive fs events")
                .expect("never closes")
            {
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
                Ok(changes) => changes,
            };

            let mut all_ignored = true;
            for change in &changes {
                info!("{change}");
                all_ignored = all_ignored && change.is_ignored;
            }

            if all_ignored {
                continue;
            }
            break;
        }
    }
}
