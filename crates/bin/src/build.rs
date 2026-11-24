use std::{path::PathBuf, process::Stdio};

use anyhow::{Context, Ok};
use futures::FutureExt;
use nix::{
    sys::signal::{Signal::SIGTERM, kill},
    unistd::Pid,
};
use tokio::{
    process::{Child, Command},
    task,
};
use tracing::{debug, info};

use crate::{
    common::{ForStdoutputLine as _, SERVE_PATH},
    serve_dir::ServeDir,
};

#[derive(Debug)]
pub(crate) struct Build {
    child: Child,
    pub(crate) id: Pid,
    stdout_join_handle: task::JoinHandle<()>,
    stderr_join_handle: task::JoinHandle<()>,
}

#[derive(Clone, derive_more::FromStr, derive_more::Debug)]
#[debug("{_0:?}")]
pub(crate) struct BuildCommandPath(PathBuf);

impl Build {
    pub(crate) async fn spawn(
        serve_dir: ServeDir,
        command: BuildCommandPath,
    ) -> anyhow::Result<Self> {
        debug!("spawning");
        let mut command = Command::new(command.0);

        command
            .env(SERVE_PATH, serve_dir.to_path_buf())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().context("spawn command")?;
        let id = Pid::from_raw(child.id().context("get id")? as i32);

        let stdout_join_handle = child.for_stdout_line(move |line| {
            info!("stdout: {line}");
            async {}.boxed()
        });

        let stderr_join_handle = child.for_stderr_line(move |line| {
            info!("stderr: {line}");
            async {}.boxed()
        });

        info!("spawned {id}");

        Ok(Self {
            child,
            id,
            stdout_join_handle,
            stderr_join_handle,
        })
    }

    pub(crate) async fn wait(self) -> anyhow::Result<bool> {
        let Self {
            mut child,
            stdout_join_handle,
            stderr_join_handle,
            ..
        } = self;

        let id = self.id;

        let code = child
            .wait()
            .await
            .with_context(|| format!("wait {id}"))?
            .code();

        tokio::try_join!(stdout_join_handle, stderr_join_handle).context("join output tasks")?;

        let success = match code {
            None => {
                info!("{id} exit without code");
                false
            }
            Some(code @ (..0 | 1..)) => {
                info!("{id} exit with {code}");
                false
            }
            Some(0) => {
                info!("success");
                true
            }
        };

        Ok(success)
    }

    pub(crate) fn terminate(pid: Pid) {
        info!("SIGTERM {pid}");
        if let Err(e) = kill(pid, SIGTERM) {
            info!("{e}");
        }
    }
}
