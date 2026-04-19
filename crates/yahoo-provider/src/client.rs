//! Yahoo Finance authenticated client and `QuoteProvider` trait.
//!
//! The [`QuoteProvider`] trait abstracts stock data fetching for testability.
//! [`YahooClient`] is the production implementation using Yahoo Finance APIs
//! with cookie+crumb authentication.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use tracing::info;

use market_core::domain::{NewsItem, PricePoint, Quote};
use market_core::http::{USER_AGENT, call_with_retry};

use crate::quotes;

/// Yahoo Finance API endpoints.
const COOKIE_URL: &str = "https://fc.yahoo.com/curveball";
const CRUMB_URL: &str = "https://query2.finance.yahoo.com/v1/test/getcrumb";
const QUOTE_URL: &str = "https://query2.finance.yahoo.com/v7/finance/quote";
const SPARK_URL: &str = "https://query2.finance.yahoo.com/v8/finance/spark";
const SCREENER_URL: &str = "https://query2.finance.yahoo.com/v1/finance/screener/predefined/saved";
const TRENDING_URL: &str = "https://query2.finance.yahoo.com/v1/finance/trending/US";
const SEARCH_URL: &str = "https://query2.finance.yahoo.com/v1/finance/search";

/// Per-request timeout — prevents UI freezes on unresponsive endpoints.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Abstraction over a stock-quote data source.
///
/// Decouples the application from the concrete Yahoo Finance client,
/// making it possible to inject test doubles.
pub trait QuoteProvider {
    /// Fetch real-time quotes for the given symbols.
    ///
    /// Returns one `Option<Quote>` per input symbol (in the same order).
    ///
    /// # Errors
    ///
    /// Returns an error if the data source is unreachable or returns
    /// unparsable data.
    fn fetch_quotes(&self, symbols: &[String]) -> Result<Vec<Option<Quote>>>;

    /// Fetch intraday sparkline data for a single symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the data source is unreachable or returns
    /// unparsable data.
    fn fetch_sparkline(&self, symbol: &str) -> Result<Vec<PricePoint>>;

    /// Fetch a Yahoo predefined screener list (e.g. `"day_gainers"`).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint is unreachable or the response
    /// cannot be parsed.
    fn fetch_screener(&self, _scr_id: &str) -> Result<Vec<Quote>> {
        bail!("screener not implemented")
    }

    /// Fetch trending tickers for the US market.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint is unreachable or the response
    /// cannot be parsed.
    fn fetch_trending(&self) -> Result<Vec<String>> {
        bail!("trending not implemented")
    }

    /// Fetch recent news headlines for a stock symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint is unreachable or the response
    /// cannot be parsed.
    fn fetch_news(&self, _symbol: &str) -> Result<Vec<NewsItem>> {
        bail!("news not implemented")
    }
}

/// Authenticated Yahoo Finance client.
///
/// Yahoo's API requires a session cookie and a crumb token on every
/// request. This client obtains both during construction and reuses
/// them for all subsequent calls.
pub struct YahooClient {
    agent: ureq::Agent,
    crumb: String,
}

impl YahooClient {
    /// Create a new client by establishing a Yahoo Finance session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cookie or crumb cannot be obtained.
    pub fn new() -> Result<Self> {
        // Disable http_status_as_error so that non-2xx responses (like the
        // 404 from the cookie endpoint) still store Set-Cookie headers in the
        // cookie jar. Without this, ureq returns Err before processing cookies.
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build();
        let agent = ureq::Agent::new_with_config(config);

        // Hit a known Yahoo endpoint to receive a session cookie.
        // The 404 response is expected — we only need the Set-Cookie header.
        let _ignore = agent
            .get(COOKIE_URL)
            .header("User-Agent", USER_AGENT)
            .call();

        // Fetch the crumb token using the session cookie.
        let mut crumb_response = agent
            .get(CRUMB_URL)
            .header("User-Agent", USER_AGENT)
            .call()
            .context("failed to fetch Yahoo Finance crumb")?;

        let status = crumb_response.status().as_u16();
        if status != 200 {
            bail!("Yahoo Finance crumb endpoint returned HTTP {status}");
        }

        let crumb = crumb_response
            .body_mut()
            .read_to_string()
            .context("failed to read crumb response body")?;

        if crumb.is_empty() {
            bail!("Yahoo Finance returned an empty crumb");
        }

        info!("Yahoo Finance session established");
        Ok(Self { agent, crumb })
    }
}

impl QuoteProvider for YahooClient {
    fn fetch_quotes(&self, symbols: &[String]) -> Result<Vec<Option<Quote>>> {
        if symbols.is_empty() {
            return Ok(vec![]);
        }

        let joined = symbols.join(",");
        let agent = self.agent.clone();
        let crumb = self.crumb.clone();

        let mut body = call_with_retry(|| {
            agent
                .get(QUOTE_URL)
                .header("User-Agent", USER_AGENT)
                .query("symbols", &joined)
                .query("crumb", &crumb)
                .query(
                    "fields",
                    "symbol,shortName,marketState,regularMarketPrice,regularMarketChange,\
                     regularMarketChangePercent,regularMarketVolume,\
                     regularMarketPreviousClose,regularMarketOpen,\
                     regularMarketDayHigh,regularMarketDayLow,\
                     fiftyTwoWeekHigh,fiftyTwoWeekLow",
                )
        })
        .context("Yahoo Finance quote request failed")?;

        let json: serde_json::Value = body
            .read_json()
            .context("failed to parse Yahoo Finance response")?;

        Ok(quotes::parse_quotes_response(&json, symbols))
    }

    fn fetch_sparkline(&self, symbol: &str) -> Result<Vec<PricePoint>> {
        let agent = self.agent.clone();
        let crumb = self.crumb.clone();
        let sym = symbol.to_string();

        let mut body = call_with_retry(|| {
            agent
                .get(SPARK_URL)
                .header("User-Agent", USER_AGENT)
                .query("symbols", &sym)
                .query("crumb", &crumb)
                .query("range", "1d")
                .query("interval", "5m")
        })
        .context("Yahoo Finance spark request failed")?;

        let json: serde_json::Value = body
            .read_json()
            .context("failed to parse Yahoo Finance spark response")?;

        Ok(quotes::parse_sparkline_response(&json))
    }

    fn fetch_screener(&self, scr_id: &str) -> Result<Vec<Quote>> {
        let agent = self.agent.clone();
        let crumb = self.crumb.clone();
        let id = scr_id.to_string();

        let mut body = call_with_retry(|| {
            agent
                .get(SCREENER_URL)
                .header("User-Agent", USER_AGENT)
                .query("scrIds", &id)
                .query("count", "25")
                .query("crumb", &crumb)
        })
        .context("Yahoo Finance screener request failed")?;

        let json: serde_json::Value = body
            .read_json()
            .context("failed to parse Yahoo Finance screener response")?;

        Ok(quotes::parse_screener_response(&json))
    }

    fn fetch_trending(&self) -> Result<Vec<String>> {
        let agent = self.agent.clone();
        let crumb = self.crumb.clone();

        let mut body = call_with_retry(|| {
            agent
                .get(TRENDING_URL)
                .header("User-Agent", USER_AGENT)
                .query("count", "25")
                .query("crumb", &crumb)
        })
        .context("Yahoo Finance trending request failed")?;

        let json: serde_json::Value = body
            .read_json()
            .context("failed to parse Yahoo Finance trending response")?;

        Ok(quotes::parse_trending_response(&json))
    }

    fn fetch_news(&self, symbol: &str) -> Result<Vec<NewsItem>> {
        let agent = self.agent.clone();
        let crumb = self.crumb.clone();
        let sym = symbol.to_string();

        let mut body = call_with_retry(|| {
            agent
                .get(SEARCH_URL)
                .header("User-Agent", USER_AGENT)
                .query("q", &sym)
                .query("newsCount", "10")
                .query("quotesCount", "0")
                .query("listsCount", "0")
                .query("crumb", &crumb)
        })
        .context("Yahoo Finance news search request failed")?;

        let json: serde_json::Value = body
            .read_json()
            .context("failed to parse Yahoo Finance search response")?;

        Ok(quotes::parse_news_response(&json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that the default trait implementations return errors.
    struct StubProvider;

    impl QuoteProvider for StubProvider {
        fn fetch_quotes(&self, _symbols: &[String]) -> Result<Vec<Option<Quote>>> {
            Ok(vec![])
        }
        fn fetch_sparkline(&self, _symbol: &str) -> Result<Vec<PricePoint>> {
            Ok(vec![])
        }
    }

    #[test]
    fn default_screener_returns_error() {
        let p = StubProvider;
        assert!(p.fetch_screener("day_gainers").is_err());
    }

    #[test]
    fn default_trending_returns_error() {
        let p = StubProvider;
        assert!(p.fetch_trending().is_err());
    }

    #[test]
    fn default_news_returns_error() {
        let p = StubProvider;
        assert!(p.fetch_news("AAPL").is_err());
    }

    #[test]
    fn stub_provider_fetch_quotes_returns_empty() {
        let p = StubProvider;
        let result = p.fetch_quotes(&[]).expect("should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn stub_provider_fetch_sparkline_returns_empty() {
        let p = StubProvider;
        let result = p.fetch_sparkline("AAPL").expect("should succeed");
        assert!(result.is_empty());
    }
}
