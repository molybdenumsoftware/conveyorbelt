use std::{env::current_dir, path::PathBuf, sync::Arc};

use tempfile::TempDir;

#[derive(Debug)]
pub struct Config {
    pub build_command_path: PathBuf,
    pub project_root: PathBuf,
    pub serve_dir: TempDir,
}

impl Config {
    pub fn obtain() -> anyhow::Result<Arc<Self>> {
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
