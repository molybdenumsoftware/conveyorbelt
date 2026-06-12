use tracing::info;

pub(crate) mod browser;
pub(crate) mod build;
pub(crate) mod fswatch;
pub(crate) mod server;
pub(crate) mod signal;

pub(crate) trait Effect<T, E> {
    async fn effect(self) -> Result<T, anyhow::Error>;
    async fn call(self) -> Result<T, E>
    where
        Self: Sized + std::fmt::Display,
        T: std::fmt::Display,
        E: std::fmt::Display + From<anyhow::Error>,
    {
        info!("effect: {self}");
        let result = self.effect().await;

        match result {
            Ok(v) => {
                info!("{v}");
                Ok(v)
            }
            Err(err) => {
                let err = err.into();
                info!("{err}");
                Err(err)
            }
        }
    }
}
