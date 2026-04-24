use std::{convert::Infallible, path::PathBuf, vec::Vec};

use notify::INotifyWatcher;
use rxrust::prelude::*;
use tracing::info;

use crate::{
    browser::Browser,
    common::{SERVE_PATH, StateForTesting, TESTING_MODE},
    driver::{
        browser::BrowserEvent,
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
        initial_build_succeeded: bool,
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
    Terminating {
        server: Option<Server>,
        watcher: Option<INotifyWatcher>,
        browser: Option<Browser>,
    },
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
    Eprintln(String),
    Build(BuildCommand),
    Server(ServerCommand),
    FsWatch(FsWatchCommand),
    BrowserSpawn,
    BrowserReload(Browser),
    Terminate,
}

pub(crate) struct App {
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: ServeDir,
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
            .flat_map(|commands| Shared::from_iter(commands))
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
                    Command::Server(ServerCommand::Spawn(self.serve_dir)),
                    Command::FsWatch(FsWatchCommand::Init(self.project_root)),
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
                Event::Server(ServerEvent::SpawnSuccess(server)),
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
                Event::Server(ServerEvent::SpawnSuccess(server)),
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
                Event::Browser(BrowserEvent::SpawnSuccess(browser)),
            ) => (
                if std::env::var(TESTING_MODE).is_ok() {
                    let state_for_testing = StateForTesting {
                        serve_path: self.serve_dir.path().to_path_buf(),
                        serve_port: server.port().0,
                        browser_debugging_address: browser.debugging_address(),
                        browser_pid: browser.pid().unwrap(),
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
                Event::Browser(BrowserEvent::SpawnError(error)),
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
            (_, Event::Browser(_)) => unreachable!(),
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
                vec![Command::BrowserReload(browser)],
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
        })
    }
}
