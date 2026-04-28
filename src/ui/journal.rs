//! Persistent decision log and trade journal view.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::app::App;
use market_core::decisions::Action;
use market_core::theme::Theme;

use super::helpers::{highlight_style, stripe_style};

/// Render the decision journal table.
#[allow(clippy::too_many_lines)]
pub fn draw_journal(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let header = Row::new(vec![
        Cell::from("DATE"),
        Cell::from("TICKER"),
        Cell::from("ACTION"),
        Cell::from("RATING"),
        Cell::from("ENTRY $"),
        Cell::from("CURRENT $"),
        Cell::from("RETURN %"),
        Cell::from("ALPHA vs SPY"),
    ])
    .style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );

    let widths = [
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(14),
    ];

    let rows: Vec<Row> = if app.decisions.entries.is_empty() {
        vec![Row::new(vec![
            Cell::from("No decisions logged yet. Press Shift+B/S/H to record a trade.")
                .style(Style::default().fg(theme.muted)),
        ])]
    } else {
        // Sort entries by date descending (newest first).
        let mut sorted_entries = app.decisions.entries.clone();
        sorted_entries.sort_by_key(|e| std::cmp::Reverse(e.date));

        sorted_entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let date_str = entry.date.format("%Y-%m-%d").to_string();

                let action_color = match entry.action {
                    Action::Buy => theme.gain,
                    Action::Sell => theme.loss,
                    Action::Hold => theme.muted,
                };
                let action_cell = Cell::from(entry.action.to_string()).style(
                    Style::default()
                        .fg(action_color)
                        .add_modifier(Modifier::BOLD),
                );

                let (cr, cg, cb) = entry.rating.color_rgb();
                let rating_cell = Cell::from(entry.rating.label().to_string()).style(
                    Style::default()
                        .fg(Color::Rgb(cr, cg, cb))
                        .add_modifier(Modifier::BOLD),
                );

                let entry_price = format!("{:.2}", entry.price_at_decision);

                let (current_price, return_str, return_color, alpha_str) =
                    if let Some(res) = &entry.resolution {
                        let ret_color = if res.return_pct >= 0.0 {
                            theme.gain
                        } else {
                            theme.loss
                        };
                        let alpha = res
                            .alpha_vs_spy
                            .map_or_else(|| "--".to_string(), |a| format!("{a:+.2}%"));
                        (
                            format!("{:.2}", res.price_at_check),
                            format!("{:+.2}%", res.return_pct),
                            ret_color,
                            alpha,
                        )
                    } else {
                        (
                            "--".to_string(),
                            "--".to_string(),
                            theme.muted,
                            "--".to_string(),
                        )
                    };

                Row::new(vec![
                    Cell::from(date_str),
                    Cell::from(entry.ticker.clone())
                        .style(Style::default().add_modifier(Modifier::BOLD)),
                    action_cell,
                    rating_cell,
                    Cell::from(entry_price),
                    Cell::from(current_price),
                    Cell::from(return_str).style(Style::default().fg(return_color)),
                    Cell::from(alpha_str),
                ])
                .style(stripe_style(i, theme))
            })
            .collect()
    };

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(format!(
                    " Decision Journal [{}] ",
                    app.decisions.entries.len()
                ))
                .title_style(title_style),
        )
        .row_highlight_style(highlight_style(theme));

    // We just render it statically without selection for now, or use a separate state.
    // Since Journal is mostly for reading, a static render is fine.
    frame.render_widget(table, area);
}
