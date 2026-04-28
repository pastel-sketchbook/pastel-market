//! Portfolio-level risk metrics.

#![allow(clippy::cast_precision_loss, clippy::cast_lossless)]

use std::collections::HashMap;

use crate::domain::{Quote, ScreenerResult};

/// Aggregated risk metrics for a portfolio (watchlist).
#[derive(Debug, Default, Clone)]
pub struct PortfolioRisk {
    /// Average beta across all holdings.
    pub beta_exposure: f64,
    /// Breakdown of sector concentration (sector -> percentage 0.0-100.0).
    pub sector_concentration: HashMap<String, f64>,
    /// Volatility score combining beta and IV.
    pub volatility_score: f64,
    /// Estimate of correlation risk (based on highest sector overlap).
    pub correlation_risk: f64,
}

impl PortfolioRisk {
    /// Calculate risk metrics for the given quotes, using screener and implied volatility data for context.
    #[must_use]
    pub fn compute(
        quotes: &[Quote],
        screener_data: &[ScreenerResult],
        iv_data: &HashMap<String, f64>,
    ) -> Self {
        if quotes.is_empty() {
            return Self::default();
        }

        let mut total_beta = 0.0;
        let mut beta_count = 0;
        let mut sector_counts: HashMap<String, usize> = HashMap::new();
        let mut total_iv = 0.0;
        let mut iv_count = 0;

        for quote in quotes {
            // Find corresponding screener data
            let sr = screener_data.iter().find(|s| s.ticker == quote.symbol);
            
            if let Some(s) = sr {
                if let Ok(beta) = s.beta.parse::<f64>() {
                    total_beta += beta;
                    beta_count += 1;
                }

                *sector_counts.entry(s.sector.clone()).or_insert(0) += 1;
            }

            // Find IV from passed data
            if let Some(&iv) = iv_data.get(&quote.symbol) {
                total_iv += iv;
                iv_count += 1;
            }
        }

        let avg_beta = if beta_count > 0 {
            total_beta / beta_count as f64
        } else {
            1.0
        };

        let total_stocks = quotes.len() as f64;
        let mut sector_concentration = HashMap::new();
        let mut max_sector_pct = 0.0;

        for (sector, count) in &sector_counts {
            let pct = (*count as f64 / total_stocks) * 100.0;
            sector_concentration.insert(sector.clone(), pct);
            if pct > max_sector_pct {
                max_sector_pct = pct;
            }
        }

        let avg_iv = if iv_count > 0 {
            total_iv / iv_count as f64
        } else {
            20.0 // Baseline assumption
        };

        // Volatility score heuristic (0-100)
        // Normal beta = 1.0, high beta = 2.0+
        // Normal IV = 20%, high IV = 50%+
        let beta_factor = (avg_beta * 20.0).clamp(0.0, 50.0);
        let iv_factor = avg_iv.clamp(0.0, 50.0);
        let volatility_score = (beta_factor + iv_factor).clamp(0.0, 100.0);

        // Correlation risk is essentially max sector concentration
        let correlation_risk = max_sector_pct;

        Self {
            beta_exposure: avg_beta,
            sector_concentration,
            volatility_score,
            correlation_risk,
        }
    }
}
