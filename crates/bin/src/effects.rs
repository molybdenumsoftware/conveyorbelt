use std::fmt::Display;

use tracing::info;

pub(crate) mod browser;
pub(crate) mod build;
pub(crate) mod fswatch;
pub(crate) mod server;
pub(crate) mod signal;

pub(crate) trait Effect<T, E> {
    async fn effect(self) -> Result<T, E>;
    async fn do_logged(self) -> Result<T, E>
    where
        Self: Sized + Display,
        T: Display + Send + 'static,
        E: Display + Send + 'static,
    {
        info!("effect: {self}");

        let result = self.effect().await;
        match &result {
            Ok(v) => {
                info!("{v}")
            }
            Err(error) => {
                info!("{error}")
            }
        }
        result
    }
}
