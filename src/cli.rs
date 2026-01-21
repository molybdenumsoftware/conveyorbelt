use std::path::PathBuf;

use clap::Parser as _;
use tracing::debug;

#[derive(Debug, Clone, clap::Parser)]
pub struct Args {
    /// The build command
    pub build_command: PathBuf,
}

pub fn parse() -> Args {
    let args = Args::parse();
    debug!("arguments parsed: {args:?}");
    args
}
