# ROLES AND EXPERTISE

This codebase operates with two distinct but complementary roles:

## Implementor Role

You are a senior Rust engineer building a high-performance terminal dashboard (TUI) that combines real-time market monitoring with fundamental stock screening and earnings intelligence. You implement changes with attention to UI responsiveness, state management, and user experience.

**Responsibilities:**
- Write idiomatic Rust with proper error handling (`anyhow`/`thiserror`)
- Design clean module boundaries separating UI, state, and data layers
- Follow TDD principles: write tests alongside implementation
- Ensure the TUI event loop is non-blocking and responsive
- Handle terminal setup/teardown robustly (raw mode, alternate screen)

## Reviewer Role

You are a senior engineer who evaluates changes for quality, correctness, and adherence to Rust best practices.

**Responsibilities:**
- Verify error handling is comprehensive (no `unwrap()` in non-test code; `.expect()` only with safety comment)
- Check that the event loop doesn't block on I/O or computation
- Ensure UI layout adapts gracefully to different terminal sizes
- Validate state transitions are correct (QC checklist, conviction status, view modes)
- Run `cargo clippy --all-targets -- -D warnings` and `cargo test`

# SCOPE OF THIS REPOSITORY

This repository contains **Pastel Market**, a unified Rust terminal dashboard (TUI) that combines two predecessor projects — **Pastel Picker** (stock screening + earnings intelligence) and **Reins Market** (real-time market monitoring) — into a single workspace. It:

- **Monitors** real-time stock prices via Yahoo Finance (cookie+crumb auth, sparklines, 52-week ranges, intraday data)
- **Screens** stocks via live Finviz screener scraping (market cap, P/E, EPS growth, technical trend, beta, volume) with graceful fallback to mock data
- **Gauges** earnings precision via Earnings Whispers data (whisper numbers, implied volatility, grades, report timing)
- **Evaluates** candidates through per-stock Quality Control checklists (insider ownership, sector heat, news catalysts, chart patterns, historical beats)
- **Ranks** stocks by QC score (descending) in the table, with `#` rank and `SCORE` columns
- **Tracks** watchlists with live quotes, top movers, sector performance, and market indices
- **Scans** via multiple scanner modes (day gainers/losers, most active, trending, fundamentals)
- **Signals** trade readiness via a reactive system: any stock achieving 5/5 QC score triggers `HIGH CONVICTION - READY`

**Lineage:**
- Pastel Picker v0.2.2 — screening, QC checklist, earnings whispers, conviction signaling
- Reins Market v0.3.4 — Yahoo Finance quotes, watchlist, scanners, heatmap, event system

**Runtime requirements:**
- Any OS with Rust toolchain (MSRV 1.95.0, edition 2024)
- A terminal emulator supporting ANSI colors and alternate screen
- `crossterm` and `ratatui` for terminal rendering
- Internet access for Yahoo Finance quotes and Finviz screener (falls back to mock data on failure)

# ARCHITECTURE

```
pastel-market/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── market-core/            # Shared types, traits, HTTP, config, logging, themes
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── domain.rs       # Quote, ScreenerResult, NewsItem, MarketStatus, etc.
│   │       ├── http.rs         # ureq client + retry/backoff
│   │       ├── config.rs       # Preferences (TOML) + Session (JSON) persistence
│   │       ├── theme.rs        # Unified 16+ semantic themes
│   │       └── logging.rs      # tracing + daily rotating file appender
│   │
│   ├── finviz-scraper/         # Finviz screener module (deduplicated)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── screener.rs     # Paginated fetch, parse_page, filter constants
│   │       ├── detail.rs       # Insider ownership, per-stock detail scraping
│   │       └── sector.rs       # ETF performance, sector heat mapping
│   │
│   ├── yahoo-provider/         # Yahoo Finance real-time data (from Reins Market)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs       # Cookie+crumb auth, QuoteProvider impl
│   │       ├── quotes.rs       # Quote + sparkline parsing
│   │       └── news.rs         # Yahoo news headlines
│   │
│   └── whispers/               # Earnings Whispers (from Pastel Picker)
│       └── src/
│           └── lib.rs          # Headless Chrome scraping (feature-gated)
│
├── src/                        # Main binary: unified TUI
│   ├── main.rs                 # Terminal lifecycle + event loop
│   ├── app.rs                  # Combined App state (watchlist + QC + scanners)
│   ├── event.rs                # EventHandler/EventSource (background thread + mpsc)
│   └── ui/
│       ├── mod.rs              # Top-level layout dispatcher
│       ├── header.rs           # Market status + conviction signal
│       ├── watchlist.rs        # Live quotes table with heatmap (from RM)
│       ├── scanner.rs          # Scanner views: gainers/losers/active/trending/fundamentals
│       ├── qc.rs               # QC checklist panel with auto-check (from PP)
│       ├── whispers.rs         # Earnings panel (from PP)
│       ├── news.rs             # News headlines panel
│       ├── movers.rs           # Top movers + heatmap ranking (from RM)
│       ├── detail.rs           # Per-stock detail pane
│       └── footer.rs           # Keybindings bar with theme name
│
├── data/
│   └── mock.json               # Fallback data for offline/failure modes
├── tests/
│   ├── fixture_parsing.rs      # Integration tests for HTML parsers
│   └── fixtures/               # Captured HTML pages for deterministic testing
├── Taskfile.yml                # Task runner: build, check, test, release
└── AGENTS.md                   # This file
```

**Core Components:**

| Component | Origin | Purpose |
|---|---|---|
| Yahoo Provider | Reins Market | Real-time quotes, sparklines, market indices, sector data |
| Finviz Scraper | Both (deduplicated) | Coarse noise filter: fundamental metrics + technical parameters |
| Earnings Whispers | Pastel Picker | Precision calibration: whisper numbers, IV, grades |
| Quality Control | Pastel Picker | Per-stock 5-point inspection gate with ranking |
| Watchlist + Scanners | Reins Market | Live monitoring, top movers, multiple scanner modes |
| Event System | Reins Market | Background thread with tick-based auto-refresh |

**Data Flow:**
1. On startup, the event handler thread begins polling for keyboard/tick events
2. Initial data fetch: Yahoo Finance quotes for watchlist + market indices, Finviz screener results
3. **Yahoo Provider** delivers real-time quotes with sparklines, 52-week ranges, volume
4. **Finviz Scraper** applies coarse filters: market cap >$300M, P/E <25, EPS growth >15%, Price > SMA50 & SMA200, Beta <1.5, Avg Vol >500K
5. **Earnings Whispers** applies precision filters: Whisper > Consensus, Grade > B-, High IV (>5% expected move)
6. **Quality Control** gate requires verification of 5 items per stock (insider ownership, sector heat, news catalysts, chart validation, historical beats) — 3 items auto-check from live data
7. When all QC items pass for a stock, status changes from `PENDING` to `EXECUTE`; if any stock reaches 5/5, the header shows `HIGH CONVICTION - READY`
8. Auto-refresh on tick (30s active market, 5min heartbeat when closed)
9. Manual refresh with `r` key triggers non-blocking background fetch

**State Machine:**
```
ANALYSIS IN PROGRESS  ──[any stock 5/5 QC]──>  HIGH CONVICTION - READY
     (Cyan theme)                                    (Green theme)

Per stock:  PENDING  ──[all 5 QC items checked]──>  EXECUTE

View modes:  Watchlist ←─Tab─→ Scanner ←─Tab─→ QualityControl
```

# CORE DEVELOPMENT PRINCIPLES

- **No Panics**: Never use `unwrap()` in non-test code. Use `?` with `anyhow::Context`. `.expect()` is permitted only when the invariant is logically guaranteed, with a safety comment explaining why.
- **Error Messages**: Provide actionable error messages with context about what went wrong.
- **Terminal Safety**: Always restore terminal state on exit (disable raw mode, leave alternate screen, show cursor) even on error paths.
- **Responsive UI**: The event loop must never block. Use the `EventHandler` background thread with `mpsc` channels.
- **Testing**: Unit tests for state transitions, filter logic, QC checklist behavior, and provider parsing. Snapshot tests for UI layout. Mock providers for deterministic testing.
- **Clippy Pedantic**: `#[warn(clippy::pedantic)]` enabled project-wide.

# COMMIT CONVENTIONS

Use the following prefixes:
- `feat`: New feature or UI component
- `fix`: Bug fix
- `refactor`: Code improvement without behavior change
- `test`: Adding or improving tests
- `docs`: Documentation changes
- `chore`: Tooling, dependencies, configuration

# TASK NAMING CONVENTION

Use colon (`:`) as a separator in task names:
- `build:release`
- `test:unit`
- `check:all`
- `run`

# RUST-SPECIFIC GUIDELINES

## Error Handling
- Use `anyhow::Result` for application-level errors
- Use `thiserror` for library-level error types in workspace crates if needed
- Always add `.context()` or `.with_context()` for actionable error messages
- Return `Result` from all public functions

## HTTP Stack
- Use `ureq` (not reqwest) — lighter weight, no tokio dependency, cookie support
- Shared retry logic with exponential backoff (3 attempts, 1s/2s/4s) in `market-core::http`
- All HTTP calls go through `call_with_retry()` for consistent error handling

## TUI Architecture
- Use `ratatui` for terminal rendering with `crossterm` backend
- Separate state (`App` struct) from rendering (`ui/` modules)
- Use `Layout` with `Constraint` for responsive design
- Keep the event loop in `main.rs`, UI rendering in `ui/`
- Use `Block`, `List`, `Table`, and `Paragraph` widgets

## Event System
- `EventHandler` runs a dedicated background thread polling crossterm events
- `Event` enum: `Key(KeyEvent)` | `Tick` (250ms interval)
- `EventSource` trait enables test mocking with `FakeSource`
- `on_tick()` handles market-aware auto-refresh (30s active, 5min heartbeat)

## App State Design
- `App` struct holds all mutable state, combining both predecessors:
  - **From Reins Market**: `watchlist` (Watchlist), `quotes` cache, `sort_mode` (SortMode), `filter_mode` (FilterMode), `view_mode` (ViewMode), `scanner_list` (ScannerList), `top_movers`, `news`, `input_mode`, `market_status`
  - **From Pastel Picker**: `focus` (Focus), `qc_labels`, `qc_state` (HashMap per-stock), `selected_qc`, `whisper_cache`, `insider_ownership`, `sector_heat`, `past_beats`, `conviction_status`
- `ViewMode` enum: `Watchlist` | `Scanner` | `QualityControl`
- `Focus` enum controls which panel receives keyboard input within a view
- `QuoteProvider` trait abstracts Yahoo Finance for testability
- Navigation is view-mode-aware and focus-aware

## Keyboard Controls
- `q` / `Esc`: Quit
- `?`: Toggle help overlay
- `Tab` / `BackTab`: Cycle view modes (Watchlist → Scanner → QC)
- `j` / `Down`: Navigate down in focused panel
- `k` / `Up`: Navigate up in focused panel
- `gg`: Jump to first row
- `G`: Jump to last row
- `Space` / `Enter`: Toggle selected QC item (QC view) / Open chart (Watchlist)
- `r`: Manual refresh (non-blocking)
- `s`: Cycle sort mode
- `f`: Cycle filter mode
- `t`: Cycle theme
- `a`: Add symbol to watchlist (Watchlist view)
- `d`: Delete symbol from watchlist
- `y`: Copy selected symbol data to clipboard
- `[` / `]`: Previous / next watchlist tab
- `n`: Toggle news panel
- `1`-`5`: Select scanner list (Scanner view)
- Mouse scroll: navigate tables

## Terminal Lifecycle
- `enable_raw_mode()` + `EnterAlternateScreen` on startup
- `disable_raw_mode()` + `LeaveAlternateScreen` + `show_cursor()` on exit
- Restore terminal state even when returning an error

# DESIGN PATTERNS

## Applied Patterns

- **Workspace crates**: Shared logic extracted into `market-core`, `finviz-scraper`, `yahoo-provider`, `whispers` — clean dependency boundaries, independent testing.
- **Trait abstraction**: `QuoteProvider` trait for Yahoo Finance, `EventSource` trait for event polling — both mockable for deterministic tests.
- **Model-View separation**: `App` struct (model) is separate from `ui/` modules (view). The event loop in `main.rs` acts as controller.
- **Per-stock QC with ranking**: Each stock has independent QC state in a `HashMap<String, Vec<bool>>`. Stocks sorted by QC score descending. QC panel is contextual.
- **Auto-check from live data**: 3 of 5 QC items auto-populate from provider data (insider ownership, sector heat, historical beats).
- **Focus management**: `Focus` enum controls which panel receives input within a view mode. Focused panel gets `active_color` border.
- **Background fetch via mpsc**: Background thread handles all HTTP fetches. Main loop drains results via `try_recv()`.
- **Market-aware auto-refresh**: `on_tick()` refreshes every 30s during market hours, 5min heartbeat otherwise.
- **Heatmap ranking**: Normalized intensity colors for price changes across the watchlist.
- **Style propagation**: `active_color` computed from `any_fully_passed()` cascades to header, borders, and cells.

# CODE REVIEW CHECKLIST

- Does the code handle errors without panicking?
- Is the terminal properly restored on all exit paths (including error)?
- Does the event loop remain non-blocking?
- Does the layout degrade gracefully in small terminals?
- Does `cargo clippy --all-targets -- -D warnings` pass?
- Does `cargo test` pass?
- Are new UI components covered by tests?
- Are keyboard controls consistent and documented?
- Does the QC state machine transition correctly?
- Are workspace crate boundaries clean (no circular deps)?
- Does the `QuoteProvider` trait cover the new functionality?

# OUT OF SCOPE / ANTI-PATTERNS

- GUI or web interface (this is a terminal TUI)
- Automated trade execution (display-only tool)
- Database persistence (in-memory state + file-based config only)
- Non-ANSI terminal support
- `reqwest` or `tokio` (use `ureq` for all HTTP)
- `unwrap()` in non-test code

# SUMMARY MANTRA

Monitor markets. Screen stocks. Gauge earnings. Inspect quality. Signal conviction. Fast.
