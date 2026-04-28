//! Persistent decision log for tracking and analyzing past trades.
//!
//! Stores `DecisionEntry` records locally, resolving their outcome
//! dynamically based on current market data.

#![allow(clippy::missing_errors_doc)]

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::Rating;

/// A logged trade decision, waiting for or having a resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEntry {
    /// Unique identifier for the decision.
    pub id: String,
    /// The ticker symbol.
    pub ticker: String,
    /// Timestamp of when the decision was made.
    pub date: DateTime<Utc>,
    /// The action taken (Buy, Sell, Hold).
    pub action: Action,
    /// The rating at the time of the decision.
    pub rating: Rating,
    /// The composite analyst score at the time.
    pub composite_score: u8,
    /// Quality Control score (e.g., 5 for 5/5) at the time.
    pub qc_score: usize,
    /// Stock price at the time of decision.
    pub price_at_decision: f64,
    /// SPY price at the time of decision (for calculating alpha).
    pub spy_at_decision: Option<f64>,
    /// P/E ratio at the time of decision (fundamentals snapshot).
    pub pe_at_decision: Option<f64>,
    /// Current resolution status.
    pub resolution: Option<Resolution>,
}

/// The action taken in a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Buy,
    Sell,
    Hold,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buy => write!(f, "BUY"),
            Self::Sell => write!(f, "SELL"),
            Self::Hold => write!(f, "HOLD"),
        }
    }
}

/// The current outcome of a past decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// The stock price at the time of resolution.
    pub price_at_check: f64,
    /// Return percentage since the decision.
    pub return_pct: f64,
    /// Alpha against the S&P 500 benchmark.
    pub alpha_vs_spy: Option<f64>,
}

impl DecisionEntry {
    /// Resolve the entry with current market data.
    pub fn resolve(&mut self, current_price: f64, current_spy: Option<f64>) {
        if self.price_at_decision <= 0.0 {
            return; // Prevent division by zero
        }

        let mut return_pct =
            ((current_price - self.price_at_decision) / self.price_at_decision) * 100.0;

        // Reverse return if it was a Sell decision (short proxy)
        if self.action == Action::Sell {
            return_pct = -return_pct;
        }

        let alpha_vs_spy =
            if let (Some(spy_entry), Some(spy_current)) = (self.spy_at_decision, current_spy) {
                if spy_entry > 0.0 {
                    let spy_return = ((spy_current - spy_entry) / spy_entry) * 100.0;
                    Some(return_pct - spy_return)
                } else {
                    None
                }
            } else {
                None
            };

        self.resolution = Some(Resolution {
            price_at_check: current_price,
            return_pct,
            alpha_vs_spy,
        });
    }
}

/// The local store of decisions.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DecisionLog {
    pub entries: Vec<DecisionEntry>,
}

impl DecisionLog {
    /// Load the decision log from disk.
    #[must_use]
    pub fn load() -> Self {
        let path = Self::file_path();
        if !path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(e) => {
                tracing::warn!(error = %e, "failed to read decision log");
                Self::default()
            }
        }
    }

    /// Save the decision log to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize decision log")?;
        fs::write(&path, json).context("Failed to write decision log to disk")?;
        Ok(())
    }

    /// Append a new decision entry.
    pub fn append(&mut self, entry: DecisionEntry) {
        self.entries.push(entry);
    }

    fn file_path() -> PathBuf {
        let mut path = crate::config::app_dir();
        path.push("decisions.json");
        path
    }
}
