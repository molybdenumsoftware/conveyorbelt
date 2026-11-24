use anyhow::Context;
use notify::{INotifyWatcher, RecursiveMode::Recursive, Watcher as _};
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};

use crate::git::GitRepo;

#[derive(Debug)]
pub(crate) struct FsWatch {
    _watcher: INotifyWatcher,
    git_repo: GitRepo,
    receiver: mpsc::UnboundedReceiver<notify::Result<notify::Event>>,
}

impl FsWatch {
    pub(crate) fn init(git_repo: GitRepo) -> anyhow::Result<Self> {
        debug!("fs watch init");
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(sender).context("create watcher")?;

        watcher
            .watch(git_repo.workdir(), Recursive)
            .context("watch")?;

        info!("watching");

        Ok(Self {
            _watcher: watcher,
            git_repo,
            receiver,
        })
    }

    fn resolve_change(&self, event: notify::Result<notify::Event>) -> anyhow::Result<bool> {
        let event = event?;
        trace!("{event:?}");

        let (notify::EventKind::Create(_)
        | notify::EventKind::Modify(_)
        | notify::EventKind::Remove(_)) = event.kind
        else {
            return Ok(false);
        };

        let mut paths = event.paths.iter();
        let all_ignored = loop {
            let Some(path) = paths.next() else {
                break true;
            };
            if !self
                .git_repo
                .is_path_ignored(path)
                .context("ask whether path git ignored")?
            {
                break false;
            }
        };

        if all_ignored {
            debug!("all event paths are git ignored: {event:?}");
            return Ok(false);
        }

        info!("{:?} {:?}", event.kind, event.paths);
        Ok(true)
    }

    pub(crate) async fn recv(&mut self) {
        loop {
            let event = self.receiver.recv().await.expect("is open");

            match self.resolve_change(event) {
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
                Ok(false) => continue,
                Ok(true) => break,
            };
        }
    }
}
