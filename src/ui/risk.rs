//! Risk management dashboard panel.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use crate::app::App;
use market_core::risk::PortfolioRisk;
use market_core::theme::Theme;

/// Render the portfolio risk dashboard panel.
#[allow(clippy::too_many_lines)]
pub fn draw_risk_panel(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mut iv_data = std::collections::HashMap::new();
    for (sym, w) in &app.whisper_cache {
        if let Some(vol_str) = &w.volatility {
            let cleaned = vol_str.replace('%', "").trim().to_string();
            if let Ok(iv) = cleaned.parse::<f64>() {
                iv_data.insert(sym.clone(), iv);
            }
        }
    }

    let risk = PortfolioRisk::compute(
        &app.watchlist.quotes().iter().flatten().cloned().collect::<Vec<_>>(),
        &app.screener_results,
        &iv_data,
    );

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top label
            Constraint::Length(1), // Beta gauge
            Constraint::Length(1), // Volatility gauge
            Constraint::Length(1), // Correlation gauge
            Constraint::Min(0),    // Top sectors
        ])
        .split(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(" \u{26A0} Risk Dashboard ") // Warning icon
                .title_style(title_style)
                .inner(area),
        );

    // Frame the whole area
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" \u{26A0} Risk Dashboard ")
            .title_style(title_style),
        area,
    );

    // If no quotes, just show empty
    if app.watchlist.quotes().iter().flatten().count() == 0 {
        let msg = Paragraph::new("Add symbols to calculate risk.")
            .style(Style::default().fg(theme.muted));
        frame.render_widget(msg, chunks[0]);
        return;
    }

    // Gauges
    let beta_ratio = (risk.beta_exposure / 3.0).clamp(0.0, 1.0);
    let beta_color = if risk.beta_exposure > 1.5 {
        theme.loss
    } else if risk.beta_exposure < 0.8 {
        theme.gain
    } else {
        theme.accent
    };

    let beta_gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(beta_color))
        .ratio(beta_ratio)
        .label(format!("Avg Beta: {:.2}", risk.beta_exposure));
    frame.render_widget(beta_gauge, chunks[1]);

    let vol_ratio = (risk.volatility_score / 100.0).clamp(0.0, 1.0);
    let vol_color = if risk.volatility_score > 60.0 {
        theme.loss
    } else if risk.volatility_score < 30.0 {
        theme.gain
    } else {
        theme.accent
    };

    let vol_gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(vol_color))
        .ratio(vol_ratio)
        .label(format!("Volatility Score: {:.0}", risk.volatility_score));
    frame.render_widget(vol_gauge, chunks[2]);

    let corr_ratio = (risk.correlation_risk / 100.0).clamp(0.0, 1.0);
    let corr_color = if risk.correlation_risk > 50.0 {
        theme.loss
    } else if risk.correlation_risk < 20.0 {
        theme.gain
    } else {
        theme.accent
    };

    let corr_gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(corr_color))
        .ratio(corr_ratio)
        .label(format!("Max Sector Conc: {:.0}%", risk.correlation_risk));
    frame.render_widget(corr_gauge, chunks[3]);

    // Top Sectors
    let mut sectors: Vec<_> = risk.sector_concentration.into_iter().collect();
    sectors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut sector_lines = vec![Line::from(Span::styled("Top Sectors:", Style::default().add_modifier(Modifier::BOLD)))];
    for (sec, pct) in sectors.into_iter().take(3) {
        if sec != "-" {
            sector_lines.push(Line::from(format!("  {sec} - {pct:.0}%")));
        }
    }

    if chunks[4].height > 0 {
        frame.render_widget(Paragraph::new(sector_lines).style(Style::default().fg(theme.fg)), chunks[4]);
    }
}
