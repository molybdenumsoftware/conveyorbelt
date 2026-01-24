use std::{
    path::PathBuf,
    process::{ Stdio},
    sync::Arc,
};

use process_wrap::tokio::CommandWrapper;
use tracing::info;
use watchexec::{
    Id as JobId, action::ActionHandler, command::{Command, Program, SpawnOptions}
};

use crate::{
    common::{ForStdoutputLine as _, SERVE_PATH},
    config::Config,
};

pub(crate) fn spawn(action: &mut ActionHandler, config: Arc<Config>) -> JobId {
    let (id, job) = action.create_job(Arc::new(Command {
        program: Program::Exec {
            prog: config.build_command_path.clone(),
            args: Vec::new(),
        },
        options: SpawnOptions::default(),
    }));

    job.set_spawn_hook(move |command,_| {
        command.wrap(CommandWrap{ serve_path: config.serve_dir.path().to_path_buf() });
    });

    id
}

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


