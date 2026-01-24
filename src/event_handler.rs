use chromiumoxide::Browser;
use watchexec::command::{Program, SpawnOptions};
use std::sync::Mutex;
use std::{sync::Arc, time::Duration};
use tracing::info;
use watchexec::Id as JobId;
use watchexec_events::ProcessEnd;

use watchexec_events::Tag;

use crate::build_command;
use crate::config::Config;

#[derive(Debug)]
enum State {
    Initial,
    StartingServer {
        build_job: JobId,
        server_job: JobId,
    },
    StartingBrowser {
        build_job: JobId,
        server: (),
        browser_job: JobId,
    },
    Ready {
        build_job: JobId,
        server: (),
        browser_job: JobId,
        browser: Browser,
    },
}

pub(crate) fn set(wx_config: &mut Arc<watchexec::Config>, config: Arc<Config>) {
    let state = Arc::new(Mutex::new(State::Initial));

    wx_config
        .throttle(Duration::ZERO)
        .on_action(move |mut action| {
            let state_lock = state.lock().unwrap();
            let state = &mut *state_lock;
            let signal = action.signals().next();

            if let Some(signal) = signal {
                info!("Signal {signal}; terminating");
                action.quit_gracefully(signal, Duration::MAX);
                return action;
            }

            let [event] = action.events.as_ref() else {
                unreachable!("thanks to zero throttling");
            };

            match state {
                State::Initial => {
                    let id = build_command::spawn(&mut action, Arc::clone(&config));
                }
                State::StartingServer {
                    build_job,
                    server_job,
                } => todo!(),
                State::StartingBrowser {
                    build_job,
                    server,
                    browser_job,
                } => todo!(),
                State::Ready {
                    build_job,
                    server,
                    browser_job,
                    browser,
                } => todo!(),
            }

            if event.paths().count() > 0 {
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
        });
}
