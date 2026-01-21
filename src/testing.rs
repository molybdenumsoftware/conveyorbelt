use std::path::PathBuf;

use anyhow::Context as _;
use tracing::debug;

use crate::common::StateForTesting;

impl StateForTesting {
    pub(crate) fn print(
        serve_path: PathBuf,
        serve_port: u16,
        browser_debugging_address: String,
        browser_pid: u32,
    ) -> anyhow::Result<()> {
        let state_for_testing = Self {
            serve_path,
            serve_port,
            browser_debugging_address,
            browser_pid,
        };

        debug!("{state_for_testing:?}");

        let state_for_testing = serde_json::to_string(&state_for_testing)
            .context("failed to serialize state for testing")?;

        println!("{state_for_testing}");
        Ok(())
    }
}
