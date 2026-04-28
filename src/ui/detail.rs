//! Detail pane, top movers, and news panel rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use market_core::domain::{Mover, NewsItem, TopMovers};
use market_core::theme::Theme;

use super::helpers::format_volume;

/// Render the detail pane for the selected quote.
#[allow(clippy::too_many_lines)]
pub fn draw_detail(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let label_style = Style::default().fg(theme.accent);

    let content = if let Some(q) = app.watchlist.selected_quote() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Open: ", label_style),
                Span::raw(format!("{:.2}", q.regular_market_open)),
                Span::raw("  "),
                Span::styled("Prev Close: ", label_style),
                Span::raw(format!("{:.2}", q.regular_market_previous_close)),
            ]),
            Line::from(vec![
                Span::styled("Day H/L: ", label_style),
                Span::raw(format!("{:.2}", q.regular_market_day_high)),
                Span::raw(" / "),
                Span::raw(format!("{:.2}", q.regular_market_day_low)),
                Span::raw("  "),
                Span::styled("52W H/L: ", label_style),
                Span::raw(format!("{:.2}", q.fifty_two_week_high)),
                Span::raw(" / "),
                Span::raw(format!("{:.2}", q.fifty_two_week_low)),
            ]),
            Line::from(vec![
                Span::styled("Volume: ", label_style),
                Span::raw(format_volume(q.regular_market_volume)),
            ]),
        ];

        // Pre/Post market prices when available.
        if let (Some(price), Some(chg_pct)) = (q.pre_market_price, q.pre_market_change_percent) {
            let color = if chg_pct >= 0.0 {
                theme.gain
            } else {
                theme.loss
            };
            lines.push(Line::from(vec![
                Span::styled("Pre-Mkt: ", label_style),
                Span::raw(format!("{price:.2}")),
                Span::styled(format!(" ({chg_pct:+.2}%)"), Style::default().fg(color)),
            ]));
        }
        if let (Some(price), Some(chg_pct)) = (q.post_market_price, q.post_market_change_percent) {
            let color = if chg_pct >= 0.0 {
                theme.gain
            } else {
                theme.loss
            };
            lines.push(Line::from(vec![
                Span::styled("After-Hrs: ", label_style),
                Span::raw(format!("{price:.2}")),
                Span::styled(format!(" ({chg_pct:+.2}%)"), Style::default().fg(color)),
            ]));
        }

        // 52-week range bar
        let range = q.fifty_two_week_high - q.fifty_two_week_low;
        if range > 0.0 {
            let position =
                ((q.regular_market_price - q.fifty_two_week_low) / range).clamp(0.0, 1.0);
            let bar_width: usize = 20;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let marker_pos = (position * bar_width as f64).round() as usize;
            let marker_pos = marker_pos.min(bar_width);

            let bar: String = (0..=bar_width)
                .map(|i| {
                    if i == marker_pos {
                        '\u{25c6}'
                    } else {
                        '\u{2500}'
                    }
                })
                .collect();

            let pct_of_range = position * 100.0;
            lines.push(Line::from(vec![
                Span::styled("52W: ", label_style),
                Span::raw(format!("{:.0} ", q.fifty_two_week_low)),
                Span::styled(
                    bar,
                    Style::default().fg(if pct_of_range >= 80.0 {
                        theme.status
                    } else if pct_of_range >= 50.0 {
                        theme.accent
                    } else {
                        theme.muted
                    }),
                ),
                Span::raw(format!(" {:.0}", q.fifty_two_week_high)),
                Span::styled(
                    format!(" ({pct_of_range:.0}%)"),
                    Style::default().fg(theme.fg),
                ),
            ]));
        }

        // Analyst Scores
        let report = app.analyze_stock(&q.symbol);
        lines.push(Line::from(vec![
            Span::styled("Analysis: ", label_style),
            Span::raw(format!(
                "Comp {} | Fund {} | Tech {} | Sent {} | Cat {}",
                report.composite,
                report.fundamentals,
                report.technical,
                report.sentiment,
                report.news_catalyst
            )),
        ]));

        lines
    } else {
        vec![Line::from("No symbol selected")]
    };

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let detail = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Details ")
            .title_style(title_style),
    );

    frame.render_widget(detail, area);
}

/// Render the top movers panel.
pub fn draw_top_movers(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let movers = TopMovers::from_quotes(app.watchlist.quotes(), 3);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let gainer_lines: Vec<Line> = movers
        .gainers
        .iter()
        .map(|m| Line::from(format_mover_spans(m, true, theme)))
        .collect();

    let gainers_block = Paragraph::new(gainer_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" \u{25b2} Gainers ")
            .title_style(title_style),
    );
    frame.render_widget(gainers_block, columns[0]);

    let loser_lines: Vec<Line> = movers
        .losers
        .iter()
        .map(|m| Line::from(format_mover_spans(m, false, theme)))
        .collect();

    let losers_block = Paragraph::new(loser_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" \u{25bc} Losers ")
            .title_style(title_style),
    );
    frame.render_widget(losers_block, columns[1]);
}

/// Format a single mover as styled spans.
fn format_mover_spans(mover: &Mover, is_gain: bool, theme: &Theme) -> Vec<Span<'static>> {
    let color = if is_gain { theme.gain } else { theme.loss };
    vec![
        Span::styled(
            format!(" {:<6}", mover.symbol),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("${:.2}", mover.price)),
        Span::styled(
            format!(" {:+.2}%", mover.change_percent),
            Style::default().fg(color),
        ),
    ]
}

/// Render the news headlines panel.
pub fn draw_news_panel(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let title = app
        .watchlist
        .selected_quote()
        .map_or_else(|| " News ".to_string(), |q| format!(" News: {} ", q.symbol));

    let lines: Vec<Line> = if app.news_headlines.is_empty() {
        vec![Line::from(Span::styled(
            "No headlines — press [n] to toggle",
            Style::default().fg(theme.muted),
        ))]
    } else {
        app.news_headlines
            .iter()
            .map(|item| Line::from(format_news_spans(item, theme)))
            .collect()
    };

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(title)
            .title_style(title_style),
    );

    frame.render_widget(panel, area);
}

/// Format a single news headline as styled spans.
fn format_news_spans(item: &NewsItem, theme: &Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(" {}", item.title),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" \u{2014} {}", item.publisher),
            Style::default().fg(theme.muted),
        ),
    ]
}
