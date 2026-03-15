//! Structured file logging with daily rotation.
//!
//! Logs are written to `~/.seval/logs/` using `tracing-appender` with
//! non-blocking I/O. The caller must hold the returned [`WorkerGuard`] for the
//! lifetime of the application to ensure all buffered log entries are flushed.

use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Returns the path to the Seval log directory.
///
/// Checks `SEVAL_LOG_DIR` env var first, then falls back to `~/.seval/logs/`.
fn log_dir() -> anyhow::Result<PathBuf> {
    if let Ok(dir) = std::env::var("SEVAL_LOG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    Ok(base.home_dir().join(".seval").join("logs"))
}

/// Initialise the global tracing subscriber that writes structured logs to
/// `~/.seval/logs/` with daily file rotation.
///
/// Returns a [`WorkerGuard`] that **must** be held for the application
/// lifetime. Dropping it flushes buffered log entries and shuts down the
/// background writer thread.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined or the log
/// directory cannot be created.
pub fn init_logging() -> anyhow::Result<WorkerGuard> {
    let dir = log_dir()?;
    std::fs::create_dir_all(&dir)?;

    let file_appender = tracing_appender::rolling::daily(&dir, "seval.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_file(true)
                .with_line_number(true),
        )
        .init();

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use tempfile::TempDir;

    /// Verify that the log directory helper produces a path ending in
    /// `.seval/logs`.
    #[test]
    fn log_dir_ends_with_seval_logs() {
        let dir = super::log_dir().expect("should resolve home directory");
        assert!(dir.ends_with(".seval/logs"), "unexpected log dir: {dir:?}");
    }

    /// Verify that `tracing-appender` can write a log entry to a temporary
    /// directory. We cannot call `init_logging` twice (global subscriber), so
    /// we replicate the core logic with a temp path instead.
    #[test]
    fn writes_log_to_file() {
        let tmp = TempDir::new().unwrap();
        let file_appender = tracing_appender::rolling::daily(tmp.path(), "test.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let subscriber = tracing_subscriber::fmt()
            .with_writer(non_blocking)
            .with_ansi(false)
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("hello from test");
        });

        // Drop guard to flush.
        drop(guard);

        // Read all files in the temp directory — the rolling appender adds a
        // date suffix, so we cannot predict the exact filename.
        let mut found = false;
        for entry in std::fs::read_dir(tmp.path()).unwrap() {
            let entry = entry.unwrap();
            let mut contents = String::new();
            std::fs::File::open(entry.path())
                .unwrap()
                .read_to_string(&mut contents)
                .unwrap();
            if contents.contains("hello from test") {
                found = true;
                // Also verify no ANSI codes.
                assert!(
                    !contents.contains("\x1b["),
                    "log should not contain ANSI codes"
                );
            }
        }
        assert!(found, "expected log file containing 'hello from test'");
    }
}
