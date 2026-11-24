use std::path::{Path, PathBuf};

use anyhow::{Context as _, Ok};
use tracing::info;

#[derive(derive_more::Debug)]
pub(crate) struct GitRepo {
    #[debug(skip)]
    repo: git2::Repository,
    workdir: PathBuf,
}

impl GitRepo {
    pub(crate) fn resolve() -> anyhow::Result<Self> {
        let repo = git2::Repository::open_from_env().context("find git repo")?;
        let workdir = repo.workdir().context("resolve workdir")?.to_path_buf();
        info!("workdir {workdir:?}");
        Ok(Self { repo, workdir })
    }

    pub(crate) fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub(crate) fn is_path_ignored(&self, path: impl AsRef<Path>) -> anyhow::Result<bool> {
        let answer = self.repo.is_path_ignored(&path).with_context(|| {
            let path = path.as_ref();
            format!("ask whether {path:?} is ignored")
        })?;

        Ok(answer)
    }
}
