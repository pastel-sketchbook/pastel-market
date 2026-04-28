//! Multi-analyst pipeline and bull/bear signal computation.
//!
//! Provides four algorithmic scorers (fundamentals, technical, sentiment,
//! news catalyst) whose weighted average yields a composite score that
//! maps to a [`Rating`].  Also computes bull/bear signal lists from
//! available data.
//!
//! Inspired by the TradingAgents analyst-team → researcher-debate →
//! portfolio-manager pipeline, but implemented as pure functions with
//! no LLM dependency.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::similar_names,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::missing_panics_doc,
    clippy::doc_markdown,
    clippy::items_after_statements
)]

use crate::domain::{NewsItem, Quote, Rating, ScreenerResult};

// ---------------------------------------------------------------------------
// Per-Analyst Scores
// ---------------------------------------------------------------------------

/// Result of running all four analyst scorers on a single stock.
#[derive(Debug, Clone)]
pub struct AnalysisReport {
    pub fundamentals: u8,
    pub technical: u8,
    pub sentiment: u8,
    pub news_catalyst: u8,
    pub composite: u8,
    pub rating: Rating,
    pub bull_signals: Vec<Signal>,
    pub bear_signals: Vec<Signal>,
}

/// A single bull or bear signal with a short description.
#[derive(Debug, Clone)]
pub struct Signal {
    pub label: String,
}

impl Signal {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Input bundle
// ---------------------------------------------------------------------------

/// Data available for analysis on a single stock.
///
/// The caller assembles this from whatever data is currently cached.
/// All fields are optional — scorers degrade gracefully when data is missing.
#[derive(Debug, Default)]
pub struct AnalysisInput<'a> {
    pub quote: Option<&'a Quote>,
    pub screener: Option<&'a ScreenerResult>,
    pub news: &'a [NewsItem],
    pub insider_ownership_pct: Option<f64>,
    pub sector_heat: Option<f64>,
    pub past_beats: Option<bool>,
    pub qc_score: Option<(usize, usize)>, // (score, total)
    /// Historical close prices for computing RSI/MACD (newest last).
    pub prices: &'a [f64],
}

// ---------------------------------------------------------------------------
// Composite pipeline
// ---------------------------------------------------------------------------

/// Default weights for each analyst (must sum to 100).
const W_FUNDAMENTALS: u16 = 30;
const W_TECHNICAL: u16 = 30;
const W_SENTIMENT: u16 = 20;
const W_NEWS_CATALYST: u16 = 20;

/// Run the full analyst pipeline and produce an [`AnalysisReport`].
#[must_use]
pub fn analyze(input: &AnalysisInput<'_>) -> AnalysisReport {
    let fundamentals = score_fundamentals(input);
    let technical = score_technical(input);
    let sentiment = score_sentiment(input);
    let news_catalyst = score_news_catalyst(input);

    #[allow(clippy::cast_possible_truncation)]
    let composite = ((u16::from(fundamentals) * W_FUNDAMENTALS
        + u16::from(technical) * W_TECHNICAL
        + u16::from(sentiment) * W_SENTIMENT
        + u16::from(news_catalyst) * W_NEWS_CATALYST)
        / 100) as u8;

    let rating = Rating::from_score(composite);

    let bull_signals = derive_bull_signals(input);
    let bear_signals = derive_bear_signals(input);

    AnalysisReport {
        fundamentals,
        technical,
        sentiment,
        news_catalyst,
        composite,
        rating,
        bull_signals,
        bear_signals,
    }
}

// ---------------------------------------------------------------------------
// Individual scorers (each returns 0–100)
// ---------------------------------------------------------------------------

/// Fundamentals score based on Finviz screener data.
///
/// Factors: P/E ratio, market cap, change direction.
#[must_use]
fn score_fundamentals(input: &AnalysisInput<'_>) -> u8 {
    let Some(sr) = input.screener else {
        return 50; // neutral when no data
    };

    let mut score: i16 = 50;

    // P/E: lower is better (value), up to +20 / -20
    if let Ok(pe) = sr.pe.parse::<f64>() {
        if pe > 0.0 && pe < 15.0 {
            score += 20;
        } else if pe < 25.0 {
            score += 10;
        } else if pe > 60.0 {
            score -= 20;
        } else if pe > 40.0 {
            score -= 15;
        }
    }

    // Positive change is bullish
    if sr.change.starts_with('+') || (!sr.change.starts_with('-') && sr.change != "0.00%") {
        score += 10;
    } else if sr.change.starts_with('-') {
        score -= 10;
    }

    // Market cap: prefer large caps
    let cap = sr.market_cap.to_lowercase();
    if cap.contains('b') {
        score += 10;
    } else if cap.contains('m') {
        // Small cap gets a small penalty for higher risk.
        score -= 5;
    }

    score.clamp(0, 100) as u8
}

/// Technical score based on price action vs 52-week range.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn score_technical(input: &AnalysisInput<'_>) -> u8 {
    let Some(q) = input.quote else {
        return 50;
    };

    let mut score: f64 = 50.0;

    // Position within 52-week range (0.0 = at low, 1.0 = at high).
    let range = q.fifty_two_week_high - q.fifty_two_week_low;
    if range > 0.0 {
        let position = (q.regular_market_price - q.fifty_two_week_low) / range;
        // Mid-range is ideal; near-high is overbought; near-low is oversold.
        if (0.3..=0.7).contains(&position) {
            score += 15.0; // healthy mid-range
        } else if position > 0.9 {
            score -= 10.0; // near 52-week high, limited upside
        } else if position < 0.1 {
            score -= 10.0; // near 52-week low, risk of further decline
        }
    }

    // Intraday momentum: positive change is bullish.
    if q.regular_market_change_percent > 2.0 {
        score += 15.0;
    } else if q.regular_market_change_percent > 0.0 {
        score += 8.0;
    } else if q.regular_market_change_percent < -2.0 {
        score -= 15.0;
    } else if q.regular_market_change_percent < 0.0 {
        score -= 8.0;
    }

    // Volume as a confirmation signal (rough proxy: high volume = conviction).
    if q.regular_market_volume > 5_000_000 {
        score += 5.0;
    } else if q.regular_market_volume < 100_000 {
        score -= 5.0;
    }

    // RSI-based signal (if historical prices available).
    if input.prices.len() > 14 {
        let rsi = crate::indicators::compute_rsi(input.prices, 14);
        if let Some(&last_rsi) = rsi.last() {
            if !last_rsi.is_nan() {
                if last_rsi < 30.0 {
                    score += 10.0; // oversold — bullish signal
                } else if last_rsi > 70.0 {
                    score -= 10.0; // overbought — bearish signal
                }
            }
        }
    }

    score.clamp(0.0, 100.0) as u8
}

/// Sentiment score from headline analysis (simple heuristic, no NLP).
///
/// Looks at positive/negative keyword ratios in news titles.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn score_sentiment(input: &AnalysisInput<'_>) -> u8 {
    if input.news.is_empty() {
        return 50;
    }

    const POSITIVE: &[&str] = &[
        "surge", "soar", "beat", "record", "upgrade", "strong", "growth",
        "rally", "gain", "profit", "boost", "outperform", "bullish", "buy",
        "positive", "optimis", "upgrad",
    ];
    const NEGATIVE: &[&str] = &[
        "drop", "fall", "miss", "cut", "downgrade", "weak", "decline",
        "crash", "loss", "risk", "warning", "bearish", "sell", "concern",
        "negative", "pessimis", "downgr",
    ];

    let mut pos = 0_u32;
    let mut neg = 0_u32;

    for item in input.news {
        let lower = item.title.to_lowercase();
        for kw in POSITIVE {
            if lower.contains(kw) {
                pos += 1;
            }
        }
        for kw in NEGATIVE {
            if lower.contains(kw) {
                neg += 1;
            }
        }
    }

    let total = pos + neg;
    if total == 0 {
        return 50;
    }

    // Ratio of positive to total keywords → 0.0–1.0 → map to 20–80 range.
    let ratio = pos as f64 / total as f64;
    let score = 20.0 + ratio * 60.0;
    score.clamp(0.0, 100.0) as u8
}

/// News catalyst score from headline recency and volume.
///
/// More recent headlines and higher headline counts → higher catalyst score.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn score_news_catalyst(input: &AnalysisInput<'_>) -> u8 {
    if input.news.is_empty() {
        return 30; // no news = slightly negative (no catalyst)
    }

    let mut score: f64 = 40.0;

    // More headlines = more attention.
    let count = input.news.len();
    if count >= 10 {
        score += 20.0;
    } else if count >= 5 {
        score += 15.0;
    } else if count >= 2 {
        score += 5.0;
    }

    // Recency: check if any headline is within the last 24 hours.
    let now = chrono::Utc::now().timestamp();
    let has_recent = input.news.iter().any(|n| {
        n.publish_time
            .is_some_and(|ts| (now - ts) < 86_400)
    });
    if has_recent {
        score += 20.0;
    }

    // Very recent (last 2 hours) is even better.
    let has_very_recent = input.news.iter().any(|n| {
        n.publish_time
            .is_some_and(|ts| (now - ts) < 7_200)
    });
    if has_very_recent {
        score += 10.0;
    }

    score.clamp(0.0, 100.0) as u8
}

// ---------------------------------------------------------------------------
// Bull / Bear signals
// ---------------------------------------------------------------------------

/// Derive bullish signals from available data.
#[must_use]
fn derive_bull_signals(input: &AnalysisInput<'_>) -> Vec<Signal> {
    let mut signals = Vec::new();

    if let Some(q) = input.quote {
        if q.regular_market_change_percent > 0.0 {
            signals.push(Signal::new(format!(
                "Positive momentum (+{:.2}%)",
                q.regular_market_change_percent
            )));
        }
        let range = q.fifty_two_week_high - q.fifty_two_week_low;
        if range > 0.0 {
            let pos = (q.regular_market_price - q.fifty_two_week_low) / range;
            if pos > 0.5 {
                signals.push(Signal::new("Above 52-week midpoint"));
            }
        }
        if q.regular_market_volume > 2_000_000 {
            signals.push(Signal::new("High volume confirmation"));
        }
    }

    if let Some(&pct) = input.insider_ownership_pct.as_ref() {
        if pct > 5.0 {
            signals.push(Signal::new(format!("Strong insider ownership ({pct:.1}%)")));
        } else if pct > 1.0 {
            signals.push(Signal::new(format!("Insider ownership ({pct:.1}%)")));
        }
    }

    if let Some(&heat) = input.sector_heat.as_ref() {
        if heat > 0.0 {
            signals.push(Signal::new(format!("Positive sector heat (+{heat:.2}%)")));
        }
    }

    if input.past_beats == Some(true) {
        signals.push(Signal::new("Historical earnings beats"));
    }

    if let Some((score, total)) = input.qc_score {
        if total > 0 && score >= 4 {
            signals.push(Signal::new(format!("QC score {score}/{total}")));
        }
    }

    if let Some(sr) = input.screener {
        if let Ok(pe) = sr.pe.parse::<f64>() {
            if pe > 0.0 && pe < 20.0 {
                signals.push(Signal::new(format!("Attractive P/E ({pe:.1})")));
            }
        }
    }

    signals
}

/// Derive bearish signals from available data.
#[must_use]
fn derive_bear_signals(input: &AnalysisInput<'_>) -> Vec<Signal> {
    let mut signals = Vec::new();

    if let Some(q) = input.quote {
        if q.regular_market_change_percent < -2.0 {
            signals.push(Signal::new(format!(
                "Negative momentum ({:.2}%)",
                q.regular_market_change_percent
            )));
        }
        if q.fifty_two_week_high > 0.0
            && q.regular_market_price >= q.fifty_two_week_high * 0.95
        {
            signals.push(Signal::new("Near 52-week high (limited upside)"));
        }
        if q.regular_market_volume < 200_000 {
            signals.push(Signal::new("Low volume (thin liquidity)"));
        }
    }

    if let Some(&heat) = input.sector_heat.as_ref() {
        if heat < 0.0 {
            signals.push(Signal::new(format!("Negative sector heat ({heat:.2}%)")));
        }
    }

    if input.past_beats == Some(false) {
        signals.push(Signal::new("Historical earnings misses"));
    }

    if let Some(sr) = input.screener {
        if let Ok(pe) = sr.pe.parse::<f64>() {
            if pe > 40.0 {
                signals.push(Signal::new(format!("High P/E ({pe:.1}) — overvalued risk")));
            }
        }
    }

    if let Some((score, total)) = input.qc_score {
        if total > 0 && score <= 1 {
            signals.push(Signal::new(format!("Low QC score {score}/{total}")));
        }
    }

    signals
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Quote;

    fn sample_quote() -> Quote {
        Quote {
            symbol: "TEST".to_string(),
            short_name: Some("Test Inc.".to_string()),
            sector: Some("Technology".to_string()),
            market_state: Some("REGULAR".to_string()),
            regular_market_price: 150.0,
            regular_market_change: 3.0,
            regular_market_change_percent: 2.04,
            regular_market_volume: 5_000_000,
            regular_market_previous_close: 147.0,
            regular_market_open: 148.0,
            regular_market_day_high: 152.0,
            regular_market_day_low: 147.0,
            fifty_two_week_high: 200.0,
            fifty_two_week_low: 100.0,
            pre_market_price: None,
            pre_market_change: None,
            pre_market_change_percent: None,
            post_market_price: None,
            post_market_change: None,
            post_market_change_percent: None,
        }
    }

    fn sample_screener() -> ScreenerResult {
        ScreenerResult {
            ticker: "TEST".to_string(),
            company: "Test Inc.".to_string(),
            sector: "Technology".to_string(),
            industry: "Software".to_string(),
            market_cap: "10.5B".to_string(),
            pe: "18.5".to_string(),
            price: "150.00".to_string(),
            change: "+2.04%".to_string(),
            volume: "5.0M".to_string(),
            beta: "1.1".to_string(),
        }
    }

    #[test]
    fn rating_from_score_boundaries() {
        assert_eq!(Rating::from_score(0), Rating::Sell);
        assert_eq!(Rating::from_score(19), Rating::Sell);
        assert_eq!(Rating::from_score(20), Rating::Underweight);
        assert_eq!(Rating::from_score(39), Rating::Underweight);
        assert_eq!(Rating::from_score(40), Rating::Hold);
        assert_eq!(Rating::from_score(59), Rating::Hold);
        assert_eq!(Rating::from_score(60), Rating::Buy);
        assert_eq!(Rating::from_score(79), Rating::Buy);
        assert_eq!(Rating::from_score(80), Rating::StrongBuy);
        assert_eq!(Rating::from_score(100), Rating::StrongBuy);
    }

    #[test]
    fn rating_from_score_clamps_above_100() {
        assert_eq!(Rating::from_score(255), Rating::StrongBuy);
    }

    #[test]
    fn analyze_with_full_data() {
        let q = sample_quote();
        let sr = sample_screener();
        let input = AnalysisInput {
            quote: Some(&q),
            screener: Some(&sr),
            news: &[],
            insider_ownership_pct: Some(8.0),
            sector_heat: Some(1.5),
            past_beats: Some(true),
            qc_score: Some((4, 5)),
            prices: &[],
        };
        let report = analyze(&input);
        assert!(report.composite > 0);
        assert!(!report.bull_signals.is_empty());
    }

    #[test]
    fn analyze_empty_input_is_neutral() {
        let input = AnalysisInput::default();
        let report = analyze(&input);
        // All scorers return ~50 (neutral) with no data.
        assert!(report.composite >= 30 && report.composite <= 70);
    }

    #[test]
    fn sentiment_balanced_keywords() {
        let news = vec![
            NewsItem {
                title: "Stock surges on strong earnings beat".to_string(),
                publisher: "Test".to_string(),
                link: String::new(),
                summary: None,
                publish_time: None,
            },
            NewsItem {
                title: "Concerns about market decline risk".to_string(),
                publisher: "Test".to_string(),
                link: String::new(),
                summary: None,
                publish_time: None,
            },
        ];
        let input = AnalysisInput {
            news: &news,
            ..Default::default()
        };
        let score = score_sentiment(&input);
        // Should be near 50 (balanced).
        assert!(score >= 30 && score <= 70);
    }

    #[test]
    fn bull_bear_signals_populated() {
        let q = sample_quote();
        let input = AnalysisInput {
            quote: Some(&q),
            insider_ownership_pct: Some(10.0),
            sector_heat: Some(2.0),
            past_beats: Some(true),
            ..Default::default()
        };
        let bulls = derive_bull_signals(&input);
        assert!(bulls.len() >= 3, "Expected at least 3 bull signals");

        // Now with bearish data.
        let mut q2 = sample_quote();
        q2.regular_market_change_percent = -4.0;
        q2.regular_market_price = 198.0; // near 52-week high
        let bear_input = AnalysisInput {
            quote: Some(&q2),
            sector_heat: Some(-1.5),
            past_beats: Some(false),
            ..Default::default()
        };
        let bears = derive_bear_signals(&bear_input);
        assert!(bears.len() >= 2, "Expected at least 2 bear signals");
    }
}
