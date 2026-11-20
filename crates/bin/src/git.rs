use std::path::{Path, PathBuf};

use anyhow::{Context as _, Ok};
use tracing::info;

pub struct GitRepo {
    repo: git2::Repository,
    workdir: PathBuf,
}

impl GitRepo {
    pub fn resolve() -> anyhow::Result<Self> {
        let repo = git2::Repository::open_from_env().context("find git repo")?;
        let workdir = repo.workdir().context("resolve workdir")?.to_path_buf();
        info!("git workdir {workdir:?}");
        Ok(Self { repo, workdir })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn is_path_ignored(&self, path: impl AsRef<Path>) -> anyhow::Result<bool> {
        let answer = self.repo.is_path_ignored(&path).with_context(|| {
            let path = path.as_ref();
            format!("ask whether {path:?} is ignored")
        })?;

        Ok(answer)
    }
}
