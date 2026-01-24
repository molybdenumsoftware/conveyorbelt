use std::{path::{Path, PathBuf}, process::Command};

use anyhow::{Context as _, Ok, bail};
use tracing::debug;

pub fn resolve(origin: &Path) -> anyhow::Result<PathBuf> {
    let mut command = Command::new("git");

    command
        .current_dir(origin)
        .args(["rev-parse", "--show-toplevel"]);

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
    debug!("git toplevel obtained: {git_toplevel}");
    Ok(git_toplevel.parse()?)
}
