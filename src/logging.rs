use tracing::Level;
use tracing_subscriber::filter::EnvFilter;

pub(crate) fn init() {
    let filter = EnvFilter::try_from_env(env!("LOG_FILTER_VAR_NAME")).unwrap_or_else(|_| {
        EnvFilter::default()
            .add_directive(Level::WARN.into())
            .add_directive(
                format!("{}=info", env!("CARGO_CRATE_NAME"))
                    .parse()
                    .unwrap(),
            )
    });

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}
