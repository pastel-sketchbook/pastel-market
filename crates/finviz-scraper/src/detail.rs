//! Per-stock detail scraping from Finviz (insider ownership).

use std::time::Duration;

use anyhow::{Context, Result, bail};
use scraper::{Html, Selector};

const DETAIL_URL: &str = "https://finviz.com/quote.ashx";

/// User-Agent header — Finviz blocks requests without one.
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) pastel-market/",
    env!("CARGO_PKG_VERSION")
);

/// Maximum number of concurrent threads for insider ownership fetches.
const INSIDER_CONCURRENCY: usize = 5;

/// Delay between consecutive requests within a single worker thread.
const INSIDER_REQUEST_DELAY: Duration = Duration::from_millis(200);

/// Fetch insider ownership percentage for a single ticker.
///
/// # Errors
///
/// Returns an error if the request fails or the HTML cannot be parsed.
pub fn fetch_insider_ownership(ticker: &str) -> Result<f64> {
    let agent = ureq::Agent::new_with_defaults();
    let url = format!("{}?ty={}", DETAIL_URL, ticker.to_lowercase());

    let mut response = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("insider ownership request failed for {ticker}"))?;

    let status = response.status();
    if status != 200 {
        bail!("Finviz returned HTTP {status} for {ticker}");
    }

    let html = response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("failed to read insider ownership response for {ticker}"))?;

    parse_insider_ownership(&html)
}

/// Parse insider ownership percentage from Finviz detail page HTML.
///
/// # Errors
///
/// Returns an error if the ownership field is not found or cannot be parsed.
///
/// # Panics
///
/// Panics if any of the hard-coded CSS selectors fail to parse (compile-time constants).
pub fn parse_insider_ownership(html: &str) -> Result<f64> {
    let document = Html::parse_document(html);

    // SAFETY: selectors are compile-time constants; parsing cannot fail.
    let table_sel = Selector::parse("table.screener_table").expect("valid selector");
    let row_sel = Selector::parse("tr.screener-body-table-row").expect("valid selector");
    let td_sel = Selector::parse("td").expect("valid selector");

    for table in document.select(&table_sel) {
        for row in table.select(&row_sel) {
            let cells: Vec<String> = row
                .select(&td_sel)
                .map(|td| td.text().collect::<String>().trim().to_string())
                .collect();

            if cells.len() >= 2 && cells[0].contains("Insider") && cells[0].contains("Ownership") {
                let pct_str = cells[1].trim_end_matches('%');
                return pct_str
                    .parse::<f64>()
                    .with_context(|| format!("failed to parse insider ownership: {}", cells[1]));
            }
        }
    }

    bail!("insider ownership not found in Finviz page")
}

/// Fetch insider ownership for multiple tickers concurrently.
///
/// Distributes tickers across up to [`INSIDER_CONCURRENCY`] scoped threads.
/// Results are sent through `tx` as they arrive.
///
/// This function blocks until all workers complete.
pub fn fetch_insider_ownership_parallel(
    tickers: &[String],
    tx: &std::sync::mpsc::Sender<(String, Result<f64>)>,
) {
    if tickers.is_empty() {
        return;
    }

    let chunk_size = tickers.len().div_ceil(INSIDER_CONCURRENCY);
    let chunks: Vec<&[String]> = tickers.chunks(chunk_size).collect();

    std::thread::scope(|s| {
        for chunk in chunks {
            let tx = tx.clone();
            s.spawn(move || {
                for (i, ticker) in chunk.iter().enumerate() {
                    if i > 0 {
                        std::thread::sleep(INSIDER_REQUEST_DELAY);
                    }
                    let result = fetch_insider_ownership(ticker);
                    let _ = tx.send((ticker.clone(), result));
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_insider_ownership_synthetic() {
        let html = r#"
        <html><body>
        <table class="screener_table">
        <tr class="screener-body-table-row"><td>Market Cap</td><td>3.2T</td></tr>
        <tr class="screener-body-table-row"><td>Insider Ownership</td><td>0.25%</td></tr>
        </table></body></html>"#;

        let result = parse_insider_ownership(html);
        assert!(result.is_ok());
        assert!((result.unwrap() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_insider_ownership_not_found() {
        let html = r#"
        <html><body>
        <table class="screener_table">
        <tr class="screener-body-table-row"><td>Market Cap</td><td>3.2T</td></tr>
        </table></body></html>"#;

        assert!(parse_insider_ownership(html).is_err());
    }

    #[test]
    fn fixture_parse_insider_ownership_aapl() {
        let html = include_str!("../../../tests/fixtures/finviz_detail_aapl.html");
        let pct = parse_insider_ownership(html).expect("fixture should parse insider ownership");
        assert!(
            (pct - 0.07).abs() < 0.001,
            "AAPL insider ownership should be 0.07%, got {pct}"
        );
    }

    #[test]
    fn parallel_empty_tickers_sends_nothing() {
        let (tx, rx) = std::sync::mpsc::channel();
        fetch_insider_ownership_parallel(&[], &tx);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn parallel_chunking_distributes_evenly() {
        let tickers: Vec<String> = (0..7).map(|i| format!("T{i}")).collect();
        let chunk_size = tickers.len().div_ceil(INSIDER_CONCURRENCY);
        assert_eq!(chunk_size, 2);
        let chunks: Vec<&[String]> = tickers.chunks(chunk_size).collect();
        assert_eq!(chunks.len(), 4);
    }
}
