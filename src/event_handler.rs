use std::sync::Mutex;
use std::{sync::Arc, time::Duration};
use tracing::info;
use watchexec::{Id as JobId, Watchexec};
use watchexec_events::{Event, Priority, Tag};

use crate::config::Config;

pub(crate) const INITIAL_BUILD: &str = "initial-build";

#[derive(Debug, PartialEq, Eq)]
enum State {
    Initial,
    InitialBuild { build_process: JobId },
}

pub(crate) async fn set(wx: &mut Watchexec, config: Arc<Config>) -> anyhow::Result<()> {
    let state = Arc::new(Mutex::new(State::Initial));

    wx.config
        .throttle(Duration::ZERO)
        .on_action(move |mut action| {
            let mut state_lock = state.lock().unwrap();
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

            if event.metadata.contains_key(INITIAL_BUILD) {
                if *state != State::Initial {
                    panic!("not supposed to happen");
                }
                let job_id = build_command::spawn(&mut action, Arc::clone(&config));
                *state = State::InitialBuild {
                    build_process: job_id,
                };
                return action;
            }

            if let Some(end) = event.tags.iter().find_map(|tag| {
                if let Tag::ProcessCompletion(end) = tag {
                    Some(end)
                } else {
                    None
                }
            }) {
                let Some(end) = end else {
                    panic!("we can't work like this");
                };
            }

            // if event.paths().count() > 0 {
            //     if action.list_jobs().count() > 0 {
            //         *is_build_queued.lock().unwrap() = true;
            //         return action;
            //     }
            //     let (_, job) = action.create_job(build_command.clone());
            //     let serve_path = serve_path.clone();

            //     job.set_spawn_hook(move |command, _| {
            //         command.wrap(CommandWrapper {
            //             serve_path: serve_path.clone(),
            //         });
            //     });

            //     return action;
            // }

            // let process_end = event.tags.iter().find_map(|tag| {
            //     if let Tag::ProcessCompletion(completion) = tag {
            //         Some(completion)
            //     } else {
            //         None
            //     }
            // });

            // if let Some(end) = process_end {
            //     let message = match end {
            //         None => "build process ended in an unknown manner".to_string(),
            //         Some(ProcessEnd::Success) => "build command succeeded".to_string(),
            //         Some(ProcessEnd::ExitError(non_zero)) => {
            //             format!("build process exited with {non_zero}")
            //         }
            //         Some(ProcessEnd::ExitSignal(signal)) => {
            //             format!("build process exited with {signal}")
            //         }
            //         Some(ProcessEnd::ExitStop(non_zero)) => {
            //             format!("build process stopped with {non_zero}")
            //         }
            //         Some(ProcessEnd::Exception(non_zero)) => {
            //             format!("build process exception {non_zero}")
            //         }
            //         Some(ProcessEnd::Continued) => "build process continued".to_string(),
            //     };

            //     info!(message);

            //     if let None
            //     | Some(ProcessEnd::Success)
            //     | Some(ProcessEnd::ExitError(_))
            //     | Some(ProcessEnd::ExitSignal(_))
            //     | Some(ProcessEnd::Exception(_)) = end
            //     {
            //         let mutex_guard = is_build_queued.lock().unwrap();

            //         if *mutex_guard {
            //             let (_id, _job) = action.create_job(Arc::clone(&build_command));
            //         }

            //         drop(mutex_guard);
            //         return action;
            //     }
            // };
            action
        });

    let mut initial_event = Event::default();
    initial_event
        .metadata
        .insert(INITIAL_BUILD.to_string(), vec![String::new()]);
    wx.send_event(initial_event, Priority::Normal).await?;
    Ok(())
}
