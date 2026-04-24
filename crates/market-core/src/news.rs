//! Google News RSS fetcher.
//!
//! Fetches stock-related news headlines from Google News RSS feed.
//! No API key required. Returns headlines with publisher and summary
//! snippet extracted from the RSS `<description>` field.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use tracing::debug;

use crate::domain::NewsItem;
use crate::http::{RetryConfig, USER_AGENT, call_with_retry_cfg};

/// Google News RSS search URL.
const GOOGLE_NEWS_RSS: &str = "https://news.google.com/rss/search";

/// Fast retry: single attempt for supplementary data.
const NEWS_RETRY: RetryConfig = RetryConfig {
    max_retries: 1,
    backoff_base_ms: 0,
};

/// Fetch news headlines for a stock ticker from Google News RSS.
///
/// Returns up to 15 headlines with title, publisher, summary snippet,
/// and publication timestamp.
///
/// # Errors
///
/// Returns an error if the RSS feed cannot be fetched or parsed.
pub fn fetch_google_news(ticker: &str) -> Result<Vec<NewsItem>> {
    let query = format!("{ticker} stock");
    let mut body = call_with_retry_cfg(
        || {
            ureq::get(GOOGLE_NEWS_RSS)
                .header("User-Agent", USER_AGENT)
                .query("q", &query)
                .query("hl", "en-US")
                .query("gl", "US")
                .query("ceid", "US:en")
        },
        &NEWS_RETRY,
    )
    .with_context(|| format!("Google News RSS request failed for {ticker}"))?;

    let xml = body
        .read_to_string()
        .context("failed to read Google News RSS body")?;

    let items = parse_rss(&xml);
    debug!(ticker, count = items.len(), "parsed Google News RSS");
    Ok(items)
}

/// Parse RSS XML into `NewsItem` list.
///
/// Uses quick-xml for structured fields (title, source, pubDate, link)
/// but extracts `<description>` raw content via string search because
/// quick-xml decodes entities and then interprets the resulting HTML
/// as XML tags, mangling the content.
#[must_use]
pub fn parse_rss(xml: &str) -> Vec<NewsItem> {
    // Pre-extract raw description content from each <item> before
    // quick-xml can decode entities and mangle embedded HTML.
    let raw_descriptions = extract_raw_descriptions(xml);

    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    let mut item_idx: usize = 0;
    let mut in_item = false;
    let mut current_tag = String::new();
    let mut title = String::new();
    let mut source = String::new();
    let mut pub_date = String::new();
    let mut link = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "item" {
                    in_item = true;
                    title.clear();
                    source.clear();
                    pub_date.clear();
                    link.clear();
                } else if in_item {
                    current_tag.clone_from(&name);
                }
            }
            Ok(Event::Text(ref e)) if in_item => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                match current_tag.as_str() {
                    "title" => title = text,
                    "source" => source = text,
                    "pubDate" => pub_date = text,
                    "link" => link = text,
                    // Skip "description" — we use raw_descriptions instead.
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "item" {
                    in_item = false;
                    if !title.is_empty() {
                        let clean_src = if source.is_empty() {
                            "Google News".to_string()
                        } else {
                            source.clone()
                        };
                        let clean = clean_title(&title, &clean_src);
                        let raw_desc = raw_descriptions.get(item_idx).map_or("", String::as_str);
                        let summary = clean_summary(raw_desc, &clean, &clean_src);
                        let publish_time = parse_rfc2822_to_epoch(&pub_date);
                        items.push(NewsItem {
                            title: clean,
                            publisher: clean_src,
                            link: link.clone(),
                            summary: if summary.is_empty() {
                                None
                            } else {
                                Some(summary)
                            },
                            publish_time,
                        });
                    }
                    item_idx += 1;
                    if items.len() >= 15 {
                        break;
                    }
                }
                current_tag.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    items
}

/// Extract raw `<description>` content from each `<item>` by string
/// search, preserving entity-encoded HTML exactly as-is.
fn extract_raw_descriptions(xml: &str) -> Vec<String> {
    let mut descriptions = Vec::new();
    let mut search_from = 0;

    while let Some(item_start) = xml[search_from..].find("<item>") {
        let item_start = search_from + item_start;
        let item_end = xml[item_start..]
            .find("</item>")
            .map_or(xml.len(), |p| item_start + p);

        let item_slice = &xml[item_start..item_end];

        let desc = if let Some(ds) = item_slice.find("<description>") {
            let content_start = ds + "<description>".len();
            if let Some(de) = item_slice[content_start..].find("</description>") {
                item_slice[content_start..content_start + de].to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        descriptions.push(desc);
        search_from = item_end;
    }

    descriptions
}

/// Clean raw text from RSS: decode HTML entities, strip HTML tags, collapse whitespace.
///
/// Google News descriptions contain entity-encoded HTML like
/// `&lt;font color=...&gt;Publisher&lt;/font&gt;`. We decode entities
/// first, then strip tags, then normalize whitespace.
fn clean_html(raw: &str) -> String {
    // Step 1: Decode HTML entities so encoded tags become real tags.
    let decoded = decode_entities(raw);
    // Step 2: Strip all HTML tags.
    let stripped = strip_tags(&decoded);
    // Step 3: Collapse whitespace.
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode common HTML entities.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
}

/// Strip HTML tags from a string, returning plain text.
fn strip_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                // Insert a space where a tag was, to avoid words merging.
                result.push(' ');
            }
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result
}

/// Strip the trailing " - Publisher" suffix that Google News appends to titles.
fn clean_title(title: &str, source: &str) -> String {
    if !source.is_empty()
        && let Some(stripped) = title.strip_suffix(&format!(" - {source}"))
    {
        return stripped.to_string();
    }
    title.to_string()
}

/// Clean a raw RSS description into a summary, removing the headline
/// and publisher text that Google News embeds in the HTML snippet.
///
/// Google News `<description>` typically looks like:
/// ```text
/// <a href="...">Headline</a>&nbsp;&nbsp;<font color="#6f6f6f">Publisher</font><br>Actual summary
/// ```
/// After `clean_html`, this becomes `"Headline Publisher Actual summary"`.
/// We strip the leading headline + publisher to avoid duplication.
fn clean_summary(raw_desc: &str, title: &str, publisher: &str) -> String {
    let text = clean_html(raw_desc);
    if text.is_empty() {
        return text;
    }

    // Try to remove the title prefix (case-insensitive match).
    let lower = text.to_lowercase();
    let title_lower = title.to_lowercase();
    let remainder = if let Some(pos) = lower.find(&title_lower) {
        &text[pos + title.len()..]
    } else {
        &text
    };

    // Remove the publisher name if it immediately follows.
    let trimmed = remainder.trim();
    let pub_lower = publisher.to_lowercase();
    let remainder = if trimmed.to_lowercase().starts_with(&pub_lower) {
        trimmed[publisher.len()..].trim_start()
    } else {
        trimmed
    };

    let cleaned = remainder.split_whitespace().collect::<Vec<_>>().join(" ");

    // If stripping title+publisher left nothing, there's no real
    // summary content — return empty so the UI can handle it.
    cleaned
}

/// Parse RFC 2822 date to Unix epoch seconds.
fn parse_rfc2822_to_epoch(date_str: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc2822(date_str)
        .ok()
        .map(|dt| dt.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rss() -> &'static str {
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<rss version=\"2.0\"><channel>",
            "<title>AAPL stock - Google News</title>",
            "<item>",
            "<title>Apple reports record Q4 earnings</title>",
            "<link>https://news.google.com/articles/abc123</link>",
            "<source url=\"https://reuters.com\">Reuters</source>",
            "<description>iPhone demand surges in holiday quarter</description>",
            "<pubDate>Fri, 24 Apr 2026 14:30:00 GMT</pubDate>",
            "</item>",
            "<item>",
            "<title>Apple stock hits all-time high</title>",
            "<link>https://news.google.com/articles/def456</link>",
            "<source url=\"https://marketwatch.com\">MarketWatch</source>",
            "<description>Shares climb 3% after strong guidance</description>",
            "<pubDate>Fri, 24 Apr 2026 12:00:00 GMT</pubDate>",
            "</item>",
            "<item>",
            "<title></title>",
            "<link>https://example.com</link>",
            "<source url=\"https://example.com\">Example</source>",
            "<description>Should be skipped</description>",
            "</item>",
            "</channel></rss>",
        )
    }

    #[test]
    fn parse_rss_extracts_items() {
        let items = parse_rss(sample_rss());
        assert_eq!(items.len(), 2);
        assert!(items[0].title.contains("Q4 earnings"));
        assert_eq!(items[0].publisher, "Reuters");
        assert!(items[0].summary.is_some());
        assert!(items[0].publish_time.is_some());
        assert!(items[1].title.contains("all-time high"));
        assert_eq!(items[1].publisher, "MarketWatch");
    }

    #[test]
    fn parse_rss_summary_is_plain_text() {
        let items = parse_rss(sample_rss());
        let summary = items[0].summary.as_deref().expect("should have summary");
        assert!(!summary.contains('<'));
        assert!(!summary.contains('>'));
        assert!(summary.contains("iPhone"));
    }

    #[test]
    fn parse_rss_skips_empty_titles() {
        let items = parse_rss(sample_rss());
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn parse_rss_handles_empty_input() {
        assert!(parse_rss("").is_empty());
        assert!(parse_rss("<rss><channel></channel></rss>").is_empty());
    }

    #[test]
    fn clean_html_strips_real_tags() {
        assert_eq!(clean_html("<b>bold</b>"), "bold");
    }

    #[test]
    fn clean_html_decodes_entities_then_strips() {
        // Simulates Google News description with entity-encoded HTML.
        let input = "&lt;a href=\"x\"&gt;Headline&lt;/a&gt;&amp;nbsp;&lt;font color=\"#6f6f6f\"&gt;Reuters&lt;/font&gt;";
        let result = clean_html(input);
        assert!(
            !result.contains("font"),
            "should not contain 'font' tag text: {result}"
        );
        assert!(!result.contains('<'), "should not contain '<': {result}");
        assert!(result.contains("Headline"));
        assert!(result.contains("Reuters"));
    }

    #[test]
    fn clean_html_plain_text() {
        assert_eq!(clean_html("plain text"), "plain text");
    }

    #[test]
    fn clean_html_collapses_whitespace() {
        assert_eq!(clean_html("  lots   of   space  "), "lots of space");
    }

    #[test]
    fn parse_rss_with_entity_encoded_html_description() {
        // This mimics real Google News RSS where <description> contains
        // entity-encoded HTML like &lt;font&gt;...&lt;/font&gt;.
        let rss = concat!(
            "<rss><channel>",
            "<item>",
            "<title>Headline</title>",
            "<link>https://example.com</link>",
            "<source url=\"https://reuters.com\">Reuters</source>",
            "<description>&lt;a href=\"x\"&gt;Headline&lt;/a&gt;",
            "&amp;nbsp;&amp;nbsp;",
            "&lt;font color=\"#6f6f6f\"&gt;Reuters&lt;/font&gt;",
            "&lt;br&gt;Actual summary content here</description>",
            "<pubDate>Thu, 24 Apr 2026 14:30:00 GMT</pubDate>",
            "</item>",
            "</channel></rss>",
        );
        let items = parse_rss(rss);
        assert_eq!(items.len(), 1);
        let summary = items[0].summary.as_deref().expect("should have summary");
        eprintln!("DEBUG summary: {summary:?}");
        assert!(
            !summary.contains('/'),
            "summary contains stray /: {summary}"
        );
        assert!(!summary.contains('<'), "summary contains <: {summary}");
        assert!(summary.contains("summary content"));
        // Title and publisher should NOT appear in summary.
        assert!(
            !summary.starts_with("Headline"),
            "summary starts with headline: {summary}"
        );
    }

    #[test]
    fn extract_raw_descriptions_preserves_entities() {
        let xml = concat!(
            "<rss><channel>",
            "<item>",
            "<description>&lt;font color=\"#6f6f6f\"&gt;Reuters&lt;/font&gt;",
            "&lt;br&gt;Real content</description>",
            "</item>",
            "</channel></rss>",
        );
        let descs = extract_raw_descriptions(xml);
        assert_eq!(descs.len(), 1);
        // Raw description preserves entities.
        assert!(descs[0].contains("&lt;font"));
        assert!(descs[0].contains("&lt;/font&gt;"));
        // clean_html decodes then strips.
        let cleaned = clean_html(&descs[0]);
        eprintln!("DEBUG cleaned: {cleaned:?}");
        assert!(
            !cleaned.contains("/font"),
            "cleaned contains /font: {cleaned}"
        );
        assert!(!cleaned.contains('<'), "cleaned contains <: {cleaned}");
        assert!(cleaned.contains("Reuters"));
        assert!(cleaned.contains("Real content"));
    }

    #[test]
    fn clean_summary_removes_title_and_publisher() {
        let raw = "&lt;a href=\"x\"&gt;Apple beats estimates&lt;/a&gt;&amp;nbsp;&lt;font&gt;Reuters&lt;/font&gt;&lt;br&gt;Revenue grew 15%";
        let result = clean_summary(raw, "Apple beats estimates", "Reuters");
        assert_eq!(result, "Revenue grew 15%");
    }

    #[test]
    fn clean_summary_handles_no_overlap() {
        let raw = "Completely different text here";
        let result = clean_summary(raw, "Some title", "Publisher");
        assert_eq!(result, "Completely different text here");
    }

    #[test]
    fn clean_summary_falls_back_empty_when_only_headline() {
        // Description is just headline + publisher, no extra text.
        let raw = "&lt;a href=\"x\"&gt;Apple beats estimates&lt;/a&gt;&amp;nbsp;&lt;font&gt;Reuters&lt;/font&gt;";
        let result = clean_summary(raw, "Apple beats estimates", "Reuters");
        assert!(
            result.is_empty(),
            "should be empty when no real summary: {result}"
        );
    }

    #[test]
    fn clean_title_strips_publisher_suffix() {
        assert_eq!(
            clean_title("Apple earnings beat estimates - Reuters", "Reuters"),
            "Apple earnings beat estimates"
        );
    }

    #[test]
    fn clean_title_no_suffix() {
        assert_eq!(
            clean_title("Apple earnings beat estimates", "Reuters"),
            "Apple earnings beat estimates"
        );
    }

    #[test]
    fn parse_rss_strips_title_suffix() {
        let rss = concat!(
            "<rss><channel>",
            "<item>",
            "<title>Big news - Reuters</title>",
            "<link>https://example.com</link>",
            "<source url=\"https://reuters.com\">Reuters</source>",
            "<description>Details here</description>",
            "<pubDate>Thu, 24 Apr 2026 14:30:00 GMT</pubDate>",
            "</item>",
            "</channel></rss>",
        );
        let items = parse_rss(rss);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Big news");
    }

    #[test]
    fn parse_rfc2822_valid() {
        let ts = parse_rfc2822_to_epoch("Fri, 24 Apr 2026 14:30:00 GMT");
        assert!(ts.is_some());
    }

    #[test]
    fn parse_rfc2822_invalid_returns_none() {
        assert!(parse_rfc2822_to_epoch("not a date").is_none());
    }

    #[test]
    fn parse_rfc2822_empty_returns_none() {
        assert!(parse_rfc2822_to_epoch("").is_none());
    }
}
