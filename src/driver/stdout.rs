use std::convert::Infallible;

use rxrust::{LocalBoxedObservable, prelude::*};

pub(crate) struct StdoutDriver;

#[derive(Debug, Clone)]
pub(crate) struct StdoutDriverCommand(pub String);

impl StdoutDriver {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn init(
        self,
        commands: LocalBoxedObservable<'static, StdoutDriverCommand, Infallible>,
    ) -> BoxedSubscription {
        commands
            .delay(Duration::ZERO)
            .subscribe(move |StdoutDriverCommand(str)| {
                print!("{str}");
            })
            .into_boxed()
    }
}
