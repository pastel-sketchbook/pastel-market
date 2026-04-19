# Pastel Market

A terminal dashboard (TUI) that combines real-time market monitoring with
fundamental stock screening and earnings intelligence. Built with
[ratatui](https://ratatui.rs) and [crossterm](https://docs.rs/crossterm).

```
Monitor markets. Screen stocks. Gauge earnings. Inspect quality. Signal conviction. Fast.
```

## Features

- **Watchlist** — live quotes from Yahoo Finance with sparklines, 52-week
  ranges, sector tags, and heatmap-ranked price changes
- **Scanners** — day gainers, day losers, most active, trending tickers, and
  Finviz fundamental screener
- **Quality Control** — per-stock 5-point inspection checklist with 3 items
  auto-populated from live data (insider ownership, sector heat, historical
  earnings beats)
- **Conviction signaling** — any stock achieving 5/5 QC score triggers
  `HIGH CONVICTION - READY` in the header
- **Earnings intelligence** — whisper numbers, implied volatility, grades, and
  report timing via Earnings Whispers (feature-gated)
- **16 themes** — 8 dark + 8 light, cycled with `t`, persisted across sessions
- **Non-blocking UI** — all HTTP fetches run on background threads; the event
  loop never stalls

## Requirements

- Rust toolchain (MSRV 1.95.0, edition 2024)
- Terminal emulator supporting ANSI colors and alternate screen
- Internet access for Yahoo Finance and Finviz (falls back to mock data)

## Build and run

```sh
cargo run --release
```

Or with [Task](https://taskfile.dev):

```sh
task run
```

## Keybindings

| Key | Action |
|---|---|
| `q` / `Esc` | Quit |
| `Tab` / `BackTab` | Cycle view: Watchlist → Scanner → QC |
| `j` / `k` | Navigate down / up |
| `gg` / `G` | Jump to first / last row |
| `h` / `l` | Switch focus (QC view: table ↔ checklist) |
| `Space` / `Enter` | Toggle QC item (QC view) |
| `r` | Refresh data |
| `s` | Cycle sort mode |
| `f` | Cycle filter mode |
| `t` | Cycle theme |
| `a` | Add symbol (Watchlist view) |
| `d` | Delete symbol (Watchlist view) |
| `n` | Toggle news panel |
| `1`–`5` | Select scanner (Scanner view) |

## Architecture

```
pastel-market/
├── crates/
│   ├── market-core/       Shared types, HTTP, config, logging, themes
│   ├── finviz-scraper/    Screener, insider detail, sector ETF performance
│   ├── yahoo-provider/    Cookie+crumb auth, QuoteProvider trait
│   └── whispers/          Earnings Whispers (feature-gated)
├── src/
│   ├── main.rs            Terminal lifecycle + event loop
│   ├── app.rs             App state, key handlers, data refresh
│   ├── event.rs           EventHandler background thread
│   ├── worker.rs          Background HTTP fetch worker
│   └── ui/                Rendering modules
└── docs/rationale/        Design decision records
```

## License

MIT License. See [LICENSE](LICENSE).
