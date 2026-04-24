//! Footer bar with keyboard shortcuts and status indicators.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Focus, InputMode};
use market_core::domain::ViewMode;
use market_core::theme::Theme;

use super::helpers::{key_badge, muted_span, refresh_indicator};

/// Render the footer bar.
pub fn draw_footer(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let sep = muted_span("\u{2502}", theme);

    let spans = match app.input_mode {
        InputMode::Normal => build_normal_footer(app, theme, &sep),
        InputMode::Adding => vec![
            Span::styled(" Add symbol: ", Style::default().fg(theme.accent)),
            Span::styled(
                app.input_buffer.clone(),
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            ),
            muted_span("_", theme),
        ],
    };

    let left = Paragraph::new(Line::from(spans));

    // Right-aligned: refresh indicator + theme name + version.
    let indicator = refresh_indicator(app.market_status.is_active(), app.ticks_since_refresh, theme);
    let theme_name = app.theme().name;
    let version = env!("CARGO_PKG_VERSION");
    let right_spans = vec![
        indicator,
        muted_span(" \u{2502} ", theme),
        muted_span(&format!("{theme_name} "), theme),
        muted_span(&format!("v{version} "), theme),
    ];

    #[allow(clippy::cast_possible_truncation)]
    let right_width: u16 = right_spans.iter().map(|s| s.content.len()).sum::<usize>() as u16 + 1;

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_width)])
        .split(area);

    frame.render_widget(left, cols[0]);
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        cols[1],
    );
}

/// Build footer spans for normal mode.
fn build_normal_footer<'a>(app: &App, theme: &'a Theme, sep: &Span<'a>) -> Vec<Span<'a>> {
    let mut s = vec![
        key_badge("q", theme),
        muted_span(" Quit ", theme),
        sep.clone(),
        key_badge("Tab", theme),
        muted_span(" View ", theme),
        sep.clone(),
        key_badge("j/k", theme),
        muted_span(" Nav ", theme),
        sep.clone(),
        key_badge("r", theme),
        muted_span(" Refresh ", theme),
        sep.clone(),
        key_badge("s", theme),
        muted_span(" Sort ", theme),
        sep.clone(),
        key_badge("f", theme),
        muted_span(" Filter ", theme),
        sep.clone(),
        key_badge("t", theme),
        muted_span(" Theme ", theme),
        sep.clone(),
        key_badge("n", theme),
        muted_span(" News ", theme),
        sep.clone(),
        key_badge("?", theme),
        muted_span(" Help ", theme),
    ];

    // Context-dependent keys.
    match app.view_mode {
        ViewMode::Scanner => {
            s.push(sep.clone());
            s.push(key_badge("1-5", theme));
            s.push(muted_span(&format!(" {} ", app.scanner_list), theme));
        }
        ViewMode::QualityControl => {
            s.push(sep.clone());
            s.push(key_badge("Space", theme));
            s.push(muted_span(" Toggle ", theme));
            s.push(sep.clone());
            s.push(key_badge("h/l", theme));
            let focus_label = match app.focus {
                Focus::Table => "Table",
                Focus::QcChecklist => "QC",
            };
            s.push(muted_span(&format!(" {focus_label} "), theme));
        }
        ViewMode::Watchlist => {
            s.push(sep.clone());
            s.push(key_badge("a", theme));
            s.push(muted_span(" Add ", theme));
            s.push(sep.clone());
            s.push(key_badge("d", theme));
            s.push(muted_span(" Del ", theme));
            s.push(sep.clone());
            s.push(key_badge("[/]", theme));
            s.push(muted_span(" Tab ", theme));
        }
    }

    // Status message inline.
    if !app.status_message.is_empty() {
        s.push(sep.clone());
        s.push(muted_span(&format!(" {} ", app.status_message), theme));
    }

    s
}
