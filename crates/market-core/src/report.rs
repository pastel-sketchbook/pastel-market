//! Markdown report generation for stock analysis.

#![allow(
    clippy::format_push_string,
    clippy::cast_precision_loss,
    clippy::missing_errors_doc,
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::single_char_add_str,
    clippy::uninlined_format_args
)]

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::analysis::AnalysisReport;
use crate::domain::{NewsItem, Quote, ScreenerResult};

/// Data required to generate a comprehensive markdown report.
pub struct ReportData<'a> {
    pub ticker: String,
    pub quote: Option<&'a Quote>,
    pub screener: Option<&'a ScreenerResult>,
    pub analysis: AnalysisReport,
    pub qc_labels: &'a [String],
    pub qc_state: Option<&'a [bool]>,
    pub news: &'a [NewsItem],
}

/// Generate a markdown report string.
#[must_use]
pub fn generate_markdown(data: &ReportData<'_>) -> String {
    let mut md = String::new();
    let date = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    let company_name = data
        .quote
        .and_then(|q| q.short_name.as_deref())
        .or_else(|| data.screener.map(|s| s.company.as_str()))
        .unwrap_or(&data.ticker);

    let price = data
        .quote
        .map_or(0.0, |q| q.regular_market_price);

    md.push_str(&format!("# {} ({})\n\n", company_name, data.ticker));
    md.push_str(&format!("**Generated:** {}\n", date));
    md.push_str(&format!("**Current Price:** ${:.2}\n", price));
    md.push_str(&format!("**Overall Rating:** {} (Score: {})\n\n", data.analysis.rating, data.analysis.composite));
    
    md.push_str("---\n\n");

    // Analyst Scores
    md.push_str("## 📊 Analyst Pipeline Scores\n\n");
    md.push_str(&format!("- **Fundamentals:** {}/100\n", data.analysis.fundamentals));
    md.push_str(&format!("- **Technical:** {}/100\n", data.analysis.technical));
    md.push_str(&format!("- **Sentiment:** {}/100\n", data.analysis.sentiment));
    md.push_str(&format!("- **Catalyst:** {}/100\n\n", data.analysis.news_catalyst));

    // Thesis (Bull / Bear)
    md.push_str("## ⚖️ Investment Thesis\n\n");
    
    md.push_str("### 🟢 Bull Signals\n");
    if data.analysis.bull_signals.is_empty() {
        md.push_str("- *No strong bull signals detected.*\n");
    } else {
        for sig in &data.analysis.bull_signals {
            md.push_str(&format!("- {}\n", sig.label));
        }
    }
    md.push_str("\n");

    md.push_str("### 🔴 Bear Signals\n");
    if data.analysis.bear_signals.is_empty() {
        md.push_str("- *No strong bear signals detected.*\n");
    } else {
        for sig in &data.analysis.bear_signals {
            md.push_str(&format!("- {}\n", sig.label));
        }
    }
    md.push_str("\n");

    // Fundamentals Snapshot
    if let Some(sr) = data.screener {
        md.push_str("## 📈 Fundamentals Snapshot\n\n");
        md.push_str(&format!("- **Sector:** {} / {}\n", sr.sector, sr.industry));
        md.push_str(&format!("- **Market Cap:** {}\n", sr.market_cap));
        md.push_str(&format!("- **P/E Ratio:** {}\n", sr.pe));
        md.push_str(&format!("- **Change:** {}\n", sr.change));
        md.push_str(&format!("- **Volume:** {}\n\n", sr.volume));
    }

    // Quality Control Checklist
    if !data.qc_labels.is_empty() {
        md.push_str("## 📋 Quality Control Checklist\n\n");
        let state = data.qc_state;
        for (i, label) in data.qc_labels.iter().enumerate() {
            let passed = state.is_some_and(|s| s.get(i).copied().unwrap_or(false));
            let check = if passed { "x" } else { " " };
            md.push_str(&format!("- [{}] {}\n", check, label));
        }
        md.push_str("\n");
    }

    // Recent News
    if !data.news.is_empty() {
        md.push_str("## 📰 Recent Headlines\n\n");
        for (i, item) in data.news.iter().take(5).enumerate() {
            md.push_str(&format!("{}. **{}** — {}\n", i + 1, item.title, item.publisher));
        }
        md.push_str("\n");
    }

    md
}

/// Export a markdown report for a stock to disk.
pub fn export_report(data: &ReportData<'_>) -> Result<PathBuf> {
    let markdown = generate_markdown(data);
    
    let mut dir = crate::config::app_dir();
    dir.push("reports");

    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create reports directory")?;
    }

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let filename = format!("{}_{}.md", data.ticker, date);
    
    let mut path = dir;
    path.push(filename);

    fs::write(&path, markdown).context("Failed to write report to disk")?;

    Ok(path)
}
