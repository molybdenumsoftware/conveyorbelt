use tempfile::TempDir;
use tracing::{debug, info};

pub(crate) fn obtain() -> anyhow::Result<TempDir> {
    let serve_dir = TempDir::new()?;
    info!("serve path: {serve_dir:?}");
    Ok(serve_dir)
}
