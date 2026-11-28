use anyhow::Context;
use nix::{
    sys::signal::{Signal::SIGTERM, kill},
    unistd::Pid,
};
use tracing::{info, warn};

#[derive(Debug, derive_more::Deref, derive_more::DerefMut)]
pub struct DroppyChild(std::process::Child);

impl DroppyChild {
    pub fn new(child: std::process::Child) -> Self {
        Self(child)
    }
}

impl Drop for DroppyChild {
    fn drop(&mut self) {
        let id = self.0.id();
        if let Err(e) = kill(Pid::from_raw(self.id() as i32), SIGTERM)
            .with_context(|| format!("SIGTERM {id} on drop"))
        {
            warn!("{e}");
            return;
        }
        let Ok(status) = self.0.wait() else { return };
        if status.success() {
            return;
        }
        info!("{id} {status}")
    }
}
