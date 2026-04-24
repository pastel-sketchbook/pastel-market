//! Help overlay showing all keybindings.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use market_core::theme::Theme;

/// Keybinding entries: (key, description).
const BINDINGS: &[(&str, &str)] = &[
    ("q / Esc", "Quit"),
    ("?", "Toggle this help"),
    ("Tab / BackTab", "Cycle view mode"),
    ("j / k / ↑ / ↓", "Navigate up / down"),
    ("gg", "Jump to first row"),
    ("G", "Jump to last row"),
    ("Enter", "Open chart (Watchlist/Scanner)"),
    ("Space / Enter", "Toggle QC item (QC view)"),
    ("h / l / ← / →", "Switch focus (QC view)"),
    ("r", "Refresh data"),
    ("s", "Cycle sort mode"),
    ("f", "Cycle filter mode"),
    ("t", "Cycle theme"),
    ("n", "Toggle news panel"),
    ("a", "Add symbol (Watchlist)"),
    ("d", "Delete symbol (Watchlist)"),
    ("1-5", "Select scanner list (Scanner)"),
    ("Ctrl+C", "Force quit"),
];

/// Draw the help overlay centered on the screen.
pub fn draw_help_overlay(frame: &mut Frame, theme: &'static Theme) {
    let area = frame.area();
    let overlay = centered_rect(area, 60, 80);

    frame.render_widget(Clear, overlay);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            " Keybindings ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    let inner = overlay.inner(Margin::new(2, 1));
    frame.render_widget(block, overlay);

    let lines: Vec<Line> = BINDINGS
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{key:<22}"),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(theme.fg)),
            ])
        })
        .collect();

    let help_text = Paragraph::new(lines);
    frame.render_widget(help_text, inner);

    // Footer hint.
    let footer_area = Rect {
        x: overlay.x,
        y: overlay.bottom().saturating_sub(1),
        width: overlay.width,
        height: 1,
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("?", Style::default().fg(theme.accent)),
        Span::styled(" or ", Style::default().fg(theme.muted)),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::styled(" to close", Style::default().fg(theme.muted)),
    ]))
    .alignment(Alignment::Center)
    .style(Style::default().bg(theme.bg));
    frame.render_widget(footer, footer_area);
}

/// Compute a centered rectangle within `area` using percentage dimensions.
fn centered_rect(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(vert[1])[1]
}
