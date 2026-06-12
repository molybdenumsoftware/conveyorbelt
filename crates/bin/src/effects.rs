use tracing::info;

pub(crate) mod browser;
pub(crate) mod build;
pub(crate) mod fswatch;
pub(crate) mod server;
pub(crate) mod signal;

pub(crate) trait Effect<T, E: std::error::Error> {
    async fn effect(self) -> Result<T, E>;
    async fn call(self) -> Result<T, E>
    where
        Self: Sized,
    {
        info!("effect: {self}");
        let result = self.effect().await;
        info!("result: {result}");
        result
    }
}
