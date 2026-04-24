use std::fmt;

use serde::Deserialize;

/// Represents a real-time quote for a single stock symbol.
#[derive(Debug, Clone, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub short_name: Option<String>,
    pub sector: Option<String>,
    pub market_state: Option<String>,
    pub regular_market_price: f64,
    pub regular_market_change: f64,
    pub regular_market_change_percent: f64,
    pub regular_market_volume: u64,
    pub regular_market_previous_close: f64,
    pub regular_market_open: f64,
    pub regular_market_day_high: f64,
    pub regular_market_day_low: f64,
    pub fifty_two_week_high: f64,
    pub fifty_two_week_low: f64,
    /// Pre-market price (available when `market_state` is `PRE` or `REGULAR`).
    pub pre_market_price: Option<f64>,
    /// Pre-market change from previous close.
    pub pre_market_change: Option<f64>,
    /// Pre-market change percent.
    pub pre_market_change_percent: Option<f64>,
    /// Post-market (after-hours) price.
    pub post_market_price: Option<f64>,
    /// Post-market change from regular close.
    pub post_market_change: Option<f64>,
    /// Post-market change percent.
    pub post_market_change_percent: Option<f64>,
}

impl Quote {
    /// Returns `true` when the price change is non-negative.
    #[must_use]
    pub fn is_gain(&self) -> bool {
        self.regular_market_change >= 0.0
    }

    /// Display name: short name if available, otherwise the symbol.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.short_name.as_deref().unwrap_or(&self.symbol)
    }
}

impl fmt::Display for Quote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ${:.2} ({:+.2} / {:+.2}%)",
            self.symbol,
            self.regular_market_price,
            self.regular_market_change,
            self.regular_market_change_percent,
        )
    }
}

/// A price point for sparkline / chart history.
#[derive(Debug, Clone)]
pub struct PricePoint {
    /// Unix timestamp (seconds since epoch). `None` for legacy sparkline data.
    pub timestamp: Option<i64>,
    pub close: f64,
}

// ---------------------------------------------------------------------------
// Chart range
// ---------------------------------------------------------------------------

/// Selectable time range for the performance chart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartRange {
    #[default]
    Day1,
    Day5,
    Month1,
    Month3,
    Month6,
    Ytd,
    Year1,
    Year5,
}

impl ChartRange {
    /// Yahoo Finance `range` query parameter value.
    #[must_use]
    pub fn yahoo_range(self) -> &'static str {
        match self {
            Self::Day1 => "1d",
            Self::Day5 => "5d",
            Self::Month1 => "1mo",
            Self::Month3 => "3mo",
            Self::Month6 => "6mo",
            Self::Ytd => "ytd",
            Self::Year1 => "1y",
            Self::Year5 => "5y",
        }
    }

    /// Yahoo Finance `interval` appropriate for this range.
    #[must_use]
    pub fn yahoo_interval(self) -> &'static str {
        match self {
            Self::Day1 => "5m",
            Self::Day5 => "15m",
            Self::Month1 | Self::Month3 | Self::Month6 | Self::Ytd => "1d",
            Self::Year1 => "1wk",
            Self::Year5 => "1mo",
        }
    }

    /// Short display label for the UI tab bar.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Day1 => "1D",
            Self::Day5 => "5D",
            Self::Month1 => "1M",
            Self::Month3 => "3M",
            Self::Month6 => "6M",
            Self::Ytd => "YTD",
            Self::Year1 => "1Y",
            Self::Year5 => "5Y",
        }
    }

    /// All variants in display order.
    pub const ALL: [Self; 8] = [
        Self::Day1,
        Self::Day5,
        Self::Month1,
        Self::Month3,
        Self::Month6,
        Self::Ytd,
        Self::Year1,
        Self::Year5,
    ];

    /// Move to the next range.
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&r| r == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Move to the previous range.
    #[must_use]
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&r| r == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// A single news headline for a stock symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItem {
    /// Headline text.
    pub title: String,
    /// Publisher name (e.g. "Reuters", "Bloomberg").
    pub publisher: String,
    /// Link to the full article.
    pub link: String,
    /// Brief summary (populated on demand when user selects).
    pub summary: Option<String>,
    /// Publication timestamp (Unix epoch seconds).
    pub publish_time: Option<i64>,
}

/// An SEC EDGAR filing for a company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecFiling {
    /// Filing type (e.g. "10-K", "10-Q", "8-K", "4").
    pub form_type: String,
    /// Filing date (YYYY-MM-DD).
    pub filed_date: String,
    /// Primary document description.
    pub description: String,
    /// Link to the filing on SEC.gov.
    pub link: String,
    /// Accession number (unique identifier).
    pub accession: String,
}

/// Holds the user's watchlist and the fetched quote data.
#[derive(Debug)]
pub struct Watchlist {
    symbols: Vec<String>,
    quotes: Vec<Option<Quote>>,
    selected: usize,
}

impl Watchlist {
    /// Create a new watchlist from the given symbols.
    #[must_use]
    pub fn new(symbols: Vec<String>) -> Self {
        let len = symbols.len();
        Self {
            symbols,
            quotes: vec![None; len],
            selected: 0,
        }
    }

    /// The list of symbols being watched.
    #[must_use]
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// The currently selected index.
    #[must_use]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// All quotes (in same order as symbols). `None` means not yet fetched.
    #[must_use]
    pub fn quotes(&self) -> &[Option<Quote>] {
        &self.quotes
    }

    /// Returns the quote for the currently selected symbol, if available.
    #[must_use]
    pub fn selected_quote(&self) -> Option<&Quote> {
        self.quotes.get(self.selected).and_then(Option::as_ref)
    }

    /// Move selection down by one.
    pub fn select_next(&mut self) {
        if !self.symbols.is_empty() {
            self.selected = (self.selected + 1) % self.symbols.len();
        }
    }

    /// Move selection up by one.
    pub fn select_previous(&mut self) {
        if !self.symbols.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.symbols.len().saturating_sub(1));
        }
    }

    /// Jump to the first item in the list.
    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    /// Jump to the last item in the list.
    pub fn select_last(&mut self) {
        if !self.symbols.is_empty() {
            self.selected = self.symbols.len().saturating_sub(1);
        }
    }

    /// Add a new symbol to the watchlist.
    pub fn add_symbol(&mut self, symbol: &str) {
        let upper = symbol.to_uppercase();
        if !self.symbols.contains(&upper) {
            self.symbols.push(upper);
            self.quotes.push(None);
        }
    }

    /// Remove the currently selected symbol.
    pub fn remove_selected(&mut self) {
        if !self.symbols.is_empty() {
            self.symbols.remove(self.selected);
            self.quotes.remove(self.selected);
            if self.selected >= self.symbols.len() && !self.symbols.is_empty() {
                self.selected = self.symbols.len().saturating_sub(1);
            }
        }
    }

    /// Replace all quotes with freshly fetched data.
    pub fn update_quotes(&mut self, quotes: Vec<Option<Quote>>) {
        self.quotes = quotes;
    }

    /// Set the quote for a single symbol by index.
    pub fn set_quote(&mut self, index: usize, quote: Option<Quote>) {
        if index < self.quotes.len() {
            self.quotes[index] = quote;
        }
    }

    /// Returns `true` when the watchlist has no symbols.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Returns indices into `symbols`/`quotes` in the display order
    /// determined by `mode`.
    ///
    /// The underlying vectors are never reordered — the UI uses these
    /// indices to render rows in sorted order while keeping add/remove
    /// operations index-stable.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // volume u64→f64 is fine for sort comparison
    pub fn sorted_indices(&self, mode: SortMode) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.symbols.len()).collect();

        match mode {
            SortMode::Default => {} // insertion order
            SortMode::ChangeDesc => {
                indices.sort_by(|&a, &b| {
                    let ca = self.quotes[a]
                        .as_ref()
                        .map_or(f64::NEG_INFINITY, |q| q.regular_market_change_percent);
                    let cb = self.quotes[b]
                        .as_ref()
                        .map_or(f64::NEG_INFINITY, |q| q.regular_market_change_percent);
                    cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortMode::ChangeAsc => {
                indices.sort_by(|&a, &b| {
                    let ca = self.quotes[a]
                        .as_ref()
                        .map_or(f64::INFINITY, |q| q.regular_market_change_percent);
                    let cb = self.quotes[b]
                        .as_ref()
                        .map_or(f64::INFINITY, |q| q.regular_market_change_percent);
                    ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortMode::PriceDesc => {
                indices.sort_by(|&a, &b| {
                    let pa = self.quotes[a]
                        .as_ref()
                        .map_or(f64::NEG_INFINITY, |q| q.regular_market_price);
                    let pb = self.quotes[b]
                        .as_ref()
                        .map_or(f64::NEG_INFINITY, |q| q.regular_market_price);
                    pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortMode::VolumeDesc => {
                indices.sort_by(|&a, &b| {
                    let va = self.quotes[a]
                        .as_ref()
                        .map_or(0_u64, |q| q.regular_market_volume);
                    let vb = self.quotes[b]
                        .as_ref()
                        .map_or(0_u64, |q| q.regular_market_volume);
                    vb.cmp(&va)
                });
            }
            SortMode::Symbol => {
                indices.sort_by(|&a, &b| self.symbols[a].cmp(&self.symbols[b]));
            }
        }

        indices
    }
}

/// The current state of the US stock market as reported by Yahoo Finance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MarketStatus {
    /// Market is in its regular trading session (9:30 AM – 4:00 PM ET).
    Open,
    /// Pre-market trading session (before 9:30 AM ET).
    PreMarket,
    /// After-hours trading session (after 4:00 PM ET).
    AfterHours,
    /// Market is closed (weekends, holidays, outside all sessions).
    #[default]
    Closed,
}

impl MarketStatus {
    /// Parse Yahoo Finance's `marketState` string into a `MarketStatus`.
    #[must_use]
    pub fn from_yahoo(state: &str) -> Self {
        match state {
            "REGULAR" => Self::Open,
            "PRE" | "PREPRE" => Self::PreMarket,
            "POST" | "POSTPOST" => Self::AfterHours,
            _ => Self::Closed,
        }
    }

    /// Returns `true` when the market is in any trading session.
    #[must_use]
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Closed)
    }
}

impl fmt::Display for MarketStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "OPEN"),
            Self::PreMarket => write!(f, "PRE-MARKET"),
            Self::AfterHours => write!(f, "AFTER-HOURS"),
            Self::Closed => write!(f, "CLOSED"),
        }
    }
}

/// Sort mode for the watchlist table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortMode {
    #[default]
    Default,
    ChangeDesc,
    ChangeAsc,
    PriceDesc,
    VolumeDesc,
    Symbol,
}

impl SortMode {
    /// Advance to the next sort mode in the cycle.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Default => Self::ChangeDesc,
            Self::ChangeDesc => Self::ChangeAsc,
            Self::ChangeAsc => Self::PriceDesc,
            Self::PriceDesc => Self::VolumeDesc,
            Self::VolumeDesc => Self::Symbol,
            Self::Symbol => Self::Default,
        }
    }
}

impl fmt::Display for SortMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "Default"),
            Self::ChangeDesc => write!(f, "Change% \u{2193}"),
            Self::ChangeAsc => write!(f, "Change% \u{2191}"),
            Self::PriceDesc => write!(f, "Price \u{2193}"),
            Self::VolumeDesc => write!(f, "Volume \u{2193}"),
            Self::Symbol => write!(f, "Symbol"),
        }
    }
}

/// Sort and filter a flat list of quotes, returning indices into the original slice.
#[must_use]
pub fn sorted_filtered_indices(quotes: &[Quote], sort: SortMode, filter: FilterMode) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..quotes.len())
        .filter(|&i| filter.matches(&quotes[i]))
        .collect();

    match sort {
        SortMode::Default => {}
        SortMode::ChangeDesc => indices.sort_by(|&a, &b| {
            quotes[b]
                .regular_market_change_percent
                .partial_cmp(&quotes[a].regular_market_change_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortMode::ChangeAsc => indices.sort_by(|&a, &b| {
            quotes[a]
                .regular_market_change_percent
                .partial_cmp(&quotes[b].regular_market_change_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortMode::PriceDesc => indices.sort_by(|&a, &b| {
            quotes[b]
                .regular_market_price
                .partial_cmp(&quotes[a].regular_market_price)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortMode::VolumeDesc => {
            indices.sort_by(|&a, &b| {
                quotes[b]
                    .regular_market_volume
                    .cmp(&quotes[a].regular_market_volume)
            });
        }
        SortMode::Symbol => {
            indices.sort_by(|&a, &b| quotes[a].symbol.cmp(&quotes[b].symbol));
        }
    }

    indices
}

/// A single stock in the top-movers list.
#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub symbol: String,
    pub price: f64,
    pub change_percent: f64,
}

/// Top N gainers and top N losers from a set of quotes.
#[derive(Debug, Clone, Default)]
pub struct TopMovers {
    pub gainers: Vec<Mover>,
    pub losers: Vec<Mover>,
}

impl TopMovers {
    /// Extract the top-`n` gainers and top-`n` losers from a quote slice.
    #[must_use]
    pub fn from_quotes(quotes: &[Option<Quote>], n: usize) -> Self {
        let mut available: Vec<Mover> = quotes
            .iter()
            .flatten()
            .map(|q| Mover {
                symbol: q.symbol.clone(),
                price: q.regular_market_price,
                change_percent: q.regular_market_change_percent,
            })
            .collect();

        available.sort_by(|a, b| {
            b.change_percent
                .partial_cmp(&a.change_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let gainers: Vec<Mover> = available
            .iter()
            .filter(|m| m.change_percent >= 0.0)
            .take(n)
            .cloned()
            .collect();

        let losers: Vec<Mover> = available
            .iter()
            .rev()
            .filter(|m| m.change_percent < 0.0)
            .take(n)
            .cloned()
            .collect();

        Self { gainers, losers }
    }
}

/// Ranking badge for a quote in the watchlist heatmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankBadge {
    TopGainer,
    TopLoser,
    None,
}

/// Ranking metadata for a single quote used by the heatmap renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuoteRank {
    pub intensity: f64,
    pub is_gain: bool,
    pub badge: RankBadge,
}

/// Compute heatmap rankings for a slice of quotes.
#[must_use]
pub fn rank_by_change(quotes: &[Option<Quote>], badge_count: usize) -> Vec<Option<QuoteRank>> {
    let mut entries: Vec<(usize, f64)> = quotes
        .iter()
        .enumerate()
        .filter_map(|(i, q)| q.as_ref().map(|q| (i, q.regular_market_change_percent)))
        .collect();

    if entries.is_empty() {
        return vec![None; quotes.len()];
    }

    let max_abs = entries
        .iter()
        .map(|(_, pct)| pct.abs())
        .fold(0.0_f64, f64::max);

    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let top_gainer_indices: Vec<usize> = entries
        .iter()
        .filter(|(_, pct)| *pct >= 0.0)
        .take(badge_count)
        .map(|(i, _)| *i)
        .collect();

    let top_loser_indices: Vec<usize> = entries
        .iter()
        .rev()
        .filter(|(_, pct)| *pct < 0.0)
        .take(badge_count)
        .map(|(i, _)| *i)
        .collect();

    quotes
        .iter()
        .enumerate()
        .map(|(i, q)| {
            q.as_ref().map(|q| {
                let pct = q.regular_market_change_percent;
                let intensity = if max_abs > 0.0 {
                    pct.abs() / max_abs
                } else {
                    0.0
                };
                let badge = if top_gainer_indices.contains(&i) {
                    RankBadge::TopGainer
                } else if top_loser_indices.contains(&i) {
                    RankBadge::TopLoser
                } else {
                    RankBadge::None
                };
                QuoteRank {
                    intensity,
                    is_gain: pct >= 0.0,
                    badge,
                }
            })
        })
        .collect()
}

/// Filter mode for narrowing the watchlist display.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FilterMode {
    #[default]
    All,
    Gainers,
    Losers,
    BigMovers,
    HighVolume,
    Near52WkHigh,
}

impl FilterMode {
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Gainers,
            Self::Gainers => Self::Losers,
            Self::Losers => Self::BigMovers,
            Self::BigMovers => Self::HighVolume,
            Self::HighVolume => Self::Near52WkHigh,
            Self::Near52WkHigh => Self::All,
        }
    }

    #[must_use]
    pub fn matches(&self, quote: &Quote) -> bool {
        match self {
            Self::All => true,
            Self::Gainers => quote.regular_market_change_percent >= 0.0,
            Self::Losers => quote.regular_market_change_percent < 0.0,
            Self::BigMovers => quote.regular_market_change_percent.abs() > 2.0,
            Self::HighVolume => quote.regular_market_volume > 1_000_000,
            Self::Near52WkHigh => {
                quote.fifty_two_week_high > 0.0
                    && quote.regular_market_price >= quote.fifty_two_week_high * 0.95
            }
        }
    }
}

impl fmt::Display for FilterMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::Gainers => write!(f, "Gainers"),
            Self::Losers => write!(f, "Losers"),
            Self::BigMovers => write!(f, "Big Movers"),
            Self::HighVolume => write!(f, "High Vol"),
            Self::Near52WkHigh => write!(f, "Near 52W High"),
        }
    }
}

/// View mode for switching between main panels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// User's personal watchlist.
    #[default]
    Watchlist,
    /// Yahoo pre-built scanner list.
    Scanner,
    /// Per-stock quality control checklist.
    QualityControl,
}

impl ViewMode {
    /// Cycle to the next view mode.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Watchlist => Self::Scanner,
            Self::Scanner => Self::QualityControl,
            Self::QualityControl => Self::Watchlist,
        }
    }

    /// Cycle to the previous view mode.
    #[must_use]
    pub fn prev(self) -> Self {
        match self {
            Self::Watchlist => Self::QualityControl,
            Self::Scanner => Self::Watchlist,
            Self::QualityControl => Self::Scanner,
        }
    }
}

impl fmt::Display for ViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Watchlist => write!(f, "Watchlist"),
            Self::Scanner => write!(f, "Scanner"),
            Self::QualityControl => write!(f, "Quality Control"),
        }
    }
}

/// Yahoo pre-built screener list identifiers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScannerList {
    #[default]
    DayGainers,
    DayLosers,
    MostActive,
    Trending,
    Fundamentals,
}

impl ScannerList {
    /// The Yahoo screener ID string for this list.
    ///
    /// `Fundamentals` returns a sentinel — the app dispatches to Finviz.
    #[must_use]
    pub fn screener_id(&self) -> &'static str {
        match self {
            Self::DayGainers => "day_gainers",
            Self::DayLosers => "day_losers",
            Self::MostActive => "most_actives",
            Self::Trending => "trending",
            Self::Fundamentals => "finviz_fundamentals",
        }
    }

    /// Select a scanner list by number (1-5).
    #[must_use]
    pub fn from_number(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::DayGainers),
            2 => Some(Self::DayLosers),
            3 => Some(Self::MostActive),
            4 => Some(Self::Trending),
            5 => Some(Self::Fundamentals),
            _ => None,
        }
    }
}

impl fmt::Display for ScannerList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayGainers => write!(f, "Day Gainers"),
            Self::DayLosers => write!(f, "Day Losers"),
            Self::MostActive => write!(f, "Most Active"),
            Self::Trending => write!(f, "Trending"),
            Self::Fundamentals => write!(f, "Fundamentals"),
        }
    }
}

/// Finviz screener result for a single stock.
#[derive(Debug, Clone)]
pub struct ScreenerResult {
    pub ticker: String,
    pub company: String,
    pub sector: String,
    pub industry: String,
    pub market_cap: String,
    pub pe: String,
    pub price: String,
    pub change: String,
    pub volume: String,
}

/// Mock data types for offline/fallback mode.
pub mod mock {
    use anyhow::{Context, Result};
    use serde::Deserialize;

    const MOCK_JSON: &str = include_str!("../../../data/mock.json");

    #[derive(Debug, Deserialize)]
    pub struct MockData {
        pub finviz_filters: FinvizFilters,
        pub whisper_data: WhisperData,
        pub qc_checklist: QcChecklist,
        pub maintenance_schedule: MaintenanceSchedule,
    }

    #[derive(Debug, Deserialize)]
    pub struct FinvizFilters {
        pub title: String,
        pub display_items: Vec<FilterItem>,
    }

    #[derive(Debug, Deserialize)]
    pub struct FilterItem {
        pub label: String,
        pub highlight: bool,
    }

    #[derive(Debug, Deserialize)]
    pub struct WhisperData {
        pub title: String,
        pub display_items: Vec<WhisperItem>,
    }

    #[derive(Debug, Deserialize)]
    pub struct WhisperItem {
        pub label: String,
        pub color: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct QcChecklist {
        pub title: String,
        pub items: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    pub struct MaintenanceSchedule {
        pub title: String,
        pub columns: Vec<String>,
        pub targets: Vec<Target>,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct Target {
        pub ticker: String,
        pub filters_passed: bool,
        pub whisper_edge: String,
        pub target_price: String,
        pub status: String,
        pub actual_move: Option<String>,
    }

    /// Load mock data from the embedded JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded JSON is malformed.
    pub fn load_mock_data() -> Result<MockData> {
        serde_json::from_str(MOCK_JSON).context("failed to parse embedded mock data JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_quote(symbol: &str, price: f64, change: f64) -> Quote {
        Quote {
            symbol: symbol.to_string(),
            short_name: Some(format!("{symbol} Inc.")),
            sector: Some("Technology".to_string()),
            market_state: Some("REGULAR".to_string()),
            regular_market_price: price,
            regular_market_change: change,
            regular_market_change_percent: change / (price - change) * 100.0,
            regular_market_volume: 1_000_000,
            regular_market_previous_close: price - change,
            regular_market_open: price - change,
            regular_market_day_high: price + 1.0,
            regular_market_day_low: price - 1.0,
            fifty_two_week_high: price + 50.0,
            fifty_two_week_low: price - 50.0,
            pre_market_price: None,
            pre_market_change: None,
            pre_market_change_percent: None,
            post_market_price: None,
            post_market_change: None,
            post_market_change_percent: None,
        }
    }

    #[test]
    fn quote_is_gain_positive_change() {
        let q = sample_quote("AAPL", 150.0, 2.5);
        assert!(q.is_gain());
    }

    #[test]
    fn quote_is_gain_negative_change() {
        let q = sample_quote("AAPL", 150.0, -3.0);
        assert!(!q.is_gain());
    }

    #[test]
    fn quote_display_name_uses_short_name() {
        let q = sample_quote("AAPL", 150.0, 1.0);
        assert_eq!(q.display_name(), "AAPL Inc.");
    }

    #[test]
    fn quote_display_name_falls_back_to_symbol() {
        let mut q = sample_quote("AAPL", 150.0, 1.0);
        q.short_name = None;
        assert_eq!(q.display_name(), "AAPL");
    }

    #[test]
    fn watchlist_navigation_wraps_around() {
        let mut wl = Watchlist::new(vec!["A".into(), "B".into(), "C".into()]);
        assert_eq!(wl.selected(), 0);
        wl.select_next();
        assert_eq!(wl.selected(), 1);
        wl.select_next();
        assert_eq!(wl.selected(), 2);
        wl.select_next();
        assert_eq!(wl.selected(), 0);
        wl.select_previous();
        assert_eq!(wl.selected(), 2);
    }

    #[test]
    fn watchlist_add_symbol_deduplicates() {
        let mut wl = Watchlist::new(vec!["AAPL".into()]);
        wl.add_symbol("aapl");
        assert_eq!(wl.symbols().len(), 1);
        wl.add_symbol("MSFT");
        assert_eq!(wl.symbols().len(), 2);
    }

    #[test]
    fn watchlist_remove_selected_adjusts_index() {
        let mut wl = Watchlist::new(vec!["A".into(), "B".into(), "C".into()]);
        wl.select_next();
        wl.select_next();
        wl.remove_selected();
        assert_eq!(wl.symbols(), &["A", "B"]);
        assert_eq!(wl.selected(), 1);
    }

    #[test]
    fn watchlist_remove_from_empty_is_noop() {
        let mut wl = Watchlist::new(vec![]);
        wl.remove_selected();
        assert!(wl.is_empty());
    }

    #[test]
    fn empty_watchlist_navigation_is_noop() {
        let mut wl = Watchlist::new(vec![]);
        wl.select_next();
        wl.select_previous();
        wl.select_first();
        wl.select_last();
        assert_eq!(wl.selected(), 0);
    }

    #[test]
    fn market_status_from_yahoo_variants() {
        assert_eq!(MarketStatus::from_yahoo("REGULAR"), MarketStatus::Open);
        assert_eq!(MarketStatus::from_yahoo("PRE"), MarketStatus::PreMarket);
        assert_eq!(MarketStatus::from_yahoo("POST"), MarketStatus::AfterHours);
        assert_eq!(MarketStatus::from_yahoo("CLOSED"), MarketStatus::Closed);
        assert_eq!(MarketStatus::from_yahoo("UNKNOWN"), MarketStatus::Closed);
    }

    #[test]
    fn market_status_is_active() {
        assert!(MarketStatus::Open.is_active());
        assert!(MarketStatus::PreMarket.is_active());
        assert!(MarketStatus::AfterHours.is_active());
        assert!(!MarketStatus::Closed.is_active());
    }

    #[test]
    fn sort_mode_cycles_through_all_variants() {
        let mut mode = SortMode::Default;
        mode = mode.next();
        assert_eq!(mode, SortMode::ChangeDesc);
        mode = mode.next();
        assert_eq!(mode, SortMode::ChangeAsc);
        mode = mode.next();
        assert_eq!(mode, SortMode::PriceDesc);
        mode = mode.next();
        assert_eq!(mode, SortMode::VolumeDesc);
        mode = mode.next();
        assert_eq!(mode, SortMode::Symbol);
        mode = mode.next();
        assert_eq!(mode, SortMode::Default);
    }

    #[test]
    fn filter_mode_cycles() {
        let mut mode = FilterMode::All;
        mode = mode.next();
        assert_eq!(mode, FilterMode::Gainers);
        mode = mode.next();
        assert_eq!(mode, FilterMode::Losers);
        mode = mode.next();
        assert_eq!(mode, FilterMode::BigMovers);
        mode = mode.next();
        assert_eq!(mode, FilterMode::HighVolume);
        mode = mode.next();
        assert_eq!(mode, FilterMode::Near52WkHigh);
        mode = mode.next();
        assert_eq!(mode, FilterMode::All);
    }

    #[test]
    fn view_mode_cycles() {
        let mut mode = ViewMode::Watchlist;
        mode = mode.next();
        assert_eq!(mode, ViewMode::Scanner);
        mode = mode.next();
        assert_eq!(mode, ViewMode::QualityControl);
        mode = mode.next();
        assert_eq!(mode, ViewMode::Watchlist);
    }

    #[test]
    fn view_mode_prev_cycles() {
        let mut mode = ViewMode::Watchlist;
        mode = mode.prev();
        assert_eq!(mode, ViewMode::QualityControl);
        mode = mode.prev();
        assert_eq!(mode, ViewMode::Scanner);
        mode = mode.prev();
        assert_eq!(mode, ViewMode::Watchlist);
    }

    #[test]
    fn scanner_list_from_number_valid() {
        assert_eq!(ScannerList::from_number(1), Some(ScannerList::DayGainers));
        assert_eq!(ScannerList::from_number(5), Some(ScannerList::Fundamentals));
        assert_eq!(ScannerList::from_number(0), None);
        assert_eq!(ScannerList::from_number(6), None);
    }

    #[test]
    fn top_movers_from_quotes() {
        let quotes = vec![
            Some(sample_quote("AAPL", 150.0, 3.0)),
            Some(sample_quote("MSFT", 400.0, -5.0)),
            Some(sample_quote("TSLA", 250.0, 10.0)),
        ];
        let movers = TopMovers::from_quotes(&quotes, 2);
        assert_eq!(movers.gainers.len(), 2);
        assert_eq!(movers.gainers[0].symbol, "TSLA");
        assert_eq!(movers.losers.len(), 1);
        assert_eq!(movers.losers[0].symbol, "MSFT");
    }

    #[test]
    fn rank_by_change_assigns_badges() {
        let quotes = vec![
            Some(sample_quote("AAPL", 150.0, 3.0)),
            Some(sample_quote("MSFT", 400.0, -5.0)),
            Some(sample_quote("TSLA", 250.0, 10.0)),
        ];
        let ranks = rank_by_change(&quotes, 1);
        assert_eq!(ranks[2].as_ref().unwrap().badge, RankBadge::TopGainer);
        assert_eq!(ranks[1].as_ref().unwrap().badge, RankBadge::TopLoser);
        assert_eq!(ranks[0].as_ref().unwrap().badge, RankBadge::None);
    }

    #[test]
    fn mock_data_loads() {
        let data = mock::load_mock_data().unwrap();
        assert!(!data.finviz_filters.display_items.is_empty());
        assert!(!data.qc_checklist.items.is_empty());
    }

    // -- ChartRange -----------------------------------------------------------

    #[test]
    fn chart_range_next_cycles() {
        let r = ChartRange::Day1;
        assert_eq!(r.next(), ChartRange::Day5);
        assert_eq!(ChartRange::Year5.next(), ChartRange::Day1);
    }

    #[test]
    fn chart_range_prev_cycles() {
        assert_eq!(ChartRange::Day1.prev(), ChartRange::Year5);
        assert_eq!(ChartRange::Day5.prev(), ChartRange::Day1);
    }

    #[test]
    fn chart_range_yahoo_params() {
        assert_eq!(ChartRange::Day1.yahoo_range(), "1d");
        assert_eq!(ChartRange::Day1.yahoo_interval(), "5m");
        assert_eq!(ChartRange::Year1.yahoo_range(), "1y");
        assert_eq!(ChartRange::Year1.yahoo_interval(), "1wk");
    }

    #[test]
    fn chart_range_labels() {
        assert_eq!(ChartRange::Day1.label(), "1D");
        assert_eq!(ChartRange::Ytd.label(), "YTD");
        assert_eq!(ChartRange::ALL.len(), 8);
    }
}
