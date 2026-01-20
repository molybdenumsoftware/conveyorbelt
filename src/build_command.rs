use std::{path::PathBuf, process::Stdio};

use anyhow::Context as _;
use tokio::process::Command;
use tracing::info;

use crate::common::{ForStdoutputLine as _, SERVE_PATH};

#[derive(Debug, Clone)]
pub struct BuildCommand {
    path: PathBuf,
    serve_path: PathBuf,
}

impl BuildCommand {
    pub fn new(path: PathBuf, serve_path: PathBuf) -> Self {
        Self { path, serve_path }
    }

    pub async fn invoke(&self) -> anyhow::Result<()> {
        let mut build_command = Command::new(&self.path);

        build_command
            .env(SERVE_PATH, &self.serve_path)
            .kill_on_drop(true)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());

        let mut build_process = build_command
            .spawn()
            .with_context(|| format!("failed to spawn build command {build_command:?}"))
            .unwrap();

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
            .await
            .context("failed to obtain build process exit status")
            .unwrap();

        if build_process_exit_status.success() {
            info!("build command succeeded");
        } else {
            info!("build command {build_command:?} exited with {build_process_exit_status}");
        };

        Ok(())
    }
}
