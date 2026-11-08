use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Ok, bail};
use tracing::info;

use crate::effects::Effect;

#[derive(Debug, derive_more::Display)]
#[display("resolve project path")]
pub(crate) struct GetProjectPath;

impl Effect<PathBuf, anyhow::Error> for GetProjectPath {
    async fn inner(self) -> anyhow::Result<T> {
        let mut command = Command::new("git");

        command.args(["rev-parse", "--show-toplevel"]);

        let output = command
            .output()
            .with_context(|| format!("failed to run {command:?}"))?;

        if !output.status.success() {
            bail!(
                "command {:?} exited with {}. stderr: {}",
                command,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let git_toplevel: String = output
            .stdout
            .try_into()
            .with_context(|| format!("command printed non-UTF-8: {command:?}"))?;

        let git_toplevel = git_toplevel.trim_end().to_string();
        info!("git toplevel obtained: {git_toplevel}");
        Ok(git_toplevel.parse()?)
    }
}
