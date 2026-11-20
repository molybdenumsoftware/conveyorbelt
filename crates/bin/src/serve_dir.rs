use std::path::PathBuf;

use anyhow::Context as _;
use tempfile::TempDir;
use tracing::{debug, info};

#[derive(Clone, Debug, derive_more::Deref)]
#[deref(forward)]
pub(crate) struct ServeDir(PathBuf);

impl ServeDir {
    pub fn create() -> anyhow::Result<Self> {
        debug!("create serve dir");
        let dir = TempDir::new().context("create temp dir")?.keep();
        info!("created {dir:?}");
        Ok(ServeDir(dir))
    }
}
