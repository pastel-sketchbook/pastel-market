//! SEC EDGAR filing fetcher.
//!
//! Fetches recent filings for a company from the SEC EDGAR full-text
//! search API. No authentication required — only a descriptive
//! `User-Agent` header (SEC policy).

use anyhow::{Context, Result, bail};
use tracing::debug;

use crate::domain::SecFiling;
use crate::http::{USER_AGENT, call_with_retry};

/// EDGAR company filings endpoint (submissions).
const SUBMISSIONS_URL: &str = "https://data.sec.gov/submissions/CIK";

/// Fetch recent SEC filings for the given ticker.
///
/// Uses the EDGAR company-search API to resolve the CIK, then fetches
/// the latest filings from the submissions endpoint.
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

/// Resolve a ticker symbol to a zero-padded CIK using the EDGAR company
/// tickers JSON file.
fn resolve_cik(ticker: &str) -> Result<String> {
    let mut body = call_with_retry(|| {
        ureq::get("https://www.sec.gov/files/company_tickers.json")
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
    })
    .context("SEC company tickers request failed")?;

    let json: serde_json::Value = body
        .read_json()
        .context("failed to parse SEC company tickers JSON")?;

    let upper = ticker.to_uppercase();
    if let Some(obj) = json.as_object() {
        for (_key, entry) in obj {
            if entry["ticker"].as_str() == Some(&upper)
                && let Some(cik) = entry["cik_str"].as_u64()
            {
                return Ok(format!("{cik:010}"));
            }
        }
    }

    bail!("ticker {ticker} not found in SEC company tickers")
}

/// Fetch filings from the EDGAR submissions endpoint for a given CIK.
fn fetch_filings_by_cik(cik: &str) -> Result<Vec<SecFiling>> {
    let url = format!("{SUBMISSIONS_URL}{cik}.json");
    let mut body = call_with_retry(|| {
        ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
    })
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
}
