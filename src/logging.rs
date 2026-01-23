use tracing::{info, level_filters::LevelFilter};

pub fn init() {
    let filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .with_env_var(env!("LOG_FILTER_VAR_NAME"))
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();

    info!("{} starting", env!("CARGO_PKG_NAME"));
}
