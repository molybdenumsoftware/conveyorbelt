use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use anyhow::Context as _;
use tracing::info;

use crate::common::{DroppyChild, ForStdoutputLine as _, SERVE_PATH};

#[derive(Debug, Clone, Copy)]
enum SyncState {
    NotRunning,
    Running,
    RunningAndQueued,
}

#[derive(Debug, Clone)]
pub struct BuildCommand {
    path: PathBuf,
    serve_path: PathBuf,
    sync_state: Arc<Mutex<SyncState>>,
}

impl BuildCommand {
    pub fn new(path: PathBuf, serve_path: PathBuf) -> Self {
        Self {
            path,
            serve_path,
            sync_state: Arc::new(Mutex::new(SyncState::NotRunning)),
        }
    }

    fn invoke_and_wait(&self) -> anyhow::Result<()> {
        let mut build_command = Command::new(&self.path);

        build_command
            .env(SERVE_PATH, &self.serve_path)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());

        let build_process = build_command
            .spawn()
            .with_context(|| format!("failed to spawn build command {build_command:?}"))?;

        info!("build command spawned");

        let mut build_process = DroppyChild::new(build_process);

        build_process
            .for_stdout_line(|line| {
                info!("build command stdout: {line}");
            })
            .unwrap();

        build_process
            .for_stderr_line(|line| {
                info!("build command stderr: {line}");
            })
            .unwrap();

        let build_process_exit_status = build_process
            .wait()
            .context("failed to obtain build process exit status")?;

        if build_process_exit_status.success() {
            info!("build command succeeded");
        } else {
            info!("build command {build_process_exit_status}, {build_command:?}");
        };

        Ok(())
    }

    pub fn invoke_or_queue(&self) {
        let clone = self.clone();

        std::thread::spawn(move || {
            let mut mutex_guard = clone.sync_state.lock().unwrap();

            match *mutex_guard {
                SyncState::NotRunning => {
                    (*mutex_guard) = SyncState::Running;
                    drop(mutex_guard);
                    clone.invoke_and_wait().unwrap();
                    let mut mutex_guard = clone.sync_state.lock().unwrap();

                    match *mutex_guard {
                        SyncState::NotRunning => unreachable!(),
                        SyncState::Running => {
                            *mutex_guard = SyncState::NotRunning;
                            drop(mutex_guard);
                        }
                        SyncState::RunningAndQueued => {
                            drop(mutex_guard);
                            clone.invoke_or_queue();
                        }
                    }
                }
                SyncState::Running => {
                    (*mutex_guard) = SyncState::RunningAndQueued;
                    drop(mutex_guard);
                }
                SyncState::RunningAndQueued => {
                    drop(mutex_guard);
                }
            }
        });
    }
}
