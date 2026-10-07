//! Optional application setup. Libraries emit through `tracing`; only the
//! executable chooses and installs a subscriber.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Json,
    Pretty,
}

/// Installs a stdout subscriber with `RUST_LOG` filtering (INFO by default).
/// Returns an error if a subscriber is already installed; never replaces it.
pub fn init(format: Format) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    let subscriber = tracing_subscriber::fmt().with_env_filter(filter);
    match format {
        Format::Json => subscriber.json().try_init(),
        Format::Pretty => subscriber.pretty().try_init(),
    }
}
