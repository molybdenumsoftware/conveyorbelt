use anyhow::anyhow;
use tracing::{Level, info};
use tracing_subscriber::{filter::EnvFilter, fmt::time::LocalTime};

const ENV_FILTER_VAR: &str = "LOG";

pub(crate) fn init() {
    let (filter, from_env_error) = match EnvFilter::try_from_env(ENV_FILTER_VAR)
        .map_err(|e| anyhow!("{e} reading tracing filter from `{ENV_FILTER_VAR}`"))
    {
        Ok(filter) => (filter, None),
        Err(e) => {
            let filter = EnvFilter::default()
                .add_directive(Level::WARN.into())
                .add_directive(
                    format!("{}=info", env!("CARGO_CRATE_NAME"))
                        .parse()
                        .expect("literal"),
                );
            (filter, Some(e))
        }
    };

    tracing_subscriber::fmt()
        .compact()
        .with_timer(LocalTime::new(time::macros::format_description!(
            "[hour]:[minute]:[second]"
        )))
        .with_writer(std::io::stderr)
        .with_env_filter(filter.clone())
        .init();

    match from_env_error {
        Some(e) => info!(
            "{e:#}. Using default filter `{filter}`. `tracing_subscriber::EnvFilter` can be provided."
        ),
        None => info!("using `EnvFilter` from {ENV_FILTER_VAR}: {filter}"),
    }
}
