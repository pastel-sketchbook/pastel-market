//! Integration tests that exercise HTML parsers against fixture files.
//!
//! Each fixture in `tests/fixtures/` represents a captured Finviz or Yahoo page
//! to ensure parsers handle real-world markup correctly.

use std::path::Path;

/// Read a fixture file relative to the workspace root.
fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Finviz screener: parse_page
// ---------------------------------------------------------------------------

#[test]
fn parse_screener_page1_returns_five_stocks() {
    let html = fixture("finviz_screener_page1.html");
    let results = finviz_scraper::screener::parse_page(&html).unwrap();

    assert_eq!(results.len(), 5, "expected 5 rows in fixture");

    // Spot-check first and last.
    assert_eq!(results[0].ticker, "AAPL");
    assert_eq!(results[0].company, "Apple Inc.");
    assert_eq!(results[0].sector, "Technology");
    assert_eq!(results[0].price, "233.22");
    assert_eq!(results[0].change, "0.42%");

    assert_eq!(results[4].ticker, "META");
    assert_eq!(results[4].price, "626.38");
    assert_eq!(results[4].change, "2.31%");
}

#[test]
fn parse_screener_empty_returns_empty_vec() {
    let html = fixture("finviz_screener_empty.html");
    let results = finviz_scraper::screener::parse_page(&html).unwrap();
    assert!(results.is_empty(), "empty fixture should yield no results");
}

#[test]
fn parse_screener_changed_headers_detects_structure_change() {
    let html = fixture("finviz_screener_changed_headers.html");
    let result = finviz_scraper::screener::parse_page(&html);

    // The header row has "Symbol" instead of "Ticker", so the canary check
    // should bail with an error about missing "Ticker" column.
    assert!(
        result.is_err(),
        "expected error when headers don't contain 'Ticker'"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("Ticker"),
        "error should mention missing Ticker column, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Finviz detail: parse_insider_ownership
// ---------------------------------------------------------------------------

#[test]
fn parse_insider_ownership_from_detail_page() {
    let html = fixture("finviz_detail_aapl.html");
    let pct = finviz_scraper::detail::parse_insider_ownership(&html).unwrap();
    assert!(
        (pct - 0.07).abs() < f64::EPSILON,
        "expected 0.07%, got {pct}"
    );
}

// ---------------------------------------------------------------------------
// Finviz ETF: parse_etf_change
// ---------------------------------------------------------------------------

#[test]
fn parse_etf_change_from_xlk_page() {
    let html = fixture("finviz_etf_xlk.html");
    let change = finviz_scraper::sector::parse_etf_change(&html).unwrap();
    assert!(
        (change - 0.52).abs() < f64::EPSILON,
        "expected 0.52%, got {change}"
    );
}
