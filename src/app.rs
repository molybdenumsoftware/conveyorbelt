use std::{convert::Infallible, path::PathBuf, sync::Arc, vec::Vec};

use notify::INotifyWatcher;
use rxrust::prelude::*;
use tracing::info;

use crate::{
    browser::Browser,
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser::{BrowserCommand, BrowserEvent},
        build::{BuildCommand, BuildEvent},
        fswatch::{FsEvent, FsWatchCommand},
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
    Building {
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
    fn shut_down(
        message: impl Into<String>,
        server: Option<Server>,
        watcher: Option<INotifyWatcher>,
    ) -> (Vec<Command>, State) {
        let mut commands = vec![Command::Eprintln(message.into())];

        let server = if let Some(server) = server {
            commands.push(Command::Server(ServerCommand::Terminate(server)));
            ShuttingDownServerState::Terminating
        } else {
            ShuttingDownServerState::Spawning
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
        (
            vec![
                Command::Eprintln("terminating".to_string()),
                Command::Terminate,
            ],
            State::Terminating,
        )
    }

    fn terminate_error(message: impl Into<String>) -> (Vec<Command>, State) {
        (
            vec![Command::Eprintln(message.into()), Command::Terminate],
            State::Terminating,
        )
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    Init,
    Server(ServerEvent),
    Build(BuildEvent),
    Browser(BrowserEvent),
    Fs(FsEvent),
}

#[derive(Debug)]
pub(crate) enum Command {
    Println(String),
    Eprintln(
        // TODO use Error
        String,
    ),
    Build(BuildCommand),
    Server(ServerCommand),
    FsWatch(FsWatchCommand),
    Browser(BrowserCommand),
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
            .tap(|command| info!("command: {command:?}"))
            .box_it()
    }

    fn event_handler(&self, state: &mut State, event: Event) -> Vec<Command> {
        let build_command = BuildCommand {
            path: self.build_command_path.clone(),
            envs: vec![(
                SERVE_PATH.to_string(),
                self.serve_dir.path().to_str().unwrap().to_string(),
            )],
        };

        info!("event: {event:?}");

        replace_with::replace_with_or_abort_and_return(state, |state| match (state, event) {
            (State::Blank, Event::Init) => (
                vec![
                    Command::Build(build_command),
                    Command::Server(ServerCommand::Spawn(self.serve_dir.clone())),
                    Command::FsWatch(FsWatchCommand::Init(self.project_root.clone())),
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
                Event::Build(BuildEvent::SpawnError(error)),
            ) => State::shut_down(
                format!("could not spawn build command: {error}"),
                server,
                watcher,
            ),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server: Some(server),
                    watcher,
                },
                Event::Build(BuildEvent::WaitError(error)),
            ) => State::shut_down(
                format!("failed to wait on build process termination: {error}"),
                Some(server),
                watcher,
            ),
            (
                State::Initializing {
                    initial_build: InitialBuildState::Pending,
                    server,
                    watcher,
                },
                Event::Build(BuildEvent::TerminatedWithFailure),
            ) => State::shut_down("initial build failed", server, watcher),
            (
                State::Initializing {
                    server, watcher, ..
                },
                Event::Server(ServerEvent::SpawnError(error)),
            ) => State::shut_down(format!("failed to spawn server: {error}"), server, watcher),
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
                Event::Server(ServerEvent::SpawnSuccess(server)),
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
                Event::Server(ServerEvent::SpawnSuccess(server)),
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
                Event::Fs(FsEvent::WatcherCreationError(error)),
            ) => State::shut_down(format!("failed to create watcher {error}"), server, watcher),
            (
                State::Initializing {
                    server,
                    watcher: watcher @ None,
                    ..
                },
                Event::Fs(FsEvent::WatcherWatchError(error)),
            ) => State::shut_down(format!("failed to start watcher {error}"), server, watcher),
            (
                state @ (State::Initializing {
                    watcher: Some(_), ..
                }
                | State::SpawningBrowser { .. }
                | State::Idle { .. }
                | State::Building { .. }),
                Event::Fs(FsEvent::EventError(error)),
            ) => (vec![Command::Eprintln(error.to_string())], state),
            (
                State::SpawningBrowser { server, watcher },
                Event::Browser(BrowserEvent::SpawnSuccess(browser)),
            ) => (
                if std::env::var(TESTING_MODE).is_ok() {
                    let state_for_testing = StateForTesting {
                        serve_path: self.serve_dir.path().to_path_buf(),
                        serve_port: server.address().port(),
                        browser_debugging_address: browser.debugging_address(),
                        browser_pid: browser.pid(),
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
                State::SpawningBrowser {
                    server, watcher, ..
                },
                Event::Browser(BrowserEvent::SpawnError(error)),
            ) => State::shut_down(
                format!("Browser failed to spawn: {error}"),
                Some(server),
                Some(watcher),
            ),
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
            (
                state @ (State::Initializing {
                    initial_build: InitialBuildState::Pending,
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
                    initial_build: InitialBuildState::Pending,
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
            (state, Event::Fs(FsEvent::Event(_))) => (vec![], state),
            (
                State::Building {
                    server,
                    browser,
                    watcher,
                },
                Event::Build(BuildEvent::TerminatedSuccessfully),
            ) => (
                vec![Command::Browser(BrowserCommand::Reload(browser))],
                State::Reloading { server, watcher },
            ),
            (_, Event::Build(_)) => unreachable!(),
            (
                State::Reloading { server, watcher },
                Event::Browser(BrowserEvent::ReloadSuccess(browser)),
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
                Event::Browser(BrowserEvent::ReloadError(browser, error)),
            ) => (
                vec![Command::Eprintln(format!(
                    "failed to reload browser: {error}"
                ))],
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
                },
                Event::Server(ServerEvent::SpawnError(error)),
            ) => State::terminate_error(format!("failed to spawn server: {error}")),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Spawning,
                    watcher,
                },
                Event::Server(ServerEvent::SpawnSuccess(server)),
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
                Event::Server(ServerEvent::TerminationError(error)),
            ) => State::terminate_error(format!("failed to terminate server: {error}")),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                },
                Event::Server(ServerEvent::TerminationError(error)),
            ) => (
                vec![Command::Eprintln(format!(
                    "failed to terminate server: {error}"
                ))],
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
                Event::Server(ServerEvent::TerminationJoinError(error)),
            ) => State::terminate_error(format!("failed to join server task: {error}")),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                },
                Event::Server(ServerEvent::TerminationJoinError(error)),
            ) => (
                vec![Command::Eprintln(format!(
                    "failed to join server task: {error}"
                ))],
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
                Event::Server(ServerEvent::TerminationSuccess),
            ) => State::terminate(),
            (
                State::ShuttingDown {
                    server: ShuttingDownServerState::Terminating,
                    watcher: watcher @ ShuttingDownWatcherState::Spawning,
                },
                Event::Server(ServerEvent::TerminationSuccess),
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
            v @ (_, Event::Fs(_)) => unreachable!("{v:?}"),
        })
    }
}
