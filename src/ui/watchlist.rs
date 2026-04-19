//! Watchlist table rendering with heatmap, sorting, filtering, and rank badges.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::App;
use market_core::domain::{FilterMode, Quote, QuoteRank, SortMode, rank_by_change};
use market_core::theme::Theme;

use super::helpers::{format_change_cell, format_volume, heatmap_color, highlight_style, stripe_style};

/// Column widths shared between watchlist and scanner tables.
pub const TABLE_WIDTHS: [Constraint; 6] = [
    Constraint::Length(8),
    Constraint::Min(16),
    Constraint::Length(10),
    Constraint::Length(10),
    Constraint::Length(10),
    Constraint::Length(10),
];

/// Build the shared header row.
pub fn table_header(theme: &Theme) -> Row<'static> {
    Row::new(vec![
        Cell::from("Symbol"),
        Cell::from("Name"),
        Cell::from("Price"),
        Cell::from("Change"),
        Cell::from("Change%"),
        Cell::from("Volume"),
    ])
    .style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )
}

/// Build a single table row from a quote.
pub fn build_quote_row<'a>(
    quote: Option<&Quote>,
    symbol: Option<&str>,
    rank: Option<&QuoteRank>,
    theme: &Theme,
) -> Row<'a> {
    if let Some(q) = quote {
        let change_style = rank.map_or_else(
            || {
                let fg = if q.is_gain() { theme.gain } else { theme.loss };
                Style::default().fg(fg)
            },
            |r| {
                Style::default()
                    .bg(heatmap_color(r))
                    .fg(ratatui::style::Color::White)
                    .add_modifier(Modifier::BOLD)
            },
        );

        Row::new(vec![
            Cell::from(q.symbol.clone()),
            Cell::from(q.display_name().to_string()),
            Cell::from(format!("{:.2}", q.regular_market_price)),
            Cell::from(format!("{:+.2}", q.regular_market_change)).style(change_style),
            Cell::from(format_change_cell(q.regular_market_change_percent, rank))
                .style(change_style),
            Cell::from(format_volume(q.regular_market_volume)),
        ])
    } else {
        Row::new(vec![
            Cell::from(symbol.unwrap_or("--").to_string()),
            Cell::from("--"),
            Cell::from("--"),
            Cell::from("--"),
            Cell::from("--"),
            Cell::from("--"),
        ])
    }
}

/// Build a placeholder row for empty tables.
pub fn empty_state_row(message: &str, theme: &Theme) -> Row<'static> {
    Row::new(vec![
        Cell::from(message.to_string()).style(Style::default().fg(theme.muted)),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
        Cell::from(""),
    ])
}

/// Render the watchlist table.
pub fn draw_watchlist_table(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let header = table_header(theme);
    let sorted = app.watchlist.sorted_indices(app.sort_mode);
    let ranks = rank_by_change(app.watchlist.quotes(), 3);

    let filtered: Vec<usize> = sorted
        .iter()
        .copied()
        .filter(|&i| {
            app.watchlist
                .quotes()
                .get(i)
                .and_then(Option::as_ref)
                .map_or(app.filter_mode == FilterMode::All, |q| {
                    app.filter_mode.matches(q)
                })
        })
        .collect();

    let rows: Vec<Row> = if filtered.is_empty() && app.filter_mode != FilterMode::All {
        vec![empty_state_row("No matches for current filter", theme)]
    } else {
        filtered
            .iter()
            .enumerate()
            .map(|(row_idx, &i)| {
                let row = build_quote_row(
                    app.watchlist.quotes().get(i).and_then(Option::as_ref),
                    app.watchlist.symbols().get(i).map(String::as_str),
                    ranks.get(i).and_then(Option::as_ref),
                    theme,
                );
                row.style(stripe_style(row_idx, theme))
            })
            .collect()
    };

    let raw_selected = app.watchlist.selected();
    let display_selected = filtered
        .iter()
        .position(|&i| i == raw_selected)
        .unwrap_or(0);

    let sort_label = if app.sort_mode == SortMode::Default {
        String::new()
    } else {
        format!(" [{}]", app.sort_mode)
    };

    let filter_label = if app.filter_mode == FilterMode::All {
        String::new()
    } else {
        format!(" <{}>", app.filter_mode)
    };

    let table = Table::new(rows, TABLE_WIDTHS)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(format!(" Watchlist{sort_label}{filter_label} ")),
        )
        .row_highlight_style(highlight_style(theme));

    frame.render_stateful_widget(
        table,
        area,
        &mut ratatui::widgets::TableState::default().with_selected(Some(display_selected)),
    );
}
