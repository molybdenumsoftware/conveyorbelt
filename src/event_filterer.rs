use std::path::PathBuf;

use ignore_files::IgnoreFilter;
use watchexec::filter::Filterer;
use watchexec_events::{Event, Tag, filekind::FileEventKind};
use watchexec_filterer_ignore::IgnoreFilterer;

#[derive(Debug)]
pub struct EventFilterer {
    project_root: PathBuf,
    ignore_filterer: IgnoreFilterer,
}

impl EventFilterer {
    pub async fn new(path: PathBuf) -> anyhow::Result<Self> {
        let mut ignore_filter = IgnoreFilter::new(&path, &[]).await?;
        ignore_filter.finish();

        Ok(Self {
            ignore_filterer: IgnoreFilterer(ignore_filter),
            project_root: path,
        })
    }
}

impl Filterer for EventFilterer {
    fn check_event(
        &self,
        event: &Event,
        priority: watchexec_events::Priority,
    ) -> Result<bool, watchexec::error::RuntimeError> {
        let dot_git = self.project_root.join(".git");

        if let Some(path) = event.tags.iter().find_map(|tag| {
            if let Tag::Path { path, .. } = tag {
                Some(path)
            } else {
                None
            }
        }) && path.starts_with(dot_git)
        {
            return Ok(false);
        };

        if event.metadata.contains_key("initial-build") {
            dbg!(event);
            return Ok(true);
        }

        let kind = event.tags.iter().find_map(|tag| {
            if let Tag::FileEventKind(kind) = tag {
                Some(kind)
            } else {
                None
            }
        });

        if !matches!(
            kind,
            Some(FileEventKind::Create(_) | FileEventKind::Modify(_) | FileEventKind::Remove(_))
        ) {
            return Ok(false);
        }

        self.ignore_filterer.check_event(event, priority)
    }
}
