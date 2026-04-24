//! Scanner table rendering (day gainers/losers, most active, trending, fundamentals).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Table};

use crate::app::App;
use market_core::domain::{FilterMode, Quote, SortMode, rank_by_change, sorted_filtered_indices};
use market_core::theme::Theme;

use super::helpers::{highlight_style, stripe_style};
use super::watchlist::{TABLE_WIDTHS, build_quote_row, empty_state_row, table_header};

/// Render the scanner results table.
pub fn draw_scanner_table(frame: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let header = table_header(theme);

    let as_options: Vec<Option<Quote>> = app.scanner_quotes.iter().cloned().map(Some).collect();
    let ranks = rank_by_change(&as_options, 3);

    let filtered = sorted_filtered_indices(&app.scanner_quotes, app.sort_mode, app.filter_mode);

    let rows: Vec<ratatui::widgets::Row> = if filtered.is_empty() {
        let msg = if app.scanner_quotes.is_empty() {
            "No results — press [r] to refresh"
        } else {
            "No matches for current filter"
        };
        vec![empty_state_row(msg, theme)]
    } else {
        filtered
            .iter()
            .enumerate()
            .map(|(row_idx, &orig_idx)| {
                let q = &app.scanner_quotes[orig_idx];
                let row = build_quote_row(
                    Some(q),
                    Some(q.symbol.as_str()),
                    ranks.get(orig_idx).and_then(Option::as_ref),
                    theme,
                    None,
                );
                row.style(stripe_style(row_idx, theme))
            })
            .collect()
    };

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

    let title = format!(" Scanner: {}{sort_label}{filter_label} ", app.scanner_list);
    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let selected = if filtered.is_empty() {
        0
    } else {
        // Map scanner_selected (original index) to filtered position.
        filtered
            .iter()
            .position(|&i| i == app.scanner_selected)
            .unwrap_or(0)
    };

    let table = Table::new(rows, TABLE_WIDTHS)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(title)
                .title_style(title_style),
        )
        .row_highlight_style(highlight_style(theme));

    app.scanner_table_state.select(Some(selected));
    frame.render_stateful_widget(table, area, &mut app.scanner_table_state);
}
