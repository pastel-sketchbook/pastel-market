//! Alpha Vantage data provider (skeleton implementation).
//!
//! This crate provides a `QuoteProvider` implementation backed by the
//! Alpha Vantage API. It requires an API key set via the
//! `ALPHA_VANTAGE_API_KEY` environment variable.
//!
//! **Current status:** infrastructure only. The trait methods return
//! placeholder errors indicating the provider is not yet fully implemented.
//! This crate exists to validate the pluggable provider architecture.

#![allow(clippy::missing_errors_doc)]

use anyhow::{Result, bail};

use market_core::domain::{ChartRange, NewsItem, PricePoint, Quote};

// Re-export the trait so consumers can use it without importing yahoo-provider.
pub use yahoo_provider::QuoteProvider;

/// Alpha Vantage API client.
///
/// Implements [`QuoteProvider`] for use as an alternative data source.
/// Requires `ALPHA_VANTAGE_API_KEY` environment variable.
pub struct AlphaVantageClient {
    #[allow(dead_code)]
    api_key: String,
    #[allow(dead_code)]
    _agent: ureq::Agent,
}

impl AlphaVantageClient {
    /// Create a new client from the `ALPHA_VANTAGE_API_KEY` environment variable.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is not set.
    pub fn new() -> Result<Self> {
        let api_key = std::env::var("ALPHA_VANTAGE_API_KEY")
            .map_err(|_| anyhow::anyhow!("ALPHA_VANTAGE_API_KEY not set"))?;

        Ok(Self {
            api_key,
            _agent: ureq::Agent::new_with_config(ureq::config::Config::default()),
        })
    }

    /// Check if an API key is available (without constructing the client).
    #[must_use]
    pub fn is_available() -> bool {
        std::env::var("ALPHA_VANTAGE_API_KEY").is_ok()
    }

    /// The base URL for the Alpha Vantage API.
    #[must_use]
    fn _base_url(&self) -> String {
        format!("https://www.alphavantage.co/query?apikey={}", self.api_key)
    }
}

impl QuoteProvider for AlphaVantageClient {
    fn fetch_quotes(&self, _symbols: &[String]) -> Result<Vec<Option<Quote>>> {
        bail!("Alpha Vantage quote provider not yet implemented (API key present but fetch_quotes pending)")
    }

    fn fetch_sparkline(&self, _symbol: &str, _range: ChartRange) -> Result<Vec<PricePoint>> {
        bail!("Alpha Vantage sparkline provider not yet implemented")
    }

    fn fetch_screener(&self, _scr_id: &str) -> Result<Vec<Quote>> {
        bail!("Alpha Vantage does not support screener queries")
    }

    fn fetch_trending(&self) -> Result<Vec<String>> {
        bail!("Alpha Vantage does not support trending queries")
    }

    fn fetch_news(&self, _symbol: &str) -> Result<Vec<NewsItem>> {
        bail!("Alpha Vantage news provider not yet implemented")
    }
}
