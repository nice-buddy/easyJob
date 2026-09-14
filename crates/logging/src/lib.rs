use easyjob_common::{Error, Result};
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub fn init_logging(log_dir: &Path, to_console: bool, default_level: &str) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir).map_err(Error::Io)?;

    let file_appender = tracing_appender::rolling::daily(log_dir, "agent.log");
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_appender)
        .with_ansi(false)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(env_filter.clone());

    let registry = tracing_subscriber::registry().with(file_layer);

    if to_console {
        let console_layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_filter(env_filter);
        registry
            .with(console_layer)
            .try_init()
            .map_err(|e| Error::Other(e.to_string()))?;
    } else {
        registry
            .try_init()
            .map_err(|e| Error::Other(e.to_string()))?;
    }

    Ok(guard)
}
