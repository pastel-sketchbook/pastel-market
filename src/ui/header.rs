//! Header bar: title, market status, index quotes, sector performance.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use crate::app::{App, INDEX_SYMBOLS, SECTOR_SYMBOLS};
use market_core::domain::{MarketStatus, Quote};
use market_core::theme::Theme;

use super::helpers::{active_color, format_price};

/// Pastel palette for the title letters.
const PASTEL_PALETTE: [Color; 12] = [
    Color::Rgb(255, 154, 162),
    Color::Rgb(255, 183, 178),
    Color::Rgb(255, 218, 193),
    Color::Rgb(255, 236, 179),
    Color::Rgb(226, 240, 203),
    Color::Rgb(181, 234, 215),
    Color::Rgb(163, 226, 233),
    Color::Rgb(155, 207, 232),
    Color::Rgb(175, 187, 235),
    Color::Rgb(199, 178, 232),
    Color::Rgb(224, 177, 225),
    Color::Rgb(245, 175, 212),
];

/// Build "PASTEL MARKET" title with pastel-colored letters.
fn title_spans(theme: &Theme) -> Vec<Span<'static>> {
    let title = "PASTEL MARKET";
    let mut spans = Vec::with_capacity(title.len() + 1);
    let mut color_idx = 0;
    for ch in title.chars() {
        if ch == ' ' {
            spans.push(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                String::from(ch),
                Style::default()
                    .fg(PASTEL_PALETTE[color_idx % PASTEL_PALETTE.len()])
                    .add_modifier(Modifier::BOLD),
            ));
            color_idx += 1;
        }
    }
    spans.push(Span::styled(
        " TERMINAL",
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
    ));
    spans
}

/// Market status badge span.
fn market_status_badge(status: MarketStatus, theme: &Theme) -> Span<'static> {
    let (color, label) = match status {
        MarketStatus::Open => (theme.gain, " OPEN "),
        MarketStatus::PreMarket => (theme.accent, " PRE-MARKET "),
        MarketStatus::AfterHours => (theme.accent, " AFTER-HOURS "),
        MarketStatus::Closed => (theme.loss, " CLOSED "),
    };
    Span::styled(
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

/// Header border color based on market status.
fn header_border_color(status: MarketStatus, any_passed: bool, theme: &Theme) -> Color {
    if any_passed {
        return active_color(true, theme);
    }
    match status {
        MarketStatus::Open => theme.gain,
        MarketStatus::PreMarket | MarketStatus::AfterHours => theme.accent,
        MarketStatus::Closed => theme.loss,
    }
}

/// Render the header bar.
pub fn draw_header(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let any_passed = app.any_fully_passed();
    let border_color = header_border_color(app.market_status, any_passed, theme);

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));
    let inner = header_block.inner(area);
    frame.render_widget(header_block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    // Left: title + conviction status.
    let mut left_spans = title_spans(theme);
    let status_label = if any_passed {
        " [HIGH CONVICTION - READY] "
    } else {
        " [ANALYSIS IN PROGRESS] "
    };
    let ac = active_color(any_passed, theme);
    left_spans.push(Span::styled(
        status_label,
        Style::default().fg(ac).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(Line::from(left_spans)), cols[0]);

    // Right: market status + index summary.
    let mut right_spans: Vec<Span> = vec![
        market_status_badge(app.market_status, theme),
        Span::raw("  "),
    ];
    for (i, sym) in INDEX_SYMBOLS.iter().enumerate() {
        let quote = app.index_quotes.get(i).and_then(Option::as_ref);
        right_spans.extend(format_index_spans(sym, quote, theme));
        right_spans.push(Span::raw("  "));
    }
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        cols[1],
    );
}

/// Format a single index quote as spans.
fn format_index_spans<'a>(symbol: &'a str, quote: Option<&Quote>, theme: &Theme) -> Vec<Span<'a>> {
    let label = match symbol {
        "^GSPC" => "S&P",
        "^DJI" => "DOW",
        "^IXIC" => "NDQ",
        "^RUT" => "R2K",
        "^VIX" => "VIX",
        other => other,
    };

    if let Some(q) = quote {
        let color = if q.is_gain() { theme.gain } else { theme.loss };
        vec![
            Span::styled(
                label.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(format_price(q.regular_market_price)),
            Span::styled(
                format!(" {:+.2}%", q.regular_market_change_percent),
                Style::default().fg(color),
            ),
        ]
    } else {
        vec![
            Span::styled(
                label.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" --"),
        ]
    }
}

/// Render sector performance row.
pub fn draw_sector_row(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let mut spans = vec![Span::raw(" ")];

    for (i, sym) in SECTOR_SYMBOLS.iter().enumerate() {
        let quote = app.sector_quotes.get(i).and_then(Option::as_ref);
        spans.extend(format_sector_badge(sym, quote, theme));
        spans.push(Span::raw(" "));
    }

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let row = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Sectors ")
            .title_style(title_style),
    );

    frame.render_widget(row, area);
}

/// Format a single sector ETF badge.
fn format_sector_badge<'a>(symbol: &'a str, quote: Option<&Quote>, theme: &Theme) -> Vec<Span<'a>> {
    if let Some(q) = quote {
        let color = if q.is_gain() { theme.gain } else { theme.loss };
        vec![
            Span::styled(symbol, Style::default().add_modifier(Modifier::DIM)),
            Span::styled(
                format!("{:+.1}%", q.regular_market_change_percent),
                Style::default().fg(color),
            ),
        ]
    } else {
        vec![
            Span::styled(symbol, Style::default().add_modifier(Modifier::DIM)),
            Span::raw("--"),
        ]
    }
}
