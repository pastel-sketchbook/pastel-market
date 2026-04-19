//! UI rendering — dispatches to per-panel modules based on view mode.

mod detail;
mod footer;
mod header;
pub mod helpers;
mod qc;
mod scanner;
mod watchlist;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::widgets::{Block, Clear};
use ratatui::Frame;

use crate::app::App;
use market_core::domain::ViewMode;

use helpers::render_size_guard;

/// Render the entire TUI into the given frame.
///
/// View-mode dispatch:
/// - `Watchlist` / `Scanner` — RM-style layout (header, sectors, table, movers/news, detail, footer)
/// - `QualityControl` — PP-style layout (header, 3-column middle, screener table, footer)
pub fn draw(frame: &mut Frame, app: &mut App) {
    // Advance animation counter each render.
    app.tick = app.tick.wrapping_add(1);

    let theme = app.theme();

    // Terminal size guard.
    if render_size_guard(frame, theme) {
        return;
    }

    let area = frame.area();

    // Paint background.
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );

    match app.view_mode {
        ViewMode::Watchlist | ViewMode::Scanner => draw_market_layout(frame, app),
        ViewMode::QualityControl => draw_qc_layout(frame, app),
    }
}

/// RM-style layout: header | sectors | table | movers/news | detail | footer.
fn draw_market_layout(frame: &mut Frame, app: &mut App) {
    let theme = app.theme();
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(3), // sector row
            Constraint::Min(5),   // table (flexible)
            Constraint::Length(6), // movers / news
            Constraint::Length(5), // detail
            Constraint::Length(1), // footer
        ])
        .split(area);

    header::draw_header(frame, app, theme, chunks[0]);
    header::draw_sector_row(frame, app, theme, chunks[1]);

    if app.view_mode == ViewMode::Scanner {
        scanner::draw_scanner_table(frame, app, theme, chunks[2]);
    } else {
        watchlist::draw_watchlist_table(frame, app, theme, chunks[2]);
    }

    if app.show_news {
        detail::draw_news_panel(frame, app, theme, chunks[3]);
    } else {
        detail::draw_top_movers(frame, app, theme, chunks[3]);
    }

    detail::draw_detail(frame, app, theme, chunks[4]);
    footer::draw_footer(frame, app, theme, chunks[5]);
}

/// PP-style layout: header | 3-column middle | screener table | footer.
fn draw_qc_layout(frame: &mut Frame, app: &mut App) {
    let theme = app.theme();
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Fill(35), // middle panels (35% of remaining)
            Constraint::Fill(65), // screener table (65% of remaining)
            Constraint::Length(1), // footer
        ])
        .split(area);

    header::draw_header(frame, app, theme, chunks[0]);
    qc::draw_qc_middle(frame, app, theme, chunks[1]);
    qc::draw_screener_table(frame, app, theme, chunks[2]);
    footer::draw_footer(frame, app, theme, chunks[3]);
}
