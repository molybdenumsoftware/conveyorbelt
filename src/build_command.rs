use std::{path::PathBuf, process::{Command, Stdio}};

use anyhow::Context as _;
use tracing::info;

use crate::common::{DroppyChild, ForStdoutputLine as _, SERVE_PATH};

#[derive(Debug)]
pub struct BuildCommand {
    path: PathBuf,
    serve_path: PathBuf,
    is_running:
}

impl BuildCommand {
    pub fn new(path: PathBuf, serve_path: PathBuf) -> Self {
        Self { path, serve_path }
    }

    fn invoke(&self) -> anyhow::Result<()> {
        let mut build_command = Command::new(&self.path);

        build_command
            .env(SERVE_PATH, &self.serve_path)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());

        let build_process = build_command
            .spawn()
            .with_context(|| format!("failed to spawn build command {build_command:?}"))
            ?;

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
            .context("failed to obtain build process exit status")
            ?;

        if build_process_exit_status.success() {
            info!("build command succeeded");
        } else {
            info!("build command {build_process_exit_status}, {build_command:?}");
        };

        Ok(())
    }

    pub fn queue_invocation(&mut self) {
        std::thread::spawn(move || {
            info!("build command invocation queued");
        });
    }
}
