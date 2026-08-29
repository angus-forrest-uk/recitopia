use thiserror::Error;
use tracing_subscriber::EnvFilter;

/// Installs the JSON tracing subscriber and the process panic hook.
///
/// # Errors
///
/// Returns [`LoggingError`] when another global tracing subscriber is already installed.
pub fn init() -> Result<(), LoggingError> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("recitopia_api_rs=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(false)
        .try_init()
        .map_err(|error| LoggingError(error.to_string()))?;

    std::panic::set_hook(Box::new(|panic| {
        tracing::error!(
            event = "panic",
            severity = "FAULT",
            panic = %panic,
            "unhandled Rust panic"
        );
    }));
    Ok(())
}

pub fn fault(event: &str, message: &str) {
    tracing::error!(event, severity = "FAULT", message, "fault");
}

#[derive(Debug, Error)]
#[error("could not initialize tracing: {0}")]
pub struct LoggingError(String);
