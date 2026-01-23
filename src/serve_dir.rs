use tempfile::TempDir;
use tracing::debug;

pub fn obtain() -> anyhow::Result<TempDir> {
    let serve_dir = TempDir::with_prefix(
        "not-hidden-", // https://github.com/static-web-server/static-web-server/pull/606
    )?;
    debug!("serve path: {serve_dir:?}");
    Ok(serve_dir)
}
