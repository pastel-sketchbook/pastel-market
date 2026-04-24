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

/// Build the SEC User-Agent string.
///
/// SEC EDGAR fair access policy requires a contact email in the User-Agent.
/// Reads `SEC_CONTACT_EMAIL` env var; falls back to `user@example.com`.
fn sec_user_agent() -> String {
    let email =
        std::env::var("SEC_CONTACT_EMAIL").unwrap_or_else(|_| "user@example.com".to_owned());
    format!("pastel-market/{} ({email})", env!("CARGO_PKG_VERSION"))
}

/// Build a ureq agent with a short timeout for SEC requests.
fn sec_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(SEC_TIMEOUT))
        .user_agent(&*sec_user_agent())
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

// ---------------------------------------------------------------------------
// Filing content fetch
// ---------------------------------------------------------------------------

/// Timeout for SEC filing content requests (longer than metadata).
const SEC_CONTENT_TIMEOUT: Duration = Duration::from_secs(15);

/// Build a ureq agent with a longer timeout for filing content.
fn sec_content_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(SEC_CONTENT_TIMEOUT))
        .user_agent(&*sec_user_agent())
        .build()
        .new_agent()
}

/// Fetch a filing document and extract readable text.
///
/// Downloads the HTML/XML from `url`, strips tags and excess whitespace,
/// and returns plain text suitable for display in a terminal panel.
/// The response is capped at [`MAX_FILING_BYTES`] to avoid downloading
/// enormous filings.
///
/// # Errors
///
/// Returns an error if the document cannot be fetched or read.
pub fn fetch_filing_content(url: &str) -> Result<String> {
    let mut body = sec_content_agent()
        .get(url)
        .header("Accept", "text/html, application/xhtml+xml, text/xml, */*")
        .call()
        .with_context(|| format!("failed to fetch filing from {url}"))?
        .into_body();

    let html = body
        .read_to_string()
        .context("failed to read filing body")?;

    // Truncate very large filings before parsing to keep TUI responsive.
    let truncated = if html.len() > 512_000 {
        &html[..512_000]
    } else {
        &html
    };

    let text = strip_html_to_text(truncated);
    debug!(url, chars = text.len(), "fetched filing content");
    Ok(text)
}

/// Strip HTML/XML tags and decode common entities to produce readable text.
fn strip_html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 3);
    let mut in_tag = false;
    let mut last_was_space = true;

    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                in_tag = true;
                // Insert newline for block elements.
                if !last_was_space {
                    // Peek to detect block tags (p, div, tr, br, h1-h6, li).
                    let rest: String = chars.clone().take(4).collect();
                    let tag = rest.to_lowercase();
                    if tag.starts_with("br")
                        || tag.starts_with("p ")
                        || tag.starts_with("p>")
                        || tag.starts_with("/p")
                        || tag.starts_with("/d")
                        || tag.starts_with("/t")
                        || tag.starts_with("tr")
                        || tag.starts_with("li")
                        || tag.starts_with("h1")
                        || tag.starts_with("h2")
                        || tag.starts_with("h3")
                        || tag.starts_with("h4")
                        || tag.starts_with("h5")
                        || tag.starts_with("h6")
                    {
                        out.push('\n');
                        last_was_space = true;
                    }
                }
            }
            '>' if in_tag => {
                in_tag = false;
            }
            '&' if !in_tag => {
                // Decode common HTML entities.
                let entity: String = chars.by_ref().take_while(|&ch| ch != ';').collect();
                match entity.as_str() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    "nbsp" | "#160" => out.push(' '),
                    "#8212" | "mdash" => out.push('—'),
                    "#8211" | "ndash" => out.push('–'),
                    "#8217" | "rsquo" => out.push('\u{2019}'),
                    "#8220" | "ldquo" => out.push('\u{201C}'),
                    "#8221" | "rdquo" => out.push('\u{201D}'),
                    _ => {
                        // Unknown entity — skip it.
                    }
                }
                last_was_space = false;
            }
            _ if !in_tag => {
                if c.is_whitespace() {
                    if !last_was_space {
                        out.push(' ');
                        last_was_space = true;
                    }
                } else {
                    out.push(c);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    // Collapse multiple blank lines.
    let mut result = String::with_capacity(out.len());
    let mut blank_count = 0;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(trimmed);
            result.push('\n');
        }
    }
    result.trim().to_string()
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

    #[test]
    fn strip_html_basic() {
        let html = "<html><body><p>Hello <b>world</b>!</p><p>Second paragraph.</p></body></html>";
        let text = strip_html_to_text(html);
        assert!(text.contains("Hello world!"));
        assert!(text.contains("Second paragraph."));
    }

    #[test]
    fn strip_html_entities() {
        let html = "AT&amp;T &lt;corp&gt; &quot;test&quot;";
        let text = strip_html_to_text(html);
        assert_eq!(text, "AT&T <corp> \"test\"");
    }

    #[test]
    fn strip_html_collapses_whitespace() {
        let html = "<div>  lots   of   spaces  </div><div></div><div></div><div>after gap</div>";
        let text = strip_html_to_text(html);
        // Multiple blank lines collapsed to at most one.
        assert!(!text.contains("\n\n\n"));
        assert!(text.contains("lots of spaces"));
        assert!(text.contains("after gap"));
    }
}
