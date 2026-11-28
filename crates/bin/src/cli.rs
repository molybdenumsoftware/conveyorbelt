use clap::Parser as _;
use indoc::concatdoc;
use tracing::debug;

use crate::{build::BuildCommandPath, serve_path::ServePath};

#[derive(Debug, Clone, clap::Parser)]
#[clap(
    long_about=concatdoc! {"
        A CLI daemon that can:

        - watch source
        - invoke arbitrary build command
        - statically serve
        - launch browser
        - trigger page reload
        - politely report what it's doing
    "}
)]
pub(crate) struct Args {
    /// Project files, as determined by git, are watched for changes.
    /// On change, this command is invoked.
    /// If that process exits successfully, the browser page is reloaded.
    pub(crate) build_command: BuildCommandPath,
    /// Statically served at `http://localhost:<available port>/`.
    /// Supposedly, this is where your build command writes to.
    /// It is resolved at request time, so it may be re-created and may be a symlink.
    /// A chromium browser is launched with that URL.
    pub(crate) serve_path: ServePath,
}

pub(crate) fn parse() -> Args {
    let args = Args::parse();
    debug!("arguments parsed: {args:?}");
    args
}
