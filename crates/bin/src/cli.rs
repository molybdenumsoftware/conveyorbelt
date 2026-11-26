use clap::Parser as _;
use tracing::debug;

use crate::build::BuildCommandPath;

/// A CLI daemon that can:
///
/// - watch source
/// - invoke arbitrary build command
/// - statically serve
/// - launch browser
/// - trigger page reload
/// - politely report what it's doing
///
/// A temporary directory *serve path* is created
/// and its contents statically served at `http://localhost:<available port>/`.
/// A chromium browser is launched with that URL.
///
/// On file changes the `<build command>` is invoked.
/// The *build process* receives the *serve path* via the environment variable `SERVE_PATH`.
/// When the *build process* exits successfully, the page reloads.
#[derive(Debug, Clone, clap::Parser)]
pub(crate) struct Args {
    /// The build command
    pub(crate) build_command: BuildCommandPath,
}

pub(crate) fn parse() -> Args {
    let args = Args::parse();
    debug!("arguments parsed: {args:?}");
    args
}
