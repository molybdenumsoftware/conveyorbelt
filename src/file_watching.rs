use std::{path::PathBuf, time::Duration};

use ignore_files::IgnoreFilter;
use tracing::info;
use watchexec::Watchexec;
use watchexec_events::filekind::FileEventKind;
use watchexec_filterer_ignore::IgnoreFilterer;

use crate::build_command::BuildCommand;

#[derive(Debug)]
pub struct FileWatcher {
    build_command: BuildCommand,
    path: PathBuf,
}

impl FileWatcher {
    pub fn new(build_command: BuildCommand, path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            build_command,
            path,
        })
    }

    pub async fn init(self) -> anyhow::Result<()> {
        let wx = Watchexec::new(move |action| {
                info!("change detected: {:?}", action.events);
                self.build_command.invoke();
                action
            })?;

        wx.config.throttle(Duration::ZERO);
        wx.config.pathset([self.path.as_path()]);
        let filterer = EventFilter::new(self.path.clone()).await?;
        wx.config.filterer(filterer);
        wx.main();
        Ok(())
    }
}

#[derive(Debug)]
struct EventFilter {
    path: PathBuf,
    ignore_filterer: IgnoreFilterer,
}

impl EventFilter {
    async fn new(path: PathBuf) -> anyhow::Result<Self> {
        let mut ignore_filter = IgnoreFilter::new(&path, &[]).await?;
        ignore_filter.finish();
        Ok(Self {
            ignore_filterer: IgnoreFilterer(ignore_filter),
            path,
        })
    }
}

impl watchexec::filter::Filterer for EventFilter {
    fn check_event(
        &self,
        event: &watchexec_events::Event,
        priority: watchexec_events::Priority,
    ) -> Result<bool, watchexec::error::RuntimeError> {
        let dot_git = self.path.join(".git");

        if let Some(path) = event.tags.iter().find_map(|tag| {
            if let watchexec_events::Tag::Path { path, .. } = tag {
                Some(path)
            } else {
                None
            }
        }) && path.starts_with(dot_git)
        {
            return Ok(false);
        };

        if let Some(kind) = event.tags.iter().find_map(|tag| {
            if let watchexec_events::Tag::FileEventKind(kind) = tag {
                Some(kind)
            } else {
                None
            }
        }) {
            let (FileEventKind::Create(_) | FileEventKind::Modify(_) | FileEventKind::Remove(_)) =
                kind
            else {
                return Ok(false);
            };
        }

        self.ignore_filterer.check_event(event, priority)
    }
}
