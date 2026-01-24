use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use process_wrap::tokio::CommandWrapper;
use tracing::{info, warn};

use crate::common::{DroppyChild, ForStdoutputLine as _, SERVE_PATH};

#[derive(Debug, Clone)]
pub(crate) struct CommandWrap {
    serve_path: PathBuf,
}

impl CommandWrapper for CommandWrap {
    fn pre_spawn(
        &mut self,
        command: &mut tokio::process::Command,
        _core: &process_wrap::tokio::CommandWrap,
    ) -> std::io::Result<()> {
        command
            .env(SERVE_PATH, self.serve_path.as_path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(())
    }

    fn post_spawn(
        &mut self,
        _command: &mut tokio::process::Command,
        child: &mut tokio::process::Child,
        _core: &process_wrap::tokio::CommandWrap,
    ) -> std::io::Result<()> {
        child
            .for_stdout_line(|line| {
                info!("build command stdout: {line}");
            })
            .unwrap();

        child
            .for_stderr_line(|line| {
                info!("build command stderr: {line}");
            })
            .unwrap();
        Ok(())
    }
}


// TODO seems like this struct might be extraneous
#[derive(Debug, Clone)]
pub(crate) struct BuildCommand {
    pub(crate) path: PathBuf,
    pub(crate) serve_path: PathBuf,
}

impl BuildCommand {
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
