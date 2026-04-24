//! Pastel Market — unified terminal dashboard for real-time market monitoring
//! and fundamental stock screening with earnings intelligence.

mod app;
mod event;
mod ui;
mod worker;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::KeyEventKind;
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;
use tracing::info;

use app::App;
use event::{Event, EventHandler};

/// UI tick interval (250ms) — controls draw/drain rate.
/// Actual data refresh is managed by the App's tick counter.
const TICK_RATE_MS: u64 = 250;

fn main() -> Result<()> {
    // File-based logging. Guard must stay alive until exit.
    let _log_guard = market_core::logging::init();
    info!("pastel-market starting");

    // Terminal setup.
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Build app state + initial data fetch.
    let mut app = App::new();
    app.refresh_quotes();

    // Event loop.
    let events = EventHandler::new(Duration::from_millis(TICK_RATE_MS));
    let res = run_loop(&mut terminal, &mut app, &events);

    // Restore terminal unconditionally.
    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    info!("pastel-market exiting");

    res
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events: &EventHandler,
) -> Result<()> {
    loop {
        // Drain any completed background fetches before drawing.
        app.drain_results();

        terminal.draw(|frame| ui::draw(frame, app))?;

        match events.next()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                app.handle_key(key);
            }
            Event::Tick => {
                app.on_tick();
            }
            Event::Resize(_, _) | Event::Key(_) => {}
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
