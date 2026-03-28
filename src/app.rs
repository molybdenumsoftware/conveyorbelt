use std::{convert::Infallible, path::PathBuf, rc::Rc};

use rxrust::prelude::*;
use tempfile::TempDir;
use tracing::info;

use crate::{
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser_spawn::BrowserSpawnDriverEvent,
        process_spawn::{ProcessSpawnCommand, ProcessSpawnDriverEvent},
        process_wait::{ProcessWaitCommand, ProcessWaitDriverEvent},
    },
    server::ServerPort,
};

#[derive(Default, Clone)]
enum State {
    #[default]
    Initial,
    InitialBuild,
    Idle,
    Building,
}

#[derive(Debug, Clone)]
enum InputEvent {
    Init,
    ProcessSpawn(ProcessSpawnDriverEvent),
    ProcessWait(ProcessWaitDriverEvent),
    BrowserSpawn(BrowserSpawnDriverEvent),
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    ProcessSpawn(ProcessSpawnCommand),
    ProcessWait(ProcessWaitCommand),
    BrowserSpawn,
    Stdout(String),
}

pub(crate) struct App {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: Rc<TempDir>,
    pub(crate) build_command: PathBuf,
    pub(crate) server_port: ServerPort,
    pub(crate) process_spawn_driver_events:
        LocalBoxedObservable<'static, ProcessSpawnDriverEvent, Infallible>,
    pub(crate) process_wait_driver_events:
        LocalBoxedObservable<'static, ProcessWaitDriverEvent, Infallible>,
    pub(crate) browser_spawn_driver_events:
        LocalBoxedObservable<'static, BrowserSpawnDriverEvent, Infallible>,
}

impl App {
    pub(crate) fn run(self) -> LocalBoxedObservable<'static, Command, Infallible> {
        Local::merge_observables([
            self.process_spawn_driver_events
                .map(InputEvent::ProcessSpawn)
                .box_it(),
            self.process_wait_driver_events
                .map(InputEvent::ProcessWait)
                .box_it(),
            self.browser_spawn_driver_events
                .map(InputEvent::BrowserSpawn)
                .box_it(),
        ])
        .start_with(vec![InputEvent::Init])
        .scan(
            (State::default(), Vec::new()),
            move |(mut state, mut commands), input_event| {
                info!("event: {input_event:?}");
                commands.clear();

                match input_event {
                    InputEvent::Init => {
                        commands.push(Command::ProcessSpawn(ProcessSpawnCommand {
                            path: self.build_command.clone(),
                            envs: vec![(
                                SERVE_PATH.to_string(),
                                self.serve_dir.path().to_str().unwrap().to_string(),
                            )],
                        }));
                        state = State::InitialBuild;
                    }
                    InputEvent::ProcessSpawn(Ok(child)) => {
                        // TODO maybe log this
                        commands.push(Command::ProcessWait(ProcessWaitCommand(child)));
                    }
                    InputEvent::ProcessSpawn(Err(_error)) => {
                        // TODO maybe log this
                    }
                    InputEvent::ProcessWait(ProcessWaitDriverEvent::Terminated(exit_status)) => {
                        if exit_status.success() {
                            state = State::Idle;
                            commands.push(Command::BrowserSpawn);
                        }
                    }
                    InputEvent::ProcessWait(ProcessWaitDriverEvent::FailedToWait(_)) => {
                        // TODO
                    }
                    InputEvent::ProcessWait(ProcessWaitDriverEvent::StdoutLine(_)) => {
                        // TODO
                    }
                    InputEvent::ProcessWait(ProcessWaitDriverEvent::StderrLine(_)) => {
                        // TODO
                    }
                    InputEvent::BrowserSpawn(BrowserSpawnDriverEvent(result)) => {
                        let browser = match result {
                            Ok(browser) => browser,
                            Err(_err) => todo!(),
                        };
                        if std::env::var(TESTING_MODE).is_ok() {
                            let mut browser_lock = browser.lock().unwrap();
                            let browser_pid = match browser_lock.pid() {
                                Ok(pid) => pid,
                                Err(_err) => todo!(),
                            };
                            let state_for_testing = StateForTesting {
                                serve_path: self.serve_dir.path().to_path_buf(),
                                serve_port: self.server_port.0,
                                browser_debugging_address: browser_lock.debugging_address(),
                                browser_pid,
                            };

                            info!("{state_for_testing:?}");

                            commands.push(Command::Stdout(format!("{state_for_testing}\n")));
                        }
                    }
                }

                (state, commands)
            },
        )
        .concat_map(|(_state, commands)| Local::from_iter(commands))
        .tap(|command| info!("command: {command:?}"))
        .box_it()
    }
}
