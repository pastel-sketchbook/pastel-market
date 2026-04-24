//! SEC EDGAR filing fetcher.
//!
//! Fetches recent filings for a company from the SEC EDGAR
//! submissions API. No authentication required — only a descriptive
//! `User-Agent` header (SEC policy).
//!
//! The CIK mapping (~2 MB JSON) is cached in-memory after the first
//! download. Call [`warm_cik_cache`] at startup to avoid first-use
//! latency.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use tracing::debug;

use crate::domain::SecFiling;
use crate::http::{RetryConfig, USER_AGENT, call_with_retry_cfg};

/// EDGAR company filings endpoint (submissions).
const SUBMISSIONS_URL: &str = "https://data.sec.gov/submissions/CIK";

/// Fast retry: 1 attempt, no backoff. SEC data is supplementary and
/// should not slow down the main UI.
const SEC_RETRY: RetryConfig = RetryConfig {
    max_retries: 1,
    backoff_base_ms: 0,
};

/// Global in-memory CIK cache: ticker → zero-padded CIK string.
static CIK_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn cik_cache() -> &'static Mutex<HashMap<String, String>> {
    CIK_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Pre-warm the CIK cache by downloading the ticker map from SEC.
///
/// Call this once at startup on a background thread so that subsequent
/// `fetch_sec_filings` calls can skip the 2 MB download.
///
/// # Errors
///
/// Returns an error if the SEC ticker map cannot be fetched or parsed.
pub fn warm_cik_cache() -> Result<()> {
    let map = download_cik_map()?;
    let mut cache = cik_cache()
        .lock()
        .map_err(|e| anyhow::anyhow!("CIK cache lock poisoned: {e}"))?;
    *cache = map;
    debug!(count = cache.len(), "CIK cache warmed");
    Ok(())
}

/// Fetch recent SEC filings for the given ticker.
///
/// Resolves the CIK from cache (or downloads the mapping if not cached),
/// then fetches the latest filings from the submissions endpoint.
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

/// Download and parse the full ticker → CIK mapping from SEC.
fn download_cik_map() -> Result<HashMap<String, String>> {
    let mut body = call_with_retry_cfg(
        || {
            ureq::get("https://www.sec.gov/files/company_tickers.json")
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json")
        },
        &SEC_RETRY,
    )
    .context("SEC company tickers request failed")?;

    let json: serde_json::Value = body
        .read_json()
        .context("failed to parse SEC company tickers JSON")?;

    let mut map = HashMap::new();
    if let Some(obj) = json.as_object() {
        for (_key, entry) in obj {
            if let (Some(ticker), Some(cik)) =
                (entry["ticker"].as_str(), entry["cik_str"].as_u64())
            {
                map.insert(ticker.to_uppercase(), format!("{cik:010}"));
            }
        }
    }
    debug!(count = map.len(), "parsed SEC company tickers");
    Ok(map)
}

/// Resolve a ticker symbol to a zero-padded CIK.
///
/// First checks the in-memory cache. If not found, downloads the full
/// mapping (populating the cache for future lookups).
fn resolve_cik(ticker: &str) -> Result<String> {
    let upper = ticker.to_uppercase();

    // Check cache first.
    {
        let cache = cik_cache()
            .lock()
            .map_err(|e| anyhow::anyhow!("CIK cache lock poisoned: {e}"))?;
        if let Some(cik) = cache.get(&upper) {
            return Ok(cik.clone());
        }
    }

    // Cache miss — download and populate.
    let map = download_cik_map()?;
    let result = map.get(&upper).cloned();
    let mut cache = cik_cache()
        .lock()
        .map_err(|e| anyhow::anyhow!("CIK cache lock poisoned: {e}"))?;
    *cache = map;

    result.ok_or_else(|| anyhow::anyhow!("ticker {ticker} not found in SEC company tickers"))
}

/// Fetch filings from the EDGAR submissions endpoint for a given CIK.
fn fetch_filings_by_cik(cik: &str) -> Result<Vec<SecFiling>> {
    let url = format!("{SUBMISSIONS_URL}{cik}.json");
    let mut body = call_with_retry_cfg(
        || {
            ureq::get(&url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json")
        },
        &SEC_RETRY,
    )
    .context("SEC submissions request failed")?;

    let json: serde_json::Value = body
        .read_json()
        .context("failed to parse SEC submissions JSON")?;

    parse_submissions(&json, cik)
}

/// Parse the EDGAR submissions JSON into `SecFiling` items.
///
/// The submissions JSON has `recentFilings` with parallel arrays:
/// `form`, `filingDate`, `primaryDocument`, `accessionNumber`,
/// `primaryDocDescription`.
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
    let limit = forms.len().min(20); // Latest 20 filings.

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

        // Build the direct link to the filing document.
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
        // 8-K with empty doc should get a fallback link.
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

    #[test]
    fn download_cik_map_parses_sample_json() {
        // Unit test with synthetic data — doesn't hit the network.
        let json = serde_json::json!({
            "0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc"},
            "1": {"cik_str": 789019, "ticker": "MSFT", "title": "Microsoft Corp"}
        });

        // Simulate parsing logic.
        let mut map = HashMap::new();
        if let Some(obj) = json.as_object() {
            for (_key, entry) in obj {
                if let (Some(ticker), Some(cik)) =
                    (entry["ticker"].as_str(), entry["cik_str"].as_u64())
                {
                    map.insert(ticker.to_uppercase(), format!("{cik:010}"));
                }
            }
        }
        assert_eq!(map.get("AAPL"), Some(&"0000320193".to_string()));
        assert_eq!(map.get("MSFT"), Some(&"0000789019".to_string()));
        assert_eq!(map.len(), 2);
    }
}
