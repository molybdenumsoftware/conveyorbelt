use std::fmt::Display;

use tracing::info;

pub(crate) mod browser;
pub(crate) mod build;
pub(crate) mod fswatch;
pub(crate) mod project_path;
pub(crate) mod server;
pub(crate) mod signal;

pub(crate) trait Effect<T, E> {
    async fn inner(self) -> Result<T, E>;
    async fn perform(self) -> Result<T, E>
    where
        Self: Sized + Display,
        T: Display + Send + 'static,
    {
        info!("effect: {self}");

        let result = self.inner().await;
        if let Ok(v) = &result {
            info!("event: {v}");
        }
        result
    }
}
