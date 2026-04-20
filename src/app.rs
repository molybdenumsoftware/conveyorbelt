use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{Arc, Mutex},
    vec::Vec,
};

use notify::INotifyWatcher;
use rxrust::prelude::*;
use tracing::info;

use crate::{
    browser::Browser,
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser_spawn::BrowserSpawnEvent,
        build::{BuildCommand, BuildEvent},
        fswatch::{FsEvent, FsWatchCommand},
        server::{ServeDir, Server, ServerSpawnEvent},
    },
};

#[derive(Default, Clone, Debug)]
enum State {
    #[default]
    Blank,
    Initializing {
        initial_build_succeeded: bool,
        server: Option<Arc<Server>>,
        watcher: Option<Arc<Mutex<INotifyWatcher>>>,
    },
    SpawningBrowser {
        server: Arc<Server>,
        watcher: Arc<Mutex<INotifyWatcher>>,
    },
    Idle {
        server: Arc<Server>,
        watcher: Arc<Mutex<INotifyWatcher>>,
        browser: Arc<Mutex<Browser>>,
    },
    Building {
        server: Arc<Server>,
        watcher: Arc<Mutex<INotifyWatcher>>,
        browser: Arc<Mutex<Browser>>,
    },
    Terminating {
        server: Option<Arc<Server>>,
        watcher: Option<Arc<Mutex<INotifyWatcher>>>,
        browser: Option<Arc<Mutex<Browser>>>,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Init,
    ServerSpawn(ServerSpawnEvent),
    Build(BuildEvent),
    BrowserSpawn(BrowserSpawnEvent),
    Fs(FsEvent),
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    Println(String),
    Eprintln(String),
    Build(BuildCommand),
    ServerSpawn(ServeDir),
    FsWatch(FsWatchCommand),
    BrowserSpawn,
    BrowserReload(Arc<Mutex<Browser>>),
    Terminate,
}

pub(crate) struct App {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: ServeDir,
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
        let build_command = BuildCommand {
            path: build_command.clone(),
            envs: vec![(
                SERVE_PATH.to_string(),
                serve_dir.path().to_str().unwrap().to_string(),
            )],
        };

        move |(state, _), event| {
            info!("event: {event:?}");

            let (commands, state) = match (state, event) {
                (State::Blank, Event::Init) => (
                    vec![
                        Command::Build(build_command.clone()),
                        Command::ServerSpawn(serve_dir.clone()),
                        Command::FsWatch(FsWatchCommand::Init(project_root.clone())),
                    ],
                    State::Initializing {
                        initial_build_succeeded: false,
                        server: None,
                        watcher: None,
                    },
                ),

                (State::Blank, _) => unreachable!(),
                (_, Event::Init) => unreachable!(),

                (
                    State::Initializing {
                        initial_build_succeeded: false,
                        server,
                        watcher,
                    },
                    Event::Build(BuildEvent::SpawnError(error)),
                ) => (
                    vec![
                        Command::Eprintln(format!("could not spawn build command: {error}")),
                        Command::Terminate,
                    ],
                    State::Terminating {
                        server,
                        watcher,
                        browser: None,
                    },
                ),
                (
                    State::Initializing {
                        initial_build_succeeded: false,
                        server,
                        watcher,
                    },
                    Event::Build(BuildEvent::WaitError(error)),
                ) => (
                    vec![
                        Command::Eprintln(format!(
                            "failed to wait on build process termination: {error}"
                        )),
                        Command::Terminate,
                    ],
                    State::Terminating {
                        server,
                        watcher,
                        browser: None,
                    },
                ),
                (
                    State::Initializing {
                        initial_build_succeeded: false,
                        server,
                        watcher,
                    },
                    Event::Build(BuildEvent::TerminatedWithFailure),
                ) => (
                    vec![
                        Command::Eprintln("initial build failed".into()),
                        Command::Terminate,
                    ],
                    State::Terminating {
                        server,
                        watcher,
                        browser: None,
                    },
                ),
                (
                    State::Initializing {
                        initial_build_succeeded: false,
                        server: Some(server),
                        watcher: Some(watcher),
                    },
                    Event::Build(BuildEvent::TerminatedSuccessfully),
                )
                | (
                    State::Initializing {
                        initial_build_succeeded: true,
                        server: None,
                        watcher: Some(watcher),
                    },
                    Event::ServerSpawn(ServerSpawnEvent(Ok(server))),
                )
                | (
                    State::Initializing {
                        initial_build_succeeded: true,
                        server: Some(server),
                        watcher: None,
                    },
                    Event::Fs(FsEvent::Watching(watcher)),
                ) => (
                    vec![Command::BrowserSpawn],
                    State::SpawningBrowser { server, watcher },
                ),
                (
                    State::Initializing {
                        initial_build_succeeded: false,
                        server,
                        watcher,
                    },
                    Event::Build(BuildEvent::TerminatedSuccessfully),
                ) => (
                    vec![],
                    State::Initializing {
                        initial_build_succeeded: true,
                        server,
                        watcher,
                    },
                ),
                (
                    State::Initializing {
                        initial_build_succeeded,
                        server: None,
                        watcher,
                    },
                    Event::ServerSpawn(ServerSpawnEvent(Ok(server))),
                ) => (
                    vec![],
                    State::Initializing {
                        initial_build_succeeded,
                        server: Some(server),
                        watcher,
                    },
                ),
                (
                    State::Initializing {
                        server,
                        watcher: None,
                        ..
                    },
                    Event::Fs(FsEvent::WatcherCreationError(error)),
                ) => (
                    vec![
                        Command::Eprintln(format!("failed to create watcher {error}")),
                        Command::Terminate,
                    ],
                    State::Terminating {
                        server,
                        watcher: None,
                        browser: None,
                    },
                ),
                (
                    State::Initializing {
                        server,
                        watcher: None,
                        ..
                    },
                    Event::Fs(FsEvent::WatcherWatchError(error)),
                ) => (
                    vec![
                        Command::Eprintln(format!("failed to start watcher {error}")),
                        Command::Terminate,
                    ],
                    State::Terminating {
                        server,
                        watcher: None,
                        browser: None,
                    },
                ),
                (
                    state @ (State::Initializing {
                        watcher: Some(_), ..
                    }
                    | State::SpawningBrowser { .. }
                    | State::Idle { .. }
                    | State::Building { .. }
                    | State::Terminating { .. }),
                    Event::Fs(FsEvent::EventError(error)),
                ) => (vec![Command::Eprintln(error.to_string())], state),
                (_, Event::Fs(FsEvent::Watching(_))) => unreachable!(),
                (
                    State::SpawningBrowser { server, watcher },
                    Event::BrowserSpawn(BrowserSpawnEvent(Ok(browser))),
                ) => (
                    if std::env::var(TESTING_MODE).is_ok() {
                        let mut browser_lock = browser.lock().unwrap();
                        let state_for_testing = StateForTesting {
                            serve_path: serve_dir.path().to_path_buf(),
                            serve_port: server.port().0,
                            browser_debugging_address: browser_lock.debugging_address(),
                            browser_pid: browser_lock.pid().unwrap(),
                        };
                        vec![Command::Println(format!("{state_for_testing}"))]
                    } else {
                        vec![]
                    },
                    State::Idle {
                        server,
                        watcher,
                        browser,
                    },
                ),
                (
                    State::SpawningBrowser { server, watcher },
                    Event::BrowserSpawn(BrowserSpawnEvent(Err(error))),
                ) => (
                    vec![
                        Command::Eprintln(format!("Browser failed to spawn: {error}")),
                        Command::Terminate,
                    ],
                    State::Terminating {
                        server: Some(server),
                        watcher: Some(watcher),
                        browser: None,
                    },
                ),
                (_, Event::BrowserSpawn(_)) => unreachable!(),
                (
                    State::Idle {
                        server,
                        browser,
                        watcher,
                    },
                    Event::Fs(FsEvent::Event(notify::Event {
                        kind:
                            notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_),
                        ..
                    })),
                ) => (
                    vec![Command::Build(build_command.clone())],
                    State::Building {
                        server,
                        browser,
                        watcher,
                    },
                ),
                (state, Event::Fs(FsEvent::Event(_))) => (vec![], state),
                (
                    State::Building {
                        server,
                        browser,
                        watcher,
                    },
                    Event::Build(BuildEvent::TerminatedSuccessfully),
                ) => (
                    vec![Command::BrowserReload(browser.clone())],
                    State::Idle {
                        server,
                        browser,
                        watcher,
                    },
                ),
                (
                    state @ (State::Initializing {
                        initial_build_succeeded: false,
                        ..
                    }
                    | State::Building { .. }),
                    Event::Build(BuildEvent::Stdoutln(line)),
                ) => (
                    vec![Command::Eprintln(format!(
                        "build process stdout line: {line}"
                    ))],
                    state,
                ),
                (
                    state @ (State::Initializing {
                        initial_build_succeeded: false,
                        ..
                    }
                    | State::Building { .. }),
                    Event::Build(BuildEvent::Stderrln(line)),
                ) => (
                    vec![Command::Eprintln(format!(
                        "build process stderr line: {line}"
                    ))],
                    state,
                ),

                (state, event) => {
                    todo!("unhandled event at state:\n{event:#?}\n{state:#?}")
                }
            };

            (state, commands)
        }
    }
}
