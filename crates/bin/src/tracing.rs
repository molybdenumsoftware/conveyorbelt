use indoc::formatdoc;
use tracing::{Level, info};
use tracing_subscriber::filter::EnvFilter;

pub(crate) fn init() {
    let (filter, from_env_error) = match EnvFilter::try_from_env("LOG") {
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
        .with_writer(std::io::stderr)
        .with_env_filter(filter.clone())
        .init();

    match from_env_error {
        Some(e) => {
            let msg = formatdoc! {"
                {e}
                    tip: provide an `EnvFilter` via LOG
                    https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
                    using default filter: {filter}\
            "};
            info!("{msg}")
        }
        None => info!("using `EnvFilter` from LOG: {filter}"),
    }
}
