//! Sector heat mapping via Finviz ETF performance data.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use scraper::{Html, Selector};

/// User-Agent header — Finviz blocks requests without one.
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) pastel-market/",
    env!("CARGO_PKG_VERSION")
);

/// Map Finviz sector name to the corresponding sector ETF ticker.
#[must_use]
pub fn sector_to_etf(sector: &str) -> Option<&'static str> {
    match sector.to_lowercase().as_str() {
        "technology" => Some("XLK"),
        "financial" | "financial services" => Some("XLF"),
        "healthcare" | "medical" => Some("XLV"),
        "consumer cyclical" | "consumer discretionary" => Some("XLY"),
        "consumer defensive" | "consumer staples" => Some("XLP"),
        "energy" => Some("XLE"),
        "industrials" => Some("XLI"),
        "real estate" => Some("XLRE"),
        "utilities" => Some("XLU"),
        "communication services" | "communications" => Some("XLC"),
        "basic materials" | "materials" => Some("XLB"),
        _ => None,
    }
}

/// Fetch the daily change percentage for a single ETF from Finviz.
///
/// # Errors
///
/// Returns an error if the request fails or the change cannot be parsed.
pub fn fetch_etf_performance(etf_ticker: &str) -> Result<f64> {
    let agent = ureq::Agent::new_with_defaults();
    let url = format!(
        "https://finviz.com/quote.ashx?ty={}",
        etf_ticker.to_lowercase()
    );

    let mut response = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("ETF request failed for {etf_ticker}"))?;

    if response.status() != 200 {
        bail!(
            "Finviz returned HTTP {} for ETF {}",
            response.status(),
            etf_ticker
        );
    }

    let html = response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("failed to read ETF response for {etf_ticker}"))?;

    parse_etf_change(&html)
}

/// Parse the daily change percentage from a Finviz ETF page.
///
/// # Errors
///
/// Returns an error if the change field cannot be found or parsed.
///
/// # Panics
///
/// Panics if any of the hard-coded CSS selectors fail to parse (compile-time constants).
pub fn parse_etf_change(html: &str) -> Result<f64> {
    let document = Html::parse_document(html);

    // SAFETY: selector is a compile-time constant; parsing cannot fail.
    let td_sel = Selector::parse("td").expect("valid selector: td");

    let selectors = [
        "table.screener_table tr.screener-body-table-row",
        "table.screener_table tr",
    ];

    for sel in &selectors {
        if let Ok(selector) = Selector::parse(sel) {
            for row in document.select(&selector) {
                let cells: Vec<String> = row
                    .select(&td_sel)
                    .map(|td| td.text().collect::<String>().trim().to_string())
                    .collect();

                if cells.len() >= 2 && cells[0].contains("Change") {
                    let change_str = cells[1].trim_end_matches('%');
                    return change_str
                        .parse::<f64>()
                        .with_context(|| format!("failed to parse ETF change: {}", cells[1]));
                }
            }
        }
    }

    bail!("could not find price change in ETF page")
}

/// Fetch sector heat vs SPY for a set of sectors.
///
/// Returns a map from sector name to (sector ETF change - SPY change).
/// Sectors without a known ETF mapping or where the fetch fails are omitted.
#[must_use]
pub fn fetch_sector_heat(sectors: &[String]) -> HashMap<String, f64> {
    let spy_change = fetch_etf_performance("SPY").unwrap_or(0.0);

    let mut result = HashMap::new();
    for sector in sectors {
        if result.contains_key(sector.as_str()) {
            continue;
        }
        if let Some(etf) = sector_to_etf(sector)
            && let Ok(etf_change) = fetch_etf_performance(etf)
        {
            result.insert(sector.clone(), etf_change - spy_change);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector_to_etf_known_sectors() {
        assert_eq!(sector_to_etf("Technology"), Some("XLK"));
        assert_eq!(sector_to_etf("Financial"), Some("XLF"));
        assert_eq!(sector_to_etf("Financial Services"), Some("XLF"));
        assert_eq!(sector_to_etf("Healthcare"), Some("XLV"));
        assert_eq!(sector_to_etf("Energy"), Some("XLE"));
        assert_eq!(sector_to_etf("Consumer Cyclical"), Some("XLY"));
        assert_eq!(sector_to_etf("Consumer Defensive"), Some("XLP"));
        assert_eq!(sector_to_etf("Industrials"), Some("XLI"));
        assert_eq!(sector_to_etf("Real Estate"), Some("XLRE"));
        assert_eq!(sector_to_etf("Utilities"), Some("XLU"));
        assert_eq!(sector_to_etf("Communication Services"), Some("XLC"));
        assert_eq!(sector_to_etf("Basic Materials"), Some("XLB"));
    }

    #[test]
    fn sector_to_etf_unknown_returns_none() {
        assert_eq!(sector_to_etf("Alien Technology"), None);
    }

    #[test]
    fn sector_to_etf_case_insensitive() {
        assert_eq!(sector_to_etf("technology"), Some("XLK"));
        assert_eq!(sector_to_etf("TECHNOLOGY"), Some("XLK"));
    }

    #[test]
    fn fixture_parse_etf_change_xlk() {
        let html = include_str!("../../../tests/fixtures/finviz_etf_xlk.html");
        let change = parse_etf_change(html).expect("fixture should parse ETF change");
        assert!(
            (change - 0.52).abs() < 0.001,
            "XLK change should be 0.52%, got {change}"
        );
    }
}
