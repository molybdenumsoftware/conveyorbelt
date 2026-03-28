use std::{convert::Infallible, path::PathBuf, process::ExitStatus, rc::Rc};

use rxrust::{observable, prelude::*};
use tempfile::TempDir;
use tracing::{debug, info};

use crate::{
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser_spawn::{BrowserSpawnDriverCommand, BrowserSpawnDriverEvent},
        process_spawn::{ProcessSpawnDriverCommand, ProcessSpawnDriverEvent},
        process_wait::{ProcessWaitDriverCommand, ProcessWaitDriverEvent},
        stdout::StdoutDriverCommand,
    },
    server::ServerPort,
};

pub(crate) struct Inputs {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: Rc<TempDir>,
    pub(crate) build_command_path: PathBuf,
    pub(crate) server_port: ServerPort,
    pub(crate) process_spawn_driver_events:
        LocalBoxedObservable<'static, ProcessSpawnDriverEvent, Infallible>,
    pub(crate) process_wait_driver_events:
        LocalBoxedObservable<'static, ProcessWaitDriverEvent, Infallible>,
    pub(crate) browser_spawn_driver_events:
        LocalBoxedObservable<'static, BrowserSpawnDriverEvent, Infallible>,
}

pub(crate) struct Outputs {
    pub(crate) process_spawn_driver_commands:
        LocalBoxedObservable<'static, ProcessSpawnDriverCommand, Infallible>,
    pub(crate) process_wait_driver_commands:
        LocalBoxedObservable<'static, ProcessWaitDriverCommand, Infallible>,
}

#[derive(Default, Clone)]
enum State {
    #[default]
    Initial,
    InitialBuild,
    Idle,
    Building,
}

#[derive(Debug, Clone)]
pub(crate) enum DriverCommand {
    ProcessSpawn(ProcessSpawnDriverCommand),
    BrowserSpawn(BrowserSpawnDriverCommand),
    ProcessWait(ProcessWaitDriverCommand),
    Stdout(StdoutDriverCommand),
}

#[derive(Debug, Clone)]
enum InputEvent {
    Init,
    ProcessSpawn(ProcessSpawnDriverEvent),
    ProcessWait(ProcessWaitDriverEvent),
    BrowserSpawn(BrowserSpawnDriverEvent),
}

pub(crate) type CommandStream = LocalBoxedObservable<'static, DriverCommand, Infallible>;

pub(crate) fn run(inputs: Inputs) -> CommandStream {
    Local::merge_observables([
        inputs
            .process_spawn_driver_events
            .map(InputEvent::ProcessSpawn)
            .box_it(),
        inputs
            .process_wait_driver_events
            .map(InputEvent::ProcessWait)
            .box_it(),
        inputs
            .browser_spawn_driver_events
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
                    commands.push(DriverCommand::ProcessSpawn(ProcessSpawnDriverCommand {
                        path: inputs.build_command_path.clone(),
                        envs: vec![(
                            SERVE_PATH.to_string(),
                            inputs.serve_dir.path().to_str().unwrap().to_string(),
                        )],
                    }));
                    state = State::InitialBuild;
                }
                InputEvent::ProcessSpawn(Ok(child)) => {
                    // TODO maybe log this
                    commands.push(DriverCommand::ProcessWait(ProcessWaitDriverCommand(child)));
                }
                InputEvent::ProcessSpawn(Err(error)) => {
                    // TODO maybe log this
                }
                InputEvent::ProcessWait(ProcessWaitDriverEvent::Terminated(exit_status)) => {
                    if exit_status.success() {
                        state = State::Idle;
                        commands.push(DriverCommand::BrowserSpawn(BrowserSpawnDriverCommand));
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
                        Err(err) => todo!(),
                    };
                    if std::env::var(TESTING_MODE).is_ok() {
                        let mut browser_lock = browser.lock().unwrap();
                        let browser_pid = match browser_lock.pid() {
                            Ok(pid) => pid,
                            Err(err) => todo!(),
                        };
                        let state_for_testing = StateForTesting {
                            serve_path: inputs.serve_dir.path().to_path_buf(),
                            serve_port: inputs.server_port.0,
                            browser_debugging_address: browser_lock.debugging_address(),
                            browser_pid,
                        };

                        info!("{state_for_testing:?}");

                        commands.push(DriverCommand::Stdout(StdoutDriverCommand(format!(
                            "{state_for_testing}\n"
                        ))));
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
