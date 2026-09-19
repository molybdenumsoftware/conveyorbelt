use std::future::Future;

use rxrust::{Shared, SharedBoxedObservable, prelude::*};
use tracing::info;

pub(crate) mod browser;
pub(crate) mod build;
pub(crate) mod fswatch;
pub(crate) mod server;
pub(crate) mod signal;

pub(crate) trait Effect<T, E> {
    fn effect(self) -> impl Future<Output = Result<T, E>> + Send + 'static;
    fn call(self) -> SharedBoxedObservable<'static, T, E>
    where
        Self: Sized + std::fmt::Display,
        T: std::fmt::Display + Send + 'static,
        E: std::fmt::Display + Send + 'static,
    {
        info!("effect: {self}");

        // TODO actshually, we want to use from_future_result but rxrust doesn't seem to have error
        // handling 🤷‍♂️:
        // https://github.com/rxRust/rxRust/issues/279
        Shared::from_future_result(self.effect())
            .map_err(|error| {
                info!("{error}");
                error
            })
            .tap(|v| {
                info!("{v}");
            })
            .box_it()
    }
}
