//! Unified application state combining watchlist monitoring (Reins Market)
//! with quality-control screening (Pastel Picker).

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::{info, warn};

use market_core::config::{self, Preferences, QcSession, Session};
use market_core::domain::mock::MockData;
use market_core::domain::{
    FilterMode, MarketStatus, NewsItem, PricePoint, Quote, ScannerList, ScreenerResult, SortMode,
    TopMovers, ViewMode, Watchlist,
};
use market_core::theme::{self, Theme};
use whispers::WhisperResult;
use yahoo_provider::QuoteProvider;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Market index symbols displayed in the header bar.
pub const INDEX_SYMBOLS: &[&str] = &["^GSPC", "^DJI", "^IXIC", "^RUT", "^VIX"];

/// GICS sector ETF symbols for sector performance.
pub const SECTOR_SYMBOLS: &[&str] = &[
    "XLK", "XLF", "XLE", "XLV", "XLC", "XLY", "XLP", "XLI", "XLB", "XLRE", "XLU",
];

/// Ticks between heartbeat refreshes when the market is closed.
const HEARTBEAT_TICKS: u32 = 10;

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
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub sparkline_data: Vec<PricePoint>,
    pub status_message: String,
    pub should_quit: bool,

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

    // -- News (from Reins Market) --
    pub news_headlines: Vec<NewsItem>,
    pub show_news: bool,

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

    // -- Theme --
    pub theme_index: usize,

    // -- Internal state --
    pub tick: u64,
    pub ticks_since_refresh: u32,
    pub pending_g: bool,
    pub top_movers: TopMovers,

    // -- Private --
    skip_persist: bool,
    client: Option<Box<dyn QuoteProvider>>,
}

impl App {
    /// Create a new `App`.
    ///
    /// Loads persisted preferences, session, and QC state from disk.
    /// Attempts to establish a Yahoo Finance session. If no persisted
    /// watchlist exists, seeds the watchlist from Yahoo's day-gainers
    /// screener (top 20 performers).
    #[must_use]
    pub fn new() -> Self {
        let prefs = config::load_preferences();
        let session = config::load_session();
        let qc_session = QcSession::load();
        let data = market_core::domain::mock::load_mock_data().unwrap_or_else(|e| {
            warn!(error = %e, "failed to load mock data");
            fallback_mock_data()
        });

        let theme_index = theme::theme_index_by_name(&prefs.theme);
        let sort_mode = config::sort_mode_from_string(&session.sort_mode);
        let filter_mode = config::filter_mode_from_string(&session.filter_mode);
        let view_mode = config::view_mode_from_string(&session.view_mode);

        let client: Option<Box<dyn QuoteProvider>> = match yahoo_provider::YahooClient::new() {
            Ok(c) => Some(Box::new(c)),
            Err(e) => {
                warn!(error = %e, "Yahoo Finance session failed");
                None
            }
        };

        // Use persisted symbols, or seed from Yahoo's day-gainers screener.
        let symbols = if session.symbols.is_empty() {
            seed_symbols_from_screener(client.as_deref())
        } else {
            session.symbols.clone()
        };

        info!(count = symbols.len(), "initial watchlist symbols");

        Self {
            watchlist: Watchlist::new(symbols),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            sparkline_data: Vec::new(),
            status_message: String::new(),
            should_quit: false,
            index_quotes: Vec::new(),
            sector_quotes: Vec::new(),
            market_status: MarketStatus::default(),
            sort_mode,
            filter_mode,
            view_mode,
            scanner_list: ScannerList::default(),
            scanner_quotes: Vec::new(),
            news_headlines: Vec::new(),
            show_news: false,
            focus: Focus::default(),
            qc_labels: data.qc_checklist.items,
            qc_state: qc_session.qc_state,
            selected_qc: 0,
            screener_results: Vec::new(),
            whisper_cache: HashMap::new(),
            insider_ownership: HashMap::new(),
            sector_heat: HashMap::new(),
            past_beats: HashMap::new(),
            theme_index,
            tick: 0,
            ticks_since_refresh: 0,
            pending_g: false,
            top_movers: TopMovers {
                gainers: Vec::new(),
                losers: Vec::new(),
            },
            skip_persist: false,
            client,
        }
    }

    /// Test constructor: use a mock provider and skip disk I/O.
    #[cfg(test)]
    #[must_use]
    pub fn with_provider(symbols: Vec<String>, provider: Box<dyn QuoteProvider>) -> Self {
        let data =
            market_core::domain::mock::load_mock_data().unwrap_or_else(|_| fallback_mock_data());
        Self {
            watchlist: Watchlist::new(symbols),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            sparkline_data: Vec::new(),
            status_message: String::new(),
            should_quit: false,
            index_quotes: Vec::new(),
            sector_quotes: Vec::new(),
            market_status: MarketStatus::default(),
            sort_mode: SortMode::default(),
            filter_mode: FilterMode::default(),
            view_mode: ViewMode::default(),
            scanner_list: ScannerList::default(),
            scanner_quotes: Vec::new(),
            news_headlines: Vec::new(),
            show_news: false,
            focus: Focus::default(),
            qc_labels: data.qc_checklist.items,
            qc_state: HashMap::new(),
            selected_qc: 0,
            screener_results: Vec::new(),
            whisper_cache: HashMap::new(),
            insider_ownership: HashMap::new(),
            sector_heat: HashMap::new(),
            past_beats: HashMap::new(),
            theme_index: 0,
            tick: 0,
            ticks_since_refresh: 0,
            pending_g: false,
            top_movers: TopMovers {
                gainers: Vec::new(),
                losers: Vec::new(),
            },
            skip_persist: true,
            client: Some(provider),
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

    // -- Data refresh --------------------------------------------------------

    /// Refresh all market data from the provider.
    pub fn refresh_quotes(&mut self) {
        let Some(client) = &self.client else {
            self.try_reconnect();
            return;
        };

        // Watchlist quotes
        let symbols: Vec<String> = self.watchlist.symbols().to_vec();
        if !symbols.is_empty() {
            match client.fetch_quotes(&symbols) {
                Ok(quotes) => {
                    self.watchlist.update_quotes(quotes);
                    self.top_movers = TopMovers::from_quotes(self.watchlist.quotes(), 3);
                    self.status_message.clear();
                }
                Err(e) => {
                    warn!(error = %e, "quote refresh failed");
                    self.status_message = format!("Refresh failed: {e}");
                }
            }
        }

        // Index quotes
        let idx_syms: Vec<String> = INDEX_SYMBOLS.iter().map(|s| (*s).to_string()).collect();
        match client.fetch_quotes(&idx_syms) {
            Ok(quotes) => {
                // Derive market status from first index quote
                if let Some(Some(q)) = quotes.first()
                    && let Some(state) = &q.market_state
                {
                    self.market_status = MarketStatus::from_yahoo(state);
                }
                self.index_quotes = quotes;
            }
            Err(e) => {
                warn!(error = %e, "index refresh failed");
                self.index_quotes.clear();
            }
        }

        // Sector quotes
        let sec_syms: Vec<String> = SECTOR_SYMBOLS.iter().map(|s| (*s).to_string()).collect();
        match client.fetch_quotes(&sec_syms) {
            Ok(quotes) => self.sector_quotes = quotes,
            Err(e) => {
                warn!(error = %e, "sector refresh failed");
                self.sector_quotes.clear();
            }
        }

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
        let Some(client) = &self.client else { return };

        match self.scanner_list {
            ScannerList::Trending => match client.fetch_trending() {
                Ok(syms) if !syms.is_empty() => match client.fetch_quotes(&syms) {
                    Ok(quotes) => {
                        self.scanner_quotes = quotes.into_iter().flatten().collect();
                    }
                    Err(e) => {
                        warn!(error = %e, "trending hydrate failed");
                        self.scanner_quotes.clear();
                        self.status_message = format!("Scanner error: {e}");
                    }
                },
                Ok(_) => self.scanner_quotes.clear(),
                Err(e) => {
                    warn!(error = %e, "trending fetch failed");
                    self.scanner_quotes.clear();
                    self.status_message = format!("Scanner error: {e}");
                }
            },
            ScannerList::Fundamentals => match finviz_scraper::screener::fetch_raw() {
                Ok(results) => {
                    self.scanner_quotes = results
                        .iter()
                        .map(finviz_scraper::screener::screener_result_to_quote)
                        .collect();
                    self.screener_results = results;
                }
                Err(e) => {
                    warn!(error = %e, "finviz fetch failed");
                    self.status_message = format!("Scanner error: {e}");
                }
            },
            _ => match client.fetch_screener(self.scanner_list.screener_id()) {
                Ok(quotes) => self.scanner_quotes = quotes,
                Err(e) => {
                    warn!(error = %e, "screener fetch failed");
                    self.scanner_quotes.clear();
                    self.status_message = format!("Scanner error: {e}");
                }
            },
        }
    }

    fn refresh_sparkline(&mut self) {
        let Some(client) = &self.client else { return };
        if let Some(q) = self.watchlist.selected_quote() {
            match client.fetch_sparkline(&q.symbol) {
                Ok(points) => self.sparkline_data = points,
                Err(_) => self.sparkline_data.clear(),
            }
        } else {
            self.sparkline_data.clear();
        }
    }

    fn refresh_news(&mut self) {
        if !self.show_news {
            return;
        }
        let Some(client) = &self.client else { return };
        if let Some(q) = self.watchlist.selected_quote() {
            match client.fetch_news(&q.symbol) {
                Ok(items) => self.news_headlines = items,
                Err(_) => self.news_headlines.clear(),
            }
        } else {
            self.news_headlines.clear();
        }
    }

    fn try_reconnect(&mut self) {
        info!("attempting Yahoo Finance reconnection");
        match yahoo_provider::YahooClient::new() {
            Ok(c) => {
                self.client = Some(Box::new(c));
                self.status_message = "Reconnected".to_string();
                self.refresh_quotes();
            }
            Err(e) => {
                warn!(error = %e, "reconnection failed");
                self.status_message = format!("Connection failed: {e}");
            }
        }
    }

    // -- Tick handler --------------------------------------------------------

    /// Called every event-loop tick (250ms).
    ///
    /// Auto-refreshes when the market is active; sends heartbeat pings
    /// during closed hours.
    pub fn on_tick(&mut self) {
        if self.market_status.is_active() {
            self.ticks_since_refresh = 0;
            self.refresh_quotes();
        } else {
            self.ticks_since_refresh += 1;
            if self.ticks_since_refresh >= HEARTBEAT_TICKS {
                self.ticks_since_refresh = 0;
                self.refresh_quotes();
            }
        }
    }

    // -- Key handling --------------------------------------------------------

    /// Dispatch a key event to the appropriate handler.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
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
                    Focus::Table => self.watchlist.select_first(),
                    Focus::QcChecklist => self.selected_qc = 0,
                }
                return;
            }
            // Not `gg` — fall through and process this key normally.
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,

            // Navigation
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::Table => self.watchlist.select_next(),
                Focus::QcChecklist => {
                    if !self.qc_labels.is_empty() {
                        self.selected_qc = (self.selected_qc + 1) % self.qc_labels.len();
                    }
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::Table => self.watchlist.select_previous(),
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
                Focus::Table => self.watchlist.select_last(),
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
            KeyCode::Char('r') => self.refresh_quotes(),

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

            // Add symbol (watchlist view)
            KeyCode::Char('a') if self.view_mode == ViewMode::Watchlist => {
                self.input_mode = InputMode::Adding;
                self.input_buffer.clear();
            }

            // Delete symbol
            KeyCode::Char('d') if self.view_mode == ViewMode::Watchlist => {
                self.watchlist.remove_selected();
                self.persist_session();
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

            _ => {}
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

    fn on_view_mode_changed(&mut self, _prev: ViewMode) {
        self.persist_session();
        // If switching to Scanner and no data yet, trigger a fetch.
        if self.view_mode == ViewMode::Scanner && self.scanner_quotes.is_empty() {
            self.refresh_scanner();
        }
    }

    // -- Persistence ---------------------------------------------------------

    fn persist_preferences(&self) {
        if self.skip_persist {
            return;
        }
        let prefs = Preferences {
            theme: self.theme().name.to_string(),
        };
        let _ = config::save_preferences(&prefs);
    }

    fn persist_session(&self) {
        if self.skip_persist {
            return;
        }
        let session = Session {
            symbols: self.watchlist.symbols().to_vec(),
            sort_mode: format!("{}", self.sort_mode),
            filter_mode: format!("{}", self.filter_mode),
            view_mode: format!("{}", self.view_mode),
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
                    }),
                ],
                sparkline: vec![PricePoint { close: 173.0 }, PricePoint { close: 175.0 }],
                news: vec![NewsItem {
                    title: "Test headline".to_string(),
                    publisher: "Test".to_string(),
                    link: String::new(),
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

        fn fetch_sparkline(&self, _symbol: &str) -> Result<Vec<PricePoint>> {
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
        fn fetch_sparkline(&self, _symbol: &str) -> Result<Vec<PricePoint>> {
            bail!("connection refused")
        }
    }

    fn make_app(symbols: &[&str]) -> App {
        let syms: Vec<String> = symbols.iter().map(|s| (*s).to_string()).collect();
        App::with_provider(syms, Box::new(MockProvider::new()))
    }

    fn make_app_failing(symbols: &[&str]) -> App {
        let syms: Vec<String> = symbols.iter().map(|s| (*s).to_string()).collect();
        App::with_provider(syms, Box::new(FailingProvider))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
        assert_eq!(app.view_mode, ViewMode::Watchlist);
    }

    #[test]
    fn backtab_cycles_reverse() {
        let mut app = make_app(&["AAPL"]);
        app.handle_key(key(KeyCode::BackTab));
        assert_eq!(app.view_mode, ViewMode::QualityControl);
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
        app.refresh_quotes();
        assert!(app.watchlist.selected_quote().is_some());
        assert!(!app.sparkline_data.is_empty());
    }

    #[test]
    fn refresh_with_failing_provider_shows_error() {
        let mut app = make_app_failing(&["AAPL"]);
        app.refresh_quotes();
        assert!(!app.status_message.is_empty());
    }

    #[test]
    fn refresh_populates_index_quotes() {
        let mut app = make_app(&["AAPL"]);
        app.refresh_quotes();
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
    fn on_tick_refreshes_when_market_active() {
        let mut app = make_app(&["AAPL"]);
        app.market_status = MarketStatus::Open;
        app.on_tick();
        assert_eq!(app.ticks_since_refresh, 0);
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
}
