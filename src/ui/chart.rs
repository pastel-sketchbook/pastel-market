//! Performance chart overlay — renders a full-screen line chart with
//! selectable time range tabs.

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph, Widget};

use market_core::domain::ChartRange;
use market_core::theme::Theme;

use crate::app::App;

/// Draw the chart overlay centered on the screen.
pub fn draw_chart_overlay(frame: &mut Frame, app: &App, theme: &'static Theme) {
    let area = frame.area();

    // Overlay takes ~90% of the screen, centered.
    let chart_area = centered_rect(area, 90, 90);

    // Clear the area behind the overlay and fill with chart background.
    frame.render_widget(Clear, chart_area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.chart_bg)),
        chart_area,
    );

    // Build the range tab bar.
    let tab_spans: Vec<Span> = ChartRange::ALL
        .iter()
        .map(|&r| {
            if r == app.chart_range {
                Span::styled(
                    format!(" {} ", r.label()),
                    Style::default()
                        .fg(theme.chart_bg)
                        .bg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(format!(" {} ", r.label()), Style::default().fg(theme.muted))
            }
        })
        .collect();

    let title = format!(" {} — Performance ", app.chart_symbol);

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // range tabs
            Constraint::Min(3),    // chart
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
        .style(Style::default().bg(theme.chart_bg).fg(theme.fg));
    frame.render_widget(block, chart_area);

    // Range tab bar.
    let tabs = Paragraph::new(Line::from(tab_spans))
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.chart_bg));
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
            .style(Style::default().fg(theme.muted).bg(theme.chart_bg));
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
    .alignment(Alignment::Center)
    .style(Style::default().bg(theme.chart_bg));
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
    let min_y = data.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
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
    let line_color = if last >= first {
        theme.gain
    } else {
        theme.loss
    };

    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(line_color))
        .data(&data);

    // Y-axis labels: 5 evenly spaced for subtle grid lines.
    let y_step = (y_hi - y_lo) / 4.0;
    let y_labels: Vec<Span> = (0..5)
        .map(|i| {
            let val = y_lo + y_step * f64::from(i);
            let color = if i == 0 || i == 4 {
                theme.muted
            } else {
                theme.fg
            };
            Span::styled(format_price(val), Style::default().fg(color))
        })
        .collect();

    // X-axis labels: 5 evenly spaced date/time labels from timestamps.
    let x_label_count = 5_usize;
    let x_labels = build_x_labels(points, x_label_count, app.chart_range, theme);

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

    let grid_style = Style::default().fg(theme.border);

    // Build y_labels clone for grid calculation (Chart consumes the original).
    let y_label_widths: Vec<usize> = y_labels.iter().map(Span::width).collect();

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(Span::styled(
                    summary,
                    Style::default().fg(line_color).add_modifier(Modifier::BOLD),
                ))
                .title_alignment(Alignment::Right)
                .borders(Borders::NONE)
                .style(Style::default().bg(theme.chart_bg)),
        )
        .x_axis(
            Axis::default()
                .labels(x_labels)
                .bounds([0.0, max_x])
                .style(grid_style),
        )
        .y_axis(
            Axis::default()
                .labels(y_labels)
                .bounds([y_lo, y_hi])
                .style(grid_style),
        )
        .style(Style::default().fg(theme.fg).bg(theme.chart_bg));

    frame.render_widget(chart, area);

    // Draw subtle grid lines AFTER the chart so they aren't
    // overwritten. The Braille data markers already occupy their cells,
    // and grid dots only fill empty-looking cells.
    draw_grid_lines(frame, &y_label_widths, x_label_count, theme, area);
}

/// Draw subtle dotted grid lines at interior label positions (both axes).
///
/// The Chart widget's graph area starts after y-axis labels (left) and
/// ends before the x-axis label row (bottom). We estimate that region
/// and draw dotted lines at each interior label position.
fn draw_grid_lines(
    frame: &mut Frame,
    y_label_widths: &[usize],
    x_label_count: usize,
    theme: &'static Theme,
    area: Rect,
) {
    #[allow(clippy::cast_possible_truncation)]
    let y_label_width = y_label_widths.iter().copied().max().unwrap_or(0) as u16;
    // +1 for the axis tick character the Chart widget reserves.
    let graph_left = area.x.saturating_add(y_label_width).saturating_add(1);
    let graph_right = area.right();
    // Chart reserves 1 row for x-axis labels at the bottom.
    let graph_top = area.y.saturating_add(1); // +1 for the block title row
    let graph_bottom = area.bottom().saturating_sub(2); // -1 x-axis label, -1 padding
    let graph_height = graph_bottom.saturating_sub(graph_top);
    let graph_width = graph_right.saturating_sub(graph_left);

    if graph_left >= graph_right || graph_height == 0 {
        return;
    }

    let style = Style::default().fg(theme.muted);

    // Horizontal grid lines at interior Y positions (3 lines for 5 labels).
    let h_line = GridLine {
        x_start: graph_left,
        x_end: graph_right,
        y: 0,
        style,
        vertical: false,
    };
    for i in 1..4_u16 {
        let row = graph_bottom.saturating_sub(i * graph_height / 4);
        if row > graph_top && row < graph_bottom {
            frame.render_widget(GridLine { y: row, ..h_line }, area);
        }
    }

    // Vertical grid lines at interior X positions.
    let segments = x_label_count.saturating_sub(1);
    if segments < 2 || graph_width == 0 {
        return;
    }
    let v_line = GridLine {
        x_start: graph_top,  // reuse fields: x_start=top, x_end=bottom for vertical
        x_end: graph_bottom,
        y: 0,
        style,
        vertical: true,
    };
    for i in 1..segments {
        #[allow(clippy::cast_possible_truncation)]
        let col = graph_left + (graph_width * i as u16) / segments as u16;
        if col > graph_left && col < graph_right {
            frame.render_widget(GridLine { y: col, ..v_line }, area);
        }
    }
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

/// Build evenly spaced x-axis date/time labels from `PricePoint` timestamps.
fn build_x_labels(
    points: &[market_core::domain::PricePoint],
    count: usize,
    range: ChartRange,
    theme: &'static Theme,
) -> Vec<Span<'static>> {
    if count < 2 || points.len() < 2 {
        return vec![Span::raw("")];
    }
    let segments = count.saturating_sub(1);
    let len = points.len();
    (0..count)
        .map(|i| {
            let idx = if i == segments { len - 1 } else { i * len / segments };
            let label = points
                .get(idx)
                .and_then(|p| p.timestamp)
                .map_or_else(String::new, |ts| format_timestamp(ts, range));
            let color = if i == 0 || i == segments {
                theme.muted
            } else {
                theme.fg
            };
            Span::styled(label, Style::default().fg(color))
        })
        .collect()
}

/// Format a Unix timestamp for the x-axis based on the chart range.
fn format_timestamp(ts: i64, range: ChartRange) -> String {
    let dt: DateTime<Utc> = DateTime::from_timestamp(ts, 0).unwrap_or_default();
    let local = dt.with_timezone(&chrono::Local);
    match range {
        ChartRange::Day1 => local.format("%H:%M").to_string(),
        ChartRange::Day5 => local.format("%a %H:%M").to_string(),
        ChartRange::Month1 => local.format("%b %d").to_string(),
        ChartRange::Month3 | ChartRange::Month6 | ChartRange::Ytd => {
            local.format("%b %d").to_string()
        }
        ChartRange::Year1 | ChartRange::Year5 => local.format("%b '%y").to_string(),
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

/// A single dotted grid line (horizontal or vertical).
#[derive(Clone, Copy)]
struct GridLine {
    /// For horizontal: x range. For vertical: y range (top, bottom).
    x_start: u16,
    x_end: u16,
    /// For horizontal: the row. For vertical: the column.
    y: u16,
    style: Style,
    vertical: bool,
}

impl Widget for GridLine {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        let buf_area = buf.area();
        if self.vertical {
            // x_start = top row, x_end = bottom row, y = column
            let col = self.y;
            if col < buf_area.x || col >= buf_area.right() {
                return;
            }
            let top = self.x_start.max(buf_area.y);
            let bottom = self.x_end.min(buf_area.bottom());
            for row in top..bottom {
                if (row.wrapping_sub(top)) % 3 == 0
                    && let Some(cell) = buf.cell_mut((col, row))
                    && cell.symbol() == " "
                {
                    cell.set_symbol("·").set_style(self.style);
                }
            }
        } else {
            if self.y < buf_area.y || self.y >= buf_area.bottom() {
                return;
            }
            let start = self.x_start.max(buf_area.x);
            let end = self.x_end.min(buf_area.right());
            for x in start..end {
                if (x.wrapping_sub(start)) % 3 == 0
                    && let Some(cell) = buf.cell_mut((x, self.y))
                    && cell.symbol() == " "
                {
                    cell.set_symbol("·").set_style(self.style);
                }
            }
        }
    }
}
