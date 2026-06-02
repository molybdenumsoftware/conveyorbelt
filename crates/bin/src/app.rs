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
        fswatch::{FsChange, FsWatchCommand, FsWatchEvent},
        server::{ServeDir, Server, ServerCommand, ServerEvent},
        signal::{SignalCommand, SignalEvent},
    },
};

#[derive(Default, Debug)]
enum State {
    #[default]
    Blank,
    InstallingSignalHandler,
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
        code: i32,
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
    ShuttingDown,
    ShutDownFail,
    Shutdown,
}

#[derive(Debug)]
enum ShuttingDownWatcherState {
    Spawning,
    Dropped,
}

impl State {
    fn shut_down(
        server: Option<Server>,
        watcher: Option<INotifyWatcher>,
        code: i32,
    ) -> (Vec<Control>, State) {
        let (controls, server) = if let Some(server) = server {
            (
                vec![Control::Command(Command::Server(ServerCommand::Shutdown(
                    server,
                )))],
                ShuttingDownServerState::ShuttingDown,
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

        (
            controls,
            State::ShuttingDown {
                server,
                watcher,
                code,
            },
        )
    }

    fn terminate(code: i32) -> (Vec<Control>, State) {
        (vec![Control::Exit(code)], State::Terminating)
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
    Fs(FsWatchEvent),
    #[display("signal: {_0}")]
    Signal(SignalEvent),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum Control {
    #[display("command: {_0}")]
    Command(Command),
    #[display("exit: {_0}")]
    Exit(i32),
}

#[derive(Debug, derive_more::Display)]
pub(crate) enum Command {
    #[display("build: {_0}")]
    Build(BuildCommand),
    #[display("server: {_0}")]
    Server(ServerCommand),
    #[display("fs: {_0}")]
    Fs(FsWatchCommand),
    #[display("browser: {_0}")]
    Browser(BrowserCommand),
    #[display("signal: {_0}")]
    Signal(SignalCommand),
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
    ) -> SharedBoxedObservable<'static, Control, Infallible> {
        let mut state = State::Blank;

        events
            .start_with(vec![Event::Init])
            .map(move |event| self.event_handler(&mut state, event))
            .flat_map(Shared::from_iter)
            .tap(|control| info!("{control}"))
            .box_it()
    }

    fn event_handler(&self, state: &mut State, event: Event) -> Vec<Control> {
        match &event {
            event @ Event::Fs(FsWatchEvent::EventError(_)) => {
                warn!("event: {event}");
            }
            _ => {
                info!("event: {event}");
            }
        }

        let build_command_path = self.build_command_path.clone();

        replace_with::replace_with_or_abort_and_return(state, |state| match (state, event) {
            (State::Blank, Event::Init) => (
                vec![Control::Command(Command::Signal(
                    SignalCommand::InstallHandler,
                ))],
                State::InstallingSignalHandler,
            ),
            (State::InstallingSignalHandler, Event::Signal(SignalEvent::HandlerInstalled)) => (
                vec![
                    Control::Command(Command::Build(BuildCommand::Spawn {
                        path: build_command_path.clone(),
                        envs: vec![(
                            SERVE_PATH.to_string(),
                            self.serve_dir.path().to_str().unwrap().to_string(),
                        )],
                    })),
                    Control::Command(Command::Server(ServerCommand::Spawn(
                        self.serve_dir.clone(),
                    ))),
                    Control::Command(Command::Fs(FsWatchCommand::Init(self.project_root.clone()))),
                ],
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: None,
                    watcher: None,
                },
            ),
            (State::InstallingSignalHandler, Event::Signal(SignalEvent::HandlerInstallFail(_))) => {
                State::terminate(1)
            }
            (_, Event::Signal(SignalEvent::HandlerInstallFail(_))) => unreachable!(),
            (_, Event::Signal(SignalEvent::HandlerInstalled)) => unreachable!(),
            (State::InstallingSignalHandler, Event::Signal(SignalEvent::Received(_))) => {
                unreachable!()
            }
            (State::Blank, _) => unreachable!(),
            (_, Event::Init) => unreachable!(),

            (
                State::Initializing {
                    server, watcher, ..
                },
                Event::Signal(SignalEvent::Received(_)),
            ) => State::shut_down(server, watcher, 0),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::SpawnError(_)),
            ) => State::shut_down(server, watcher, 1),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: Some(server),
                    watcher,
                },
                Event::Build(BuildEvent::WaitError(_)),
            ) => State::shut_down(Some(server), watcher, 1),
            (
                state @ State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    ..
                },
                Event::Build(BuildEvent::Spawn(_)),
            ) => (vec![], state),
            (
                State::Initializing {
                    server, watcher, ..
                },
                Event::Server(ServerEvent::SpawnError(_)),
            ) => State::shut_down(server, watcher, 1),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: Some(server),
                    watcher: Some(watcher),
                },
                Event::Build(BuildEvent::Exited(Some(0))),
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
                Event::Fs(FsWatchEvent::Watching(watcher)),
            ) => (
                vec![Control::Command(Command::Browser(BrowserCommand::Spawn {
                    url: format!("http://{}", server.address()),
                }))],
                State::SpawningBrowser { server, watcher },
            ),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::Exited(Some(0))),
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
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::Exited(None | Some(_))),
            ) => State::shut_down(server, watcher, 1),
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
                Event::Fs(FsWatchEvent::WatcherCreationError(_)),
            ) => State::shut_down(server, watcher, 1),
            (
                State::Initializing {
                    initial_build,
                    server,
                    watcher: None,
                },
                Event::Fs(FsWatchEvent::Watching(watcher)),
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
                Event::Fs(FsWatchEvent::WatcherWatchError(_)),
            ) => State::shut_down(server, watcher, 1),
            (
                State::SpawningBrowser { server, watcher }
                | State::Idle {
                    server, watcher, ..
                }
                | State::BuildSpawning {
                    server, watcher, ..
                }
                | State::BuildWaiting {
                    server, watcher, ..
                }
                | State::Reloading {
                    server, watcher, ..
                },
                Event::Signal(SignalEvent::Received(_)),
            ) => State::shut_down(Some(server), Some(watcher), 0),
            (
                state @ (State::Initializing {
                    watcher: Some(_), ..
                }
                | State::SpawningBrowser { .. }
                | State::Idle { .. }
                | State::BuildSpawning { .. }
                | State::BuildWaiting { .. }),
                Event::Fs(FsWatchEvent::EventError(_)),
            ) => (vec![], state),
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
            ) => State::shut_down(Some(server), Some(watcher), 1),
            (
                State::Idle {
                    server,
                    browser,
                    watcher,
                },
                Event::Fs(FsWatchEvent::Change(FsChange {
                    is_ignored: false, ..
                })),
            ) => (
                vec![Control::Command(Command::Build(BuildCommand::Spawn {
                    path: build_command_path.clone(),
                    envs: vec![(
                        SERVE_PATH.to_string(),
                        self.serve_dir.path().to_str().unwrap().to_string(),
                    )],
                }))],
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
            ) => State::shut_down(Some(server), Some(watcher), 1),
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
                Event::Fs(FsWatchEvent::Change(FsChange {
                    is_ignored: false, ..
                })),
            ) => (
                vec![Control::Command(Command::Build(BuildCommand::Signal(
                    pid, SIGTERM,
                )))],
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
            (state, Event::Fs(FsWatchEvent::Change(_))) => (vec![], state),
            (
                State::BuildWaiting {
                    is_restarting: false,
                    server,
                    browser,
                    watcher,
                    ..
                },
                Event::Build(BuildEvent::Exited(Some(0))),
            ) => (
                vec![Control::Command(Command::Browser(BrowserCommand::Reload(
                    browser,
                )))],
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
                Event::Build(BuildEvent::Exited(_)),
            ) => (
                vec![Control::Command(Command::Build(BuildCommand::Spawn {
                    path: build_command_path.clone(),
                    envs: vec![(
                        SERVE_PATH.to_string(),
                        self.serve_dir.path().to_str().unwrap().to_string(),
                    )],
                }))],
                State::BuildSpawning {
                    server,
                    watcher,
                    browser,
                },
            ),
            (
                State::BuildWaiting {
                    is_restarting: false,
                    server,
                    watcher,
                    browser,
                    ..
                },
                Event::Build(BuildEvent::Exited(_)),
            ) => (
                vec![],
                State::Idle {
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
            (_, Event::Browser(_)) => unreachable!(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Spawning,
                    watcher: _,
                    ..
                },
                Event::Server(ServerEvent::SpawnError(_)),
            ) => State::terminate(1),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Spawning,
                    watcher,
                    code,
                },
                Event::Server(ServerEvent::Spawn(server)),
            ) => (
                vec![Control::Command(Command::Server(ServerCommand::Shutdown(
                    server,
                )))],
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher,
                    code,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher: ShuttingDownWatcherState::Dropped,
                    ..
                },
                Event::Server(ServerEvent::ShutdownError(_)),
            ) => State::terminate(1),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                    ..
                },
                Event::Server(ServerEvent::ShutdownError(_)),
            ) => (
                vec![],
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher,
                    code: 1,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher: ShuttingDownWatcherState::Dropped,
                    ..
                },
                Event::Server(ServerEvent::TaskJoinError(_)),
            ) => State::terminate(1),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                    ..
                },
                Event::Server(ServerEvent::TaskJoinError(_)),
            ) => (
                vec![],
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShutDownFail,
                    watcher,
                    code: 1,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher: ShuttingDownWatcherState::Dropped,
                    code,
                },
                Event::Server(ServerEvent::Shutdown),
            ) => State::terminate(code),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::ShuttingDown,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                    code,
                },
                Event::Server(ServerEvent::Shutdown),
            ) => (
                vec![],
                State::ShuttingDown {
                    server: ShuttingDownServerState::Shutdown,
                    watcher,
                    code,
                },
            ),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Shutdown,
                    watcher: ShuttingDownWatcherState::Spawning,
                    code,
                },
                Event::Fs(FsWatchEvent::Watching(watcher)),
            ) => {
                drop(watcher);
                State::terminate(code)
            }
            (
                State::ShuttingDown {
                    server,
                    watcher: ShuttingDownWatcherState::Spawning,
                    code,
                },
                Event::Fs(FsWatchEvent::Watching(watcher)),
            ) => {
                drop(watcher);
                (
                    vec![],
                    State::ShuttingDown {
                        server,
                        watcher: ShuttingDownWatcherState::Dropped,
                        code,
                    },
                )
            }
            (state @ State::ShuttingDown { .. }, Event::Signal(SignalEvent::Received(_))) => {
                (vec![], state)
            }
            (State::Terminating, Event::Signal(SignalEvent::Received(_))) => {
                (vec![], State::Terminating)
            }
            (_, Event::Server(_)) => unreachable!(),
            value @ (_, Event::Fs(_)) => unreachable!("{value:#?}"),
        })
    }
}
