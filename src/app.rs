use std::{
    convert::Infallible,
    path::PathBuf,
    process::{Child, ExitStatus},
    rc::Rc,
    sync::Mutex,
    vec::Vec,
};

use rxrust::prelude::*;
use tempfile::TempDir;
use tracing::info;

use crate::{
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser_spawn::BrowserSpawnEvent,
        process_spawn::{ProcessSpawnCommand, ProcessSpawnEvent},
        process_wait::{ProcessWaitCommand, ProcessWaitEvent},
        server::{Server, ServerSpawnCommand, ServerSpawnEvent},
    },
};

#[derive(Debug, Clone)]
enum BuildStateTrio {
    Spawning,
    Waiting,
    Succeeded,
}
#[derive(Debug, Clone)]
enum BuildStateDuo {
    Spawning,
    Waiting,
}

#[derive(Debug, Clone)]
enum ServerSpawnAndInitialBuild {
    Spawning { initial_build: BuildStateTrio },
    Spawned { initial_build: BuildStateDuo },
}

#[derive(Default, Clone, Debug)]
enum State {
    #[default]
    Initial,
    ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild),
    SpawningBrowser,
    Idle,
    Building,
}

#[derive(Debug)]
pub(crate) enum Event {
    Init,
    ServerSpawn(ServerSpawnEvent),
    ProcessSpawn(ProcessSpawnEvent),
    ProcessWait(ProcessWaitEvent),
    BrowserSpawn(BrowserSpawnEvent),
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    ServerSpawn(ServerSpawnCommand),
    ProcessSpawn(ProcessSpawnCommand),
    ProcessWait(ProcessWaitCommand),
    BrowserSpawn,
    Stdout(String),
}

pub(crate) struct App {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: Rc<TempDir>,
    pub(crate) build_command: PathBuf,
}

impl App {
    pub(crate) fn run(
        &self,
        events: LocalBoxedObservable<'static, Event, Infallible>,
    ) -> LocalBoxedObservable<'static, Command, Infallible> {
        events
            .start_with(vec![Event::Init])
            .scan((State::default(), Vec::new()), self.scanner())
            .flat_map(|(_state, commands)| Local::from_iter(commands))
            .tap(|command| info!("command: {command:?}"))
            .box_it()
    }

    fn scanner(&self) -> impl Fn((State, Vec<Command>), Event) -> (State, Vec<Command>) + 'static {
        let build_command = self.build_command.clone();
        let serve_dir = self.serve_dir.clone();
        move |(mut state, mut commands), event| {
            info!("event: {event:?}");
            commands.clear();

            match (&mut state, event) {
                (State::Initial, Event::Init) => {
                    commands.extend([
                        Command::ServerSpawn(ServerSpawnCommand {
                            serve_dir: serve_dir.clone(),
                        }),
                        Command::ProcessSpawn(ProcessSpawnCommand {
                            path: build_command.clone(),
                            envs: vec![(
                                SERVE_PATH.to_string(),
                                serve_dir.path().to_str().unwrap().to_string(),
                            )],
                        }),
                    ]);
                    state =
                        State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::Spawning {
                            initial_build: BuildStateTrio::Spawning,
                        });
                }

                (State::Initial, _) => unreachable!(),

                (_, Event::Init) => unreachable!(),

                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::Spawning {
                        initial_build: initial_build_state @ BuildStateTrio::Spawning,
                    }),
                    Event::ProcessSpawn(ProcessSpawnEvent(Ok(child))),
                ) => {
                    commands.push(Command::ProcessWait(ProcessWaitCommand(child.clone())));
                    *initial_build_state = BuildStateTrio::Waiting;
                }
                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::Spawning {
                        initial_build: initial_build_state @ BuildStateTrio::Waiting,
                    }),
                    Event::ProcessWait(ProcessWaitEvent::Terminated(exit_status)),
                ) => {
                    if exit_status.success() {
                        *initial_build_state = BuildStateTrio::Succeeded;
                    } else {
                        todo!("process failed")
                    }
                }

                (state, event) => {
                    todo!("unhandled event at state:\n{event:#?}\n{state:#?}")
                }
            };

            (state, commands)
        }
    }
}
