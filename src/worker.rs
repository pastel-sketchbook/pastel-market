//! Background worker: offloads all HTTP fetches to a thread pool so the
//! event loop never blocks.
//!
//! The worker owns an `Arc<dyn QuoteProvider>` and a `mpsc::Sender` for
//! results. [`App`] calls `submit_*` methods to enqueue jobs; the main
//! loop drains results via [`Worker::try_recv`].

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use market_core::domain::{ChartRange, NewsItem, PricePoint, Quote, ScannerList, ScreenerResult};
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
    /// Chart data for the performance chart overlay.
    Chart {
        symbol: String,
        range: ChartRange,
        points: Vec<PricePoint>,
    },
    /// News headlines.
    News { items: Vec<NewsItem> },
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
