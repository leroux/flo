use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize file-based logging to ~/.flo/logs/.
/// Returns a guard that must be held for the lifetime of the program
/// to ensure all logs are flushed.
pub fn init() -> WorkerGuard {
    let log_dir = dirs::home_dir()
        .expect("could not find home directory")
        .join(".flo")
        .join("logs");

    std::fs::create_dir_all(&log_dir).expect("failed to create log directory");

    let file_appender = tracing_appender::rolling::daily(&log_dir, "flo.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    guard
}
