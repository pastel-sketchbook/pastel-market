//! Shared UI helpers: formatting, colors, size guard, row striping.

use market_core::domain::{QuoteRank, RankBadge};
use market_core::theme::Theme;
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Minimum terminal width for the TUI to render properly.
pub const MIN_TERM_WIDTH: u16 = 80;

/// Minimum terminal height for the TUI to render properly.
pub const MIN_TERM_HEIGHT: u16 = 20;

/// Render a centered size warning and return `true` if the terminal is too small.
pub fn render_size_guard(frame: &mut Frame, theme: &Theme) -> bool {
    let area = frame.area();
    if area.width >= MIN_TERM_WIDTH && area.height >= MIN_TERM_HEIGHT {
        return false;
    }
    frame.render_widget(Clear, area);
    let msg = format!(
        "Terminal too small ({}\u{00d7}{}). Need at least {MIN_TERM_WIDTH}\u{00d7}{MIN_TERM_HEIGHT}.",
        area.width, area.height,
    );
    let paragraph = Paragraph::new(msg)
        .style(Style::default().fg(theme.error))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
    frame.render_widget(paragraph, area);
    true
}

/// Format volume with K/M/B suffixes.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn format_volume(volume: u64) -> String {
    let v = volume as f64;
    if v >= 1_000_000_000.0 {
        format!("{:.2}B", v / 1_000_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.2}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else {
        volume.to_string()
    }
}

/// Format change percent with badge indicator.
#[must_use]
pub fn format_change_cell(change_percent: f64, rank: Option<&QuoteRank>) -> String {
    let arrow = if change_percent >= 0.0 { "▲" } else { "▼" };
    let badge = rank.map_or("", |r| match r.badge {
        RankBadge::TopGainer => "🔥",
        RankBadge::TopLoser => "💧",
        RankBadge::None => "",
    });
    format!("{badge}{arrow}{change_percent:+.2}%")
}

/// Compute heatmap background color from a quote rank.
#[must_use]
pub fn heatmap_color(rank: &QuoteRank) -> Color {
    let base = 20_u8;
    let full = 140_u8;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let channel = base + (f64::from(full - base) * rank.intensity) as u8;
    if rank.is_gain {
        Color::Rgb(0, channel, 0)
    } else {
        Color::Rgb(channel, 0, 0)
    }
}

/// Apply row striping: even rows get `stripe_bg`.
#[must_use]
pub fn stripe_style(index: usize, theme: &Theme) -> Style {
    if index.is_multiple_of(2) {
        Style::default().bg(theme.stripe_bg)
    } else {
        Style::default()
    }
}

/// Style for the highlighted (selected) row.
#[must_use]
pub fn highlight_style(theme: &Theme) -> Style {
    Style::default()
        .bg(theme.highlight_bg)
        .fg(theme.highlight_fg)
        .add_modifier(Modifier::BOLD)
}

/// Create a key badge span (e.g. " q " in `key_fg`/`key_bg`).
#[must_use]
pub fn key_badge<'a>(key: &str, theme: &Theme) -> Span<'a> {
    Span::styled(
        format!(" {key} "),
        Style::default()
            .fg(theme.key_fg)
            .bg(theme.key_bg)
            .add_modifier(Modifier::BOLD),
    )
}

/// Create a muted description span.
#[must_use]
pub fn muted_span<'a>(text: &str, theme: &Theme) -> Span<'a> {
    Span::styled(text.to_string(), Style::default().fg(theme.muted))
}

/// Active color: status color (green) if conviction ready, accent (cyan) otherwise.
#[must_use]
pub fn active_color(any_passed: bool, theme: &Theme) -> Color {
    if any_passed {
        theme.status
    } else {
        theme.accent
    }
}

/// Refresh indicator span.
#[must_use]
pub fn refresh_indicator<'a>(is_active: bool, theme: &Theme) -> Span<'a> {
    if is_active {
        Span::styled(" ⟳ 30s ", Style::default().fg(theme.accent))
    } else {
        Span::styled(" ⏸ 5m ", Style::default().fg(theme.muted))
    }
}

/// Tick-based spinner frame (Braille dots cycling at ~4 fps with 250ms ticks).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn spinner_frame(tick: u64) -> char {
    const FRAMES: &[char] = &[
        '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}',
        '\u{2827}', '\u{2807}', '\u{280f}',
    ];
    FRAMES[(tick as usize) % FRAMES.len()]
}
