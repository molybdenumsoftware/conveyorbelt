use std::{env::current_dir, path::PathBuf, sync::Arc};

use tempfile::TempDir;

#[derive(Debug)]
pub(crate) struct Config {
    pub(crate) build_command_path: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) serve_dir: TempDir,
}

impl Config {
    pub(crate) fn obtain() -> anyhow::Result<Arc<Self>> {
        let args = crate::cli::parse();
        let project_root = crate::project_path::resolve(&current_dir()?)?;
        let serve_dir = crate::serve_dir::obtain()?;

        Ok(Arc::new(Self {
            build_command_path: args.build_command,
            project_root,
            serve_dir,
        }))
    }
}
