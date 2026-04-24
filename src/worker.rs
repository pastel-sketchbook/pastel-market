//! Background worker: offloads all HTTP fetches to a thread pool so the
//! event loop never blocks.
//!
//! The worker owns an `Arc<dyn QuoteProvider>` and a `mpsc::Sender` for
//! results. [`App`] calls `submit_*` methods to enqueue jobs; the main
//! loop drains results via [`Worker::try_recv`].

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use market_core::domain::{
    ChartRange, NewsItem, PricePoint, Quote, ScannerList, ScreenerResult, SecFiling,
};
use whispers::WhisperResult;
use yahoo_provider::QuoteProvider;

// ---------------------------------------------------------------------------
// Result types returned from background threads
// ---------------------------------------------------------------------------

/// Payload delivered by a background fetch.
#[allow(clippy::large_enum_variant)]
pub enum FetchResult {
    /// Watchlist quotes refreshed.
    Quotes { quotes: Vec<Option<Quote>> },
    /// Market index quotes.
    IndexQuotes { quotes: Vec<Option<Quote>> },
    /// Sector ETF quotes.
    SectorQuotes { quotes: Vec<Option<Quote>> },
    /// Sparkline data for the selected symbol.
    Sparkline { points: Vec<PricePoint> },
    /// Per-symbol sparkline data for inline watchlist display.
    SparklineAll {
        sparklines: std::collections::HashMap<String, Vec<PricePoint>>,
    },
    /// Chart data for the performance chart overlay.
    Chart {
        symbol: String,
        range: ChartRange,
        points: Vec<PricePoint>,
    },
    /// News headlines.
    News { items: Vec<NewsItem> },
    /// News headlines scoped to the chart overlay (per-stock).
    StockNews {
        ticker: String,
        items: Vec<NewsItem>,
    },
    /// SEC EDGAR filings for a ticker.
    SecFilings {
        ticker: String,
        filings: Vec<SecFiling>,
    },
    /// Fetched text content of a single SEC filing document.
    FilingContent { url: String, content: String },
    /// Scanner quotes (Yahoo screener or trending).
    Scanner {
        quotes: Vec<Quote>,
        screener_results: Option<Vec<ScreenerResult>>,
    },
    /// Quote for a single newly-added symbol.
    AddedSymbol { quote: Option<Quote>, index: usize },
    /// Insider ownership percentage for a ticker.
    InsiderOwnership { ticker: String, pct: f64 },
    /// Sector heat map data.
    SectorHeat {
        heat: std::collections::HashMap<String, f64>,
    },
    /// Whisper data for a ticker.
    Whisper {
        ticker: String,
        result: WhisperResult,
    },
    /// A fetch failed — carry the error message for status display.
    Error {
        context: &'static str,
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// Manages background HTTP fetches.
///
/// Each `submit_*` method spawns a short-lived thread that performs the
/// blocking HTTP call and sends the result back via an `mpsc` channel.
pub struct Worker {
    client: Arc<dyn QuoteProvider>,
    tx: mpsc::Sender<FetchResult>,
    rx: mpsc::Receiver<FetchResult>,
}

impl Worker {
    /// Create a new worker wrapping the given provider.
    #[must_use]
    pub fn new(client: Arc<dyn QuoteProvider>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { client, tx, rx }
    }

    /// Drain all available results without blocking.
    pub fn try_recv(&self) -> Vec<FetchResult> {
        let mut results = Vec::new();
        while let Ok(r) = self.rx.try_recv() {
            results.push(r);
        }
        results
    }

    /// Replace the inner client (e.g. after reconnection).
    pub fn set_client(&mut self, client: Arc<dyn QuoteProvider>) {
        self.client = client;
    }

    /// Get a reference to the client (for synchronous seed on startup).
    pub fn client(&self) -> &dyn QuoteProvider {
        &*self.client
    }

    // -- Submit methods -------------------------------------------------------

    /// Fetch watchlist quotes in the background.
    pub fn submit_quotes(&self, symbols: Vec<String>) {
        if symbols.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_quotes(&symbols) {
                Ok(quotes) => FetchResult::Quotes { quotes },
                Err(e) => FetchResult::Error {
                    context: "quote refresh",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch market index quotes in the background.
    pub fn submit_index_quotes(&self, symbols: Vec<String>) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_quotes(&symbols) {
                Ok(quotes) => FetchResult::IndexQuotes { quotes },
                Err(e) => FetchResult::Error {
                    context: "index refresh",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch sector ETF quotes in the background.
    pub fn submit_sector_quotes(&self, symbols: Vec<String>) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_quotes(&symbols) {
                Ok(quotes) => FetchResult::SectorQuotes { quotes },
                Err(e) => FetchResult::Error {
                    context: "sector refresh",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch sparkline data for a symbol in the background (1D/5m).
    pub fn submit_sparkline(&self, symbol: String) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_sparkline(&symbol, ChartRange::Day1) {
                Ok(points) => FetchResult::Sparkline { points },
                Err(_) => FetchResult::Sparkline { points: Vec::new() },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch sparklines for all symbols in the background (for inline display).
    ///
    /// Uses `thread::scope` to fetch multiple symbols in parallel, bounded
    /// to avoid spawning too many threads at once.
    pub fn submit_sparklines_all(&self, symbols: Vec<String>) {
        if symbols.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            // Fetch in parallel using scoped threads (max 8 concurrent).
            const MAX_CONCURRENT: usize = 8;
            let mut sparklines = std::collections::HashMap::new();
            for chunk in symbols.chunks(MAX_CONCURRENT) {
                let results: Vec<(String, Vec<PricePoint>)> = thread::scope(|s| {
                    let handles: Vec<_> = chunk
                        .iter()
                        .map(|sym| {
                            let client = &client;
                            let sym = sym.clone();
                            s.spawn(move || {
                                let points = client
                                    .fetch_sparkline(&sym, ChartRange::Day1)
                                    .unwrap_or_default();
                                (sym, points)
                            })
                        })
                        .collect();
                    handles.into_iter().filter_map(|h| h.join().ok()).collect()
                });
                for (sym, pts) in results {
                    if !pts.is_empty() {
                        sparklines.insert(sym, pts);
                    }
                }
            }
            let _ = tx.send(FetchResult::SparklineAll { sparklines });
        });
    }

    /// Fetch chart data for a symbol with a specific range in the background.
    pub fn submit_chart(&self, symbol: String, range: ChartRange) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_sparkline(&symbol, range) {
                Ok(points) => FetchResult::Chart {
                    symbol,
                    range,
                    points,
                },
                Err(e) => FetchResult::Error {
                    context: "chart",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch news headlines in the background.
    pub fn submit_news(&self, symbol: String) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_news(&symbol) {
                Ok(items) => FetchResult::News { items },
                Err(_) => FetchResult::News { items: Vec::new() },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch news headlines for the chart overlay (per-stock).
    ///
    /// Spawns two threads in parallel: Yahoo Finance search + Google News
    /// RSS. Results are merged with deduplication, preferring items that
    /// have summaries (Google News RSS includes description snippets).
    pub fn submit_stock_news(&self, ticker: String) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        let sym = ticker.clone();
        thread::spawn(move || {
            // Fetch Yahoo and Google News in parallel.
            let sym2 = sym.clone();
            let google_handle = thread::spawn(move || {
                market_core::news::fetch_google_news(&sym2).unwrap_or_default()
            });
            let yahoo_items = client.fetch_news(&sym).unwrap_or_default();
            let google_items = google_handle.join().unwrap_or_default();

            let merged = merge_news(yahoo_items, google_items);
            let _ = tx.send(FetchResult::StockNews {
                ticker,
                items: merged,
            });
        });
    }

    /// Fetch SEC EDGAR filings in the background.
    pub fn submit_sec_filings(&self, ticker: String) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = match market_core::sec::fetch_sec_filings(&ticker) {
                Ok(filings) => FetchResult::SecFilings { ticker, filings },
                Err(e) => {
                    tracing::warn!(error = %e, "SEC filing fetch failed");
                    FetchResult::SecFilings {
                        ticker,
                        filings: Vec::new(),
                    }
                }
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch the text content of a single SEC filing document.
    pub fn submit_filing_content(&self, url: String) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = match market_core::sec::fetch_filing_content(&url) {
                Ok(content) => FetchResult::FilingContent { url, content },
                Err(e) => {
                    tracing::warn!(error = %e, "filing content fetch failed");
                    FetchResult::FilingContent {
                        url,
                        content: format!("Failed to load filing: {e}"),
                    }
                }
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch scanner data in the background.
    pub fn submit_scanner(&self, scanner_list: ScannerList) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match scanner_list {
                ScannerList::Trending => match client.fetch_trending() {
                    Ok(syms) if !syms.is_empty() => match client.fetch_quotes(&syms) {
                        Ok(quotes) => FetchResult::Scanner {
                            quotes: quotes.into_iter().flatten().collect(),
                            screener_results: None,
                        },
                        Err(e) => FetchResult::Error {
                            context: "scanner",
                            message: e.to_string(),
                        },
                    },
                    Ok(_) => FetchResult::Scanner {
                        quotes: Vec::new(),
                        screener_results: None,
                    },
                    Err(e) => FetchResult::Error {
                        context: "scanner",
                        message: e.to_string(),
                    },
                },
                ScannerList::Fundamentals => match finviz_scraper::screener::fetch_raw() {
                    Ok(results) => {
                        let quotes = results
                            .iter()
                            .map(finviz_scraper::screener::screener_result_to_quote)
                            .collect();
                        FetchResult::Scanner {
                            quotes,
                            screener_results: Some(results),
                        }
                    }
                    Err(e) => FetchResult::Error {
                        context: "scanner",
                        message: e.to_string(),
                    },
                },
                _ => match client.fetch_screener(scanner_list.screener_id()) {
                    Ok(quotes) => FetchResult::Scanner {
                        quotes,
                        screener_results: None,
                    },
                    Err(e) => FetchResult::Error {
                        context: "scanner",
                        message: e.to_string(),
                    },
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch the quote for a single newly-added symbol.
    pub fn submit_added_symbol(&self, symbol: String, index: usize) {
        let tx = self.tx.clone();
        let client = Arc::clone(&self.client);
        thread::spawn(move || {
            let result = match client.fetch_quotes(&[symbol]) {
                Ok(mut quotes) => FetchResult::AddedSymbol {
                    quote: quotes.pop().flatten(),
                    index,
                },
                Err(e) => FetchResult::Error {
                    context: "added symbol",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch insider ownership from Finviz in the background.
    pub fn submit_insider_ownership(&self, ticker: String) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = match finviz_scraper::detail::fetch_insider_ownership(&ticker) {
                Ok(pct) => FetchResult::InsiderOwnership { ticker, pct },
                Err(e) => FetchResult::Error {
                    context: "insider ownership",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }

    /// Fetch sector heat map from Finviz in the background.
    pub fn submit_sector_heat(&self, sectors: Vec<String>) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let heat = finviz_scraper::sector::fetch_sector_heat(&sectors);
            let _ = tx.send(FetchResult::SectorHeat { heat });
        });
    }

    /// Fetch whisper data in the background.
    pub fn submit_whisper(&self, ticker: String) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = match whispers::fetch(&ticker) {
                Ok(w) => FetchResult::Whisper { ticker, result: w },
                Err(e) => FetchResult::Error {
                    context: "whisper",
                    message: e.to_string(),
                },
            };
            let _ = tx.send(result);
        });
    }
}

/// Merge Yahoo and Google News items with deduplication.
///
/// Google News items are preferred when they have summaries. Yahoo items
/// that duplicate a Google headline (by fuzzy title match) are dropped.
/// Final list is sorted by publish time (newest first), capped at 20.
fn merge_news(
    yahoo: Vec<market_core::domain::NewsItem>,
    google: Vec<market_core::domain::NewsItem>,
) -> Vec<market_core::domain::NewsItem> {
    use market_core::domain::NewsItem;

    let mut merged: Vec<NewsItem> = Vec::with_capacity(yahoo.len() + google.len());

    // Start with Google items (they have summaries).
    merged.extend(google);

    // Add Yahoo items that aren't duplicates.
    let google_titles: Vec<String> = merged.iter().map(|n| normalize_title(&n.title)).collect();
    for item in yahoo {
        let norm = normalize_title(&item.title);
        let is_dup = google_titles.iter().any(|gt| titles_similar(gt, &norm));
        if !is_dup {
            merged.push(item);
        }
    }

    // Sort newest first.
    merged.sort_by(|a, b| {
        let ta = a.publish_time.unwrap_or(0);
        let tb = b.publish_time.unwrap_or(0);
        tb.cmp(&ta)
    });

    merged.truncate(20);
    merged
}

/// Normalize a title for fuzzy comparison: lowercase, remove punctuation.
fn normalize_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if two normalized titles are similar enough to be duplicates.
///
/// Uses a simple "one contains the other" check plus word overlap ratio.
fn titles_similar(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.contains(b) || b.contains(a) {
        return true;
    }
    // Word overlap: if >60% of the shorter title's words appear in the
    // longer one, treat as duplicate.
    let words_a: Vec<&str> = a.split_whitespace().collect();
    let words_b: Vec<&str> = b.split_whitespace().collect();
    let (shorter, longer) = if words_a.len() <= words_b.len() {
        (&words_a, &words_b)
    } else {
        (&words_b, &words_a)
    };
    if shorter.len() < 3 {
        return false;
    }
    let overlap = shorter.iter().filter(|w| longer.contains(w)).count();
    #[allow(clippy::cast_precision_loss)]
    let ratio = overlap as f64 / shorter.len() as f64;
    ratio > 0.6
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use market_core::domain::NewsItem;

    fn item(title: &str, publisher: &str, summary: Option<&str>, ts: Option<i64>) -> NewsItem {
        NewsItem {
            title: title.to_string(),
            publisher: publisher.to_string(),
            link: String::new(),
            summary: summary.map(String::from),
            publish_time: ts,
        }
    }

    #[test]
    fn merge_deduplicates_by_title() {
        let yahoo = vec![item(
            "Apple reports record earnings",
            "Yahoo Finance",
            None,
            Some(100),
        )];
        let google = vec![item(
            "Apple reports record earnings",
            "Reuters",
            Some("Summary here"),
            Some(100),
        )];
        let merged = merge_news(yahoo, google);
        assert_eq!(merged.len(), 1);
        // Google version preferred (has summary).
        assert_eq!(merged[0].publisher, "Reuters");
        assert!(merged[0].summary.is_some());
    }

    #[test]
    fn merge_keeps_unique_items() {
        let yahoo = vec![item(
            "Tesla deliveries beat expectations",
            "Yahoo",
            None,
            Some(200),
        )];
        let google = vec![item(
            "Apple launches new MacBook Pro",
            "Reuters",
            Some("Snippet"),
            Some(100),
        )];
        let merged = merge_news(yahoo, google);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_sorts_newest_first() {
        let yahoo = vec![item("Old news", "Yahoo", None, Some(100))];
        let google = vec![item("New news", "Reuters", Some("s"), Some(200))];
        let merged = merge_news(yahoo, google);
        assert_eq!(merged[0].title, "New news");
        assert_eq!(merged[1].title, "Old news");
    }

    #[test]
    fn merge_caps_at_20() {
        let yahoo: Vec<NewsItem> = (0..15)
            .map(|i| item(&format!("Yahoo {i}"), "Y", None, Some(i)))
            .collect();
        let google: Vec<NewsItem> = (0..15)
            .map(|i| item(&format!("Google {i}"), "G", Some("s"), Some(100 + i)))
            .collect();
        let merged = merge_news(yahoo, google);
        assert!(merged.len() <= 20);
    }

    #[test]
    fn fuzzy_dedup_catches_similar_titles() {
        let yahoo = vec![item(
            "Apple reports record Q4 earnings, stock surges",
            "Yahoo",
            None,
            Some(100),
        )];
        let google = vec![item(
            "Apple reports record Q4 earnings",
            "Reuters",
            Some("Summary"),
            Some(100),
        )];
        let merged = merge_news(yahoo, google);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn titles_similar_exact_match() {
        assert!(titles_similar("hello world", "hello world"));
    }

    #[test]
    fn titles_similar_containment() {
        let a = normalize_title("Apple earnings surge");
        let b = normalize_title("Apple earnings surge in Q4");
        assert!(titles_similar(&a, &b));
    }

    #[test]
    fn titles_similar_short_titles_not_fuzzy() {
        // Short titles (< 3 words) should not fuzzy match.
        let a = normalize_title("Apple");
        let b = normalize_title("Microsoft");
        assert!(!titles_similar(&a, &b));
    }
}
