//! Scanner table rendering (day gainers/losers, most active, trending, fundamentals).

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Table};
use ratatui::Frame;

use crate::app::App;
use market_core::domain::{Quote, rank_by_change};
use market_core::theme::Theme;

use super::helpers::stripe_style;
use super::watchlist::{TABLE_WIDTHS, build_quote_row, empty_state_row, table_header};

/// Render the scanner results table.
pub fn draw_scanner_table(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let header = table_header(theme);

    let as_options: Vec<Option<Quote>> = app.scanner_quotes.iter().cloned().map(Some).collect();
    let ranks = rank_by_change(&as_options, 3);

    let rows: Vec<ratatui::widgets::Row> = if app.scanner_quotes.is_empty() {
        vec![empty_state_row("No results — press [r] to refresh", theme)]
    } else {
        app.scanner_quotes
            .iter()
            .enumerate()
            .map(|(row_idx, q)| {
                let row = build_quote_row(
                    Some(q),
                    Some(q.symbol.as_str()),
                    ranks.get(row_idx).and_then(Option::as_ref),
                    theme,
                );
                row.style(stripe_style(row_idx, theme))
            })
            .collect()
    };

    let title = format!(" Scanner: {} ", app.scanner_list);

    let table = Table::new(rows, TABLE_WIDTHS).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(title),
    );

    frame.render_widget(table, area);
}
