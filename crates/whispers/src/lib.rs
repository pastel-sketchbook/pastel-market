//! Earnings Whispers scraper.
//!
//! When the `chrome` feature is enabled (default), uses headless Chrome via
//! the Chrome `DevTools` Protocol to render JS and extract earnings data.
//!
//! When `chrome` is disabled, [`fetch`] returns an error indicating that
//! the feature is not available — this allows faster builds and avoids
//! requiring a Chrome installation.

#![warn(clippy::pedantic)]

use anyhow::Result;

// ---------------------------------------------------------------------------
// Public types (always available — App needs WhisperResult for caching)
// ---------------------------------------------------------------------------

/// Earnings data scraped from Earnings Whispers for a single ticker.
#[derive(Debug, Clone)]
pub struct WhisperResult {
    /// Stock ticker symbol.
    pub ticker: String,
    /// Next earnings report date (e.g. "Thursday Jan 30 4:30 PM").
    pub earnings_date: Option<String>,
    /// Whisper EPS estimate (crowd-sourced).
    pub whisper: Option<String>,
    /// Wall Street consensus EPS estimate.
    pub consensus: Option<String>,
    /// Expected implied volatility / move (e.g. "4.0%").
    pub volatility: Option<String>,
    /// Earnings Whispers numeric score.
    pub score: Option<String>,
    /// Market sentiment (Up / Flat / Down).
    pub sentiment: Option<String>,
    /// Letter grade (e.g. "B+", "A-").
    pub grade: Option<String>,
    /// Company lifecycle stage.
    pub lifecycle: Option<String>,
    /// Whether the stock has a history of beating earnings estimates.
    pub past_beats: Option<bool>,
}

// ===========================================================================
// Chrome-backed implementation (feature = "chrome")
// ===========================================================================

#[cfg(feature = "chrome")]
mod chrome_impl {
    use super::*;
    use anyhow::Context;
    use headless_chrome::Browser;

    const WHISPERS_URL: &str = "https://www.earningswhispers.com/stocks/";

    /// Maximum time (seconds) to wait for page content to appear.
    const CONTENT_WAIT_SECS: u64 = 15;

    /// Polling interval (ms) when checking for JS-rendered content.
    const POLL_INTERVAL_MS: u64 = 300;

    /// Fetch earnings whisper data for a single ticker via headless Chrome.
    ///
    /// # Errors
    ///
    /// Returns an error if Chrome cannot be launched, the page fails to load,
    /// or the JS evaluation fails.
    pub fn fetch(ticker: &str) -> Result<WhisperResult> {
        let browser = Browser::default().context("failed to launch Chrome — is it installed?")?;
        let tab = browser.new_tab().context("failed to create Chrome tab")?;

        let url = format!("{}{}", WHISPERS_URL, ticker.to_lowercase());
        tab.navigate_to(&url)
            .with_context(|| format!("failed to navigate to {url}"))?;

        tab.wait_until_navigated().context("navigation timed out")?;

        // Poll until JS-rendered content is ready.
        wait_for_content(&tab)?;

        // Dismiss cookie consent if present (fire-and-forget).
        let _ = tab.evaluate(
            r#"
            (function() {
                var btns = document.querySelectorAll('button, a');
                for (var i = 0; i < btns.length; i++) {
                    var t = btns[i].textContent.trim().toLowerCase();
                    if (t === 'accept' || t === 'accept all' || t === 'i accept' || t === 'agree') {
                        btns[i].click();
                        return 'clicked';
                    }
                }
                return 'no button';
            })()
            "#,
            false,
        );

        let result = extract_whisper_data(&tab, ticker)?;
        let _ = tab.close(true);
        Ok(result)
    }

    /// Poll until `document.body.innerText` contains a known marker.
    fn wait_for_content(tab: &std::sync::Arc<headless_chrome::Tab>) -> Result<()> {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(CONTENT_WAIT_SECS);

        loop {
            if std::time::Instant::now() >= deadline {
                // Timeout is not fatal — proceed with whatever the page has.
                return Ok(());
            }

            let ready = tab
                .evaluate(
                    r#"
                    (function() {
                        var t = document.body ? document.body.innerText : '';
                        return t.includes('Earnings Date') || t.includes('Consensus');
                    })()
                    "#,
                    false,
                )
                .ok()
                .and_then(|rv| rv.value)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if ready {
                return Ok(());
            }

            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        }
    }

    fn extract_whisper_data(
        tab: &std::sync::Arc<headless_chrome::Tab>,
        ticker: &str,
    ) -> Result<WhisperResult> {
        let json_str: String = tab
            .evaluate(
                r##"
            (function() {
                const result = {
                    earningsDate: null, whisper: null, consensus: null,
                    volatility: null, score: null, sentiment: null,
                    grade: null, lifecycle: null, pastBeats: null
                };
                const lines = document.body.innerText.split('\n')
                    .map(l => l.trim()).filter(l => l.length > 0);

                for (let i = 0; i < lines.length; i++) {
                    const line = lines[i];

                    if (line.includes('Earnings Date')) {
                        let parts = [];
                        for (let j = i+1; j < Math.min(i+4, lines.length); j++) {
                            if (lines[j].match(/AM|PM/i)) { parts.push(lines[j]); break; }
                            else if (lines[j].match(/Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec/i))
                                parts.push(lines[j]);
                            else if (lines[j].match(/Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday/i))
                                parts.push(lines[j]);
                        }
                        if (parts.length > 0) result.earningsDate = parts.join(' ');
                    }
                    if (line.includes('Consensus:')) {
                        const m = line.match(/Consensus:\s*\$?([\d.]+)/);
                        if (m) result.consensus = m[1];
                    }
                    if (line.includes('Volatility') && !result.volatility) {
                        for (let j = i+1; j < Math.min(i+6, lines.length); j++) {
                            const m = lines[j].match(/^([\d.]+)%$/);
                            if (m && !lines[j].includes('Revenue')) {
                                result.volatility = m[1] + '%'; break;
                            }
                        }
                    }
                    if (line === 'Score' && i+1 < lines.length) {
                        const m = lines[i+1].match(/^([\d.]+)$/);
                        if (m) result.score = m[1];
                    }
                    if (line === 'Sentiment' && i+1 < lines.length) {
                        if (lines[i+1].match(/^(Up|Flat|Down)$/i)) result.sentiment = lines[i+1];
                    }
                    if (line === 'Grade' && i+1 < lines.length) {
                        if (lines[i+1].match(/^[A-F][+-]?$/)) result.grade = lines[i+1];
                    }
                    if (line === 'Life Cycle' && i+1 < lines.length) {
                        const lc = lines[i+1];
                        if (lc.length > 0 && lc.length < 30 && !lc.includes('%'))
                            result.lifecycle = lc;
                    }
                    if ((line.toLowerCase().includes('beat') || line.toLowerCase().includes('of last'))
                        && result.pastBeats === null) {
                        const bm = line.match(/(\d+)\s+of\s+(?:the\s+)?last\s+(\d+)/i);
                        if (bm) result.pastBeats = (parseInt(bm[1]) / parseInt(bm[2])) > 0.5;
                        else if (line.toLowerCase().includes('beat')) result.pastBeats = true;
                    }
                }
                return JSON.stringify(result);
            })()
            "##,
                false,
            )
            .context("failed to evaluate JS on whisper page")?
            .value
            .ok_or_else(|| anyhow::anyhow!("no return value from JS evaluation"))?
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("JS evaluation result is not a string"))?
            .to_string();

        let data: serde_json::Value = serde_json::from_str(&json_str)
            .with_context(|| format!("failed to parse whisper JSON: {json_str}"))?;

        let get_str = |key: &str| -> Option<String> {
            data.get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty() && *s != "-")
                .map(String::from)
        };

        Ok(WhisperResult {
            ticker: ticker.to_string(),
            earnings_date: get_str("earningsDate"),
            whisper: get_str("whisper"),
            consensus: get_str("consensus"),
            volatility: get_str("volatility"),
            score: get_str("score"),
            sentiment: get_str("sentiment"),
            grade: get_str("grade"),
            lifecycle: get_str("lifecycle"),
            past_beats: data.get("pastBeats").and_then(serde_json::Value::as_bool),
        })
    }
}

// ===========================================================================
// Stub fallback (no Chrome)
// ===========================================================================

#[cfg(not(feature = "chrome"))]
mod chrome_impl {
    use super::{Result, WhisperResult};
    use anyhow::bail;

    /// Stub: returns an error explaining the `chrome` feature is required.
    ///
    /// # Errors
    ///
    /// Always returns an error.
    pub fn fetch(_ticker: &str) -> Result<WhisperResult> {
        bail!(
            "whisper scraping requires the `chrome` feature \
             (build with `cargo build --features chrome`)"
        )
    }
}

// ---------------------------------------------------------------------------
// Re-export the active implementation
// ---------------------------------------------------------------------------

pub use chrome_impl::fetch;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{WhisperResult, fetch};

    #[test]
    #[ignore] // Requires Chrome installed — launches headless browser
    #[cfg(feature = "chrome")]
    fn test_fetch_aapl() {
        let result = fetch("AAPL");
        assert!(result.is_ok(), "fetch should succeed: {:?}", result.err());
        let w = result.unwrap();
        assert_eq!(w.ticker, "AAPL");
        assert!(
            w.earnings_date.is_some() || w.consensus.is_some(),
            "Expected at least one data field populated: {w:?}",
        );
    }

    #[test]
    #[cfg(not(feature = "chrome"))]
    fn fetch_without_chrome_returns_error() {
        let result = fetch("AAPL");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("chrome"), "error should mention chrome feature");
    }

    #[test]
    fn whisper_result_default_fields() {
        let w = WhisperResult {
            ticker: "TEST".to_string(),
            earnings_date: None,
            whisper: None,
            consensus: None,
            volatility: None,
            score: None,
            sentiment: None,
            grade: None,
            lifecycle: None,
            past_beats: None,
        };
        assert_eq!(w.ticker, "TEST");
        assert!(w.earnings_date.is_none());
        assert!(w.past_beats.is_none());
    }
}
