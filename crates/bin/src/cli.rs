use clap::Parser as _;
use tracing::debug;

use crate::build::BuildCommandPath;

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
