use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{Arc, Mutex},
    vec::Vec,
};

use notify::INotifyWatcher;
use rxrust::prelude::*;
use tempfile::TempDir;
use tracing::info;

use crate::{
    browser::Browser,
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser_spawn::BrowserSpawnEvent,
        fswatch::{FsWatchCommand, FsWatchEvent},
        process_spawn::{ProcessSpawnCommand, ProcessSpawnEvent},
        process_wait::{ProcessWaitCommand, ProcessWaitEvent},
        server::{Server, ServerSpawnCommand, ServerSpawnEvent},
    },
};

#[derive(Debug, Clone)]
enum ServerSpawnAndInitialBuild {
    Nothing,
    ServerRunning(Arc<Server>),
    ServerRunningAndBuildWaiting(Arc<Server>),
    BuildWaiting,
    BuildSucceeded,
}

#[derive(Default, Clone, Debug)]
enum State {
    #[default]
    Initial,
    ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild),
    SpawningBrowser {
        server: Arc<Server>,
    },
    InitializingFsWatch {
        server: Arc<Server>,
        browser: Arc<Mutex<Browser>>,
    },
    Idle {
        server: Arc<Server>,
        browser: Arc<Mutex<Browser>>,
        watcher: Arc<Mutex<INotifyWatcher>>,
    },
    BuildCommandSpawning {
        server: Arc<Server>,
        browser: Arc<Mutex<Browser>>,
        watcher: Arc<Mutex<INotifyWatcher>>,
    },
    BuildCommandWaiting {
        server: Arc<Server>,
        browser: Arc<Mutex<Browser>>,
        watcher: Arc<Mutex<INotifyWatcher>>,
    },
    Terminating(i32),
}

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Init,
    ServerSpawn(ServerSpawnEvent),
    ProcessSpawn(ProcessSpawnEvent),
    ProcessWait(ProcessWaitEvent),
    BrowserSpawn(BrowserSpawnEvent),
    FsWatch(FsWatchEvent),
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    ServerSpawn(ServerSpawnCommand),
    ProcessSpawn(ProcessSpawnCommand),
    ProcessWait(ProcessWaitCommand),
    BrowserSpawn,
    Stdout(String),
    Stderr(String),
    Terminate(i32),
    FsWatch(FsWatchCommand),
}

pub(crate) struct App {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: Arc<TempDir>,
    pub(crate) build_command: PathBuf,
}

impl App {
    pub(crate) fn run(
        &self,
        events: SharedBoxedObservable<'static, Event, Infallible>,
    ) -> SharedBoxedObservable<'static, Command, Infallible> {
        events
            .start_with(vec![Event::Init])
            .scan((State::default(), Vec::new()), self.scanner())
            .flat_map(|(_state, commands)| Shared::from_iter(commands))
            .tap(|command| info!("command: {command:?}"))
            .box_it()
    }

    fn scanner(&self) -> impl Fn((State, Vec<Command>), Event) -> (State, Vec<Command>) + 'static {
        let build_command = self.build_command.clone();
        let serve_dir = self.serve_dir.clone();
        let project_root = self.project_root.clone();
        let process_spawn_command = ProcessSpawnCommand {
            path: build_command.clone(),
            envs: vec![(
                SERVE_PATH.to_string(),
                serve_dir.path().to_str().unwrap().to_string(),
            )],
        };

        move |(mut state, mut commands), event| {
            info!("event: {event:?}");
            commands.clear();

            match (&mut state, event) {
                (State::Initial, Event::Init) => {
                    commands.extend([
                        Command::ServerSpawn(ServerSpawnCommand {
                            serve_dir: serve_dir.clone(),
                        }),
                        Command::ProcessSpawn(process_spawn_command.clone()),
                    ]);
                    state = State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::Nothing);
                }

                (State::Initial, _) => unreachable!(),

                (_, Event::Init) => unreachable!(),

                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::Nothing),
                    Event::ProcessSpawn(ProcessSpawnEvent(Ok(child))),
                ) => {
                    commands.push(Command::ProcessWait(ProcessWaitCommand(child.clone())));
                    state =
                        State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::BuildWaiting);
                }
                (
                    State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::Nothing
                        | ServerSpawnAndInitialBuild::ServerRunning(_),
                    ),
                    Event::ProcessSpawn(ProcessSpawnEvent(Err(error))),
                ) => {
                    const CODE: i32 = 1;
                    commands.extend([
                        Command::Stderr(format!("build command failed to spawn: {error}\n")),
                        Command::Terminate(CODE),
                    ]);
                    state = State::Terminating(CODE)
                }
                (
                    State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::ServerRunningAndBuildWaiting(_)
                        | ServerSpawnAndInitialBuild::BuildWaiting
                        | ServerSpawnAndInitialBuild::BuildSucceeded,
                    ),
                    Event::ProcessSpawn(ProcessSpawnEvent(Err(_))),
                ) => {
                    unreachable!()
                }
                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::BuildWaiting),
                    Event::ProcessWait(ProcessWaitEvent::TerminatedSuccessfully),
                ) => {
                    state = State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::BuildSucceeded,
                    )
                }
                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::BuildSucceeded),
                    Event::ServerSpawn(ServerSpawnEvent(Ok(server))),
                ) => {
                    commands.push(Command::BrowserSpawn);
                    state = State::SpawningBrowser { server };
                }
                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::Nothing),
                    Event::ServerSpawn(ServerSpawnEvent(Ok(server))),
                ) => {
                    state = State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::ServerRunning(server),
                    )
                }
                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::BuildWaiting),
                    Event::ServerSpawn(ServerSpawnEvent(Ok(server))),
                ) => {
                    state = State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::ServerRunningAndBuildWaiting(server),
                    )
                }
                (
                    State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::ServerRunningAndBuildWaiting(_),
                    ),
                    Event::ProcessSpawn(ProcessSpawnEvent(Ok(_child))),
                ) => {
                    unreachable!()
                }
                (
                    State::ServerSpawnAndInitialBuild(ServerSpawnAndInitialBuild::ServerRunning(
                        server,
                    )),
                    Event::ProcessSpawn(ProcessSpawnEvent(Ok(child))),
                ) => {
                    commands.push(Command::ProcessWait(ProcessWaitCommand(child)));
                    state = State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::ServerRunningAndBuildWaiting(server.clone()),
                    );
                }
                (
                    State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::ServerRunningAndBuildWaiting(server),
                    ),
                    Event::ProcessWait(ProcessWaitEvent::TerminatedSuccessfully),
                ) => {
                    commands.push(Command::BrowserSpawn);
                    state = State::SpawningBrowser {
                        server: server.clone(),
                    };
                }
                (
                    State::ServerSpawnAndInitialBuild(
                        ServerSpawnAndInitialBuild::BuildWaiting
                        | ServerSpawnAndInitialBuild::ServerRunningAndBuildWaiting(_),
                    ),
                    Event::ProcessWait(ProcessWaitEvent::TerminatedWithFailure),
                ) => {
                    const CODE: i32 = 1;
                    commands.extend([
                        Command::Stderr("initial build failed".into()),
                        Command::Terminate(CODE),
                    ]);
                    state = State::Terminating(CODE);
                }
                (
                    State::SpawningBrowser { server },
                    Event::BrowserSpawn(BrowserSpawnEvent(Ok(browser))),
                ) => {
                    if std::env::var(TESTING_MODE).is_ok() {
                        let mut browser_lock = browser.lock().unwrap();
                        let state_for_testing = StateForTesting {
                            serve_path: serve_dir.path().to_path_buf(),
                            serve_port: server.port().0,
                            browser_debugging_address: browser_lock.debugging_address(),
                            browser_pid: browser_lock.pid().unwrap(),
                        };
                        commands.push(Command::Stdout(format!("{state_for_testing}\n")));
                    }
                    commands.push(Command::FsWatch(FsWatchCommand::Init(project_root.clone())));
                    state = State::InitializingFsWatch {
                        server: server.clone(),
                        browser,
                    };
                }
                (
                    State::SpawningBrowser { .. },
                    Event::BrowserSpawn(BrowserSpawnEvent(Err(error))),
                ) => {
                    const CODE: i32 = 1;
                    commands.extend([
                        Command::Stderr(format!("Browser failed to spawn: {error}")),
                        Command::Terminate(CODE),
                    ]);
                    state = State::Terminating(CODE);
                }
                (_, Event::BrowserSpawn(_)) => {
                    unreachable!();
                }
                (
                    State::InitializingFsWatch { server, browser },
                    Event::FsWatch(FsWatchEvent::Watching(watcher)),
                ) => {
                    state = State::Idle {
                        server: server.clone(),
                        browser: browser.clone(),
                        watcher,
                    }
                }
                (_, Event::FsWatch(FsWatchEvent::Watching(_))) => {
                    unreachable!();
                }
                (
                    State::Idle {
                        server,
                        browser,
                        watcher,
                    }
                    | State::BuildCommandSpawning {
                        server,
                        browser,
                        watcher,
                    }
                    | State::BuildCommandWaiting {
                        server,
                        browser,
                        watcher,
                    },
                    Event::FsWatch(FsWatchEvent::Event(notify::Event {
                        kind:
                            notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_),
                        paths,
                        ..
                    })),
                ) => {
                    commands.push(Command::ProcessSpawn(process_spawn_command.clone()));
                    state = State::BuildCommandSpawning {
                        server: server.clone(),
                        browser: browser.clone(),
                        watcher: watcher.clone(),
                    };
                }
                (
                    State::Idle {
                        server,
                        browser,
                        watcher,
                    }
                    | State::BuildCommandSpawning {
                        server,
                        browser,
                        watcher,
                    }
                    | State::BuildCommandWaiting {
                        server,
                        browser,
                        watcher,
                    },
                    Event::FsWatch(FsWatchEvent::Event(_)),
                ) => {}
                (state, event @ Event::FsWatch(FsWatchEvent::Event(_))) => {
                    dbg!(state, event);
                    unreachable!();
                }
                (
                    State::BuildCommandSpawning {
                        server,
                        browser,
                        watcher,
                    },
                    Event::ProcessSpawn(ProcessSpawnEvent(Ok(child))),
                ) => {
                    commands.push(Command::ProcessWait(ProcessWaitCommand(child)));
                    state = State::BuildCommandWaiting {
                        server: server.clone(),
                        browser: browser.clone(),
                        watcher: watcher.clone(),
                    };
                }
                (_, Event::ProcessSpawn(_)) => {
                    unreachable!();
                }
                (
                    State::BuildCommandWaiting {
                        server,
                        browser,
                        watcher,
                    },
                    Event::ProcessWait(ProcessWaitEvent::TerminatedSuccessfully),
                ) => {
                    state = State::Idle {
                        server: server.clone(),
                        browser: browser.clone(),
                        watcher: watcher.clone(),
                    };
                }

                (state, event) => {
                    todo!("unhandled event at state:\n{event:#?}\n{state:#?}")
                }
            };

            (state, commands)
        }
    }
}
