//! Finviz screener scraper.
//!
//! Fetches the Finviz stock screener overview page and parses the HTML
//! results table into structured [`ScreenerResult`](market_core::domain::ScreenerResult) records.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use scraper::{Html, Selector};
use tracing::{debug, info, warn};

use market_core::domain::{Quote, ScreenerResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SCREENER_URL: &str = "https://finviz.com/screener.ashx";

/// User-Agent header — Finviz blocks requests without one.
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) pastel-market/",
    env!("CARGO_PKG_VERSION")
);

/// Finviz filter tokens matching the coarse-filter spec:
///
/// | Filter              | Token            |
/// |---------------------|------------------|
/// | Market Cap > $300M  | `cap_smallover`  |
/// | P/E < 25            | `fa_pe_u25`      |
/// | EPS QoQ Growth > 15%| `fa_epsqoq_o15`  |
/// | Price > SMA 50      | `ta_sma50_pa`    |
/// | Price > SMA 200     | `ta_sma200_pa`   |
/// | Avg Volume > 500K   | `sh_avgvol_o500` |
/// | Beta < 1.5          | `ta_beta_u1.5`   |
const FILTERS: &[&str] = &[
    "cap_smallover",
    "fa_pe_u25",
    "fa_epsqoq_o15",
    "ta_sma50_pa",
    "ta_sma200_pa",
    "sh_avgvol_o500",
    "ta_beta_u1.5",
];

/// Number of rows Finviz returns per page.
const PAGE_SIZE: usize = 20;

/// Safety cap so we never loop forever if the HTML changes.
const MAX_PAGES: usize = 50;

/// Delay between page fetches to avoid Finviz 429 rate-limiting.
const PAGE_DELAY: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// URL builders
// ---------------------------------------------------------------------------

/// Build the base screener URL without pagination offset.
#[cfg(test)]
#[must_use]
pub fn build_url() -> String {
    let filter_str: String = FILTERS.join(",");
    format!("{SCREENER_URL}?v=111&f={filter_str}")
}

/// Build a paginated screener URL.
///
/// `start_row` is 1-based: page 1 -> r=1, page 2 -> r=21, etc.
#[must_use]
fn build_url_page(start_row: usize) -> String {
    let filter_str: String = FILTERS.join(",");
    format!("{SCREENER_URL}?v=111&f={filter_str}&r={start_row}")
}

// ---------------------------------------------------------------------------
// HTML parsing
// ---------------------------------------------------------------------------

/// Parse a single page of screener results from raw HTML.
///
/// Returns an empty `Vec` when no data rows are found (normal for
/// pagination past the last page).
///
/// A canary check validates the header row contains "Ticker" — if the
/// Finviz HTML structure changes, this returns an error immediately
/// rather than silently producing garbage data.
///
/// # Errors
///
/// Returns an error if the HTML header row is present but the "Ticker"
/// column is missing.
///
/// # Panics
///
/// Panics if any of the hard-coded CSS selectors fail to parse (compile-time constants).
pub fn parse_page(html: &str) -> Result<Vec<ScreenerResult>> {
    let document = Html::parse_document(html);

    // SAFETY: selectors are compile-time constants; parsing cannot fail.
    let table_sel =
        Selector::parse("table.screener_table").expect("valid selector: table.screener_table");
    let row_sel = Selector::parse("tr.styled-row").expect("valid selector: tr.styled-row");
    let cell_sel = Selector::parse("td").expect("valid selector: td");
    let th_sel = Selector::parse("th").expect("valid selector: th");

    let Some(table) = document.select(&table_sel).next() else {
        return Ok(Vec::new());
    };

    // Canary: verify the header row contains "Ticker".
    let headers: Vec<String> = table
        .select(&th_sel)
        .map(|th| th.text().collect::<String>().trim().to_string())
        .collect();
    if !headers.is_empty() && !headers.iter().any(|h| h == "Ticker") {
        bail!(
            "Finviz HTML structure changed: header row missing \"Ticker\" column \
             (found: {headers:?})"
        );
    }

    let mut results = Vec::new();

    for row in table.select(&row_sel) {
        let cells: Vec<String> = row
            .select(&cell_sel)
            .map(|td| td.text().collect::<String>().trim().to_string())
            .collect();

        // Overview rows have 11 columns (No. through Volume).
        if cells.len() < 11 {
            continue;
        }

        results.push(ScreenerResult {
            ticker: cells[1].clone(),
            company: cells[2].clone(),
            sector: cells[3].clone(),
            industry: cells[4].clone(),
            market_cap: cells[6].clone(),
            pe: cells[7].clone(),
            price: cells[8].clone(),
            change: cells[9].clone(),
            volume: cells[10].clone(),
        });
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Conversion: ScreenerResult -> Quote
// ---------------------------------------------------------------------------

/// Convert a [`ScreenerResult`] into a [`Quote`] for display in the scanner table.
///
/// String fields are parsed to numeric types. Unparseable values default to zero.
#[must_use]
pub fn screener_result_to_quote(sr: &ScreenerResult) -> Quote {
    let price = parse_price(&sr.price);
    let change_pct = parse_change_percent(&sr.change);
    let volume = parse_volume(&sr.volume);

    // Derive absolute change from percentage.
    let change_abs = if (100.0 + change_pct).abs() > f64::EPSILON {
        price * change_pct / (100.0 + change_pct)
    } else {
        0.0
    };

    let prev_close = price - change_abs;

    Quote {
        symbol: sr.ticker.clone(),
        short_name: Some(sr.company.clone()),
        sector: Some(sr.sector.clone()),
        market_state: Some("REGULAR".to_string()),
        regular_market_price: price,
        regular_market_change: change_abs,
        regular_market_change_percent: change_pct,
        regular_market_volume: volume,
        regular_market_previous_close: prev_close,
        regular_market_open: prev_close,
        regular_market_day_high: price,
        regular_market_day_low: price,
        fifty_two_week_high: 0.0,
        fifty_two_week_low: 0.0,
    }
}

/// Parse a price string like "195.50" or "1,234.56" into f64.
fn parse_price(s: &str) -> f64 {
    s.replace(',', "").parse::<f64>().unwrap_or(0.0)
}

/// Parse a change percentage string like "1.20%" or "-0.30%" into f64.
fn parse_change_percent(s: &str) -> f64 {
    s.trim_end_matches('%')
        .replace(',', "")
        .parse::<f64>()
        .unwrap_or(0.0)
}

/// Parse a volume string like "55,000,000" or "47M" into u64.
fn parse_volume(s: &str) -> u64 {
    let clean = s.replace(',', "");
    if let Ok(v) = clean.parse::<u64>() {
        return v;
    }
    let (num_part, multiplier) = if let Some(n) = clean.strip_suffix('M') {
        (n, 1_000_000.0)
    } else if let Some(n) = clean.strip_suffix('B') {
        (n, 1_000_000_000.0)
    } else if let Some(n) = clean.strip_suffix('K') {
        (n, 1_000.0)
    } else {
        return 0;
    };
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    {
        num_part
            .parse::<f64>()
            .map_or(0, |n| (n * multiplier) as u64)
    }
}

// ---------------------------------------------------------------------------
// Network fetch
// ---------------------------------------------------------------------------

/// Fetch all pages of Finviz screener results and convert to [`Quote`].
///
/// Paginates through `&r=1`, `&r=21`, `&r=41`, ... until a page returns
/// fewer than [`PAGE_SIZE`] rows or the safety cap is reached.
///
/// # Errors
///
/// Returns an error if any page request fails or if the HTML structure
/// has changed (canary check fails).
pub fn fetch() -> Result<Vec<Quote>> {
    let agent = ureq::Agent::new_with_defaults();

    let mut all_results: Vec<ScreenerResult> = Vec::new();

    for page in 0..MAX_PAGES {
        let start_row = page * PAGE_SIZE + 1;
        let url = build_url_page(start_row);

        debug!(page, start_row, "fetching Finviz screener page");

        let mut response = agent
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .with_context(|| format!("Finviz request failed (page {page})"))?;

        let status = response.status();
        if status != 200 {
            warn!(status = %status, page, "Finviz returned non-200 status");
            bail!("Finviz returned HTTP {status} (page {page})");
        }

        let html = response
            .body_mut()
            .read_to_string()
            .with_context(|| format!("failed to read Finviz response body (page {page})"))?;

        let page_results =
            parse_page(&html).with_context(|| format!("failed to parse screener page {page}"))?;

        let count = page_results.len();
        all_results.extend(page_results);

        if count < PAGE_SIZE {
            break;
        }

        std::thread::sleep(PAGE_DELAY);
    }

    info!(total = all_results.len(), "Finviz screener fetch complete");

    Ok(all_results.iter().map(screener_result_to_quote).collect())
}

/// Fetch all pages of raw [`ScreenerResult`] records (without Quote conversion).
///
/// # Errors
///
/// Same as [`fetch`].
pub fn fetch_raw() -> Result<Vec<ScreenerResult>> {
    let agent = ureq::Agent::new_with_defaults();

    let mut all_results: Vec<ScreenerResult> = Vec::new();

    for page in 0..MAX_PAGES {
        let start_row = page * PAGE_SIZE + 1;
        let url = build_url_page(start_row);

        debug!(page, start_row, "fetching Finviz screener page (raw)");

        let mut response = agent
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .with_context(|| format!("Finviz request failed (page {page})"))?;

        let status = response.status();
        if status != 200 {
            bail!("Finviz returned HTTP {status} (page {page})");
        }

        let html = response
            .body_mut()
            .read_to_string()
            .with_context(|| format!("failed to read Finviz response body (page {page})"))?;

        let page_results =
            parse_page(&html).with_context(|| format!("failed to parse screener page {page}"))?;

        let count = page_results.len();
        all_results.extend(page_results);

        if count < PAGE_SIZE {
            break;
        }

        std::thread::sleep(PAGE_DELAY);
    }

    if all_results.is_empty() {
        bail!("Screener returned 0 results -- filters may be too strict or HTML changed");
    }

    Ok(all_results)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL builder tests ---

    #[test]
    fn build_url_contains_all_filters() {
        let url = build_url();
        assert!(url.starts_with("https://finviz.com/screener.ashx?v=111&f="));
        for f in FILTERS {
            assert!(url.contains(f), "URL missing filter token: {f}");
        }
    }

    #[test]
    fn build_url_page_includes_start_row() {
        let url = build_url_page(21);
        assert!(url.contains("&r=21"), "URL should contain &r=21: {url}");
    }

    // --- parse helpers ---

    #[test]
    fn parse_price_simple() {
        assert!((parse_price("195.50") - 195.50).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_price_with_commas() {
        assert!((parse_price("1,234.56") - 1234.56).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_price_invalid_returns_zero() {
        assert!((parse_price("-") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_change_percent_positive() {
        assert!((parse_change_percent("1.20%") - 1.20).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_change_percent_negative() {
        assert!((parse_change_percent("-0.30%") - (-0.30)).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_volume_with_commas() {
        assert_eq!(parse_volume("55,000,000"), 55_000_000);
    }

    #[test]
    fn parse_volume_suffix_m() {
        assert_eq!(parse_volume("47M"), 47_000_000);
    }

    #[test]
    fn parse_volume_suffix_b() {
        assert_eq!(parse_volume("1.5B"), 1_500_000_000);
    }

    #[test]
    fn parse_volume_suffix_k() {
        assert_eq!(parse_volume("500K"), 500_000);
    }

    #[test]
    fn parse_volume_invalid_returns_zero() {
        assert_eq!(parse_volume("-"), 0);
    }

    // --- ScreenerResult -> Quote conversion ---

    #[test]
    fn screener_result_to_quote_basic() {
        let sr = ScreenerResult {
            ticker: "AAPL".into(),
            company: "Apple Inc.".into(),
            sector: "Technology".into(),
            industry: "Consumer Electronics".into(),
            market_cap: "3.20T".into(),
            pe: "32.50".into(),
            price: "195.50".into(),
            change: "1.20%".into(),
            volume: "55,000,000".into(),
        };
        let q = screener_result_to_quote(&sr);
        assert_eq!(q.symbol, "AAPL");
        assert_eq!(q.short_name.as_deref(), Some("Apple Inc."));
        assert!((q.regular_market_price - 195.50).abs() < f64::EPSILON);
        assert!((q.regular_market_change_percent - 1.20).abs() < f64::EPSILON);
        assert_eq!(q.regular_market_volume, 55_000_000);
    }

    #[test]
    fn screener_result_to_quote_negative_change() {
        let sr = ScreenerResult {
            ticker: "MSFT".into(),
            company: "Microsoft Corp".into(),
            sector: "Technology".into(),
            industry: "Software".into(),
            market_cap: "2.90T".into(),
            pe: "35.10".into(),
            price: "420.00".into(),
            change: "-0.30%".into(),
            volume: "22,000,000".into(),
        };
        let q = screener_result_to_quote(&sr);
        assert!(q.regular_market_change < 0.0);
        assert!(q.regular_market_change_percent < 0.0);
    }

    // --- HTML parsing tests ---

    #[test]
    fn parse_empty_html_returns_empty() {
        let result = parse_page("<html><body></body></html>").expect("should not error");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_synthetic_two_row_table() {
        let html = r#"
        <html><body>
        <table class="styled-table-new screener_table">
        <thead><tr><th>No.</th><th>Ticker</th><th>Company</th><th>Sector</th>
        <th>Industry</th><th>Country</th><th>Market Cap</th><th>P/E</th>
        <th>Price</th><th>Change</th><th>Volume</th></tr></thead>
        <tr class="styled-row"><td>1</td><td>AAPL</td><td>Apple Inc.</td>
        <td>Technology</td><td>Consumer Electronics</td><td>USA</td>
        <td>3.20T</td><td>32.50</td><td>195.50</td><td>1.20%</td>
        <td>55,000,000</td></tr>
        <tr class="styled-row"><td>2</td><td>MSFT</td><td>Microsoft Corp</td>
        <td>Technology</td><td>Software</td><td>USA</td>
        <td>2.90T</td><td>35.10</td><td>420.00</td><td>-0.30%</td>
        <td>22,000,000</td></tr>
        </table></body></html>"#;

        let results = parse_page(html).expect("should parse synthetic table");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].ticker, "AAPL");
        assert_eq!(results[0].company, "Apple Inc.");
        assert_eq!(results[0].sector, "Technology");
        assert_eq!(results[0].industry, "Consumer Electronics");
        assert_eq!(results[0].market_cap, "3.20T");
        assert_eq!(results[0].pe, "32.50");
        assert_eq!(results[0].price, "195.50");
        assert_eq!(results[0].change, "1.20%");
        assert_eq!(results[0].volume, "55,000,000");
        assert_eq!(results[1].ticker, "MSFT");
    }

    #[test]
    fn parse_skips_rows_with_insufficient_columns() {
        let html = r#"
        <html><body>
        <table class="screener_table">
        <tr class="styled-row"><td>1</td><td>AAPL</td><td>Apple</td></tr>
        <tr class="styled-row"><td>2</td><td>MSFT</td><td>Microsoft</td>
        <td>Technology</td><td>Software</td><td>USA</td>
        <td>2.9T</td><td>35</td><td>420</td><td>-0.3%</td>
        <td>22M</td></tr>
        </table></body></html>"#;

        let results = parse_page(html).expect("should parse valid rows");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].ticker, "MSFT");
    }

    #[test]
    fn parse_canary_detects_changed_headers() {
        let html = r#"<html><body>
            <table class="screener_table">
            <thead><tr><th>Num</th><th>Symbol</th><th>Name</th></tr></thead>
            <tr class="styled-row"><td>1</td><td>AAPL</td><td>Apple</td></tr>
            </table></body></html>"#;
        let result = parse_page(html);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Ticker"));
    }

    /// Helper: generate a synthetic HTML page containing `count` screener rows.
    fn make_page_html(start_no: usize, count: usize) -> String {
        use std::fmt::Write;
        let mut rows = String::new();
        for i in 0..count {
            let no = start_no + i;
            let ticker = format!("T{no:03}");
            let _ = write!(
                rows,
                r#"<tr class="styled-row"><td>{no}</td><td>{ticker}</td><td>Company {no}</td>
                <td>Technology</td><td>Software</td><td>USA</td>
                <td>1B</td><td>20</td><td>{price}</td><td>0.5%</td>
                <td>1,000,000</td></tr>"#,
                price = 100 + no,
            );
        }
        format!(
            r#"<html><body>
            <table class="screener_table">
            <thead><tr><th>No.</th><th>Ticker</th><th>Company</th><th>Sector</th>
            <th>Industry</th><th>Country</th><th>Market Cap</th><th>P/E</th>
            <th>Price</th><th>Change</th><th>Volume</th></tr></thead>
            {rows}
            </table></body></html>"#
        )
    }

    #[test]
    fn parse_full_page_20_rows() {
        let html = make_page_html(1, 20);
        let results = parse_page(&html).expect("should parse full page");
        assert_eq!(results.len(), 20);
        assert_eq!(results[0].ticker, "T001");
        assert_eq!(results[19].ticker, "T020");
    }

    #[test]
    fn parse_partial_last_page() {
        let html = make_page_html(181, 7);
        let results = parse_page(&html).expect("should parse partial page");
        assert_eq!(results.len(), 7);
    }

    #[test]
    fn parse_multi_page_simulation() {
        let mut all: Vec<ScreenerResult> = Vec::new();
        for page in 0..10 {
            let start_no = page * PAGE_SIZE + 1;
            let html = make_page_html(start_no, PAGE_SIZE);
            let page_results = parse_page(&html).expect("should parse page");
            assert_eq!(page_results.len(), PAGE_SIZE);
            all.extend(page_results);
        }
        assert_eq!(all.len(), 200);
        assert_eq!(all[0].ticker, "T001");
        assert_eq!(all[199].ticker, "T200");
    }

    // --- Fixture tests ---

    #[test]
    fn fixture_parse_screener_page1() {
        let html = include_str!("../../../tests/fixtures/finviz_screener_page1.html");
        let results = parse_page(html).expect("fixture page1 should parse");
        assert_eq!(results.len(), 5, "page1 fixture has 5 rows");
        assert_eq!(results[0].ticker, "AAPL");
        assert_eq!(results[0].company, "Apple Inc.");
        assert_eq!(results[0].sector, "Technology");
        assert_eq!(results[0].market_cap, "3.58T");
        assert_eq!(results[1].ticker, "MSFT");
        assert_eq!(results[2].ticker, "GOOG");
        assert_eq!(results[3].ticker, "AMZN");
        assert_eq!(results[4].ticker, "META");
    }

    #[test]
    fn fixture_parse_screener_empty_page() {
        let html = include_str!("../../../tests/fixtures/finviz_screener_empty.html");
        let results = parse_page(html).expect("empty fixture should parse");
        assert!(results.is_empty());
    }

    #[test]
    fn fixture_parse_screener_changed_headers_detected() {
        let html = include_str!("../../../tests/fixtures/finviz_screener_changed_headers.html");
        let result = parse_page(html);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Ticker"));
    }

    #[test]
    fn filters_has_7_entries() {
        assert_eq!(FILTERS.len(), 7);
    }

    #[test]
    fn page_size_is_20() {
        assert_eq!(PAGE_SIZE, 20);
    }
}
