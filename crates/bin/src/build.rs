use std::{path::PathBuf, process::Stdio};

use anyhow::Context;
use futures::FutureExt;
use nix::unistd::Pid;
use tokio::{
    process::{Child, Command},
    task,
};
use tracing::info;

use crate::{
    common::{ForStdoutputLine as _, SERVE_PATH},
    server::ServeDir,
};

// TODO #[display("spawn {path:?} with serve dir {serve_dir:?}")]
// TODO #[display("spawn pid {pid}")]

#[derive(Debug)]
pub struct Build {
    child: Child,
    stdout_join_handle: task::JoinHandle<()>,
    stderr_join_handle: task::JoinHandle<()>,
}

#[derive(Debug, Clone, derive_more::FromStr)]
pub struct BuildCommandPath(PathBuf);

impl Build {
    pub fn pid(&self) -> anyhow::Result<Pid> {
        Ok(Pid::from_raw(
            self.child.id().context("get build pid")? as i32
        ))
    }

    pub async fn spawn(serve_dir: ServeDir, command: BuildCommandPath) -> anyhow::Result<Self> {
        let mut command = Command::new(command.0);

        command
            .env(SERVE_PATH, serve_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        info!("spawn build {command:?}");
        let mut child = command.spawn().context("spawn command")?;

        let stdout_join_handle = child.for_stdout_line(move |line| {
            eprintln!("stdout: {line}");
            async {}.boxed()
        });

        let stderr_join_handle = child.for_stderr_line(move |line| {
            eprintln!("stderr: {line}");
            async {}.boxed()
        });

        Ok(Self {
            child,
            stdout_join_handle,
            stderr_join_handle,
        })
    }

    pub async fn wait(self) -> anyhow::Result<Option<i32>> {
        let Self {
            mut child,
            stdout_join_handle,
            stderr_join_handle,
        } = self;

        let code = child
            .wait()
            .await
            .with_context(|| format!("wait on {child:?}"))?
            .code();

        tokio::try_join!(stdout_join_handle, stderr_join_handle).context("join output tasks")?;

        Ok(code)
    }
}
// TODO Some(non_zero) => bail!("build exited with: {non_zero}"),
// TODO None => bail!("build exited with no code"),

// TODO #[display("wait for {child:?}")]

// TODO #[display("build succeeded")]

// TODO #[display("send {signal} to {pid}")]

// TODO #[display("signal sent")]
