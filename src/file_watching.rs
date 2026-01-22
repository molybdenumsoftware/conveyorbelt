use std::{
    path::PathBuf, process::Stdio, sync::{Arc, Mutex}, time::Duration
};
use tracing::info;
use watchexec_events::{ProcessEnd, filekind::FileEventKind};

use ignore_files::IgnoreFilter;
use watchexec::{
    Watchexec,
    command::{Command, Program, SpawnOptions},
};
use watchexec_events::Tag;
use watchexec_filterer_ignore::IgnoreFilterer;

use crate::build_command::BuildCommand;

#[derive(Debug)]
pub struct FileWatcher {
    build_command: Arc<Command>,
    path: PathBuf,
}

impl FileWatcher {
    pub fn new(build_command: BuildCommand, path: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            build_command: Arc::new(Command {
                program: Program::Exec {
                    prog: build_command.path,
                    args: Vec::new(),
                },
                options: SpawnOptions::default(),
            }),
            path,
        })
    }

    pub async fn init(self) -> anyhow::Result<()> {
        let is_build_queued = Arc::new(Mutex::new(false));

        let wx = Watchexec::new(move |mut action| {
            let signal = action.signals().next();

            if let Some(signal) = signal {
                action.quit_gracefully(signal, Duration::MAX);
                return action;
            }

            let [event] = action.events.as_ref() else {
                unreachable!("thanks to zero throttling");
            };

            if event.paths().count() > 0 {
                if action.list_jobs().count() > 0 {
                    *is_build_queued.lock().unwrap() = true;
                    return action;
                }

                let (_, job) = action.create_job(Arc::clone(&self.build_command));
                job.set_spawn_hook(|command, _| {
                    command.command_mut().stdout(Stdio::piped()).stderr(Stdio::piped()); 
                });
                return action;
            }

            let process_end = event.tags.iter().find_map(|tag| {
                if let Tag::ProcessCompletion(completion) = tag {
                    Some(completion)
                } else {
                    None
                }
            });

            if let Some(end) = process_end {
                let message = match end {
                    None => format!("build process ended in an unknown manner"),
                    Some(ProcessEnd::Success) => "build process succeeded".to_string(),
                    Some(ProcessEnd::ExitError(non_zero)) => {
                        format!("build process exited with {non_zero}")
                    }
                    Some(ProcessEnd::ExitSignal(signal)) => {
                        format!("build process exited with {signal}")
                    }
                    Some(ProcessEnd::ExitStop(non_zero)) => {
                        format!("build process stopped with {non_zero}")
                    }
                    Some(ProcessEnd::Exception(non_zero)) => {
                        format!("build process exception {non_zero}")
                    }
                    Some(ProcessEnd::Continued) => format!("build process continued"),
                };

                info!(message);

                if let None
                | Some(ProcessEnd::Success)
                | Some(ProcessEnd::ExitError(_))
                | Some(ProcessEnd::ExitSignal(_))
                | Some(ProcessEnd::Exception(_)) = end
                {
                    let mutex_guard = is_build_queued.lock().unwrap();

                    if *mutex_guard {
                        action.create_job(Arc::clone(&self.build_command));
                    }

                    drop(mutex_guard);
                    return action;
                }
            };
            action
        })?;

        wx.config.throttle(Duration::ZERO); // to guarantee one event at a time
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
            if let Tag::Path { path, .. } = tag {
                Some(path)
            } else {
                None
            }
        }) && path.starts_with(dot_git)
        {
            return Ok(false);
        };

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
