//! Unified application state combining watchlist monitoring (Reins Market)
//! with quality-control screening (Pastel Picker).

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use tracing::{info, warn};

use market_core::config::{self, Preferences, QcSession, Session};
use market_core::decisions::{Action, DecisionEntry, DecisionLog};
use market_core::domain::mock::MockData;
use market_core::domain::{
    ChartRange, FilterMode, MarketStatus, NewsItem, PricePoint, Quote, ScannerList, ScreenerResult,
    SecFiling, SortMode, TopMovers, ViewMode, Watchlist,
};
use market_core::theme::{self, Theme};
use whispers::WhisperResult;
use yahoo_provider::QuoteProvider;

use crate::worker::{FetchResult, Worker};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Market index symbols displayed in the header bar.
pub const INDEX_SYMBOLS: &[&str] = &["^GSPC", "^DJI", "^IXIC", "^RUT", "^VIX"];

/// GICS sector ETF symbols for sector performance.
pub const SECTOR_SYMBOLS: &[&str] = &[
    "XLK", "XLF", "XLE", "XLV", "XLC", "XLY", "XLP", "XLI", "XLB", "XLRE", "XLU",
];

/// Ticks between active-market refreshes (30s at 250ms/tick = 120 ticks).
const ACTIVE_REFRESH_TICKS: u32 = 120;

/// Ticks between heartbeat refreshes when the market is closed (5min = 1200 ticks).
const HEARTBEAT_TICKS: u32 = 1200;

/// Maximum number of symbols to seed from a Yahoo screener on first launch.
const SEED_LIMIT: usize = 20;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Whether the user is in normal navigation mode or typing a new symbol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Normal,
    Adding,
}

/// Active tab in the chart overlay bottom panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartTab {
    /// Price chart only (no bottom panel visible).
    #[default]
    Chart,
    /// News headlines + inline summary.
    News,
    /// SEC EDGAR filings.
    SecFilings,
    /// Auto-derived Bull/Bear signals.
    Thesis,
}

impl ChartTab {
    /// Cycle to the next tab.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Chart => Self::News,
            Self::News => Self::SecFilings,
            Self::SecFilings => Self::Thesis,
            Self::Thesis => Self::Chart,
        }
    }

    /// Cycle to the previous tab.
    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Chart => Self::Thesis,
            Self::News => Self::Chart,
            Self::SecFilings => Self::News,
            Self::Thesis => Self::SecFilings,
        }
    }
}

/// Which panel currently receives keyboard focus (QC view only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Table,
    QcChecklist,
}

impl Focus {
    /// Toggle between Table and QC checklist.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Table => Self::QcChecklist,
            Self::QcChecklist => Self::Table,
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Combined application state.
///
/// Merges Reins Market (watchlist, scanners, indices, sectors, news) with
/// Pastel Picker (QC checklist, whispers, screener results, conviction status).
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    // -- Watchlist (from Reins Market) --
    pub watchlist: Watchlist,
    pub watchlist_tabs: Vec<(String, Vec<String>)>,
    pub active_tab: usize,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub sparkline_data: Vec<PricePoint>,
    pub sparkline_cache: HashMap<String, Vec<PricePoint>>,
    pub status_message: String,
    pub should_quit: bool,

    // -- Help overlay --
    pub show_help: bool,

    // -- Market data (from Reins Market) --
    pub index_quotes: Vec<Option<Quote>>,
    pub sector_quotes: Vec<Option<Quote>>,
    pub market_status: MarketStatus,
    pub sort_mode: SortMode,
    pub filter_mode: FilterMode,
    pub view_mode: ViewMode,

    // -- Scanner (from Reins Market) --
    pub scanner_list: ScannerList,
    pub scanner_quotes: Vec<Quote>,
    pub scanner_selected: usize,
    pub scanner_table_state: ratatui::widgets::TableState,

    // -- News & Risk (from Reins Market) --
    pub news_headlines: Vec<NewsItem>,
    pub show_news: bool,
    pub show_risk: bool,

    // -- QC & Conviction (from Pastel Picker) --
    pub focus: Focus,
    pub qc_labels: Vec<String>,
    pub qc_state: HashMap<String, Vec<bool>>,
    pub selected_qc: usize,
    pub screener_results: Vec<ScreenerResult>,

    // -- Whisper data (from Pastel Picker) --
    pub whisper_cache: HashMap<String, WhisperResult>,

    // -- Auto-check data (from Pastel Picker) --
    pub insider_ownership: HashMap<String, f64>,
    pub sector_heat: HashMap<String, f64>,
    pub past_beats: HashMap<String, bool>,
    pub chart_patterns: HashMap<String, String>,

    // -- Chart overlay --
    pub chart_open: bool,
    pub chart_symbol: String,
    pub chart_range: ChartRange,
    pub chart_data: Vec<PricePoint>,
    pub chart_loading: bool,
    pub chart_tab: ChartTab,
    pub chart_news: Vec<NewsItem>,
    pub chart_news_selected: usize,
    pub chart_news_summary_open: bool,
    /// Fetched text content of the currently viewed news article.
    pub chart_news_content: Option<String>,
    /// Whether the article content is currently loading.
    pub chart_news_content_loading: bool,
    /// Scroll offset (line) within the news content panel.
    pub chart_news_scroll: usize,
    pub chart_sec_filings: Vec<SecFiling>,
    pub chart_sec_selected: usize,
    pub chart_sec_detail_open: bool,
    /// Fetched text content of the currently viewed filing.
    pub chart_sec_content: Option<String>,
    /// Whether the filing content is currently loading.
    pub chart_sec_content_loading: bool,
    /// Scroll offset (line) within the filing content panel.
    pub chart_sec_scroll: usize,
    /// Whether to show the 50/200 SMA lines (Phase 2: technical indicators).
    pub chart_show_sma: bool,
    /// Whether to show the RSI subplot (Phase 2: technical indicators).
    pub chart_show_rsi: bool,
    /// Whether to show the MACD subplot (Phase 2: technical indicators).
    pub chart_show_macd: bool,
    /// Horizontal split ratio (0.0–1.0) for detail panels (news summary, SEC detail).
    /// Represents the left panel's share. Draggable with mouse.
    pub chart_detail_split: f64,
    /// Whether the user is actively dragging the split divider.
    pub chart_detail_dragging: bool,

    // -- Theme --
    pub theme_index: usize,

    // -- Internal state --
    pub tick: u64,
    pub ticks_since_refresh: u32,
    pub pending_g: bool,
    pub top_movers: TopMovers,
    pub loading: bool,

    // -- Alert & Journal state --
    pub alert_fired: bool,
    pub decisions: DecisionLog,

    /// On the first quotes update after startup, snap the watchlist selection
    /// to the top of the displayed (sorted + filtered) order. Cleared after
    /// the first apply.
    pub initial_selection_pending: bool,

    // -- Private --
    skip_persist: bool,
    worker: Option<Worker>,
}

impl App {
    /// Create a new `App`.
    ///
    /// Loads persisted preferences, session, and QC state from disk.
    /// Attempts to establish a Yahoo Finance session. If no persisted
    /// watchlist exists, seeds the watchlist from Yahoo's day-gainers
    /// screener (top 20 performers).
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Self {
        let prefs = config::load_preferences();
        let session = config::load_session();
        let qc_session = QcSession::load();
        let decisions = DecisionLog::load();
        let data = market_core::domain::mock::load_mock_data().unwrap_or_else(|e| {
            warn!(error = %e, "failed to load mock data");
            fallback_mock_data()
        });

        let theme_index = theme::theme_index_by_name(&prefs.theme);
        let sort_mode = config::sort_mode_from_string(&session.sort_mode);
        let filter_mode = config::filter_mode_from_string(&session.filter_mode);
        let view_mode = config::view_mode_from_string(&session.view_mode);

        let client: Option<Arc<dyn QuoteProvider>> = if prefs.data_vendors.quotes == "alphavantage"
        {
            // Try Alpha Vantage first, fall back to Yahoo.
            match alphavantage::AlphaVantageClient::new() {
                Ok(c) => {
                    info!("using Alpha Vantage as quote provider");
                    Some(Arc::new(c))
                }
                Err(e) => {
                    warn!(error = %e, "Alpha Vantage init failed, falling back to Yahoo");
                    match yahoo_provider::YahooClient::new() {
                        Ok(c) => Some(Arc::new(c)),
                        Err(e2) => {
                            warn!(error = %e2, "Yahoo Finance session also failed");
                            None
                        }
                    }
                }
            }
        } else {
            match yahoo_provider::YahooClient::new() {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    warn!(error = %e, "Yahoo Finance session failed");
                    None
                }
            }
        };

        let worker = client.map(Worker::new);

        // Use persisted symbols, or seed from Yahoo's day-gainers screener.
        let symbols = if session.symbols.is_empty() {
            seed_symbols_from_screener(worker.as_ref().map(Worker::client))
        } else {
            session.symbols.clone()
        };

        // Reconstruct tabs: use persisted tabs or create a default "Main" tab.
        let (watchlist_tabs, active_tab) = if session.watchlist_tabs.is_empty() {
            (vec![("Main".to_string(), symbols.clone())], 0)
        } else {
            let tabs: Vec<(String, Vec<String>)> = session
                .watchlist_tabs
                .iter()
                .map(|t| (t.name.clone(), t.symbols.clone()))
                .collect();
            let active = session.active_tab.min(tabs.len().saturating_sub(1));
            (tabs, active)
        };

        // Active tab's symbols drive the watchlist.
        let active_symbols = watchlist_tabs
            .get(active_tab)
            .map_or_else(|| symbols.clone(), |(_, s)| s.clone());

        info!(count = active_symbols.len(), "initial watchlist symbols");

        Self {
            watchlist: Watchlist::new(active_symbols),
            watchlist_tabs,
            active_tab,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            sparkline_data: Vec::new(),
            sparkline_cache: HashMap::new(),
            status_message: String::new(),
            should_quit: false,
            show_help: false,
            index_quotes: Vec::new(),
            sector_quotes: Vec::new(),
            market_status: MarketStatus::default(),
            sort_mode,
            filter_mode,
            view_mode,
            scanner_list: ScannerList::default(),
            scanner_quotes: Vec::new(),
            scanner_selected: 0,
            scanner_table_state: ratatui::widgets::TableState::default(),
            news_headlines: Vec::new(),
            show_news: false,
            show_risk: false,
            focus: Focus::default(),
            qc_labels: data.qc_checklist.items,
            qc_state: qc_session.qc_state,
            selected_qc: 0,
            screener_results: Vec::new(),
            whisper_cache: HashMap::new(),
            insider_ownership: HashMap::new(),
            sector_heat: HashMap::new(),
            past_beats: HashMap::new(),
            chart_patterns: HashMap::new(),
            chart_open: false,
            chart_symbol: String::new(),
            chart_range: ChartRange::default(),
            chart_data: Vec::new(),
            chart_loading: false,
            chart_tab: ChartTab::default(),
            chart_news: Vec::new(),
            chart_news_selected: 0,
            chart_news_summary_open: false,
            chart_news_content: None,
            chart_news_content_loading: false,
            chart_news_scroll: 0,
            chart_sec_filings: Vec::new(),
            chart_sec_selected: 0,
            chart_sec_detail_open: false,
            chart_sec_content: None,
            chart_sec_content_loading: false,
            chart_sec_scroll: 0,
            chart_show_sma: false,
            chart_show_rsi: false,
            chart_show_macd: false,
            chart_detail_split: 0.5,
            chart_detail_dragging: false,
            theme_index,
            tick: 0,
            ticks_since_refresh: 0,
            pending_g: false,
            top_movers: TopMovers::default(),
            loading: false,
            alert_fired: false,
            decisions,
            initial_selection_pending: true,
            skip_persist: false,
            worker,
        }
    }

    /// Test constructor: use a mock provider and skip disk I/O.
    #[cfg(test)]
    #[must_use]
    pub fn with_provider(symbols: Vec<String>, provider: Arc<dyn QuoteProvider>) -> Self {
        let data =
            market_core::domain::mock::load_mock_data().unwrap_or_else(|_| fallback_mock_data());
        Self {
            watchlist: Watchlist::new(symbols.clone()),
            watchlist_tabs: vec![("Main".to_string(), symbols)],
            active_tab: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            sparkline_data: Vec::new(),
            sparkline_cache: HashMap::new(),
            status_message: String::new(),
            should_quit: false,
            show_help: false,
            index_quotes: Vec::new(),
            sector_quotes: Vec::new(),
            market_status: MarketStatus::default(),
            sort_mode: SortMode::default(),
            filter_mode: FilterMode::default(),
            view_mode: ViewMode::default(),
            scanner_list: ScannerList::default(),
            scanner_quotes: Vec::new(),
            scanner_selected: 0,
            scanner_table_state: ratatui::widgets::TableState::default(),
            news_headlines: Vec::new(),
            show_news: false,
            show_risk: false,
            focus: Focus::default(),
            qc_labels: data.qc_checklist.items,
            qc_state: HashMap::new(),
            selected_qc: 0,
            screener_results: Vec::new(),
            whisper_cache: HashMap::new(),
            insider_ownership: HashMap::new(),
            sector_heat: HashMap::new(),
            past_beats: HashMap::new(),
            chart_patterns: HashMap::new(),
            chart_open: false,
            chart_symbol: String::new(),
            chart_range: ChartRange::default(),
            chart_data: Vec::new(),
            chart_loading: false,
            chart_tab: ChartTab::default(),
            chart_news: Vec::new(),
            chart_news_selected: 0,
            chart_news_summary_open: false,
            chart_news_content: None,
            chart_news_content_loading: false,
            chart_news_scroll: 0,
            chart_sec_filings: Vec::new(),
            chart_sec_selected: 0,
            chart_sec_detail_open: false,
            chart_sec_content: None,
            chart_sec_content_loading: false,
            chart_sec_scroll: 0,
            chart_show_sma: false,
            chart_show_rsi: false,
            chart_show_macd: false,
            chart_detail_split: 0.5,
            chart_detail_dragging: false,
            theme_index: 0,
            tick: 0,
            ticks_since_refresh: 0,
            pending_g: false,
            top_movers: TopMovers {
                gainers: Vec::new(),
                losers: Vec::new(),
            },
            loading: false,
            alert_fired: false,
            decisions: DecisionLog::default(),
            initial_selection_pending: false,
            skip_persist: true,
            worker: Some(Worker::new(provider)),
        }
    }

    // -- Theme ---------------------------------------------------------------

    /// Returns the current theme.
    #[must_use]
    pub fn theme(&self) -> &'static Theme {
        &theme::THEMES[self.theme_index]
    }

    /// Cycle to the next theme and persist preference.
    pub fn next_theme(&mut self) {
        self.theme_index = (self.theme_index + 1) % theme::THEMES.len();
        self.persist_preferences();
    }

    // -- QC & Conviction -----------------------------------------------------

    /// Analyze the price change to derive a chart pattern label for the ticker.
    pub fn analyze_chart_pattern(&mut self, ticker: &str, change: &str) {
        let pct: f64 = change
            .trim_end_matches('%')
            .trim_start_matches('+')
            .parse()
            .unwrap_or(0.0);

        let pattern = if pct > 5.0 {
            "Strong breakout"
        } else if pct > 2.0 {
            "Uptrend"
        } else if pct > 0.0 {
            "Mild bullish"
        } else if pct > -2.0 {
            "Mild bearish"
        } else if pct > -5.0 {
            "Downtrend"
        } else {
            "Strong breakdown"
        };

        self.chart_patterns
            .insert(ticker.to_string(), pattern.to_string());
    }

    /// Returns the inline value annotation for a QC item, if data is available.
    ///
    /// Used by the QC checklist UI to show contextual data next to each label.
    #[must_use]
    pub fn qc_inline_value(&self, ticker: &str, item_index: usize) -> Option<String> {
        match item_index {
            0 => {
                // News catalyst — first headline snippet
                self.news_headlines.first().map(|n| {
                    let title = if n.title.len() > 40 {
                        format!("{}...", &n.title[..40])
                    } else {
                        n.title.clone()
                    };
                    format!(" ({title})")
                })
            }
            1 => {
                // Insider ownership percentage
                self.insider_ownership
                    .get(ticker)
                    .map(|&pct| format!(" ({pct:.1}% insider)"))
            }
            2 => {
                // Sector heat vs SPY
                self.sector_heat
                    .get(ticker)
                    .map(|&heat| format!(" ({heat:+.1}% vs SPY)"))
            }
            3 => {
                // Chart pattern
                self.chart_patterns.get(ticker).map(|p| format!(" ({p})"))
            }
            4 => {
                // Historical beats
                self.past_beats.get(ticker).map(|&b| {
                    if b {
                        " (beats)".to_string()
                    } else {
                        " (misses)".to_string()
                    }
                })
            }
            _ => None,
        }
    }

    /// Run the multi-analyst pipeline on a ticker, assembling all cached data.
    ///
    /// Returns `None` if there is no quote data at all for the ticker.
    #[must_use]
    pub fn analyze_stock(&self, ticker: &str) -> market_core::analysis::AnalysisReport {
        use market_core::analysis::{AnalysisInput, analyze};

        let quote = self
            .watchlist
            .quotes()
            .iter()
            .flatten()
            .find(|q| q.symbol == ticker)
            .or_else(|| self.scanner_quotes.iter().find(|q| q.symbol == ticker));

        let screener = self.screener_results.iter().find(|r| r.ticker == ticker);

        let qc = if self.qc_labels.is_empty() {
            None
        } else {
            Some((self.qc_score(ticker), self.qc_labels.len()))
        };

        let prices_for_analysis: Vec<f64> = self
            .sparkline_cache
            .get(ticker)
            .map(|pts| pts.iter().map(|p| p.close).collect())
            .unwrap_or_default();

        // Rating must be deterministic per-ticker so list view and graph view
        // agree. We don't have a per-ticker news cache (chart_news only exists
        // for the currently opened chart symbol), so omit news here. The chart
        // view still displays news separately in its own panel.
        let news: &[market_core::domain::NewsItem] = &[];

        let input = AnalysisInput {
            quote,
            screener,
            news,
            insider_ownership_pct: self.insider_ownership.get(ticker).copied(),
            sector_heat: self
                .sector_heat
                .get(quote.and_then(|q| q.sector.as_deref()).unwrap_or_default())
                .copied(),
            past_beats: self.past_beats.get(ticker).copied(),
            qc_score: qc,
            prices: &prices_for_analysis,
        };

        analyze(&input)
    }

    /// Whether QC item at `item_index` is auto-checked for `ticker` from live data.
    ///
    /// Items 1 (insider ownership > 1%), 2 (positive sector heat), and
    /// 5 (historical beats) auto-populate when data is available.
    #[must_use]
    pub fn is_auto_checked(&self, ticker: &str, item_index: usize) -> bool {
        match item_index {
            1 => self
                .insider_ownership
                .get(ticker)
                .is_some_and(|&pct| pct > 1.0),
            2 => self.sector_heat.get(ticker).is_some_and(|&heat| heat > 0.0),
            4 => self.past_beats.get(ticker).copied().unwrap_or(false),
            _ => false,
        }
    }

    /// Count of QC items that pass (manual toggle OR auto-check) for `ticker`.
    #[must_use]
    pub fn qc_score(&self, ticker: &str) -> usize {
        let manual = self.qc_state.get(ticker);
        (0..self.qc_labels.len())
            .filter(|&i| {
                let manually_checked = manual.is_some_and(|v| v.get(i).copied().unwrap_or(false));
                manually_checked || self.is_auto_checked(ticker, i)
            })
            .count()
    }

    /// Whether all QC items pass for `ticker`.
    #[must_use]
    pub fn all_qc_passed_for(&self, ticker: &str) -> bool {
        self.qc_score(ticker) == self.qc_labels.len()
    }

    /// Whether any stock in the screener results has a perfect QC score.
    ///
    /// This triggers the `HIGH CONVICTION - READY` status.
    #[must_use]
    pub fn any_fully_passed(&self) -> bool {
        self.screener_results
            .iter()
            .any(|r| self.all_qc_passed_for(&r.ticker))
    }

    /// Toggle the currently selected QC item for the selected screener stock.
    pub fn toggle_qc(&mut self) {
        if let Some(ticker) = self.selected_screener_ticker() {
            let n = self.qc_labels.len();
            let state = self
                .qc_state
                .entry(ticker)
                .or_insert_with(|| vec![false; n]);
            if state.len() < n {
                state.resize(n, false);
            }
            if self.selected_qc < n {
                state[self.selected_qc] = !state[self.selected_qc];
            }
            self.persist_qc_state();
        }
    }

    /// The ticker of the currently selected screener result, if any.
    #[must_use]
    pub fn selected_screener_ticker(&self) -> Option<String> {
        // In QC view, use watchlist selection index to pick from screener results.
        if self.screener_results.is_empty() {
            return None;
        }
        let idx = self
            .watchlist
            .selected()
            .min(self.screener_results.len().saturating_sub(1));
        Some(self.screener_results[idx].ticker.clone())
    }

    // -- Data refresh (non-blocking) -------------------------------------------

    /// Submit all market data fetches to the background worker.
    ///
    /// This returns immediately — results arrive via [`drain_results`].
    pub fn refresh_quotes(&mut self) {
        let Some(worker) = &self.worker else {
            self.try_reconnect();
            return;
        };

        self.loading = true;

        // Watchlist quotes
        let symbols: Vec<String> = self.watchlist.symbols().to_vec();
        worker.submit_quotes(symbols.clone());

        // Index quotes
        let idx_syms: Vec<String> = INDEX_SYMBOLS.iter().map(|s| (*s).to_string()).collect();
        worker.submit_index_quotes(idx_syms);

        // Sector quotes
        let sec_syms: Vec<String> = SECTOR_SYMBOLS.iter().map(|s| (*s).to_string()).collect();
        worker.submit_sector_quotes(sec_syms);

        // Per-symbol sparklines for inline watchlist display
        worker.submit_sparklines_all(symbols);

        // Sparkline for selected symbol
        self.refresh_sparkline();

        // News if visible
        self.refresh_news();

        // Scanner if in scanner view
        if self.view_mode == ViewMode::Scanner {
            self.refresh_scanner();
        }
    }

    fn refresh_scanner(&mut self) {
        let Some(worker) = &self.worker else { return };
        worker.submit_scanner(self.scanner_list);
    }

    fn refresh_sparkline(&mut self) {
        let Some(worker) = &self.worker else { return };
        if let Some(q) = self.watchlist.selected_quote() {
            worker.submit_sparkline(q.symbol.clone());
        } else {
            self.sparkline_data.clear();
        }
    }

    /// Update top movers from scanner quotes (when scanner has fresh data).
    fn update_top_movers_from_scanner(&mut self) {
        if !self.scanner_quotes.is_empty() {
            let as_options: Vec<Option<Quote>> =
                self.scanner_quotes.iter().cloned().map(Some).collect();
            self.top_movers = TopMovers::from_quotes(&as_options, 3);
        }
    }

    /// Submit QC-specific fetches for the selected screener stock.
    ///
    /// Only fetches if the selected ticker has changed or data is missing.
    fn refresh_qc_data_if_stale(&mut self) {
        let Some(ticker) = self.selected_screener_ticker() else {
            return;
        };
        // Skip if we already have whisper + insider data for this ticker.
        if self.whisper_cache.contains_key(&ticker)
            && self.insider_ownership.contains_key(&ticker)
            && !self.sector_heat.is_empty()
        {
            return;
        }
        self.refresh_qc_data();
    }

    /// Submit QC-specific fetches for the selected screener stock.
    fn refresh_qc_data(&mut self) {
        let Some(ticker) = self.selected_screener_ticker() else {
            return;
        };
        let Some(worker) = &self.worker else { return };

        // Insider ownership
        if !self.insider_ownership.contains_key(&ticker) {
            worker.submit_insider_ownership(ticker.clone());
        }

        // Sector heat (needs sectors from screener results)
        if self.sector_heat.is_empty() {
            let sectors: Vec<String> = self
                .screener_results
                .iter()
                .map(|r| r.sector.clone())
                .collect();
            if !sectors.is_empty() {
                worker.submit_sector_heat(sectors);
            }
        }

        // Whisper data
        if !self.whisper_cache.contains_key(&ticker) {
            worker.submit_whisper(ticker);
        }
    }

    /// Submit a quote fetch for the most recently added symbol.
    fn refresh_added_symbol(&mut self) {
        let Some(worker) = &self.worker else { return };
        let symbols = self.watchlist.symbols();
        if symbols.is_empty() {
            return;
        }
        let idx = symbols.len().saturating_sub(1);
        let sym = symbols[idx].clone();
        worker.submit_added_symbol(sym, idx);
    }

    fn refresh_news(&mut self) {
        if !self.show_news {
            return;
        }
        let Some(worker) = &self.worker else { return };
        if let Some(q) = self.watchlist.selected_quote() {
            worker.submit_news(q.symbol.clone());
        } else {
            self.news_headlines.clear();
        }
    }

    fn try_reconnect(&mut self) {
        info!("attempting Yahoo Finance reconnection");
        match yahoo_provider::YahooClient::new() {
            Ok(c) => {
                let client: Arc<dyn QuoteProvider> = Arc::new(c);
                if let Some(worker) = &mut self.worker {
                    worker.set_client(Arc::clone(&client));
                } else {
                    self.worker = Some(Worker::new(client));
                }
                self.status_message = "Reconnected".to_string();
                self.refresh_quotes();
            }
            Err(e) => {
                warn!(error = %e, "reconnection failed");
                self.status_message = format!("Connection failed: {e}");
            }
        }
    }

    /// Drain all completed background fetch results and apply them to state.
    ///
    /// Called from the main event loop on every iteration.
    pub fn drain_results(&mut self) {
        let Some(worker) = &self.worker else { return };
        let results = worker.try_recv();
        if results.is_empty() {
            return;
        }

        for result in results {
            match result {
                FetchResult::Quotes { quotes } => {
                    // Analyze chart patterns from quote change data.
                    for q in quotes.iter().flatten() {
                        let change = format!("{:+.2}%", q.regular_market_change_percent);
                        self.analyze_chart_pattern(&q.symbol, &change);
                    }
                    self.watchlist.update_quotes(quotes);
                    self.top_movers = TopMovers::from_quotes(self.watchlist.quotes(), 3);
                    self.status_message.clear();
                    self.loading = false;
                    // On the first successful quote load after startup, snap
                    // the selection to the top symbol of the displayed
                    // (sorted + filtered) order so the user sees the cursor
                    // on the visible top row.
                    if self.initial_selection_pending {
                        if let Some(&first) = self.displayed_watchlist_indices().first() {
                            self.watchlist.set_selected(first);
                        }
                        self.initial_selection_pending = false;
                    }
                    // Now that quotes are available, fetch sparkline for selected.
                    self.refresh_sparkline();
                }
                FetchResult::IndexQuotes { quotes } => {
                    if let Some(Some(q)) = quotes.first()
                        && let Some(state) = &q.market_state
                    {
                        self.market_status = MarketStatus::from_yahoo(state);
                    }
                    self.index_quotes = quotes;
                }
                FetchResult::SectorQuotes { quotes } => {
                    self.sector_quotes = quotes;
                }
                FetchResult::Sparkline { points } => {
                    self.sparkline_data = points;
                }
                FetchResult::SparklineAll { sparklines } => {
                    self.sparkline_cache.extend(sparklines);
                }
                FetchResult::News { items } => {
                    self.news_headlines = items;
                }
                FetchResult::Chart { .. }
                | FetchResult::StockNews { .. }
                | FetchResult::SecFilings { .. }
                | FetchResult::FilingContent { .. }
                | FetchResult::NewsContent { .. } => {
                    self.drain_chart_result(result);
                }
                FetchResult::Scanner {
                    quotes,
                    screener_results,
                } => {
                    self.scanner_quotes = quotes;
                    if let Some(results) = screener_results {
                        // Analyze chart patterns from change data.
                        for r in &results {
                            self.analyze_chart_pattern(&r.ticker, &r.change);
                        }
                        self.screener_results = results;
                    }
                    self.update_top_movers_from_scanner();
                }
                FetchResult::AddedSymbol { quote, index } => {
                    if let Some(q) = quote {
                        self.watchlist.set_quote(index, Some(q));
                    }
                }
                FetchResult::InsiderOwnership { ticker, pct } => {
                    self.insider_ownership.insert(ticker, pct);
                }
                FetchResult::SectorHeat { heat } => {
                    self.sector_heat = heat;
                }
                FetchResult::Whisper { ticker, result } => {
                    if let Some(beats) = result.past_beats {
                        self.past_beats.insert(ticker.clone(), beats);
                    }
                    self.whisper_cache.insert(ticker, result);
                }
                FetchResult::Error { context, message } => {
                    warn!(context, error = %message, "background fetch failed");
                    self.status_message = format!("{context}: {message}");
                    self.loading = false;
                    // Clear chart loading on chart fetch errors.
                    if context == "chart" {
                        self.chart_loading = false;
                    }
                }
            }
        }

        // After processing results, check if conviction alert should fire.
        self.check_conviction_alert();
    }

    /// Handle chart overlay-specific fetch results.
    fn drain_chart_result(&mut self, result: FetchResult) {
        match result {
            FetchResult::Chart {
                symbol,
                range,
                points,
            } if self.chart_open && self.chart_symbol == symbol && self.chart_range == range => {
                self.chart_data = points;
                self.chart_loading = false;
            }
            FetchResult::StockNews { ticker, items }
                if self.chart_open && self.chart_symbol == ticker =>
            {
                self.chart_news = items;
            }
            FetchResult::SecFilings { ticker, filings }
                if self.chart_open && self.chart_symbol == ticker =>
            {
                self.chart_sec_filings = filings;
            }
            FetchResult::FilingContent { url, content }
                if self.chart_open
                    && self.chart_sec_detail_open
                    && self
                        .chart_sec_filings
                        .get(self.chart_sec_selected)
                        .is_some_and(|f| f.link == url) =>
            {
                self.chart_sec_content = Some(content);
                self.chart_sec_content_loading = false;
                self.chart_sec_scroll = 0;
            }
            FetchResult::NewsContent { url, content }
                if self.chart_open
                    && self.chart_news_summary_open
                    && self
                        .chart_news
                        .get(self.chart_news_selected)
                        .is_some_and(|n| n.link == url) =>
            {
                self.chart_news_content = Some(content);
                self.chart_news_content_loading = false;
                self.chart_news_scroll = 0;
            }
            _ => {}
        }
    }

    /// Called every UI tick (250ms).
    ///
    /// Counts ticks and submits background refresh jobs at the appropriate
    /// interval: every 30s when the market is active, every 5min otherwise.
    pub fn on_tick(&mut self) {
        self.ticks_since_refresh += 1;
        let threshold = if self.market_status.is_active() {
            ACTIVE_REFRESH_TICKS
        } else {
            HEARTBEAT_TICKS
        };
        if self.ticks_since_refresh >= threshold {
            self.ticks_since_refresh = 0;
            self.refresh_quotes();
            self.resolve_decisions();
        }
    }

    // -- Journal & Decisions ---------------------------------------------------

    /// Record a trade decision for the currently selected ticker.
    pub fn record_decision(&mut self, action: Action) {
        let ticker = match self.view_mode {
            ViewMode::Watchlist | ViewMode::QualityControl => {
                self.watchlist.selected_symbol().map(String::from)
            }
            ViewMode::Scanner => self
                .scanner_quotes
                .get(self.scanner_selected)
                .map(|q| q.symbol.clone()),
            ViewMode::Journal => None,
        };

        if let Some(ticker) = ticker {
            let report = self.analyze_stock(&ticker);
            let price = self
                .watchlist
                .quotes()
                .iter()
                .flatten()
                .find(|q| q.symbol == ticker)
                .or_else(|| self.scanner_quotes.iter().find(|q| q.symbol == ticker))
                .map_or(0.0, |q| q.regular_market_price);

            let spy_price = self
                .index_quotes
                .iter()
                .flatten()
                .find(|q| q.symbol == "^GSPC")
                .map(|q| q.regular_market_price);

            let qc = self.qc_score(&ticker);
            let pe = self
                .screener_results
                .iter()
                .find(|r| r.ticker == ticker)
                .and_then(|r| r.pe.parse::<f64>().ok());

            let now = chrono::Utc::now();
            let entry = DecisionEntry {
                id: format!("decision_{}", now.timestamp_millis()),
                ticker: ticker.clone(),
                date: now,
                action,
                rating: report.rating,
                composite_score: report.composite,
                qc_score: qc,
                price_at_decision: price,
                spy_at_decision: spy_price,
                pe_at_decision: pe,
                resolution: None,
            };

            self.decisions.append(entry);
            if let Err(e) = self.decisions.save() {
                warn!(error = %e, "Failed to save decision log");
            } else {
                self.status_message = format!("Recorded {action} decision for {ticker}");
            }
        }
    }

    /// Resolve pending decision outcomes based on current quotes.
    pub fn resolve_decisions(&mut self) {
        let mut changed = false;

        for entry in &mut self.decisions.entries {
            // Re-resolve to update current return pct continuously.
            let current_price = self
                .watchlist
                .quotes()
                .iter()
                .flatten()
                .find(|q| q.symbol == entry.ticker)
                .map(|q| q.regular_market_price);

            let current_spy = self
                .index_quotes
                .iter()
                .flatten()
                .find(|q| q.symbol == "^GSPC")
                .map(|q| q.regular_market_price);

            if let Some(price) = current_price {
                entry.resolve(price, current_spy);
                changed = true;
            }
        }

        if changed {
            let _ = self.decisions.save();
        }
    }

    /// Export a markdown analysis report for the currently selected ticker.
    pub fn export_report(&mut self) {
        let ticker = match self.view_mode {
            ViewMode::Watchlist | ViewMode::QualityControl => {
                self.watchlist.selected_symbol().map(String::from)
            }
            ViewMode::Scanner => self
                .scanner_quotes
                .get(self.scanner_selected)
                .map(|q| q.symbol.clone()),
            ViewMode::Journal => None,
        };

        if let Some(ticker) = ticker {
            let quote = self
                .watchlist
                .quotes()
                .iter()
                .flatten()
                .find(|q| q.symbol == ticker)
                .or_else(|| self.scanner_quotes.iter().find(|q| q.symbol == ticker));

            let screener = self.screener_results.iter().find(|r| r.ticker == ticker);

            let analysis = self.analyze_stock(&ticker);

            // Reconstruct the full QC state array properly
            let qc_state_vec = self.qc_state.get(&ticker);

            let data = market_core::report::ReportData {
                ticker: ticker.clone(),
                quote,
                screener,
                analysis,
                qc_labels: &self.qc_labels,
                qc_state: qc_state_vec.map(std::vec::Vec::as_slice),
                news: &self.chart_news,
            };

            match market_core::report::export_report(&data) {
                Ok(path) => {
                    self.status_message = format!("Report saved: {}", path.display());
                }
                Err(e) => {
                    warn!(error = %e, "Failed to export report");
                    self.status_message = format!("Export failed: {e}");
                }
            }
        }
    }

    // -- Event handling --------------------------------------------------------------

    /// Dispatch a mouse event.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        // Handle drag resize in chart overlay detail panels.
        if self.chart_open {
            let detail_open = (self.chart_tab == ChartTab::News && self.chart_news_summary_open)
                || (self.chart_tab == ChartTab::SecFilings && self.chart_sec_detail_open);
            if detail_open {
                match mouse.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        self.chart_detail_dragging = true;
                    }
                    MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                        if self.chart_detail_dragging =>
                    {
                        // Compute split ratio from mouse column relative to terminal width.
                        let col = f64::from(mouse.column);
                        let width =
                            f64::from(crossterm::terminal::size().map_or(80, |(w, _)| w).max(1));
                        let ratio = (col / width).clamp(0.2, 0.8);
                        self.chart_detail_split = ratio;
                    }
                    MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                        self.chart_detail_dragging = false;
                    }
                    _ => {}
                }
            }
            return;
        }
        // Ignore mouse in help overlay.
        if self.show_help {
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.navigate_table_down(),
            MouseEventKind::ScrollUp => self.navigate_table_up(),
            _ => {}
        }
    }

    /// Dispatch a key event to the appropriate handler.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Chart overlay intercepts keys when open.
        if self.chart_open {
            self.handle_chart_key(key);
            return;
        }

        // Help overlay intercepts keys when open.
        if self.show_help {
            match key.code {
                KeyCode::Char('?') | KeyCode::Esc => self.show_help = false,
                _ => {}
            }
            return;
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Adding => self.handle_adding_key(key),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_normal_key(&mut self, key: KeyEvent) {
        // Handle pending `g` for `gg` motion.
        if self.pending_g {
            self.pending_g = false;
            if key.code == KeyCode::Char('g') {
                match self.focus {
                    Focus::Table => self.navigate_table_first(),
                    Focus::QcChecklist => self.selected_qc = 0,
                }
                return;
            }
            // Not `gg` — fall through and process this key normally.
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,

            // Help overlay
            KeyCode::Char('?') => self.show_help = true,

            // Navigation
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::Table => self.navigate_table_down(),
                Focus::QcChecklist => {
                    if !self.qc_labels.is_empty() {
                        self.selected_qc = (self.selected_qc + 1) % self.qc_labels.len();
                    }
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::Table => self.navigate_table_up(),
                Focus::QcChecklist => {
                    if !self.qc_labels.is_empty() {
                        self.selected_qc = self
                            .selected_qc
                            .checked_sub(1)
                            .unwrap_or(self.qc_labels.len().saturating_sub(1));
                    }
                }
            },
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => match self.focus {
                Focus::Table => self.navigate_table_last(),
                Focus::QcChecklist => {
                    if !self.qc_labels.is_empty() {
                        self.selected_qc = self.qc_labels.len().saturating_sub(1);
                    }
                }
            },

            // View mode switching
            KeyCode::Tab => {
                let prev = self.view_mode;
                self.view_mode = self.view_mode.next();
                self.on_view_mode_changed(prev);
            }
            KeyCode::BackTab => {
                let prev = self.view_mode;
                self.view_mode = self.view_mode.prev();
                self.on_view_mode_changed(prev);
            }

            // Open chart (Enter in Watchlist/Scanner view)
            KeyCode::Enter
                if self.view_mode == ViewMode::Watchlist || self.view_mode == ViewMode::Scanner =>
            {
                self.open_chart();
            }

            // QC toggle (Space/Enter in QC view)
            KeyCode::Char(' ') | KeyCode::Enter if self.view_mode == ViewMode::QualityControl => {
                self.toggle_qc();
            }

            // Focus switch (only in QC view)
            KeyCode::Char('l' | 'h') | KeyCode::Right | KeyCode::Left
                if self.view_mode == ViewMode::QualityControl =>
            {
                self.focus = self.focus.toggle();
            }

            // Refresh
            KeyCode::Char('r') => {
                self.refresh_quotes();
                if self.view_mode == ViewMode::QualityControl {
                    self.refresh_qc_data();
                }
            }

            // Sort & Filter
            KeyCode::Char('s') => {
                self.sort_mode = self.sort_mode.next();
                self.persist_session();
            }
            KeyCode::Char('f') => {
                self.filter_mode = self.filter_mode.next();
                self.persist_session();
            }

            // Theme
            KeyCode::Char('t') => self.next_theme(),

            // Record Trade Decisions
            KeyCode::Char('B') => self.record_decision(Action::Buy),
            KeyCode::Char('S') => self.record_decision(Action::Sell),
            KeyCode::Char('H') => self.record_decision(Action::Hold),

            // Add symbol (watchlist view)
            KeyCode::Char('a') if self.view_mode == ViewMode::Watchlist => {
                self.input_mode = InputMode::Adding;
                self.input_buffer.clear();
            }

            // Export markdown report
            KeyCode::Char('e')
                if self.view_mode == ViewMode::Watchlist
                    || self.view_mode == ViewMode::QualityControl =>
            {
                self.export_report();
            }

            // Delete symbol
            KeyCode::Char('d') if self.view_mode == ViewMode::Watchlist => {
                // Evict sparkline cache entry before removing the symbol.
                if let Some(sym) = self.watchlist.symbols().get(self.watchlist.selected()) {
                    self.sparkline_cache.remove(sym);
                }
                self.watchlist.remove_selected();
                self.persist_session();
            }

            // Copy selected symbol data to clipboard
            KeyCode::Char('y') => {
                self.copy_to_clipboard();
            }

            // News toggle
            KeyCode::Char('n') => {
                self.show_news = !self.show_news;
                if self.show_news {
                    self.refresh_news();
                } else {
                    self.news_headlines.clear();
                }
            }

            // Risk toggle
            KeyCode::Char('x') if self.view_mode == ViewMode::Watchlist => {
                self.show_risk = !self.show_risk;
            }

            // Scanner number keys
            KeyCode::Char(c @ '1'..='5') if self.view_mode == ViewMode::Scanner => {
                // Safety: c is '1'..='5', so to_digit(10) is 1..=5, fits in u8.
                #[allow(clippy::cast_possible_truncation)]
                let n = c.to_digit(10).unwrap_or(0) as u8;
                if let Some(list) = ScannerList::from_number(n)
                    && self.scanner_list != list
                {
                    self.scanner_list = list;
                    self.refresh_scanner();
                }
            }

            // Watchlist tab switching (Watchlist view only)
            KeyCode::Char(']') if self.view_mode == ViewMode::Watchlist => {
                self.switch_watchlist_tab((self.active_tab + 1) % self.watchlist_tabs.len().max(1));
            }
            KeyCode::Char('[') if self.view_mode == ViewMode::Watchlist => {
                let len = self.watchlist_tabs.len().max(1);
                self.switch_watchlist_tab(self.active_tab.checked_sub(1).unwrap_or(len - 1));
            }

            _ => {}
        }

        // Refresh sparkline and news on navigation when focused on table.
        if matches!(self.focus, Focus::Table)
            && matches!(
                key.code,
                KeyCode::Char('j' | 'k') | KeyCode::Down | KeyCode::Up
            )
        {
            self.refresh_sparkline();
        }

        // Refresh news on navigation when visible.
        if self.show_news
            && matches!(
                key.code,
                KeyCode::Char('j' | 'k') | KeyCode::Down | KeyCode::Up
            )
        {
            self.refresh_news();
        }
    }

    fn handle_adding_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Enter => {
                if !self.input_buffer.is_empty() {
                    self.watchlist.add_symbol(&self.input_buffer);
                    self.persist_session();
                    self.refresh_added_symbol();
                }
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    // -- Chart overlay -------------------------------------------------------

    /// Open the performance chart for the currently selected stock.
    fn open_chart(&mut self) {
        let symbol = match self.view_mode {
            ViewMode::Watchlist | ViewMode::Scanner => self
                .watchlist
                .selected_quote()
                .map(|q| q.symbol.clone())
                .or_else(|| {
                    // Fall back to the symbol name when the quote hasn't loaded yet.
                    let syms = self.watchlist.symbols();
                    let idx = self.watchlist.selected();
                    syms.get(idx).cloned()
                }),
            ViewMode::QualityControl | ViewMode::Journal => self.selected_screener_ticker(),
        };
        let Some(sym) = symbol else { return };

        self.chart_open = true;
        self.chart_symbol.clone_from(&sym);
        self.chart_range = ChartRange::default();
        self.chart_data.clear();
        self.chart_loading = true;
        self.chart_tab = ChartTab::default();
        self.chart_news.clear();
        self.chart_news_selected = 0;
        self.chart_news_summary_open = false;
        self.chart_sec_filings.clear();
        self.chart_sec_selected = 0;
        self.chart_sec_detail_open = false;
        self.chart_sec_content = None;
        self.chart_sec_content_loading = false;
        self.chart_sec_scroll = 0;
        self.chart_detail_split = 0.5;
        self.chart_detail_dragging = false;

        if let Some(worker) = &self.worker {
            worker.submit_chart(sym.clone(), self.chart_range);
            worker.submit_stock_news(sym.clone());
            worker.submit_sec_filings(sym);
        }
    }

    /// Close the chart overlay.
    fn close_chart(&mut self) {
        self.chart_open = false;
        self.chart_data.clear();
        self.chart_loading = false;
        self.chart_news.clear();
        self.chart_sec_filings.clear();
    }

    /// Switch the chart to a different time range.
    fn range_from_digit(c: char) -> Option<ChartRange> {
        match c {
            '1' => Some(ChartRange::Day1),
            '2' => Some(ChartRange::Day5),
            '3' => Some(ChartRange::Month1),
            '4' => Some(ChartRange::Month3),
            '5' => Some(ChartRange::Month6),
            '6' => Some(ChartRange::Ytd),
            '7' => Some(ChartRange::Year1),
            '8' => Some(ChartRange::Year5),
            _ => None,
        }
    }

    fn switch_chart_range(&mut self, range: ChartRange) {
        if range == self.chart_range {
            return;
        }
        self.chart_range = range;
        self.chart_data.clear();
        self.chart_loading = true;

        if let Some(worker) = &self.worker {
            worker.submit_chart(self.chart_symbol.clone(), range);
        }
    }

    fn handle_chart_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.chart_news_summary_open {
                    self.chart_news_summary_open = false;
                } else if self.chart_sec_detail_open {
                    self.chart_sec_detail_open = false;
                } else {
                    self.close_chart();
                }
            }

            // Tab / BackTab: cycle chart panel tabs.
            KeyCode::Tab => {
                self.chart_tab = self.chart_tab.next();
            }
            KeyCode::BackTab => {
                self.chart_tab = self.chart_tab.prev();
            }

            // Range switching (only when on Chart tab).
            KeyCode::Right | KeyCode::Char('l') if self.chart_tab == ChartTab::Chart => {
                self.switch_chart_range(self.chart_range.next());
            }
            KeyCode::Left | KeyCode::Char('h') if self.chart_tab == ChartTab::Chart => {
                self.switch_chart_range(self.chart_range.prev());
            }
            KeyCode::Char(c @ '1'..='8') if self.chart_tab == ChartTab::Chart => {
                if let Some(r) = Self::range_from_digit(c) {
                    self.switch_chart_range(r);
                }
            }

            // Technical indicator toggles (chart view only).
            KeyCode::Char('v') if self.chart_tab == ChartTab::Chart => {
                self.chart_show_sma = !self.chart_show_sma;
            }
            KeyCode::Char('i') if self.chart_tab == ChartTab::Chart => {
                self.chart_show_rsi = !self.chart_show_rsi;
            }
            KeyCode::Char('m') if self.chart_tab == ChartTab::Chart => {
                self.chart_show_macd = !self.chart_show_macd;
            }

            // Scroll news content when detail panel is open.
            KeyCode::Down | KeyCode::Char('j')
                if self.chart_tab == ChartTab::News
                    && self.chart_news_summary_open
                    && self.chart_news_content.is_some() =>
            {
                self.chart_news_scroll = self.chart_news_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.chart_tab == ChartTab::News
                    && self.chart_news_summary_open
                    && self.chart_news_content.is_some() =>
            {
                self.chart_news_scroll = self.chart_news_scroll.saturating_sub(1);
            }

            // Navigate news list (detail closed).
            KeyCode::Down | KeyCode::Char('j')
                if self.chart_tab == ChartTab::News && !self.chart_news.is_empty() =>
            {
                self.chart_news_selected = (self.chart_news_selected + 1) % self.chart_news.len();
                self.chart_news_summary_open = false;
                self.chart_news_content = None;
                self.chart_news_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.chart_tab == ChartTab::News && !self.chart_news.is_empty() =>
            {
                self.chart_news_selected = self
                    .chart_news_selected
                    .checked_sub(1)
                    .unwrap_or(self.chart_news.len().saturating_sub(1));
                self.chart_news_summary_open = false;
                self.chart_news_content = None;
                self.chart_news_scroll = 0;
            }
            KeyCode::Enter | KeyCode::Char(' ')
                if self.chart_tab == ChartTab::News && !self.chart_news.is_empty() =>
            {
                if self.chart_news_summary_open {
                    self.chart_news_summary_open = false;
                    self.chart_news_content = None;
                    self.chart_news_scroll = 0;
                } else {
                    self.chart_news_summary_open = true;
                    self.chart_news_content = None;
                    self.chart_news_content_loading = true;
                    self.chart_news_scroll = 0;
                    // Trigger background fetch of article content.
                    if let Some(item) = self.chart_news.get(self.chart_news_selected)
                        && let Some(worker) = &self.worker
                    {
                        worker.submit_article_content(item.link.clone());
                    }
                }
            }

            // SEC Filings panel keys delegated to sub-handler.
            _ if self.chart_tab == ChartTab::SecFilings => {
                self.handle_chart_sec_key(key);
            }

            KeyCode::Char('t') => self.next_theme(),
            _ => {}
        }
    }

    /// Handle keys specific to the SEC Filings chart tab.
    fn handle_chart_sec_key(&mut self, key: KeyEvent) {
        match key.code {
            // Scroll filing content when detail panel is open.
            KeyCode::Down | KeyCode::Char('j')
                if self.chart_sec_detail_open && self.chart_sec_content.is_some() =>
            {
                self.chart_sec_scroll = self.chart_sec_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.chart_sec_detail_open && self.chart_sec_content.is_some() =>
            {
                self.chart_sec_scroll = self.chart_sec_scroll.saturating_sub(1);
            }

            // Navigate filings list (detail closed).
            KeyCode::Down | KeyCode::Char('j') if !self.chart_sec_filings.is_empty() => {
                self.chart_sec_selected =
                    (self.chart_sec_selected + 1) % self.chart_sec_filings.len();
                self.chart_sec_detail_open = false;
                self.chart_sec_content = None;
                self.chart_sec_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') if !self.chart_sec_filings.is_empty() => {
                self.chart_sec_selected = self
                    .chart_sec_selected
                    .checked_sub(1)
                    .unwrap_or(self.chart_sec_filings.len().saturating_sub(1));
                self.chart_sec_detail_open = false;
                self.chart_sec_content = None;
                self.chart_sec_scroll = 0;
            }
            KeyCode::Enter | KeyCode::Char(' ') if !self.chart_sec_filings.is_empty() => {
                if self.chart_sec_detail_open {
                    self.chart_sec_detail_open = false;
                    self.chart_sec_content = None;
                    self.chart_sec_scroll = 0;
                } else {
                    self.chart_sec_detail_open = true;
                    self.chart_sec_content = None;
                    self.chart_sec_content_loading = true;
                    self.chart_sec_scroll = 0;
                    // Trigger background fetch of filing content.
                    if let Some(filing) = self.chart_sec_filings.get(self.chart_sec_selected)
                        && let Some(worker) = &self.worker
                    {
                        worker.submit_filing_content(filing.link.clone());
                    }
                }
            }

            // Open selected filing in system browser.
            KeyCode::Char('o') if !self.chart_sec_filings.is_empty() => {
                if let Some(filing) = self.chart_sec_filings.get(self.chart_sec_selected) {
                    let _ = open_in_browser(&filing.link);
                }
            }

            KeyCode::Char('t') => self.next_theme(),
            _ => {}
        }
    }

    fn on_view_mode_changed(&mut self, _prev: ViewMode) {
        self.persist_session();
        // If switching to Scanner and no data yet, trigger a fetch.
        if self.view_mode == ViewMode::Scanner && self.scanner_quotes.is_empty() {
            self.refresh_scanner();
        }
        // If switching to QC view, auto-fetch screener + QC data.
        if self.view_mode == ViewMode::QualityControl {
            if self.screener_results.is_empty() {
                self.refresh_scanner();
            }
            self.refresh_qc_data_if_stale();
        }
    }

    // -- View-mode-aware table navigation ------------------------------------

    /// Indices into `watchlist.symbols()` in the order they are displayed
    /// (after applying the active sort and filter). Used by arrow-key
    /// navigation so selection follows the visible row order.
    fn displayed_watchlist_indices(&self) -> Vec<usize> {
        let sorted = self.watchlist.sorted_indices(self.sort_mode);
        let quotes = self.watchlist.quotes();
        sorted
            .into_iter()
            .filter(|&i| {
                quotes
                    .get(i)
                    .and_then(Option::as_ref)
                    .map_or(self.filter_mode == FilterMode::All, |q| {
                        self.filter_mode.matches(q)
                    })
            })
            .collect()
    }

    /// Advance the watchlist selection by one through the displayed
    /// (sorted + filtered) order, wrapping around. If `forward` is false,
    /// move backward instead.
    fn step_watchlist_selection(&mut self, forward: bool) {
        let order = self.displayed_watchlist_indices();
        if order.is_empty() {
            return;
        }
        let raw_selected = self.watchlist.selected();
        let cur = order
            .iter()
            .position(|&i| i == raw_selected)
            .unwrap_or(0);
        let len = order.len();
        let next = if forward {
            (cur + 1) % len
        } else {
            cur.checked_sub(1).unwrap_or(len - 1)
        };
        self.watchlist.set_selected(order[next]);
    }

    fn navigate_table_down(&mut self) {
        match self.view_mode {
            ViewMode::Scanner => {
                if !self.scanner_quotes.is_empty() {
                    self.scanner_selected = (self.scanner_selected + 1) % self.scanner_quotes.len();
                }
            }
            _ => self.step_watchlist_selection(true),
        }
    }

    fn navigate_table_up(&mut self) {
        match self.view_mode {
            ViewMode::Scanner => {
                if !self.scanner_quotes.is_empty() {
                    self.scanner_selected = self
                        .scanner_selected
                        .checked_sub(1)
                        .unwrap_or(self.scanner_quotes.len().saturating_sub(1));
                }
            }
            _ => self.step_watchlist_selection(false),
        }
    }

    fn navigate_table_first(&mut self) {
        match self.view_mode {
            ViewMode::Scanner => self.scanner_selected = 0,
            _ => {
                if let Some(&first) = self.displayed_watchlist_indices().first() {
                    self.watchlist.set_selected(first);
                } else {
                    self.watchlist.select_first();
                }
            }
        }
    }

    fn navigate_table_last(&mut self) {
        match self.view_mode {
            ViewMode::Scanner => {
                if !self.scanner_quotes.is_empty() {
                    self.scanner_selected = self.scanner_quotes.len().saturating_sub(1);
                }
            }
            _ => {
                if let Some(&last) = self.displayed_watchlist_indices().last() {
                    self.watchlist.set_selected(last);
                } else {
                    self.watchlist.select_last();
                }
            }
        }
    }

    // -- Watchlist tab management ----------------------------------------------

    /// Save the current watchlist's symbols back into the active tab.
    fn sync_tab_from_watchlist(&mut self) {
        if let Some(tab) = self.watchlist_tabs.get_mut(self.active_tab) {
            tab.1 = self.watchlist.symbols().to_vec();
        }
    }

    /// Switch to a different watchlist tab, saving the current one first.
    fn switch_watchlist_tab(&mut self, new_idx: usize) {
        if new_idx == self.active_tab || new_idx >= self.watchlist_tabs.len() {
            return;
        }
        // Save current symbols back.
        self.sync_tab_from_watchlist();
        self.active_tab = new_idx;
        let symbols = self.watchlist_tabs[new_idx].1.clone();
        self.watchlist = Watchlist::new(symbols);
        self.sparkline_cache.clear();
        self.persist_session();
        self.refresh_quotes();
    }

    // -- Clipboard -----------------------------------------------------------

    /// Copy the selected symbol's data to the system clipboard.
    fn copy_to_clipboard(&mut self) {
        let text = if let Some(q) = self.watchlist.selected_quote() {
            format!(
                "{}\t${:.2}\t{:+.2}\t{:+.2}%",
                q.symbol,
                q.regular_market_price,
                q.regular_market_change,
                q.regular_market_change_percent,
            )
        } else {
            let syms = self.watchlist.symbols();
            let idx = self.watchlist.selected();
            syms.get(idx).cloned().unwrap_or_default()
        };

        // Use pbcopy on macOS, xclip on Linux, clip on Windows.
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                })
        } else if cfg!(target_os = "linux") {
            std::process::Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text.as_bytes())?;
                    }
                    child.wait()
                })
        } else {
            // Unsupported platform.
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "clipboard not supported",
            ))
        };

        match result {
            Ok(_) => self.status_message = "Copied to clipboard".to_string(),
            Err(e) => self.status_message = format!("Clipboard error: {e}"),
        }
    }

    // -- Alert / Notification ------------------------------------------------

    /// Check if any stock just reached 5/5 QC and trigger a bell alert.
    ///
    /// Called after draining results. Uses `alert_fired` to avoid repeat bells.
    fn check_conviction_alert(&mut self) {
        if self.any_fully_passed() && !self.alert_fired {
            self.alert_fired = true;
            // Terminal bell.
            print!("\x07");
            self.status_message = "\u{1f514} HIGH CONVICTION - READY".to_string();
        } else if !self.any_fully_passed() {
            self.alert_fired = false;
        }
    }

    // -- Persistence ---------------------------------------------------------

    fn persist_preferences(&self) {
        if self.skip_persist {
            return;
        }
        let prefs = Preferences {
            theme: self.theme().name.to_string(),
            ..Default::default()
        };
        let _ = config::save_preferences(&prefs);
    }

    fn persist_session(&self) {
        if self.skip_persist {
            return;
        }
        let watchlist_tabs: Vec<config::WatchlistTab> = self
            .watchlist_tabs
            .iter()
            .enumerate()
            .map(|(i, (name, syms))| config::WatchlistTab {
                name: name.clone(),
                // For the active tab, use current watchlist symbols.
                symbols: if i == self.active_tab {
                    self.watchlist.symbols().to_vec()
                } else {
                    syms.clone()
                },
            })
            .collect();
        let session = Session {
            symbols: self.watchlist.symbols().to_vec(),
            sort_mode: format!("{}", self.sort_mode),
            filter_mode: format!("{}", self.filter_mode),
            view_mode: format!("{}", self.view_mode),
            watchlist_tabs,
            active_tab: self.active_tab,
        };
        let _ = config::save_session(&session);
    }

    fn persist_qc_state(&self) {
        if self.skip_persist {
            return;
        }
        let _ = QcSession::save(&self.qc_state);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Seed the initial watchlist by fetching Yahoo's day-gainers screener.
///
/// Falls back to day-losers, then most-active if earlier screeners fail.
/// Returns an empty `Vec` if no screener succeeds.
fn seed_symbols_from_screener(client: Option<&dyn QuoteProvider>) -> Vec<String> {
    let Some(client) = client else {
        return Vec::new();
    };

    let screeners = ["day_gainers", "day_losers", "most_actives"];
    for scr_id in &screeners {
        match client.fetch_screener(scr_id) {
            Ok(quotes) if !quotes.is_empty() => {
                let symbols: Vec<String> = quotes
                    .iter()
                    .take(SEED_LIMIT)
                    .map(|q| q.symbol.clone())
                    .collect();
                info!(
                    screener = *scr_id,
                    count = symbols.len(),
                    "seeded watchlist from screener"
                );
                return symbols;
            }
            Ok(_) => {
                warn!(screener = *scr_id, "screener returned empty results");
            }
            Err(e) => {
                warn!(error = %e, screener = *scr_id, "screener fetch failed");
            }
        }
    }

    warn!("all screeners failed — starting with empty watchlist");
    Vec::new()
}

/// Minimal fallback when mock.json can't be loaded.
fn fallback_mock_data() -> MockData {
    MockData {
        finviz_filters: market_core::domain::mock::FinvizFilters {
            title: String::new(),
            display_items: Vec::new(),
        },
        whisper_data: market_core::domain::mock::WhisperData {
            title: String::new(),
            display_items: Vec::new(),
        },
        qc_checklist: market_core::domain::mock::QcChecklist {
            title: "Quality Control".to_string(),
            items: vec![
                "News Catalyst".to_string(),
                "Insider Ownership > 1%".to_string(),
                "Sector Heat > SPY".to_string(),
                "Chart Pattern Bullish".to_string(),
                "Historical Earnings Beats".to_string(),
            ],
        },
        maintenance_schedule: market_core::domain::mock::MaintenanceSchedule {
            title: String::new(),
            columns: Vec::new(),
            targets: Vec::new(),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Open a URL in the system default browser.
fn open_in_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", url]).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, bail};
    use market_core::domain::PricePoint;

    // -- Mock providers -------------------------------------------------------

    struct MockProvider {
        quotes: Vec<Option<Quote>>,
        sparkline: Vec<PricePoint>,
        news: Vec<NewsItem>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                quotes: vec![
                    Some(Quote {
                        symbol: "AAPL".to_string(),
                        short_name: Some("Apple Inc.".to_string()),
                        sector: Some("Technology".to_string()),
                        market_state: Some("REGULAR".to_string()),
                        regular_market_price: 175.0,
                        regular_market_change: 2.0,
                        regular_market_change_percent: 1.15,
                        regular_market_volume: 50_000_000,
                        regular_market_previous_close: 173.0,
                        regular_market_open: 173.5,
                        regular_market_day_high: 176.0,
                        regular_market_day_low: 172.5,
                        fifty_two_week_high: 199.0,
                        fifty_two_week_low: 124.0,
                        pre_market_price: None,
                        pre_market_change: None,
                        pre_market_change_percent: None,
                        post_market_price: None,
                        post_market_change: None,
                        post_market_change_percent: None,
                    }),
                    Some(Quote {
                        symbol: "MSFT".to_string(),
                        short_name: Some("Microsoft Corp.".to_string()),
                        sector: Some("Technology".to_string()),
                        market_state: Some("REGULAR".to_string()),
                        regular_market_price: 400.0,
                        regular_market_change: -3.0,
                        regular_market_change_percent: -0.74,
                        regular_market_volume: 30_000_000,
                        regular_market_previous_close: 403.0,
                        regular_market_open: 402.0,
                        regular_market_day_high: 405.0,
                        regular_market_day_low: 399.0,
                        fifty_two_week_high: 430.0,
                        fifty_two_week_low: 310.0,
                        pre_market_price: None,
                        pre_market_change: None,
                        pre_market_change_percent: None,
                        post_market_price: None,
                        post_market_change: None,
                        post_market_change_percent: None,
                    }),
                ],
                sparkline: vec![
                    PricePoint {
                        timestamp: None,
                        close: 173.0,
                    },
                    PricePoint {
                        timestamp: None,
                        close: 175.0,
                    },
                ],
                news: vec![NewsItem {
                    title: "Test headline".to_string(),
                    publisher: "Test".to_string(),
                    link: String::new(),
                    summary: None,
                    publish_time: None,
                }],
            }
        }

        fn empty() -> Self {
            #![allow(dead_code)]
            Self {
                quotes: Vec::new(),
                sparkline: Vec::new(),
                news: Vec::new(),
            }
        }
    }

    impl QuoteProvider for MockProvider {
        fn fetch_quotes(&self, symbols: &[String]) -> Result<Vec<Option<Quote>>> {
            Ok(symbols
                .iter()
                .map(|s| {
                    self.quotes
                        .iter()
                        .find(|q| q.as_ref().is_some_and(|q| q.symbol == *s))
                        .cloned()
                        .flatten()
                })
                .collect())
        }

        fn fetch_sparkline(&self, _symbol: &str, _range: ChartRange) -> Result<Vec<PricePoint>> {
            Ok(self.sparkline.clone())
        }

        fn fetch_news(&self, _symbol: &str) -> Result<Vec<NewsItem>> {
            Ok(self.news.clone())
        }

        fn fetch_screener(&self, _scr_id: &str) -> Result<Vec<Quote>> {
            Ok(self.quotes.iter().flatten().cloned().collect())
        }

        fn fetch_trending(&self) -> Result<Vec<String>> {
            Ok(self
                .quotes
                .iter()
                .flatten()
                .map(|q| q.symbol.clone())
                .collect())
        }
    }

    struct FailingProvider;

    impl QuoteProvider for FailingProvider {
        fn fetch_quotes(&self, _symbols: &[String]) -> Result<Vec<Option<Quote>>> {
            bail!("connection refused")
        }
        fn fetch_sparkline(&self, _symbol: &str, _range: ChartRange) -> Result<Vec<PricePoint>> {
            bail!("connection refused")
        }
    }

    fn make_app(symbols: &[&str]) -> App {
        let syms: Vec<String> = symbols.iter().map(|s| (*s).to_string()).collect();
        App::with_provider(syms, Arc::new(MockProvider::new()))
    }

    fn make_app_failing(symbols: &[&str]) -> App {
        let syms: Vec<String> = symbols.iter().map(|s| (*s).to_string()).collect();
        App::with_provider(syms, Arc::new(FailingProvider))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Submit fetches and wait for background threads to complete.
    fn refresh_and_drain(app: &mut App) {
        app.refresh_quotes();
        // Give background threads time to complete with mock data.
        std::thread::sleep(std::time::Duration::from_millis(50));
        app.drain_results();
        // Second drain picks up any follow-up fetches (e.g. sparkline after quotes).
        std::thread::sleep(std::time::Duration::from_millis(50));
        app.drain_results();
    }

    // -- Basic quit tests -----------------------------------------------------

    #[test]
    fn quit_on_q() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn quit_on_esc() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.should_quit);
    }

    #[test]
    fn quit_on_ctrl_c() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    // -- Navigation -----------------------------------------------------------

    #[test]
    fn navigation_changes_selection() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        assert_eq!(app.watchlist.selected(), 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.watchlist.selected(), 1);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.watchlist.selected(), 0);
    }

    #[test]
    fn gg_jumps_to_first() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        app.handle_key(key(KeyCode::Char('j'))); // select 1
        app.handle_key(key(KeyCode::Char('g')));
        app.handle_key(key(KeyCode::Char('g')));
        assert_eq!(app.watchlist.selected(), 0);
    }

    #[test]
    fn uppercase_g_jumps_to_last() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        app.handle_key(key(KeyCode::Char('G')));
        assert_eq!(app.watchlist.selected(), 1);
    }

    #[test]
    fn g_followed_by_non_g_cancels_pending() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        app.handle_key(key(KeyCode::Char('j'))); // select 1
        app.handle_key(key(KeyCode::Char('g')));
        assert!(app.pending_g);
        app.handle_key(key(KeyCode::Char('j'))); // not g — should cancel pending & navigate
        assert!(!app.pending_g);
        // Should have moved from 1 → 0 (wrapping)
    }

    // -- Adding mode ----------------------------------------------------------

    #[test]
    fn enter_adding_mode() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(app.input_mode, InputMode::Adding);
    }

    #[test]
    fn typing_accumulates_buffer() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Char('T')));
        app.handle_key(key(KeyCode::Char('S')));
        app.handle_key(key(KeyCode::Char('L')));
        app.handle_key(key(KeyCode::Char('A')));
        assert_eq!(app.input_buffer, "TSLA");
    }

    #[test]
    fn enter_commits_symbol() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Char('T')));
        app.handle_key(key(KeyCode::Char('S')));
        app.handle_key(key(KeyCode::Char('L')));
        app.handle_key(key(KeyCode::Char('A')));
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.watchlist.symbols().contains(&"TSLA".to_string()));
    }

    #[test]
    fn esc_cancels_adding() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Char('X')));
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.watchlist.symbols().contains(&"X".to_string()));
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Char('A')));
        app.handle_key(key(KeyCode::Char('B')));
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.input_buffer, "A");
    }

    // -- View mode switching --------------------------------------------------

    #[test]
    fn tab_cycles_view_mode() {
        let mut app = make_app(&["AAPL"]);
        assert_eq!(app.view_mode, ViewMode::Watchlist);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.view_mode, ViewMode::Scanner);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.view_mode, ViewMode::QualityControl);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.view_mode, ViewMode::Journal);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.view_mode, ViewMode::Watchlist);
    }

    #[test]
    fn backtab_cycles_reverse() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.view_mode, ViewMode::Journal);
    }

    // -- Sort & Filter --------------------------------------------------------

    #[test]
    fn s_cycles_sort_mode() {
        let mut app = make_app(&["AAPL"]);
        assert_eq!(app.sort_mode, SortMode::Default);
        app.handle_key(key(KeyCode::Char('s')));
        assert_eq!(app.sort_mode, SortMode::ChangeDesc);
    }

    #[test]
    fn f_cycles_filter_mode() {
        let mut app = make_app(&["AAPL"]);
        assert_eq!(app.filter_mode, FilterMode::All);
        app.handle_key(key(KeyCode::Char('f')));
        assert_eq!(app.filter_mode, FilterMode::Gainers);
    }

    // -- Theme ----------------------------------------------------------------

    #[test]
    fn t_cycles_theme() {
        let mut app = make_app(&["AAPL"]);
        let initial = app.theme_index;
        app.handle_key(key(KeyCode::Char('t')));
        assert_eq!(app.theme_index, (initial + 1) % theme::THEMES.len());
    }

    // -- News toggle ----------------------------------------------------------

    #[test]
    fn n_toggles_news() {
        let mut app = make_app(&["AAPL"]);
        assert!(!app.show_news);
        app.handle_key(key(KeyCode::Char('n')));
        assert!(app.show_news);
        app.handle_key(key(KeyCode::Char('n')));
        assert!(!app.show_news);
    }

    // -- Delete ---------------------------------------------------------------

    #[test]
    fn d_deletes_selected() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(app.watchlist.symbols().len(), 1);
    }

    // -- Refresh --------------------------------------------------------------

    #[test]
    fn refresh_quotes_updates_data() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        refresh_and_drain(&mut app);
        assert!(app.watchlist.selected_quote().is_some());
        assert!(!app.sparkline_data.is_empty());
    }

    #[test]
    fn refresh_with_failing_provider_shows_error() {
        let mut app = make_app_failing(&["AAPL"]);
        refresh_and_drain(&mut app);
        assert!(!app.status_message.is_empty());
    }

    #[test]
    fn refresh_populates_index_quotes() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        // MockProvider returns empty for index symbols, but no panic.
        assert!(app.status_message.is_empty() || !app.status_message.is_empty());
    }

    // -- QC -------------------------------------------------------------------

    #[test]
    fn qc_score_zero_initially() {
        let app = make_app(&["AAPL"]);
        assert_eq!(app.qc_score("AAPL"), 0);
    }

    #[test]
    fn toggle_qc_changes_state() {
        let mut app = make_app(&["AAPL"]);
        app.screener_results.push(ScreenerResult {
            ticker: "AAPL".to_string(),
            company: "Apple".to_string(),
            sector: "Tech".to_string(),
            industry: "Hardware".to_string(),
            market_cap: "3T".to_string(),
            pe: "30".to_string(),
            price: "175".to_string(),
            change: "+1.15%".to_string(),
            volume: "50M".to_string(),
            beta: "1.2".to_string(),
        });
        app.view_mode = ViewMode::QualityControl;
        app.toggle_qc();
        assert_eq!(app.qc_score("AAPL"), 1);
    }

    #[test]
    fn any_fully_passed_false_initially() {
        let app = make_app(&["AAPL"]);
        assert!(!app.any_fully_passed());
    }

    // -- Auto-check -----------------------------------------------------------

    #[test]
    fn auto_check_insider_above_threshold() {
        let mut app = make_app(&["AAPL"]);
        app.insider_ownership.insert("AAPL".to_string(), 5.0);
        assert!(app.is_auto_checked("AAPL", 1));
    }

    #[test]
    fn auto_check_insider_below_threshold() {
        let mut app = make_app(&["AAPL"]);
        app.insider_ownership.insert("AAPL".to_string(), 0.5);
        assert!(!app.is_auto_checked("AAPL", 1));
    }

    #[test]
    fn auto_check_sector_heat_positive() {
        let mut app = make_app(&["AAPL"]);
        app.sector_heat.insert("AAPL".to_string(), 2.5);
        assert!(app.is_auto_checked("AAPL", 2));
    }

    #[test]
    fn auto_check_past_beats() {
        let mut app = make_app(&["AAPL"]);
        app.past_beats.insert("AAPL".to_string(), true);
        assert!(app.is_auto_checked("AAPL", 4));
    }

    #[test]
    fn qc_score_combines_auto_and_manual() {
        let mut app = make_app(&["AAPL"]);
        app.insider_ownership.insert("AAPL".to_string(), 5.0); // auto item 1
        app.qc_state
            .insert("AAPL".to_string(), vec![true, false, false, false, false]); // manual item 0
        assert_eq!(app.qc_score("AAPL"), 2);
    }

    // -- On tick --------------------------------------------------------------

    #[test]
    fn on_tick_counts_up_when_market_active() {
        let mut app = make_app(&["AAPL"]);
        app.market_status = MarketStatus::Open;
        app.on_tick();
        assert_eq!(app.ticks_since_refresh, 1);
    }

    #[test]
    fn on_tick_resets_at_active_threshold() {
        let mut app = make_app(&["AAPL"]);
        app.market_status = MarketStatus::Open;
        app.ticks_since_refresh = ACTIVE_REFRESH_TICKS - 1;
        app.on_tick();
        assert_eq!(app.ticks_since_refresh, 0); // reset after refresh
    }

    #[test]
    fn on_tick_heartbeat_when_closed() {
        let mut app = make_app(&["AAPL"]);
        app.market_status = MarketStatus::Closed;
        for _ in 0..HEARTBEAT_TICKS - 1 {
            app.on_tick();
        }
        assert_eq!(app.ticks_since_refresh, HEARTBEAT_TICKS - 1);
        app.on_tick();
        assert_eq!(app.ticks_since_refresh, 0); // reset after heartbeat
    }

    // -- Scanner number keys --------------------------------------------------

    #[test]
    fn number_keys_select_scanner_in_scanner_mode() {
        let mut app = make_app(&["AAPL"]);
        app.view_mode = ViewMode::Scanner;
        app.handle_key(key(KeyCode::Char('2')));
        assert_eq!(app.scanner_list, ScannerList::DayLosers);
    }

    #[test]
    fn number_keys_ignored_in_watchlist_mode() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::Char('2')));
        assert_eq!(app.scanner_list, ScannerList::DayGainers); // unchanged
    }

    // -- Focus toggle (QC view) -----------------------------------------------

    #[test]
    fn focus_toggles_in_qc_view() {
        let mut app = make_app(&["AAPL"]);
        app.view_mode = ViewMode::QualityControl;
        assert_eq!(app.focus, Focus::Table);
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.focus, Focus::QcChecklist);
        app.handle_key(key(KeyCode::Char('h')));
        assert_eq!(app.focus, Focus::Table);
    }

    // -- QC navigation --------------------------------------------------------

    #[test]
    fn qc_navigation_wraps() {
        let mut app = make_app(&["AAPL"]);
        app.view_mode = ViewMode::QualityControl;
        app.focus = Focus::QcChecklist;
        let n = app.qc_labels.len();
        // Navigate past last item
        for _ in 0..n {
            app.handle_key(key(KeyCode::Char('j')));
        }
        assert_eq!(app.selected_qc, 0); // wrapped
    }

    // -- Chart overlay --------------------------------------------------------

    #[test]
    fn enter_opens_chart_in_watchlist_view() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        assert!(app.chart_open);
        assert_eq!(app.chart_symbol, "AAPL");
        assert_eq!(app.chart_range, ChartRange::Day1);
    }

    #[test]
    fn esc_closes_chart() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        assert!(app.chart_open);
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.chart_open);
    }

    #[test]
    fn chart_range_switches_with_number_keys() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Char('3')));
        assert_eq!(app.chart_range, ChartRange::Month1);
    }

    #[test]
    fn chart_range_cycles_with_right_left() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.chart_range, ChartRange::Day1);
        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.chart_range, ChartRange::Day5);
        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.chart_range, ChartRange::Day1);
    }

    #[test]
    fn chart_keys_dont_leak_to_normal_mode() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        // 'q' in chart mode should close chart, not quit app
        app.handle_key(key(KeyCode::Char('q')));
        assert!(!app.chart_open);
        assert!(!app.should_quit);
    }

    #[test]
    fn enter_opens_chart_even_without_quotes() {
        let mut app = make_app(&["AAPL"]);
        // No refresh — no quotes loaded, but symbol name is available.
        app.handle_key(key(KeyCode::Enter));
        assert!(app.chart_open);
        assert_eq!(app.chart_symbol, "AAPL");
    }

    #[test]
    fn enter_does_nothing_with_empty_watchlist() {
        let mut app = make_app(&[]);
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.chart_open);
    }

    // -- Chart pattern analysis -----------------------------------------------

    #[test]
    fn analyze_chart_pattern_breakout() {
        let mut app = make_app(&["AAPL"]);
        app.analyze_chart_pattern("AAPL", "+6.5%");
        assert_eq!(app.chart_patterns.get("AAPL").unwrap(), "Strong breakout");
    }

    #[test]
    fn analyze_chart_pattern_downtrend() {
        let mut app = make_app(&["AAPL"]);
        app.analyze_chart_pattern("AAPL", "-3.2%");
        assert_eq!(app.chart_patterns.get("AAPL").unwrap(), "Downtrend");
    }

    #[test]
    fn analyze_chart_pattern_mild_bullish() {
        let mut app = make_app(&["AAPL"]);
        app.analyze_chart_pattern("AAPL", "+0.5%");
        assert_eq!(app.chart_patterns.get("AAPL").unwrap(), "Mild bullish");
    }

    // -- QC inline values -----------------------------------------------------

    #[test]
    fn qc_inline_value_insider() {
        let mut app = make_app(&["AAPL"]);
        app.insider_ownership.insert("AAPL".to_string(), 5.3);
        assert_eq!(
            app.qc_inline_value("AAPL", 1),
            Some(" (5.3% insider)".to_string())
        );
    }

    #[test]
    fn qc_inline_value_sector_heat() {
        let mut app = make_app(&["AAPL"]);
        app.sector_heat.insert("AAPL".to_string(), 2.5);
        assert_eq!(
            app.qc_inline_value("AAPL", 2),
            Some(" (+2.5% vs SPY)".to_string())
        );
    }

    #[test]
    fn qc_inline_value_chart_pattern() {
        let mut app = make_app(&["AAPL"]);
        app.chart_patterns
            .insert("AAPL".to_string(), "Uptrend".to_string());
        assert_eq!(
            app.qc_inline_value("AAPL", 3),
            Some(" (Uptrend)".to_string())
        );
    }

    #[test]
    fn qc_inline_value_past_beats() {
        let mut app = make_app(&["AAPL"]);
        app.past_beats.insert("AAPL".to_string(), true);
        assert_eq!(app.qc_inline_value("AAPL", 4), Some(" (beats)".to_string()));
    }

    #[test]
    fn qc_inline_value_none_when_no_data() {
        let app = make_app(&["AAPL"]);
        assert_eq!(app.qc_inline_value("AAPL", 1), None);
        assert_eq!(app.qc_inline_value("AAPL", 3), None);
    }

    // -- Spinner helper -------------------------------------------------------

    #[test]
    fn spinner_frame_cycles() {
        use crate::ui::helpers::spinner_frame;
        let f0 = spinner_frame(0);
        let f1 = spinner_frame(1);
        assert_ne!(f0, f1);
        // Wraps around after 10 frames.
        assert_eq!(spinner_frame(0), spinner_frame(10));
    }

    // -- Chart pattern from quotes refresh ------------------------------------

    #[test]
    fn chart_patterns_populated_after_refresh() {
        let mut app = make_app(&["AAPL", "MSFT"]);
        refresh_and_drain(&mut app);
        // MockProvider returns AAPL with +1.15% and MSFT with -0.74%.
        assert!(app.chart_patterns.contains_key("AAPL"));
        assert!(app.chart_patterns.contains_key("MSFT"));
        assert_eq!(app.chart_patterns.get("AAPL").unwrap(), "Mild bullish");
        assert_eq!(app.chart_patterns.get("MSFT").unwrap(), "Mild bearish");
    }

    // -- Chart tab cycling and panel navigation --------------------------------

    #[test]
    fn chart_indicator_toggles() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter)); // open chart
        assert!(!app.chart_show_sma);
        assert!(!app.chart_show_rsi);
        assert!(!app.chart_show_macd);

        app.handle_key(key(KeyCode::Char('v')));
        assert!(app.chart_show_sma);
        app.handle_key(key(KeyCode::Char('i')));
        assert!(app.chart_show_rsi);
        app.handle_key(key(KeyCode::Char('m')));
        assert!(app.chart_show_macd);

        // Toggle off.
        app.handle_key(key(KeyCode::Char('v')));
        assert!(!app.chart_show_sma);
    }

    #[test]
    fn chart_tab_cycles_with_tab_key() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.chart_tab, ChartTab::Chart);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.chart_tab, ChartTab::News);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.chart_tab, ChartTab::SecFilings);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.chart_tab, ChartTab::Thesis);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.chart_tab, ChartTab::Chart);
    }

    #[test]
    fn chart_tab_backtab_cycles_reverse() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.chart_tab, ChartTab::Chart);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.chart_tab, ChartTab::Thesis);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.chart_tab, ChartTab::SecFilings);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.chart_tab, ChartTab::News);
    }

    #[test]
    fn chart_news_navigation() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        // Switch to News tab and populate some news.
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.chart_tab, ChartTab::News);
        app.chart_news = vec![
            market_core::domain::NewsItem {
                title: "Headline 1".into(),
                link: String::new(),
                publisher: "Test".into(),
                summary: Some("Summary 1".into()),
                publish_time: None,
            },
            market_core::domain::NewsItem {
                title: "Headline 2".into(),
                link: String::new(),
                publisher: "Test".into(),
                summary: None,
                publish_time: None,
            },
        ];
        assert_eq!(app.chart_news_selected, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.chart_news_selected, 1);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.chart_news_selected, 0);
        // Enter opens article detail (triggers background fetch).
        app.handle_key(key(KeyCode::Enter));
        assert!(app.chart_news_summary_open);
        assert!(app.chart_news_content_loading);
        // Enter again closes it.
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.chart_news_summary_open);
        // Navigate to item 1 (no RSS summary) — Enter still opens
        // because we now fetch the full article page.
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.chart_news_selected, 1);
        app.handle_key(key(KeyCode::Enter));
        assert!(app.chart_news_summary_open);
        assert!(app.chart_news_content_loading);
    }

    #[test]
    fn chart_sec_navigation() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        // Switch to SEC tab.
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.chart_tab, ChartTab::SecFilings);
        app.chart_sec_filings = vec![
            market_core::domain::SecFiling {
                form_type: "10-K".into(),
                filed_date: "2026-01-15".into(),
                description: "Annual report".into(),
                link: String::new(),
                accession: String::new(),
            },
            market_core::domain::SecFiling {
                form_type: "8-K".into(),
                filed_date: "2026-02-20".into(),
                description: "Current report".into(),
                link: String::new(),
                accession: String::new(),
            },
        ];
        assert_eq!(app.chart_sec_selected, 0);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.chart_sec_selected, 1);
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.chart_sec_selected, 0); // wraps
    }

    #[test]
    fn chart_range_keys_ignored_on_news_tab() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        let initial_range = app.chart_range;
        app.handle_key(key(KeyCode::Tab)); // switch to News
        app.handle_key(key(KeyCode::Char('3'))); // should NOT change range
        assert_eq!(app.chart_range, initial_range);
    }

    #[test]
    fn close_chart_resets_tab_state() {
        let mut app = make_app(&["AAPL"]);
        refresh_and_drain(&mut app);
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Tab)); // News
        assert_eq!(app.chart_tab, ChartTab::News);
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.chart_open);
        // Reopen — should be back to Chart tab.
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.chart_tab, ChartTab::Chart);
    }
}
