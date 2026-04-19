//! Performance chart overlay — renders a full-screen line chart with
//! selectable time range tabs.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph};

use market_core::domain::ChartRange;
use market_core::theme::Theme;

use crate::app::App;

/// Draw the chart overlay centered on the screen.
pub fn draw_chart_overlay(frame: &mut Frame, app: &App, theme: &'static Theme) {
    let area = frame.area();

    // Overlay takes ~90% of the screen, centered.
    let chart_area = centered_rect(area, 90, 90);

    // Clear the area behind the overlay.
    frame.render_widget(Clear, chart_area);

    // Build the range tab bar.
    let tab_spans: Vec<Span> = ChartRange::ALL
        .iter()
        .map(|&r| {
            if r == app.chart_range {
                Span::styled(
                    format!(" {} ", r.label()),
                    Style::default()
                        .fg(theme.bg)
                        .bg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!(" {} ", r.label()),
                    Style::default().fg(theme.muted),
                )
            }
        })
        .collect();

    let title = format!(" {} — Performance ", app.chart_symbol);

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // range tabs
            Constraint::Min(3),   // chart
            Constraint::Length(1), // help bar
        ])
        .split(chart_area.inner(Margin::new(1, 1)));

    // Draw outer block.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    frame.render_widget(block, chart_area);

    // Range tab bar.
    let tabs = Paragraph::new(Line::from(tab_spans)).alignment(Alignment::Center);
    frame.render_widget(tabs, inner_layout[0]);

    // Loading state.
    if app.chart_loading || app.chart_data.is_empty() {
        let msg = if app.chart_loading {
            "Loading..."
        } else {
            "No data available"
        };
        let loading = Paragraph::new(msg)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted));
        frame.render_widget(loading, inner_layout[1]);
    } else {
        draw_line_chart(frame, app, theme, inner_layout[1]);
    }

    // Help bar.
    let help = Paragraph::new(Line::from(vec![
        Span::styled("1-8", Style::default().fg(theme.accent)),
        Span::styled(" range  ", Style::default().fg(theme.muted)),
        Span::styled("←/→", Style::default().fg(theme.accent)),
        Span::styled(" prev/next  ", Style::default().fg(theme.muted)),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::styled(" close", Style::default().fg(theme.muted)),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(help, inner_layout[2]);
}

/// Render the actual line chart from `app.chart_data`.
fn draw_line_chart(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let points = &app.chart_data;
    if points.is_empty() {
        return;
    }

    // Build (x, y) data. X is the index.
    #[allow(clippy::cast_precision_loss)]
    let data: Vec<(f64, f64)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f64, p.close))
        .collect();

    // Compute Y bounds with padding.
    let min_y = data
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);
    let max_y = data
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let y_pad = (max_y - min_y) * 0.05;
    let y_lo = min_y - y_pad;
    let y_hi = max_y + y_pad;

    #[allow(clippy::cast_precision_loss)]
    let max_x = data.len().saturating_sub(1) as f64;

    // Determine line color: green if price went up, red if down.
    let first = data.first().map_or(0.0, |(_, y)| *y);
    let last = data.last().map_or(0.0, |(_, y)| *y);
    let line_color = if last >= first { theme.gain } else { theme.loss };

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(line_color))
        .data(&data);

    // Y-axis labels: low, mid, high.
    let y_mid = f64::midpoint(y_lo, y_hi);
    let y_labels = vec![
        Span::styled(format_price(y_lo), Style::default().fg(theme.muted)),
        Span::styled(format_price(y_mid), Style::default().fg(theme.fg)),
        Span::styled(format_price(y_hi), Style::default().fg(theme.muted)),
    ];

    // X-axis labels: range label.
    let x_labels = vec![
        Span::styled("", Style::default().fg(theme.muted)),
        Span::styled(
            app.chart_range.label(),
            Style::default().fg(theme.muted),
        ),
    ];

    // Change summary.
    let change = last - first;
    let change_pct = if first.abs() > f64::EPSILON {
        (change / first) * 100.0
    } else {
        0.0
    };
    let sign = if change >= 0.0 { "+" } else { "" };
    let summary = format!(
        "{} → {}  ({sign}{:.2}  {sign}{:.2}%)",
        format_price(first),
        format_price(last),
        change,
        change_pct,
    );

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(Span::styled(
                    summary,
                    Style::default().fg(line_color).add_modifier(Modifier::BOLD),
                ))
                .title_alignment(Alignment::Right)
                .borders(Borders::NONE),
        )
        .x_axis(
            Axis::default()
                .labels(x_labels)
                .bounds([0.0, max_x]),
        )
        .y_axis(
            Axis::default()
                .labels(y_labels)
                .bounds([y_lo, y_hi]),
        );

    frame.render_widget(chart, area);
}

/// Format a price for axis labels.
fn format_price(price: f64) -> String {
    if price >= 1000.0 {
        format!("{price:.0}")
    } else if price >= 100.0 {
        format!("{price:.1}")
    } else {
        format!("{price:.2}")
    }
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
