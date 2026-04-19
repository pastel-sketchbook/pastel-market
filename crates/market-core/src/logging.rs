//! Logging setup using `tracing` with a file appender.
//!
//! Logs are written to a rotating daily file in the platform-appropriate
//! data directory. The TUI never prints to stdout/stderr, so file
//! logging is the only way to capture diagnostic output.

use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Project name for directory resolution.
const PROJECT_NAME: &str = "pastel-market";

/// Initialise the global tracing subscriber with a file appender.
///
/// Returns a [`WorkerGuard`] that **must be held alive** for the duration
/// of the program -- dropping it flushes buffered log lines to disk.
///
/// Falls back to no logging if the data directory cannot be determined.
#[must_use]
pub fn init() -> Option<WorkerGuard> {
    let proj_dirs = ProjectDirs::from("", "", PROJECT_NAME)?;
    let log_dir = proj_dirs.data_dir().join("logs");

    std::fs::create_dir_all(&log_dir).ok()?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "pastel-market.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(subscriber)
        .init();

    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_is_pastel_market() {
        assert_eq!(PROJECT_NAME, "pastel-market");
    }

    #[test]
    fn project_dirs_resolves_on_this_platform() {
        let dirs = ProjectDirs::from("", "", PROJECT_NAME);
        assert!(dirs.is_some(), "ProjectDirs should resolve on this OS");
    }

    #[test]
    fn log_dir_path_contains_project_name() {
        let dirs = ProjectDirs::from("", "", PROJECT_NAME).expect("dirs");
        let log_dir = dirs.data_dir().join("logs");
        let path = log_dir.to_string_lossy();
        assert!(
            path.contains(PROJECT_NAME),
            "log dir should contain project name: {path}"
        );
    }
}
