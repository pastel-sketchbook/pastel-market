//! Quality Control view: 3-column middle panels + screener table.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Padding, Row, Table};

use crate::app::{App, Focus};
use market_core::theme::Theme;

use super::helpers::{active_color, highlight_style, stripe_style};

/// Render the 3-column middle section: Finviz filters | Whispers | QC checklist.
pub fn draw_qc_middle(frame: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let mid_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(40),
        ])
        .split(area);

    draw_finviz_panel(frame, app, theme, title_style, mid_chunks[0]);
    draw_whisper_panel(frame, app, theme, title_style, mid_chunks[1]);
    draw_qc_checklist(frame, app, theme, title_style, mid_chunks[2]);
}

/// Finviz coarse filter panel — shows filter criteria with live match count.
fn draw_finviz_panel(frame: &mut Frame, app: &App, theme: &Theme, title_style: Style, area: Rect) {
    let count = app.screener_results.len();
    let mut items = vec![
        ListItem::new("Market Cap > $300M"),
        ListItem::new("P/E < 25"),
        ListItem::new("EPS Growth > 15%"),
        ListItem::new("Price > SMA50 & SMA200"),
        ListItem::new("Beta < 1.5"),
        ListItem::new("Avg Volume > 500K"),
    ];

    // Append a live match summary when screener data is present.
    if count > 0 {
        items.push(ListItem::new(""));
        items.push(
            ListItem::new(format!(" {count} stocks pass filters")).style(
                Style::default()
                    .fg(theme.status)
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }

    let list = List::new(items).style(Style::default().fg(theme.fg)).block(
        Block::default()
            .title(" A: COARSE FILTER ")
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(list, area);
}

/// Earnings Whispers precision gauge panel.
fn draw_whisper_panel(frame: &mut Frame, app: &App, theme: &Theme, title_style: Style, area: Rect) {
    let items: Vec<ListItem> = if let Some(ticker) = app.selected_screener_ticker() {
        if let Some(whisper) = app.whisper_cache.get(&ticker) {
            let mut lines = Vec::new();
            if let Some(date) = &whisper.earnings_date {
                lines.push(format!("Date: {date}"));
            }
            if let Some(w) = &whisper.whisper {
                lines.push(format!("Whisper: ${w}"));
            }
            if let Some(consensus) = &whisper.consensus {
                lines.push(format!("Consensus: ${consensus}"));
            }
            if let Some(vol) = &whisper.volatility {
                lines.push(format!("Volatility: {vol}"));
            }
            if let Some(grade) = &whisper.grade {
                lines.push(format!("Grade: {grade}"));
            }
            if let Some(score) = &whisper.score {
                lines.push(format!("Score: {score}"));
            }
            if let Some(sentiment) = &whisper.sentiment {
                lines.push(format!("Sentiment: {sentiment}"));
            }
            if let Some(lifecycle) = &whisper.lifecycle {
                lines.push(format!("Life Cycle: {lifecycle}"));
            }
            if let Some(beats) = whisper.past_beats {
                lines.push(format!(
                    "History: {}",
                    if beats { "Beats" } else { "Misses" }
                ));
            }
            if lines.is_empty() {
                vec![ListItem::new(" No earnings data ").style(Style::default().fg(theme.muted))]
            } else {
                lines
                    .into_iter()
                    .map(|l| ListItem::new(l).style(Style::default().fg(theme.fg)))
                    .collect()
            }
        } else {
            vec![ListItem::new(" Press 'r' to fetch ").style(Style::default().fg(theme.muted))]
        }
    } else {
        vec![ListItem::new(" (select a stock) ").style(Style::default().fg(theme.muted))]
    };

    let list = List::new(items).block(
        Block::default()
            .title(" B: PRECISION GAUGE ")
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(list, area);
}

/// Interactive QC checklist panel.
fn draw_qc_checklist(frame: &mut Frame, app: &App, theme: &Theme, title_style: Style, area: Rect) {
    let any_passed = app.any_fully_passed();
    let ac = active_color(any_passed, theme);

    let border_color = if app.focus == Focus::QcChecklist {
        ac
    } else {
        theme.border
    };

    let qc_title = match app.selected_screener_ticker() {
        Some(ticker) => format!(" C: QUALITY CONTROL : {ticker} "),
        None => " C: QUALITY CONTROL ".to_string(),
    };

    let qc_items: Vec<ListItem> = if let Some(ticker) = app.selected_screener_ticker() {
        let checks = app.qc_state.get(&ticker);
        app.qc_labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let checked = checks.is_some_and(|c| c.get(i).copied().unwrap_or(false));
                let auto_checked = app.is_auto_checked(&ticker, i);

                let prefix = if checked || auto_checked {
                    "[X]"
                } else {
                    "[ ]"
                };
                let auto_indicator = if auto_checked && !checked {
                    " \u{2713}"
                } else {
                    ""
                };

                let value_str = app.qc_inline_value(&ticker, i).unwrap_or_default();

                let style = if app.focus == Focus::QcChecklist && i == app.selected_qc {
                    let fg = if checked || auto_checked {
                        theme.status
                    } else {
                        theme.fg
                    };
                    Style::default()
                        .fg(fg)
                        .bg(theme.highlight_bg)
                        .add_modifier(Modifier::BOLD)
                } else if checked {
                    Style::default().fg(theme.status)
                } else if auto_checked {
                    Style::default().fg(theme.tag)
                } else {
                    Style::default().fg(theme.fg)
                };

                ListItem::new(format!("{prefix} {label}{auto_indicator}{value_str}")).style(style)
            })
            .collect()
    } else {
        vec![ListItem::new("(select a stock)").style(Style::default().fg(theme.muted))]
    };

    let list = List::new(qc_items).block(
        Block::default()
            .title(qc_title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(list, area);
}

/// Render the screener results table (bottom of QC view).
#[allow(clippy::too_many_lines)]
pub fn draw_screener_table(frame: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let any_passed = app.any_fully_passed();
    let ac = active_color(any_passed, theme);
    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);

    let border_color = if app.focus == Focus::Table {
        ac
    } else {
        theme.border
    };

    if app.screener_results.is_empty() {
        let block = Block::default()
            .title(" SCREENER RESULTS ")
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .padding(Padding::horizontal(1));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let msg_text = if app.loading {
            let spinner = super::helpers::spinner_frame(app.tick);
            format!("{spinner} Loading screener data...")
        } else {
            "No screener data \u{2014} press [r] to refresh".to_string()
        };

        let msg = ratatui::widgets::Paragraph::new(msg_text)
            .style(Style::default().fg(theme.muted))
            .alignment(ratatui::layout::Alignment::Center);
        let mid_y = inner.y + inner.height / 2;
        if mid_y > inner.y && inner.width > 4 {
            let msg_area = ratatui::layout::Rect::new(inner.x, mid_y, inner.width, 1);
            frame.render_widget(msg, msg_area);
        }
        return;
    }

    let header = Row::new(vec![
        Cell::from("#"),
        Cell::from("TICKER"),
        Cell::from("SECTOR"),
        Cell::from("MKT CAP"),
        Cell::from("P/E"),
        Cell::from("PRICE"),
        Cell::from("CHANGE"),
        Cell::from("SCORE"),
        Cell::from("RATING"),
        Cell::from("STATUS"),
    ])
    .style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .screener_results
        .iter()
        .enumerate()
        .map(|(rank, r)| {
            let score = app.qc_score(&r.ticker);
            let total = app.qc_labels.len();
            let all_passed = app.all_qc_passed_for(&r.ticker);

            let change_color = if r.change.starts_with('-') {
                theme.error
            } else {
                theme.status
            };

            let score_style = if all_passed {
                Style::default()
                    .fg(theme.status)
                    .add_modifier(Modifier::BOLD)
            } else if score > 0 {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.muted)
            };

            let status_cell = if all_passed {
                Cell::from("EXECUTE").style(
                    Style::default()
                        .fg(theme.status)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Cell::from("PENDING").style(Style::default().fg(theme.muted))
            };

            let report = app.analyze_stock(&r.ticker);
            let rating_cell = {
                let (cr, cg, cb) = report.rating.color_rgb();
                Cell::from(report.rating.label().to_string()).style(
                    Style::default()
                        .fg(ratatui::style::Color::Rgb(cr, cg, cb))
                        .add_modifier(Modifier::BOLD),
                )
            };

            Row::new(vec![
                Cell::from(format!("{}", rank + 1)),
                Cell::from(r.ticker.clone()),
                Cell::from(r.sector.clone()),
                Cell::from(r.market_cap.clone()),
                Cell::from(r.pe.clone()),
                Cell::from(r.price.clone()),
                Cell::from(r.change.clone()).style(Style::default().fg(change_color)),
                Cell::from(format!("{score}/{total}")).style(score_style),
                rating_cell,
                status_cell,
            ])
            .style(stripe_style(rank, theme))
        })
        .collect();

    let widths = [
        Constraint::Length(4),
        Constraint::Percentage(10),
        Constraint::Percentage(14),
        Constraint::Percentage(10),
        Constraint::Percentage(6),
        Constraint::Percentage(10),
        Constraint::Percentage(10),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Percentage(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(" SCREENER RESULTS (live) ")
                .title_style(title_style)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .padding(Padding::horizontal(1)),
        )
        .row_highlight_style(highlight_style(theme));

    frame.render_stateful_widget(
        table,
        area,
        &mut ratatui::widgets::TableState::default().with_selected(Some(
            app.watchlist
                .selected()
                .min(app.screener_results.len().saturating_sub(1)),
        )),
    );
}
