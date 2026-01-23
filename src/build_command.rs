use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use tracing::{info, warn};

use crate::common::{DroppyChild, ForStdoutputLine as _, SERVE_PATH};


// TODO seems like this struct might be extraneous
#[derive(Debug, Clone)]
pub struct BuildCommand {
    pub path: PathBuf,
    pub serve_path: PathBuf,
}

impl BuildCommand {
    pub fn new(path: PathBuf, serve_path: PathBuf) -> Self {
        Self {
            path,
            serve_path,
        }
    }

    fn invoke_and_wait(&self) {
        let mut build_command = Command::new(&self.path);

        build_command
            .env(SERVE_PATH, &self.serve_path)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());

        let build_process = match build_command.spawn() {
            Ok(child) => child,
            Err(e) => {
                warn!("{e}: build command failed to spawn: {build_command:?}");
                return;
            }
        };

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

        let build_process_exit_status = match build_process.wait() {
            Ok(status) => status,
            Err(e) => {
                warn!("{e}: failed to obtain build process exit status");
                return;
            }
        };

        if build_process_exit_status.success() {
            info!("build command succeeded");
        } else {
            info!("build command {build_process_exit_status}, {build_command:?}");
        };
    }
}
