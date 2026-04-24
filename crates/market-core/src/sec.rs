//! SEC EDGAR filing fetcher.
//!
//! Fetches recent filings for a company from the SEC EDGAR
//! submissions API. No authentication required — only a descriptive
//! `User-Agent` header (SEC policy).
//!
//! CIK resolution uses an embedded RON map (`data/cik_map.ron`)
//! compiled into the binary via `include_str!`. No runtime download
//! is needed for ticker → CIK mapping.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::debug;

use crate::domain::SecFiling;
use crate::http::USER_AGENT;

/// EDGAR company filings endpoint (submissions).
const SUBMISSIONS_URL: &str = "https://data.sec.gov/submissions/CIK";

/// Timeout for SEC HTTP requests.
const SEC_TIMEOUT: Duration = Duration::from_secs(5);

/// Embedded CIK map: ticker → zero-padded CIK string.
/// Generated from <https://www.sec.gov/files/company_tickers.json>.
const CIK_MAP_RON: &str = include_str!("../../../data/cik_map.ron");

/// Parsed CIK map, lazily initialized on first use.
static CIK_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Get the parsed CIK map, initializing it once from the embedded RON.
fn cik_map() -> &'static HashMap<String, String> {
    CIK_MAP.get_or_init(|| {
        ron::from_str(CIK_MAP_RON).unwrap_or_else(|e| {
            // Safety: the embedded RON is generated at build time and
            // validated by tests. If it's somehow corrupt, return empty
            // map and let resolve_cik fail with a clear error.
            tracing::error!(error = %e, "failed to parse embedded CIK map");
            HashMap::new()
        })
    })
}

/// Build a ureq agent with a short timeout for SEC requests.
fn sec_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(SEC_TIMEOUT))
        .build()
        .new_agent()
}

/// Fetch recent SEC filings for the given ticker.
///
/// # Errors
///
/// Returns an error if the ticker cannot be resolved or SEC is unreachable.
pub fn fetch_sec_filings(ticker: &str) -> Result<Vec<SecFiling>> {
    let cik = resolve_cik(ticker).with_context(|| format!("failed to resolve CIK for {ticker}"))?;
    let filings = fetch_filings_by_cik(&cik)
        .with_context(|| format!("failed to fetch filings for CIK {cik}"))?;
    debug!(ticker, cik = %cik, count = filings.len(), "fetched SEC filings");
    Ok(filings)
}

// ---------------------------------------------------------------------------
// CIK resolution
// ---------------------------------------------------------------------------

/// Resolve a ticker symbol to a zero-padded CIK.
///
/// Looks up the embedded CIK map (compiled into binary, ~10K tickers).
fn resolve_cik(ticker: &str) -> Result<String> {
    let upper = ticker.to_uppercase();
    cik_map()
        .get(&upper)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("ticker {ticker} not found in embedded CIK map"))
}

// ---------------------------------------------------------------------------
// Filings fetch + parse
// ---------------------------------------------------------------------------

/// Fetch filings from the EDGAR submissions endpoint for a given CIK.
fn fetch_filings_by_cik(cik: &str) -> Result<Vec<SecFiling>> {
    let url = format!("{SUBMISSIONS_URL}{cik}.json");
    let mut body = sec_agent()
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .call()
        .context("SEC submissions request failed")?
        .into_body();

    let json: serde_json::Value = body
        .read_json()
        .context("failed to parse SEC submissions JSON")?;

    parse_submissions(&json, cik)
}

/// Parse the EDGAR submissions JSON into `SecFiling` items.
fn parse_submissions(json: &serde_json::Value, cik: &str) -> Result<Vec<SecFiling>> {
    let recent = &json["filings"]["recent"];

    let forms = recent["form"]
        .as_array()
        .context("missing filings.recent.form array")?;
    let dates = recent["filingDate"]
        .as_array()
        .context("missing filings.recent.filingDate array")?;
    let docs = recent["primaryDocument"]
        .as_array()
        .context("missing filings.recent.primaryDocument array")?;
    let accessions = recent["accessionNumber"]
        .as_array()
        .context("missing filings.recent.accessionNumber array")?;
    let descriptions = recent["primaryDocDescription"].as_array();

    let mut filings = Vec::new();
    let limit = forms.len().min(20);

    for i in 0..limit {
        let form_type = forms[i].as_str().unwrap_or("").to_string();
        let filed_date = dates[i].as_str().unwrap_or("").to_string();
        let doc = docs[i].as_str().unwrap_or("");
        let accession_raw = accessions[i].as_str().unwrap_or("");
        let description = descriptions
            .and_then(|d| d.get(i))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let accession_nodash = accession_raw.replace('-', "");
        let link = if doc.is_empty() {
            format!(
                "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}&type=&dateb=&owner=include&count=20"
            )
        } else {
            format!(
                "https://www.sec.gov/Archives/edgar/data/{}/{}/{}",
                cik.trim_start_matches('0'),
                accession_nodash,
                doc
            )
        };

        filings.push(SecFiling {
            form_type,
            filed_date,
            description,
            link,
            accession: accession_raw.to_string(),
        });
    }

    Ok(filings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_cik_map_parses() {
        let map = cik_map();
        assert!(
            map.len() > 10_000,
            "expected >10K tickers, got {}",
            map.len()
        );
    }

    #[test]
    fn resolve_common_tickers() {
        assert_eq!(resolve_cik("AAPL").unwrap(), "0000320193");
        assert_eq!(resolve_cik("MSFT").unwrap(), "0000789019");
        assert_eq!(resolve_cik("TSLA").unwrap(), "0001318605");
        assert_eq!(resolve_cik("NVDA").unwrap(), "0001045810");
    }

    #[test]
    fn resolve_cik_case_insensitive() {
        assert_eq!(resolve_cik("aapl").unwrap(), "0000320193");
        assert_eq!(resolve_cik("Msft").unwrap(), "0000789019");
    }

    #[test]
    fn resolve_unknown_ticker_fails() {
        assert!(resolve_cik("ZZZZZZ999").is_err());
    }

    #[test]
    fn parse_submissions_extracts_filings() {
        let json = serde_json::json!({
            "filings": {
                "recent": {
                    "form": ["10-K", "10-Q", "8-K"],
                    "filingDate": ["2024-10-30", "2024-07-31", "2024-06-15"],
                    "primaryDocument": ["aapl-20240928.htm", "aapl-20240629.htm", ""],
                    "accessionNumber": ["0000320193-24-000123", "0000320193-24-000089", "0000320193-24-000045"],
                    "primaryDocDescription": ["Annual Report", "Quarterly Report", "Current Report"]
                }
            }
        });

        let filings = parse_submissions(&json, "0000320193").unwrap();
        assert_eq!(filings.len(), 3);
        assert_eq!(filings[0].form_type, "10-K");
        assert_eq!(filings[0].filed_date, "2024-10-30");
        assert_eq!(filings[0].description, "Annual Report");
        assert!(filings[0].link.contains("aapl-20240928.htm"));
        assert_eq!(filings[1].form_type, "10-Q");
        assert!(filings[2].link.contains("browse-edgar"));
    }

    #[test]
    fn parse_submissions_handles_missing_descriptions() {
        let json = serde_json::json!({
            "filings": {
                "recent": {
                    "form": ["4"],
                    "filingDate": ["2024-11-01"],
                    "primaryDocument": ["xslF345X05/doc.xml"],
                    "accessionNumber": ["0001-24-000999"]
                }
            }
        });

        let filings = parse_submissions(&json, "0000320193").unwrap();
        assert_eq!(filings.len(), 1);
        assert_eq!(filings[0].form_type, "4");
        assert!(filings[0].description.is_empty());
    }
}
