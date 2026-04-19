//! Shared HTTP client with retry logic.
//!
//! Uses `ureq` with exponential backoff for resilient HTTP requests.

use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};
use tracing::{debug, warn};

/// Default request timeout.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum number of retry attempts.
pub const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (1s, 2s, 4s).
pub const BACKOFF_BASE_MS: u64 = 1000;

/// User-Agent string for HTTP requests.
pub const USER_AGENT: &str = concat!("pastel-market/", env!("CARGO_PKG_VERSION"));

/// Retry configuration.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub backoff_base_ms: u64,
}

impl RetryConfig {
    /// Default retry configuration: 3 retries, 1s base backoff.
    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            backoff_base_ms: BACKOFF_BASE_MS,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Returns `true` for HTTP status codes that should trigger a retry.
#[must_use]
pub const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Execute an HTTP request with exponential backoff retry (default config).
///
/// The `build_request` closure is called on each attempt and must return
/// a fresh `ureq::RequestBuilder`.
///
/// # Errors
///
/// Returns the last error if all retries are exhausted.
pub fn call_with_retry<F>(build_request: F) -> Result<ureq::Body>
where
    F: Fn() -> ureq::RequestBuilder<ureq::typestate::WithoutBody>,
{
    call_with_retry_cfg(build_request, &RetryConfig::defaults())
}

/// Execute an HTTP request with configurable retry.
///
/// # Errors
///
/// Returns the last error if all retries are exhausted.
pub fn call_with_retry_cfg<F>(build_request: F, cfg: &RetryConfig) -> Result<ureq::Body>
where
    F: Fn() -> ureq::RequestBuilder<ureq::typestate::WithoutBody>,
{
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..cfg.max_retries {
        match build_request().call() {
            Ok(response) => {
                let status = response.status().as_u16();
                if is_retryable_status(status) {
                    let delay = cfg.backoff_base_ms * u64::from(2_u32.pow(attempt));
                    warn!(
                        status = %status,
                        attempt = attempt + 1,
                        max = cfg.max_retries,
                        delay_ms = delay,
                        "retryable HTTP status — backing off"
                    );
                    last_err = Some(anyhow::anyhow!(
                        "HTTP {status} on attempt {}/{}",
                        attempt + 1,
                        cfg.max_retries
                    ));
                    thread::sleep(Duration::from_millis(delay));
                    continue;
                }
                if status >= 400 {
                    bail!(
                        "HTTP {status} (non-retryable) on attempt {}/{}",
                        attempt + 1,
                        cfg.max_retries
                    );
                }
                debug!(status = %status, "HTTP request succeeded");
                return Ok(response.into_body());
            }
            Err(e) => {
                if let ureq::Error::StatusCode(code) = &e
                    && !is_retryable_status(*code)
                {
                    bail!(
                        "HTTP {code} (non-retryable) on attempt {}/{}",
                        attempt + 1,
                        cfg.max_retries
                    );
                }

                let delay = cfg.backoff_base_ms * u64::from(2_u32.pow(attempt));
                warn!(
                    attempt = attempt + 1,
                    max = cfg.max_retries,
                    delay_ms = delay,
                    error = %e,
                    "HTTP request failed — backing off"
                );
                last_err = Some(anyhow::Error::from(e).context(format!(
                    "attempt {}/{}",
                    attempt + 1,
                    cfg.max_retries
                )));
                thread::sleep(Duration::from_millis(delay));
            }
        }
    }

    bail!(
        "all {} attempts failed: {}",
        cfg.max_retries,
        last_err.unwrap_or_else(|| anyhow::anyhow!("unknown error"))
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_config_defaults() {
        let cfg = RetryConfig::defaults();
        assert_eq!(cfg.max_retries, 3);
        assert_eq!(cfg.backoff_base_ms, 1000);
    }

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(403));
    }
}
