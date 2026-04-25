//! Performance chart overlay — renders a full-screen line chart with
//! selectable time range tabs and a bottom panel for news / SEC filings.

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Chart, Clear, Dataset, GraphType, List, ListItem, Paragraph, Widget,
};

use market_core::domain::ChartRange;
use market_core::theme::Theme;

use crate::app::{App, ChartTab};

/// Split an area into two horizontal columns using a 0.0–1.0 ratio.
/// Returns `(left, right)` rects. Ratio is clamped to 20–80%.
fn split_detail_cols(ratio: f64, area: Rect) -> (Rect, Rect) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let left_w = (ratio * f64::from(area.width)).round() as u16;
    let left_w = left_w.clamp(1, area.width.saturating_sub(1));
    let left = Rect::new(area.x, area.y, left_w, area.height);
    let right = Rect::new(
        area.x + left_w,
        area.y,
        area.width.saturating_sub(left_w),
        area.height,
    );
    (left, right)
}

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

    let title = format!(" {} — Performance ", app.chart_symbol);

    // Outer block.
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

    let has_panel = app.chart_tab != ChartTab::Chart;

    // Split inner area: chart section + optional bottom panel.
    let inner = chart_area.inner(Margin::new(1, 1));
    let sections = if has_panel {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(inner)
    };

    // Draw chart in top section.
    draw_chart_section(frame, app, theme, sections[0]);

    // Draw bottom panel if a tab is active.
    if has_panel && sections.len() > 1 {
        draw_bottom_panel(frame, app, theme, sections[1]);
    }
}

/// Draw the chart section: range tabs, chart, help bar.
fn draw_chart_section(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
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

    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // range tabs
            Constraint::Min(3),    // chart
            Constraint::Length(1), // help bar
        ])
        .split(area);

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
        let chart_area_padded = inner_layout[1].inner(Margin::new(1, 0));
        draw_line_chart(frame, app, theme, chart_area_padded);
    }

    // Help bar with panel tab indicators.
    let help = Paragraph::new(Line::from(build_help_spans(app, theme)))
        .alignment(Alignment::Center)
        .style(Style::default().bg(theme.chart_bg));
    frame.render_widget(help, inner_layout[2]);
}

/// Build help bar spans showing available keys and panel tab indicators.
fn build_help_spans(app: &App, theme: &'static Theme) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled("Tab", Style::default().fg(theme.accent)),
        Span::styled(" panel  ", Style::default().fg(theme.muted)),
    ];

    if app.chart_tab == ChartTab::Chart {
        spans.extend([
            Span::styled("1-8", Style::default().fg(theme.accent)),
            Span::styled(" range  ", Style::default().fg(theme.muted)),
            Span::styled("←/→", Style::default().fg(theme.accent)),
            Span::styled(" prev/next  ", Style::default().fg(theme.muted)),
        ]);
    } else {
        spans.extend([
            Span::styled("j/k", Style::default().fg(theme.accent)),
            Span::styled(" navigate  ", Style::default().fg(theme.muted)),
        ]);
        if app.chart_tab == ChartTab::News {
            spans.extend([
                Span::styled("Enter", Style::default().fg(theme.accent)),
                Span::styled(" summary  ", Style::default().fg(theme.muted)),
            ]);
        }
    }

    spans.extend([
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::styled(" close  ", Style::default().fg(theme.muted)),
    ]);

    // Tab indicators.
    spans.push(Span::raw(" "));
    for tab in [ChartTab::Chart, ChartTab::News, ChartTab::SecFilings] {
        let label = match tab {
            ChartTab::Chart => "Chart",
            ChartTab::News => "News",
            ChartTab::SecFilings => "SEC",
        };
        if tab == app.chart_tab {
            spans.push(Span::styled(
                format!(" {label} "),
                Style::default()
                    .fg(theme.chart_bg)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {label} "),
                Style::default().fg(theme.muted),
            ));
        }
    }

    spans
}

/// Draw the bottom panel (News or SEC Filings).
fn draw_bottom_panel(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    match app.chart_tab {
        ChartTab::News => draw_news_panel(frame, app, theme, area),
        ChartTab::SecFilings => draw_sec_panel(frame, app, theme, area),
        ChartTab::Chart => {} // unreachable — only called when has_panel
    }
}

/// Draw the news headlines panel with optional inline summary.
fn draw_news_panel(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    if app.chart_news.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" News ")
            .title_style(title_style)
            .style(Style::default().bg(theme.chart_bg));
        let msg = Paragraph::new("Loading news...")
            .alignment(Alignment::Center)
            .block(block)
            .style(Style::default().fg(theme.muted).bg(theme.chart_bg));
        frame.render_widget(msg, area);
        return;
    }

    // If summary is open, split: list on left, summary on right.
    if app.chart_news_summary_open {
        let (left, right) = split_detail_cols(app.chart_detail_split, area);
        draw_news_list(frame, app, theme, left);
        draw_news_summary(frame, app, theme, right);
    } else {
        draw_news_list(frame, app, theme, area);
    }
}

/// Draw the news headline list.
fn draw_news_list(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let items: Vec<ListItem> = app
        .chart_news
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let age = item.publish_time.map_or_else(String::new, |ts| {
                let elapsed = Utc::now().timestamp() - ts;
                if elapsed < 3600 {
                    format!(" {}m", elapsed / 60)
                } else if elapsed < 86400 {
                    format!(" {}h", elapsed / 3600)
                } else {
                    format!(" {}d", elapsed / 86400)
                }
            });

            let style = if i == app.chart_news_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            let prefix = if i == app.chart_news_selected {
                "▶ "
            } else {
                "  "
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(item.title.clone(), style),
                Span::styled(
                    format!("  — {}{age}", item.publisher),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(format!(" News ({}) ", app.chart_news.len()))
        .title_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.chart_bg));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Draw inline summary/content for the selected news item.
fn draw_news_summary(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let item = app.chart_news.get(app.chart_news_selected);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Article Detail (j/k=scroll) ")
        .title_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.chart_bg));

    let text = if let Some(item) = item {
        if app.chart_news_content_loading {
            format!("{}\n\n{}\n\nLoading article...", item.title, item.publisher)
        } else if let Some(content) = &app.chart_news_content {
            format!("{}\n\n{}\n\n{content}", item.title, item.publisher)
        } else {
            format!(
                "{}\n\n{}\n\nNo content available.\n{}",
                item.title, item.publisher, item.link
            )
        }
    } else {
        "No article selected.".to_string()
    };

    let para = Paragraph::new(text)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true })
        .scroll((u16::try_from(app.chart_news_scroll).unwrap_or(u16::MAX), 0))
        .style(Style::default().fg(theme.fg).bg(theme.chart_bg));
    frame.render_widget(para, area);
}

/// Draw SEC EDGAR filings panel.
fn draw_sec_panel(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    if app.chart_sec_filings.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" SEC Filings ")
            .title_style(title_style)
            .style(Style::default().bg(theme.chart_bg));
        let msg = Paragraph::new("Loading SEC filings...")
            .alignment(Alignment::Center)
            .block(block)
            .style(Style::default().fg(theme.muted).bg(theme.chart_bg));
        frame.render_widget(msg, area);
        return;
    }

    // If detail is open, split horizontally: list | detail.
    if app.chart_sec_detail_open {
        let (left, right) = split_detail_cols(app.chart_detail_split, area);
        draw_sec_list(frame, app, theme, left);
        draw_sec_detail(frame, app, theme, right);
    } else {
        draw_sec_list(frame, app, theme, area);
    }
}

/// Draw the SEC filings list (left side when detail is open).
fn draw_sec_list(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let items: Vec<ListItem> = app
        .chart_sec_filings
        .iter()
        .enumerate()
        .map(|(i, filing)| {
            let style = if i == app.chart_sec_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            let prefix = if i == app.chart_sec_selected {
                "▶ "
            } else {
                "  "
            };

            // Color-code by filing type.
            let form_color = match filing.form_type.as_str() {
                "10-K" => Color::Rgb(181, 234, 215), // green pastel
                "10-Q" => Color::Rgb(155, 207, 232), // blue pastel
                "8-K" => Color::Rgb(255, 218, 193),  // orange pastel
                "4" => Color::Rgb(199, 178, 232),    // purple pastel
                _ => theme.fg,
            };

            let desc = if filing.description.is_empty() {
                String::new()
            } else {
                format!("  {}", filing.description)
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(
                    format!("{:<6}", filing.form_type),
                    Style::default().fg(form_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}  ", filing.filed_date),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(desc, style),
            ]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(format!(" SEC Filings ({}) ", app.chart_sec_filings.len()))
        .title_style(title_style)
        .style(Style::default().bg(theme.chart_bg));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Draw the SEC filing detail panel (right side).
fn draw_sec_detail(frame: &mut Frame, app: &App, theme: &'static Theme, area: Rect) {
    let filing = app.chart_sec_filings.get(app.chart_sec_selected);

    let title = if app.chart_sec_content_loading {
        " Filing Detail (loading...) "
    } else {
        " Filing Detail (o=open, j/k=scroll) "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(title)
        .title_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.chart_bg));

    let text = if let Some(content) = &app.chart_sec_content {
        // Show header + fetched content.
        if let Some(f) = filing {
            let form_label = form_type_label(&f.form_type);
            format!(
                "{} — {} — {}\n\n{}",
                form_label, f.filed_date, f.accession, content
            )
        } else {
            content.clone()
        }
    } else if app.chart_sec_content_loading {
        "Fetching filing content...".to_string()
    } else if let Some(f) = filing {
        let form_label = form_type_label(&f.form_type);
        let desc = if f.description.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", f.description)
        };
        format!(
            "{}\n\nFiled: {}\nAccession: {}{}\n\nPress Enter to load content.\nPress o to open in browser.",
            form_label, f.filed_date, f.accession, desc
        )
    } else {
        "No filing selected.".to_string()
    };

    let para = Paragraph::new(text)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true })
        .scroll((u16::try_from(app.chart_sec_scroll).unwrap_or(u16::MAX), 0))
        .style(Style::default().fg(theme.fg).bg(theme.chart_bg));
    frame.render_widget(para, area);
}

/// Map a form type code to a human-readable label.
fn form_type_label(form_type: &str) -> &str {
    match form_type {
        "10-K" => "Annual Report (10-K)",
        "10-Q" => "Quarterly Report (10-Q)",
        "8-K" => "Current Report (8-K)",
        "4" => "Insider Transaction (Form 4)",
        "S-1" => "Registration Statement (S-1)",
        "DEF 14A" => "Proxy Statement (DEF 14A)",
        other => other,
    }
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
        x_start: graph_top, // reuse fields: x_start=top, x_end=bottom for vertical
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
            let idx = if i == segments {
                len.saturating_sub(1)
            } else {
                i * len / segments
            };
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
