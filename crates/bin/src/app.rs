use std::{convert::Infallible, path::PathBuf};

use rxrust::prelude::*;
use tokio::try_join;

use crate::effects::{
    Effect as _,
    build::BuildSpawn,
    fswatch::FsWatchInit,
    server::{self, ObtainServeDir, ServerSpawn},
    signal::{InstallSignalHandler, SignalInstalled},
};

// #[derive(Default, Debug)]
// enum State {
//     #[default]
//     Blank,
//     InstallingSignalHandler,
//     Initializing {
//         initial_build: InitialBuildState,
//         server: Option<ServerDriver>,
//         watcher: Option<INotifyWatcher>,
//     },
//     SpawningBrowser {
//         server: ServerDriver,
//         watcher: INotifyWatcher,
//     },
//     Idle {
//         server: ServerDriver,
//         watcher: INotifyWatcher,
//         browser: Browser,
//     },
//     BuildSpawning {
//         server: ServerDriver,
//         watcher: INotifyWatcher,
//         browser: Browser,
//     },
//     BuildWaiting {
//         pid: Pid,
//         is_restarting: bool,
//         server: ServerDriver,
//         watcher: INotifyWatcher,
//         browser: Browser,
//     },
//     Reloading {
//         server: ServerDriver,
//         watcher: INotifyWatcher,
//     },
//     ShuttingDown {
//         server: ShuttingDownServerState,
//         watcher: ShuttingDownWatcherState,
//         code: i32,
//     },
//     Terminating,
// }

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

// impl State {
//     fn shut_down(
//         server: Option<ServerDriver>,
//         watcher: Option<INotifyWatcher>,
//         code: i32,
//     ) -> (Vec<Control>, State) {
//         let (controls, server) = if let Some(server) = server {
//             (
//                 vec![Control::Command(Command::Server(ServerCommand::Shutdown(
//                     server,
//                 )))],
//                 ShuttingDownServerState::ShuttingDown,
//             )
//         } else {
//             (vec![], ShuttingDownServerState::Spawning)
//         };
//
//         let watcher = if let Some(watcher) = watcher {
//             drop(watcher);
//             ShuttingDownWatcherState::Dropped
//         } else {
//             ShuttingDownWatcherState::Spawning
//         };
//
//         (
//             controls,
//             State::ShuttingDown {
//                 server,
//                 watcher,
//                 code,
//             },
//         )
//     }
//
//     fn terminate(code: i32) -> (Vec<Control>, State) {
//         (vec![Control::Exit(code)], State::Terminating)
//     }
// }

// #[derive(Debug, derive_more::Display)]
// pub(crate) enum Event {
//     #[display("initializing")]
//     Init,
//     #[display("server: {_0}")]
//     Server(ServerEvent),
//     #[display("build: {_0}")]
//     Build(BuildEvent),
//     #[display("browser: {_0}")]
//     Browser(BrowserEvent),
//     #[display("fs: {_0}")]
//     Fs(FsWatchEvent),
//     #[display("signal: {_0}")]
//     Signal(SignalEvent),
// }

// #[derive(Debug, derive_more::Display)]
// pub(crate) enum Command {
//     #[display("build: {_0}")]
//     Build(BuildCommand),
//     #[display("server: {_0}")]
//     Server(ServerCommand),
//     #[display("fs: {_0}")]
//     Fs(FsWatchCommand),
//     #[display("browser: {_0}")]
//     Browser(BrowserCommand),
//     #[display("signal: {_0}")]
//     Signal(SignalCommand),
// }

pub(crate) struct App {
    // TODO effect
    pub(crate) project_root: PathBuf,
    // TODO effect
    pub(crate) build_command_path: PathBuf,
}

enum Control<T> {
    Exit(i32),
    Happy(T),
}

impl<T: std::fmt::Debug> std::fmt::Debug for Control<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exit(code) => f.debug_tuple("Exit").field(code).finish(),
            Self::Happy(value) => f.debug_tuple("Happy").field(value).finish(),
        }
    }
}

impl<T, E> From<Result<T, E>> for Control<T> {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(ok) => Self::Happy(ok),
            Err(_) => Self::Exit(1),
        }
    }
}

impl<T: Clone> Clone for Control<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Exit(code) => Self::Exit(code.clone()),
            Self::Happy(value) => Self::Happy(value.clone()),
        }
    }
}

impl App {
    pub(crate) fn run(self) -> SharedBoxedObservable<'static, i32, Infallible> {
        use Control::{Exit, Happy};

        let build_command_path = self.build_command_path.clone();
        let project_root = self.project_root.clone();

        InstallSignalHandler
            .call()
            .switch_map(|result| {
                let Ok(SignalInstalled(signal_observable)) = result else {
                    return Shared::of(Exit(1)).box_it();
                };
                let serve_dir = ObtainServeDir.call().map(Control::from);
                let signal = signal_observable.map(|_| Exit(1)).box_it();

                serve_dir.merge(signal).box_it()
            })
            .switch_map(move |control| {
                let Happy(serve_dir) = control else {
                    return Shared::of(Exit(1)).box_it();
                };

                let server_spawn = ServerSpawn {
                    serve_dir: serve_dir.clone(),
                }
                .call();

                let initial_build_spawn = BuildSpawn {
                    path: build_command_path.clone(),
                    serve_dir: serve_dir.clone(),
                }
                .effect();

                let fswatch_init = FsWatchInit {
                    path: project_root.clone(),
                }
                .effect();

                let droppables =
                    Shared::from_future(
                        async move { try_join!(initial_build_spawn, fswatch_init) },
                    )
                    .switch_map(|droppables| {
                        let Ok((build_spawned, fs_watching)) = droppables else {
                            return Shared::of(Exit(1)).box_it();
                        };
                        build_spawned.wait.call()
                    });

                droppables
                    .zip(server_spawn)
                    // TODO should be switch_map?
                    .flat_map(|(droppables, server_running)| {
                        let Ok(server_running) = server_running else {
                            return Shared::of(Exit(1)).box_it();
                        };
                        let Ok((initial_build, fswatched)) = droppables else {
                            return server_running
                                .shutdown_effect
                                .call()
                                .map(|_| Exit(1))
                                .box_it();
                        };
                        Shared::of(Happy((server_running, initial_build, fswatched))).box_it()
                    })
                    .box_it()
            })
            .tap(|v| {
                //
            })
            .filter_map(|control| match control {
                Exit(code) => Some(code),
                Happy(_) => None,
            })
            .box_it()
    }

    // fn event_handler(&self, state: &mut State, event: Event) -> Vec<Control> {
    //     match &event {
    //         event @ Event::Fs(FsWatchEvent::EventError(_)) => {
    //             warn!("event: {event}");
    //         }
    //         _ => {
    //             info!("event: {event}");
    //         }
    //     }
    //
    //     let build_command_path = self.build_command_path.clone();
    //
    //     replace_with::replace_with_or_abort_and_return(state, |state| match (state, event) {
    //         (State::Blank, Event::Init) => (
    //             vec![Control::Command(Command::Signal(
    //                 SignalCommand::InstallHandler,
    //             ))],
    //             State::InstallingSignalHandler,
    //         ),
    //         (State::InstallingSignalHandler, Event::Signal(SignalEvent::HandlerInstalled)) => (
    //             vec![
    //                 Control::Command(Command::Build(BuildCommand::Spawn {
    //                     path: build_command_path.clone(),
    //                     envs: vec![(
    //                         SERVE_PATH.to_string(),
    //                         self.serve_dir.path().to_str().unwrap().to_string(),
    //                     )],
    //                 })),
    //                 Control::Command(Command::Server(ServerCommand::Spawn(
    //                     self.serve_dir.clone(),
    //                 ))),
    //                 Control::Command(Command::Fs(FsWatchCommand::Init(self.project_root.clone()))),
    //             ],
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 server: None,
    //                 watcher: None,
    //             },
    //         ),
    //         (State::InstallingSignalHandler, Event::Signal(SignalEvent::HandlerInstallFail(_))) => {
    //             State::terminate(1)
    //         }
    //         (_, Event::Signal(SignalEvent::HandlerInstallFail(_))) => unreachable!(),
    //         (_, Event::Signal(SignalEvent::HandlerInstalled)) => unreachable!(),
    //         (State::InstallingSignalHandler, Event::Signal(SignalEvent::Received(_))) => {
    //             unreachable!()
    //         }
    //         (State::Blank, _) => unreachable!(),
    //         (_, Event::Init) => unreachable!(),
    //
    //         (
    //             State::Initializing {
    //                 server, watcher, ..
    //             },
    //             Event::Signal(SignalEvent::Received(_)),
    //         ) => State::shut_down(server, watcher, 0),
    //         (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 server,
    //                 watcher,
    //             },
    //             Event::Build(BuildEvent::SpawnError(_)),
    //         ) => State::shut_down(server, watcher, 1),
    //         (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 server: Some(server),
    //                 watcher,
    //             },
    //             Event::Build(BuildEvent::WaitError(_)),
    //         ) => State::shut_down(Some(server), watcher, 1),
    //         (
    //             state @ State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 ..
    //             },
    //             Event::Build(BuildEvent::Spawn(_)),
    //         ) => (vec![], state),
    //         (
    //             State::Initializing {
    //                 server, watcher, ..
    //             },
    //             Event::Server(ServerEvent::SpawnError(_)),
    //         ) => State::shut_down(server, watcher, 1),
    //         (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 server: Some(server),
    //                 watcher: Some(watcher),
    //             },
    //             Event::Build(BuildEvent::Exited(Some(0))),
    //         )
    //         | (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Succeeded,
    //                 server: None,
    //                 watcher: Some(watcher),
    //             },
    //             Event::Server(ServerEvent::Spawn(server)),
    //         )
    //         | (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Succeeded,
    //                 server: Some(server),
    //                 watcher: None,
    //             },
    //             Event::Fs(FsWatchEvent::Watching(watcher)),
    //         ) => (
    //             vec![Control::Command(Command::Browser(BrowserCommand::Spawn {
    //                 url: format!("http://{}", server.address()),
    //             }))],
    //             State::SpawningBrowser { server, watcher },
    //         ),
    //         (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 server,
    //                 watcher,
    //             },
    //             Event::Build(BuildEvent::Exited(Some(0))),
    //         ) => (
    //             vec![],
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Succeeded,
    //                 server,
    //                 watcher,
    //             },
    //         ),
    //         (
    //             State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 server,
    //                 watcher,
    //             },
    //             Event::Build(BuildEvent::Exited(None | Some(_))),
    //         ) => State::shut_down(server, watcher, 1),
    //         (
    //             State::Initializing {
    //                 initial_build:
    //                     initial_build @ (InitialBuildState::Pending | InitialBuildState::Succeeded),
    //                 server: None,
    //                 watcher,
    //             },
    //             Event::Server(ServerEvent::Spawn(server)),
    //         ) => (
    //             vec![],
    //             State::Initializing {
    //                 initial_build,
    //                 server: Some(server),
    //                 watcher,
    //             },
    //         ),
    //         (
    //             State::Initializing {
    //                 server,
    //                 watcher: watcher @ None,
    //                 ..
    //             },
    //             Event::Fs(FsWatchEvent::WatcherCreationError(_)),
    //         ) => State::shut_down(server, watcher, 1),
    //         (
    //             State::Initializing {
    //                 initial_build,
    //                 server,
    //                 watcher: None,
    //             },
    //             Event::Fs(FsWatchEvent::Watching(watcher)),
    //         ) => (
    //             vec![],
    //             State::Initializing {
    //                 initial_build,
    //                 server,
    //                 watcher: Some(watcher),
    //             },
    //         ),
    //         (
    //             State::Initializing {
    //                 server,
    //                 watcher: watcher @ None,
    //                 ..
    //             },
    //             Event::Fs(FsWatchEvent::WatcherWatchError(_)),
    //         ) => State::shut_down(server, watcher, 1),
    //         (
    //             State::SpawningBrowser { server, watcher }
    //             | State::Idle {
    //                 server, watcher, ..
    //             }
    //             | State::BuildSpawning {
    //                 server, watcher, ..
    //             }
    //             | State::BuildWaiting {
    //                 server, watcher, ..
    //             }
    //             | State::Reloading {
    //                 server, watcher, ..
    //             },
    //             Event::Signal(SignalEvent::Received(_)),
    //         ) => State::shut_down(Some(server), Some(watcher), 0),
    //         (
    //             state @ (State::Initializing {
    //                 watcher: Some(_), ..
    //             }
    //             | State::SpawningBrowser { .. }
    //             | State::Idle { .. }
    //             | State::BuildSpawning { .. }
    //             | State::BuildWaiting { .. }),
    //             Event::Fs(FsWatchEvent::EventError(_)),
    //         ) => (vec![], state),
    //         (
    //             State::SpawningBrowser { server, watcher },
    //             Event::Browser(BrowserEvent::Spawn(browser)),
    //         ) => {
    //             if std::env::var(TESTING_MODE).is_ok() {
    //                 let state_for_testing = StateForTesting {
    //                     serve_path: self.serve_dir.path().to_path_buf(),
    //                     serve_port: server.address().port(),
    //                     browser_debugging_address: browser.debugging_address(),
    //                     browser_pid: browser.pid(),
    //                 };
    //                 println!("{state_for_testing}");
    //             }
    //
    //             (
    //                 vec![],
    //                 State::Idle {
    //                     server,
    //                     watcher,
    //                     browser,
    //                 },
    //             )
    //         }
    //         (
    //             State::SpawningBrowser {
    //                 server, watcher, ..
    //             },
    //             Event::Browser(BrowserEvent::SpawnError(_)),
    //         ) => State::shut_down(Some(server), Some(watcher), 1),
    //         (
    //             State::Idle {
    //                 server,
    //                 browser,
    //                 watcher,
    //             },
    //             Event::Fs(FsWatchEvent::Change(FsChange {
    //                 is_ignored: false, ..
    //             })),
    //         ) => (
    //             vec![Control::Command(Command::Build(BuildCommand::Spawn {
    //                 path: build_command_path.clone(),
    //                 envs: vec![(
    //                     SERVE_PATH.to_string(),
    //                     self.serve_dir.path().to_str().unwrap().to_string(),
    //                 )],
    //             }))],
    //             State::BuildSpawning {
    //                 server,
    //                 browser,
    //                 watcher,
    //             },
    //         ),
    //         (
    //             State::BuildSpawning {
    //                 server, watcher, ..
    //             },
    //             Event::Build(BuildEvent::SpawnError(_)),
    //         ) => State::shut_down(Some(server), Some(watcher), 1),
    //         (
    //             State::BuildSpawning {
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //             Event::Build(BuildEvent::Spawn(pid)),
    //         ) => (
    //             vec![],
    //             State::BuildWaiting {
    //                 pid,
    //                 is_restarting: false,
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //         ),
    //         (
    //             State::BuildWaiting {
    //                 pid,
    //                 is_restarting: false,
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //             Event::Fs(FsWatchEvent::Change(FsChange {
    //                 is_ignored: false, ..
    //             })),
    //         ) => (
    //             vec![Control::Command(Command::Build(BuildCommand::Signal(
    //                 pid, SIGTERM,
    //             )))],
    //             State::BuildWaiting {
    //                 pid,
    //                 is_restarting: true,
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //         ),
    //         (
    //             state @ (State::Initializing {
    //                 initial_build: InitialBuildState::Pending,
    //                 ..
    //             }
    //             | State::BuildSpawning { .. }
    //             | State::BuildWaiting { .. }),
    //             Event::Build(BuildEvent::OutputLine { .. }),
    //         ) => (vec![], state),
    //         (state, Event::Fs(FsWatchEvent::Change(_))) => (vec![], state),
    //         (
    //             State::BuildWaiting {
    //                 is_restarting: false,
    //                 server,
    //                 browser,
    //                 watcher,
    //                 ..
    //             },
    //             Event::Build(BuildEvent::Exited(Some(0))),
    //         ) => (
    //             vec![Control::Command(Command::Browser(BrowserCommand::Reload(
    //                 browser,
    //             )))],
    //             State::Reloading { server, watcher },
    //         ),
    //         (state @ State::BuildWaiting { .. }, Event::Build(BuildEvent::SignalSent(_, _))) => {
    //             (vec![], state)
    //         }
    //         (
    //             State::BuildWaiting {
    //                 is_restarting: true,
    //                 server,
    //                 watcher,
    //                 browser,
    //                 ..
    //             },
    //             Event::Build(BuildEvent::Exited(_)),
    //         ) => (
    //             vec![Control::Command(Command::Build(BuildCommand::Spawn {
    //                 path: build_command_path.clone(),
    //                 envs: vec![(
    //                     SERVE_PATH.to_string(),
    //                     self.serve_dir.path().to_str().unwrap().to_string(),
    //                 )],
    //             }))],
    //             State::BuildSpawning {
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //         ),
    //         (
    //             State::BuildWaiting {
    //                 is_restarting: false,
    //                 server,
    //                 watcher,
    //                 browser,
    //                 ..
    //             },
    //             Event::Build(BuildEvent::Exited(_)),
    //         ) => (
    //             vec![],
    //             State::Idle {
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //         ),
    //         (_, Event::Build(_)) => unreachable!(),
    //         (
    //             State::Reloading { server, watcher },
    //             Event::Browser(BrowserEvent::Reload(browser)),
    //         ) => (
    //             vec![],
    //             State::Idle {
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //         ),
    //         (
    //             State::Reloading { server, watcher },
    //             Event::Browser(BrowserEvent::ReloadError(browser, ..)),
    //         ) => (
    //             vec![],
    //             State::Idle {
    //                 server,
    //                 watcher,
    //                 browser,
    //             },
    //         ),
    //         (_, Event::Browser(_)) => unreachable!(),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::Spawning,
    //                 watcher: _,
    //                 ..
    //             },
    //             Event::Server(ServerEvent::SpawnError(_)),
    //         ) => State::terminate(1),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::Spawning,
    //                 watcher,
    //                 code,
    //             },
    //             Event::Server(ServerEvent::Spawn(server)),
    //         ) => (
    //             vec![Control::Command(Command::Server(ServerCommand::Shutdown(
    //                 server,
    //             )))],
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher,
    //                 code,
    //             },
    //         ),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher: ShuttingDownWatcherState::Dropped,
    //                 ..
    //             },
    //             Event::Server(ServerEvent::ShutdownError(_)),
    //         ) => State::terminate(1),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher: watcher @ ShuttingDownWatcherState::Spawning,
    //                 ..
    //             },
    //             Event::Server(ServerEvent::ShutdownError(_)),
    //         ) => (
    //             vec![],
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher,
    //                 code: 1,
    //             },
    //         ),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher: ShuttingDownWatcherState::Dropped,
    //                 ..
    //             },
    //             Event::Server(ServerEvent::TaskJoinError(_)),
    //         ) => State::terminate(1),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher: watcher @ ShuttingDownWatcherState::Spawning,
    //                 ..
    //             },
    //             Event::Server(ServerEvent::TaskJoinError(_)),
    //         ) => (
    //             vec![],
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShutDownFail,
    //                 watcher,
    //                 code: 1,
    //             },
    //         ),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher: ShuttingDownWatcherState::Dropped,
    //                 code,
    //             },
    //             Event::Server(ServerEvent::Shutdown),
    //         ) => State::terminate(code),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::ShuttingDown,
    //                 watcher: watcher @ ShuttingDownWatcherState::Spawning,
    //                 code,
    //             },
    //             Event::Server(ServerEvent::Shutdown),
    //         ) => (
    //             vec![],
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::Shutdown,
    //                 watcher,
    //                 code,
    //             },
    //         ),
    //         (
    //             State::ShuttingDown {
    //                 server: ShuttingDownServerState::Shutdown,
    //                 watcher: ShuttingDownWatcherState::Spawning,
    //                 code,
    //             },
    //             Event::Fs(FsWatchEvent::Watching(watcher)),
    //         ) => {
    //             drop(watcher);
    //             State::terminate(code)
    //         }
    //         (
    //             State::ShuttingDown {
    //                 server,
    //                 watcher: ShuttingDownWatcherState::Spawning,
    //                 code,
    //             },
    //             Event::Fs(FsWatchEvent::Watching(watcher)),
    //         ) => {
    //             drop(watcher);
    //             (
    //                 vec![],
    //                 State::ShuttingDown {
    //                     server,
    //                     watcher: ShuttingDownWatcherState::Dropped,
    //                     code,
    //                 },
    //             )
    //         }
    //         (state @ State::ShuttingDown { .. }, Event::Signal(SignalEvent::Received(_))) => {
    //             (vec![], state)
    //         }
    //         (State::Terminating, Event::Signal(SignalEvent::Received(_))) => {
    //             (vec![], State::Terminating)
    //         }
    //         (_, Event::Server(_)) => unreachable!(),
    //         value @ (_, Event::Fs(_)) => unreachable!("{value:#?}"),
    //     })
    // }
}
