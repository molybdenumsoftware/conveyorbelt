use std::{convert::Infallible, path::PathBuf, sync::Arc, vec::Vec};

use nix::{sys::signal::Signal::SIGTERM, unistd::Pid};
use notify::INotifyWatcher;
use rxrust::prelude::*;
use tracing::{info, warn};

use crate::{
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser::{Browser, BrowserCommand, BrowserEvent},
        build::{BuildCommand, BuildEvent},
        fswatch::{FsCommand, FsEvent},
        server::{ServeDir, Server, ServerCommand, ServerEvent},
    },
};

#[derive(Default, Debug)]
enum State {
    #[default]
    Blank,
    Initializing {
        initial_build: InitialBuildState,
        server: Option<Server>,
        watcher: Option<INotifyWatcher>,
    },
    SpawningBrowser {
        server: Server,
        watcher: INotifyWatcher,
    },
    Idle {
        server: Server,
        watcher: INotifyWatcher,
        browser: Browser,
    },
    BuildSpawning {
        server: Server,
        watcher: INotifyWatcher,
        browser: Browser,
    },
    BuildWaiting {
        pid: Pid,
        is_restarting: bool,
        server: Server,
        watcher: INotifyWatcher,
        browser: Browser,
    },
    Reloading {
        server: Server,
        watcher: INotifyWatcher,
    },
    ShuttingDown {
        server: ShuttingDownServerState,
        watcher: ShuttingDownWatcherState,
    },
    Terminating,
}

#[derive(Debug)]
enum InitialBuildState {
    Pending,
    Succeeded,
}

#[derive(Debug)]
enum ShuttingDownServerState {
    Spawning,
    Terminating,
    TerminationFailed,
    Terminated,
}

#[derive(Debug)]
enum ShuttingDownWatcherState {
    Spawning,
    Dropped,
}

impl State {
    fn shut_down(server: Option<Server>, watcher: Option<INotifyWatcher>) -> (Vec<Command>, State) {
        let (commands, server) = if let Some(server) = server {
            (
                vec![Command::Server(ServerCommand::Terminate(server))],
                ShuttingDownServerState::Terminating,
            )
        } else {
            (vec![], ShuttingDownServerState::Spawning)
        };

        let watcher = if let Some(watcher) = watcher {
            drop(watcher);
            ShuttingDownWatcherState::Dropped
        } else {
            ShuttingDownWatcherState::Spawning
        };

        (commands, State::ShuttingDown { server, watcher })
    }

    fn terminate() -> (Vec<Command>, State) {
        (vec![Command::Terminate], State::Terminating)
    }
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum Event {
    #[display("initializing")]
    Init,
    #[display("server: {_0}")]
    Server(ServerEvent),
    #[display("build: {_0}")]
    Build(BuildEvent),
    #[display("browser: {_0}")]
    Browser(BrowserEvent),
    #[display("fs: {_0}")]
    Fs(FsEvent),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum Command {
    #[display("build: {_0}")]
    Build(BuildCommand),
    #[display("server: {_0}")]
    Server(ServerCommand),
    #[display("fs: {_0}")]
    Fs(FsCommand),
    #[display("browser: {_0}")]
    Browser(BrowserCommand),
    #[display("terminate")]
    Terminate,
}

pub(crate) struct App {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: Arc<ServeDir>,
    pub(crate) build_command_path: PathBuf,
}

impl App {
    pub(crate) fn run(
        self,
        events: SharedBoxedObservable<'static, Event, Infallible>,
    ) -> SharedBoxedObservable<'static, Command, Infallible> {
        let mut state = State::Blank;

        events
            .start_with(vec![Event::Init])
            .map(move |event| self.event_handler(&mut state, event))
            .flat_map(Shared::from_iter)
            .tap(|command| info!("command: {command}"))
            .box_it()
    }

    fn event_handler(&self, state: &mut State, event: Event) -> Vec<Command> {
        info!("event: {event}");

        let build_command_path = self.build_command_path.clone();

        replace_with::replace_with_or_abort_and_return(state, |state| match (state, event) {
            (State::Blank, Event::Init) => (
                vec![
                    Command::Build(BuildCommand::Spawn {
                        path: build_command_path.clone(),
                        envs: vec![(
                            SERVE_PATH.to_string(),
                            self.serve_dir.path().to_str().unwrap().to_string(),
                        )],
                    }),
                    Command::Server(ServerCommand::Spawn(self.serve_dir.clone())),
                    Command::Fs(FsCommand::Init(self.project_root.clone())),
                ],
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: None,
                    watcher: None,
                },
            ),

            (State::Blank, _) => unreachable!(),
            (_, Event::Init) => unreachable!(),

            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::SpawnError(_)),
            ) => State::shut_down(server, watcher),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: Some(server),
                    watcher,
                },
                Event::Build(BuildEvent::WaitError(_)),
            ) => State::shut_down(Some(server), watcher),
            (
                state @ State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    ..
                },
                Event::Build(BuildEvent::Spawn(_)),
            ) => (vec![], state),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::TerminatedWithFailure(_)),
            ) => State::shut_down(server, watcher),
            (
                State::Initializing {
                    server, watcher, ..
                },
                Event::Server(ServerEvent::SpawnError(_)),
            ) => State::shut_down(server, watcher),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: Some(server),
                    watcher: Some(watcher),
                },
                Event::Build(BuildEvent::TerminatedSuccessfully),
            )
            | (
                State::Initializing {
                    initial_build: InitialBuildState::Succeeded,
                    server: None,
                    watcher: Some(watcher),
                },
                Event::Server(ServerEvent::Spawn(server)),
            )
            | (
                State::Initializing {
                    initial_build: InitialBuildState::Succeeded,
                    server: Some(server),
                    watcher: None,
                },
                Event::Fs(FsEvent::Watching(watcher)),
            ) => (
                vec![Command::Browser(BrowserCommand::Spawn {
                    url: format!("http://{}", server.address()),
                })],
                State::SpawningBrowser { server, watcher },
            ),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::TerminatedSuccessfully),
            ) => (
                vec![],
                State::Initializing {
                    initial_build: InitialBuildState::Succeeded,
                    server,
                    watcher,
                },
            ),
            (
                State::Initializing {
                    initial_build:
                        initial_build @ (InitialBuildState::Pending | InitialBuildState::Succeeded),
                    server: None,
                    watcher,
                },
                Event::Server(ServerEvent::Spawn(server)),
            ) => (
                vec![],
                State::Initializing {
                    initial_build,
                    server: Some(server),
                    watcher,
                },
            ),
            (
                State::Initializing {
                    server,
                    watcher: watcher @ None,
                    ..
                },
                Event::Fs(FsEvent::WatcherCreationError(_)),
            ) => State::shut_down(server, watcher),
            (
                State::Initializing {
                    initial_build,
                    server,
                    watcher: None,
                },
                Event::Fs(FsEvent::Watching(watcher)),
            ) => (
                vec![],
                State::Initializing {
                    initial_build,
                    server,
                    watcher: Some(watcher),
                },
            ),
            (
                State::Initializing {
                    server,
                    watcher: watcher @ None,
                    ..
                },
                Event::Fs(FsEvent::WatcherWatchError(_)),
            ) => State::shut_down(server, watcher),
            (
                state @ (State::Initializing {
                    watcher: Some(_), ..
                }
                | State::SpawningBrowser { .. }
                | State::Idle { .. }
                | State::BuildSpawning { .. }
                | State::BuildWaiting { .. }),
                Event::Fs(FsEvent::EventError(error)),
            ) => {
                warn!("{error}");
                (vec![], state)
            }
            (
                State::SpawningBrowser { server, watcher },
                Event::Browser(BrowserEvent::Spawn(browser)),
            ) => {
                if std::env::var(TESTING_MODE).is_ok() {
                    let state_for_testing = StateForTesting {
                        serve_path: self.serve_dir.path().to_path_buf(),
                        serve_port: server.address().port(),
                        browser_debugging_address: browser.debugging_address(),
                        browser_pid: browser.pid(),
                    };
                    println!("{state_for_testing}");
                }

                (
                    vec![],
                    State::Idle {
                        server,
                        watcher,
                        browser,
                    },
                )
            }
            (
                State::SpawningBrowser {
                    server, watcher, ..
                },
                Event::Browser(BrowserEvent::SpawnError(_)),
            ) => State::shut_down(Some(server), Some(watcher)),
            (
                State::Idle {
                    server,
                    browser,
                    watcher,
                },
                Event::Fs(FsEvent::Change(_)),
            ) => (
                vec![Command::Build(BuildCommand::Spawn {
                    path: build_command_path.clone(),
                    envs: vec![(
                        SERVE_PATH.to_string(),
                        self.serve_dir.path().to_str().unwrap().to_string(),
                    )],
                })],
                State::BuildSpawning {
                    server,
                    browser,
                    watcher,
                },
            ),
            (
                State::BuildSpawning {
                    server, watcher, ..
                },
                Event::Build(BuildEvent::SpawnError(_)),
            ) => State::shut_down(Some(server), Some(watcher)),
            (
                State::BuildSpawning {
                    server,
                    watcher,
                    browser,
                },
                Event::Build(BuildEvent::Spawn(pid)),
            ) => (
                vec![],
                State::BuildWaiting {
                    pid,
                    is_restarting: false,
                    server,
                    watcher,
                    browser,
                },
            ),
            (
                State::BuildWaiting {
                    pid,
                    is_restarting: false,
                    server,
                    watcher,
                    browser,
                },
                Event::Fs(FsEvent::Change(_)),
            ) => (
                vec![Command::Build(BuildCommand::Signal(pid, SIGTERM))],
                State::BuildWaiting {
                    pid,
                    is_restarting: true,
                    server,
                    watcher,
                    browser,
                },
            ),
            (
                state @ (State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    ..
                }
                | State::BuildSpawning { .. }
                | State::BuildWaiting { .. }),
                Event::Build(BuildEvent::OutputLine { .. }),
            ) => (vec![], state),
            (state, Event::Fs(FsEvent::Change(_))) => (vec![], state),
            (
                State::BuildWaiting {
                    is_restarting: false,
                    server,
                    browser,
                    watcher,
                    ..
                },
                Event::Build(BuildEvent::TerminatedSuccessfully),
            ) => (
                vec![Command::Browser(BrowserCommand::Reload(browser))],
                State::Reloading { server, watcher },
            ),
            (state @ State::BuildWaiting { .. }, Event::Build(BuildEvent::SignalSent(_, _))) => {
                (vec![], state)
            }
            (
                State::BuildWaiting {
                    is_restarting: true,
                    server,
                    watcher,
                    browser,
                    ..
                },
                Event::Build(BuildEvent::TerminatedWithFailure(None)),
            ) => (
                vec![Command::Build(BuildCommand::Spawn {
                    path: build_command_path.clone(),
                    envs: vec![(
                        SERVE_PATH.to_string(),
                        self.serve_dir.path().to_str().unwrap().to_string(),
                    )],
                })],
                State::BuildSpawning {
                    server,
                    watcher,
                    browser,
                },
            ),
            (_, Event::Build(_)) => unreachable!(),
            (
                State::Reloading { server, watcher },
                Event::Browser(BrowserEvent::Reload(browser)),
            ) => (
                vec![],
                State::Idle {
                    server,
                    watcher,
                    browser,
                },
            ),
            (
                State::Reloading { server, watcher },
                Event::Browser(BrowserEvent::ReloadError(browser, ..)),
            ) => (
                vec![],
                State::Idle {
                    server,
                    watcher,
                    browser,
                },
            ),
            (
                state @ (State::SpawningBrowser { .. }
                | State::Idle { .. }
                | State::BuildSpawning { .. }
                | State::BuildWaiting { .. }
                | State::Reloading { .. }
                | State::ShuttingDown {
                    server: _,
                    watcher: _,
                }),
                Event::Browser(BrowserEvent::CdpError(_)),
            ) => (vec![], state),
            (_, Event::Browser(_)) => unreachable!(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Spawning,
                    watcher: _,
                },
                Event::Server(ServerEvent::SpawnError(_)),
            ) => State::terminate(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Spawning,
                    watcher,
                },
                Event::Server(ServerEvent::Spawn(server)),
            ) => (
                vec![Command::Server(ServerCommand::Terminate(server))],
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: ShuttingDownWatcherState::Dropped,
                },
                Event::Server(ServerEvent::TerminationError(_)),
            ) => State::terminate(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                },
                Event::Server(ServerEvent::TerminationError(_)),
            ) => (
                vec![],
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: ShuttingDownWatcherState::Dropped,
                },
                Event::Server(ServerEvent::TaskJoinError(_)),
            ) => State::terminate(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                },
                Event::Server(ServerEvent::TaskJoinError(_)),
            ) => (
                vec![],
                State::ShuttingDown {
                    server: ShuttingDownServerState::TerminationFailed,
                    watcher,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: ShuttingDownWatcherState::Dropped,
                },
                Event::Server(ServerEvent::Termination),
            ) => State::terminate(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                },
                Event::Server(ServerEvent::Termination),
            ) => (
                vec![],
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminated,
                    watcher,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminated,
                    watcher: ShuttingDownWatcherState::Spawning,
                },
                Event::Fs(FsEvent::Watching(watcher)),
            ) => {
                drop(watcher);
                State::terminate()
            }
            (
                State::ShuttingDown {
                    server,
                    watcher: ShuttingDownWatcherState::Spawning,
                },
                Event::Fs(FsEvent::Watching(watcher)),
            ) => {
                drop(watcher);
                (
                    vec![],
                    State::ShuttingDown {
                        server,
                        watcher: ShuttingDownWatcherState::Dropped,
                    },
                )
            }
            (_, Event::Server(_)) => unreachable!(),
            value @ (_, Event::Fs(_)) => unreachable!("{value:#?}"),
        })
    }
}
