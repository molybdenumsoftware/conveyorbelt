use std::path::PathBuf;

use ignore_files::IgnoreFilter;
use watchexec::{error::RuntimeError, filter::Filterer};
use watchexec_events::{Event, Priority, Tag, filekind::FileEventKind};
use watchexec_filterer_ignore::IgnoreFilterer;

#[derive(Debug)]
pub(crate) struct EventFilterer {
    initial_build: InitialBuildFilterer,
    dot_git: DotGitFilterer,
    ignore: IgnoreFilterer,
    change: ChangeFilterer,
}

impl EventFilterer {
    pub(crate) async fn new(project_root: PathBuf) -> anyhow::Result<Self> {
        let mut ignore_filter = IgnoreFilter::new(&project_root, &[]).await?;
        ignore_filter.finish();

        Ok(Self {
            initial_build: InitialBuildFilterer,
            dot_git: DotGitFilterer { project_root },
            ignore: IgnoreFilterer(ignore_filter),
            change: ChangeFilterer,
        })
    }
}

impl Filterer for EventFilterer {
    fn check_event(&self, event: &Event, priority: Priority) -> Result<bool, RuntimeError> {
        Ok(self.initial_build.check_event(event, priority)?
            || self.dot_git.check_event(event, priority)?
                && self.ignore.check_event(event, priority)?
                && self.change.check_event(event, priority)?)
    }
}

#[derive(Debug)]
struct InitialBuildFilterer;

impl Filterer for InitialBuildFilterer {
    fn check_event(&self, event: &Event, _: Priority) -> Result<bool, RuntimeError> {
        Ok(event.metadata.contains_key("initial-build"))
    }
}

#[derive(Debug)]
struct DotGitFilterer {
    project_root: PathBuf,
}

impl Filterer for DotGitFilterer {
    fn check_event(&self, event: &Event, _: Priority) -> Result<bool, RuntimeError> {
        let path = event.tags.iter().find_map(|tag| {
            if let Tag::Path { path, .. } = tag {
                Some(path)
            } else {
                None
            }
        });

        let Some(path) = path else { return Ok(true) };

        Ok(!path.starts_with(self.project_root.join(".git")))
    }
}

#[derive(Debug)]
struct ChangeFilterer;

impl Filterer for ChangeFilterer {
    fn check_event(&self, event: &Event, _: Priority) -> Result<bool, RuntimeError> {
        let kind = event.tags.iter().find_map(|tag| {
            if let Tag::FileEventKind(kind) = tag {
                Some(kind)
            } else {
                None
            }
        });

        Ok(!matches!(
            kind,
            Some(FileEventKind::Create(_) | FileEventKind::Modify(_) | FileEventKind::Remove(_))
        ))
    }
}
