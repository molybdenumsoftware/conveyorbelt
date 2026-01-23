use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;
use tracing::info;
use watchexec_events::{Event, ProcessEnd};

use watchexec::error::CriticalError;
use watchexec_events::Tag;

use crate::common::{ForStdoutputLine as _, SERVE_PATH};

#[derive(Debug, Clone)]
struct CommandWrapper {
    serve_path: PathBuf,
}

impl process_wrap::tokio::CommandWrapper for CommandWrapper {
    fn pre_spawn(
        &mut self,
        command: &mut tokio::process::Command,
        _core: &process_wrap::tokio::CommandWrap,
    ) -> std::io::Result<()> {
        command
            .env(SERVE_PATH, self.serve_path.as_path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(())
    }

    fn post_spawn(
        &mut self,
        _command: &mut tokio::process::Command,
        child: &mut tokio::process::Child,
        _core: &process_wrap::tokio::CommandWrap,
    ) -> std::io::Result<()> {
        child
            .for_stdout_line(|line| {
                info!("build command stdout: {line}");
            })
            .unwrap();

        child
            .for_stderr_line(|line| {
                info!("build command stderr: {line}");
            })
            .unwrap();
        Ok(())
    }
}

#[derive(Debug)]
pub struct FileWatcherConfig {
    project_root: PathBuf,
    serve_path: PathBuf,
    build_command: PathBuf,
}

impl FileWatcherConfig {
    pub fn new(project_root: PathBuf, serve_path: PathBuf, build_command: PathBuf) -> Self {
        Self {
            build_command,
            project_root,
            serve_path,
        }
    }

    pub async fn init(self) -> anyhow::Result<JoinHandle<Result<(), CriticalError>>> {
        let is_build_queued = Arc::new(Mutex::new(false));
        let serve_path = self.serve_path.clone();
        let build_command = Arc::clone(&self.build_command);

        Ok(main)
    }
}

pub fn new() -> impl Fn(ActionHandler) -> ActionHandler + Send + Sync + 'static {
    move |mut action| {
        let signal = action.signals().next();

        if let Some(signal) = signal {
            info!("Signal {signal}; terminating");
            action.quit_gracefully(signal, Duration::MAX);
            return action;
        }

        let [event] = action.events.as_ref() else {
            unreachable!("thanks to zero throttling");
        };

        if event.metadata.contains_key("initial-build") || event.paths().count() > 0 {
            if action.list_jobs().count() > 0 {
                *is_build_queued.lock().unwrap() = true;
                return action;
            }
            let (_, job) = action.create_job(build_command.clone());
            let serve_path = serve_path.clone();

            job.set_spawn_hook(move |command, _| {
                command.wrap(CommandWrapper {
                    serve_path: serve_path.clone(),
                });
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
                None => "build process ended in an unknown manner".to_string(),
                Some(ProcessEnd::Success) => "build command succeeded".to_string(),
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
                Some(ProcessEnd::Continued) => "build process continued".to_string(),
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
                    let (_id, _job) = action.create_job(Arc::clone(&build_command));
                }

                drop(mutex_guard);
                return action;
            }
        };
        action
    }
}

pub fn initial_event() -> Event {
    Event {
        tags: Vec::new(),
        metadata: HashMap::from_iter([("initialize".to_string(), Vec::new())]),
    }
}
