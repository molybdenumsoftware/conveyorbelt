use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateForTesting {
    pub serve_port: u16,
    pub browser_pid: u32,
    pub browser_debugging_address: String,
}

pub const TESTING_MODE: &str = "_TESTING_MODE";

impl std::fmt::Display for StateForTesting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string = serde_json::to_string(&self).expect("seems to serialize I guess");
        write!(f, "{string}")
    }
}
