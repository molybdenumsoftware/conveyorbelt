use std::{path::PathBuf, process::Command};

use anyhow::{Context as _, Ok, bail};
use tracing::info;

#[derive(Debug, Clone)]
pub struct GitToplevel(PathBuf);

impl GitToplevel {
    pub fn resolve() -> anyhow::Result<Self> {
        let mut command = Command::new("git");
        command.args(["rev-parse", "--show-toplevel"]);
        let output = command.output().with_context(|| format!("{command:?}"))?;

        if !output.status.success() {
            let exit_status = output.status;
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("{command:?} exited with {exit_status}. stderr: {stderr}");
        }

        let git_toplevel = output
            .stdout
            .try_into()
            .with_context(|| format!("{command:?} wrote non-UTF-8 to stdout"))?
            .trim_end()
            .to_string()
            .parse()
            .context("parse git_toplevel into `PathBuf`")?;

        info!("git toplevel obtained: {git_toplevel}");
        Ok(git_toplevel)
    }
}
