//! Header bar: title, market status, index quotes, sector performance, market clock.

use chrono::{Datelike, FixedOffset, NaiveDate, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph};

use crate::app::{App, INDEX_SYMBOLS, SECTOR_SYMBOLS};
use market_core::domain::{MarketStatus, Quote};
use market_core::theme::Theme;

use super::helpers::active_color;

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

/// US Eastern Time offset, accounting for DST (second Sunday in March to
/// first Sunday in November).
fn eastern_offset() -> FixedOffset {
    let now = Utc::now().naive_utc().date();
    if is_us_dst(now) {
        // Safety: -4 * 3600 is within i32 range.
        FixedOffset::west_opt(4 * 3600).expect("valid offset: EDT")
    } else {
        FixedOffset::west_opt(5 * 3600).expect("valid offset: EST")
    }
}

/// Approximate US DST: second Sunday of March through first Sunday of November.
fn is_us_dst(date: NaiveDate) -> bool {
    let year = date.year();
    let march_start = nth_sunday(year, 3, 2);
    let nov_end = nth_sunday(year, 11, 1);
    date >= march_start && date < nov_end
}

/// Return the nth Sunday in the given month/year.
fn nth_sunday(year: i32, month: u32, nth: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid date");
    // weekday: Mon=0 .. Sun=6
    let days_to_sun = (6 - first.weekday().num_days_from_monday()) % 7;
    let day = 1 + days_to_sun + 7 * (nth - 1);
    NaiveDate::from_ymd_opt(year, month, day).expect("valid nth Sunday")
}

/// Market clock spans: "ET HH:MM" and optionally time-to-close.
fn market_clock_spans(status: MarketStatus, theme: &Theme) -> Vec<Span<'static>> {
    let et = Utc::now().with_timezone(&eastern_offset());
    let hm = et.format("%H:%M").to_string();

    let mut spans = vec![Span::styled(
        format!("ET {hm}"),
        Style::default().fg(theme.fg).add_modifier(Modifier::DIM),
    )];

    // Show time-to-close only when market is open (closes at 16:00 ET).
    if status == MarketStatus::Open {
        let now_secs = i64::from(et.format("%H").to_string().parse::<i32>().unwrap_or(0)) * 3600
            + i64::from(et.format("%M").to_string().parse::<i32>().unwrap_or(0)) * 60;
        let close_secs: i64 = 16 * 3600;
        let remaining = close_secs - now_secs;
        if remaining > 0 {
            let h = remaining / 3600;
            let m = (remaining % 3600) / 60;
            spans.push(Span::styled(
                format!(" ({h}h{m:02}m)"),
                Style::default().fg(theme.accent),
            ));
        }
    }

    spans
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
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
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

    // Right: market status + clock + index summary.
    let mut right_spans: Vec<Span> = vec![
        market_status_badge(app.market_status, theme),
        Span::raw(" "),
    ];
    right_spans.extend(market_clock_spans(app.market_status, theme));
    right_spans.push(Span::raw("  "));
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

/// Format index price compactly (drop decimals for large numbers).
fn format_compact_price(price: f64) -> String {
    if price >= 10_000.0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rounded = price.round() as u64;
        format!("{rounded}")
    } else if price >= 1_000.0 {
        format!("{price:.0}")
    } else {
        format!("{price:.1}")
    }
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
        let price = format_compact_price(q.regular_market_price);
        vec![
            Span::styled(
                label.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(price),
            Span::styled(
                format!(" {:+.1}%", q.regular_market_change_percent),
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
