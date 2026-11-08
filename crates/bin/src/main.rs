mod browser;
mod build;
mod cli;
#[path = "../common.rs"]
mod common;
mod fswatch;
mod git_toplevel;
mod logging;
mod server;
mod signal;

use anyhow::Context;
use futures::{FutureExt as _, StreamExt};
use rxrust::prelude::*;

use crate::cli::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    let Args { build_command } = crate::cli::parse();

    let git_toplevel = git_toplevel::get().context("get git toplevel")?;

    let mut signal_future = InstallSignalListener.do_logged().await?.receive_f;

    let serve_dir = tokio::select! {
        _ = &mut signal_future => return Ok(()),
        result = ObtainServeDir.do_logged() => result?,
    };

    // Start server, fs watcher, and first build.

    let serve_dir_clone = serve_dir.clone();
    let (server, mut fs_watching, build_spawned) = match select(
        &mut signal_future,
        select(
            Box::pin(
                ServerSpawn {
                    serve_dir: serve_dir_clone,
                }
                .do_logged(),
            ),
            Box::pin(futures::future::try_join(
                FsWatchInit {
                    path: self.project_root,
                }
                .do_logged(),
                BuildSpawn {
                    path: self.build_command_path.clone(),
                    serve_dir: serve_dir.clone(),
                }
                .do_logged(),
            )),
        ),
    )
    .await
    {
        Left((_signal, _)) => return Ok(()),
        Right((Left((Err(_server_spawn), _)), _)) => return Err(1),
        Right((Left((Ok(server), right)), signal)) => {
            tokio::select! {
                _ = signal => {
                    server.shutdown_effect.do_logged().await?;
                    return Ok(());
                },
                result = right => {
                    // TODO: is build_spawned cancellation safe?
                    let (fs_watching, build_spawned) = result?;
                    (server, fs_watching, build_spawned)
                }
            }
        }
        Right((Right((Err(_), _)), _)) => return Err(1),
        Right((Right((Ok((fs_watching, build_spawned)), server_f)), signal)) => {
            tokio::select! {
                _ = signal => return Ok(()),
                result = server_f => {
                    let server = result?;
                    (server, fs_watching, build_spawned)
                }
            }
        }
    };

    // Wait until first build succedes.

    match select(&mut signal_future, Box::pin(build_spawned.wait.do_logged())).await {
        Left((_signal, build_wait)) => {
            let build_terminate = BuildSignal {
                pid: build_spawned.pid,
                signal: nix::sys::signal::SIGTERM,
            }
            .do_logged();
            let server_shutdown = server.shutdown_effect.do_logged();
            join3(build_wait, build_terminate, server_shutdown).await;
            return Ok(());
        }
        Right((Err(_), _)) => {
            server.shutdown_effect.do_logged().await;
            return Err(1);
        }
        Right((Ok(_build_succeeded), _)) => {}
    }

    // Spawn browser.

    let Ok(browser) = BrowserSpawn {
        url: format!("http://{}", server.address),
    }
    .do_logged()
    .await
    else {
        server.shutdown_effect.do_logged().await;
        return Err(1);
    };

    if std::env::var(TESTING_MODE).is_ok() {
        let state_for_testing = StateForTesting {
            serve_path: serve_dir.path().to_path_buf(),
            serve_port: server.address.port(),
            browser_debugging_address: browser.browser.websocket_address().to_string(),
            browser_pid: browser.browser.pid(),
        };
        println!("{state_for_testing}");
    }

    // FS watch

    // Instead of sharing memory keep an Option of a task and share browser handle with it
    let mut build = None;
    while let Some(result) = fs_watching.0.next().await {
        let event = match result {
            Err(e) => {
                warn!("{e}");
                continue;
            }
            Ok(event) => event,
        };
        if event.is_ignored {
            continue;
        }
        match build {
            None => {
                let serve_dir_clone = serve_dir.clone();
                let build_command_path_clone = self.build_command_path.clone();
                build = Some(tokio::spawn(async move {
                    let spawn_result = BuildSpawn {
                        path: build_command_path_clone.clone(),
                        serve_dir: serve_dir_clone.clone(),
                    }
                    .do_logged()
                    .await;

                    let spawn = match spawn_result {
                        Err(e) => {
                            warn!("{e}");
                            return;
                        }
                        Ok(build) => build,
                    };

                    let wait_result = spawn.wait.do_logged().await;

                    if let Err(e) = wait_result {
                        warn!("{e}");
                        return;
                    }
                }));
            }
            Some(_) => todo!(),
        }
    }

    signal_future.await;
    Ok(())
}
