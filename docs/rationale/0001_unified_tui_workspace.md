# 0001 — Unified TUI Workspace

**Status:** Implemented  
**Date:** 2026-04-19  
**Commits:** `61384ae`, `4d3794e`

## Problem

Two separate Rust TUI projects existed side by side:

- **Pastel Picker** (v0.2.2) — stock screening via Finviz, earnings intelligence
  via Earnings Whispers, per-stock QC checklists, conviction signaling.
- **Reins Market** (v0.3.4) — real-time market monitoring via Yahoo Finance,
  watchlists, scanners, top movers, news, heatmap rendering.

Both shared significant overlap: Finviz scraping (~400 LOC nearly identical),
theme systems, terminal lifecycle management, and configuration persistence.
Running two separate binaries to get a complete view of both screening and
monitoring was impractical.

## Decision

Merge both projects into a single Cargo workspace called `pastel-market` with
clean crate boundaries:

| Crate | Origin | Purpose |
|---|---|---|
| `market-core` | Both (superset) | Shared types, traits, HTTP (ureq), config, logging, 16 themes |
| `finviz-scraper` | Both (deduplicated) | Screener, insider detail, sector ETF performance |
| `yahoo-provider` | Reins Market | Cookie+crumb auth, `QuoteProvider` trait, quote/sparkline/news parsing |
| `whispers` | Pastel Picker | Earnings Whispers scraping (feature-gated `chrome`) |
| Binary (`src/`) | New | Unified TUI: app state, event loop, UI modules |

### Key architectural choices

- **`ureq` over `reqwest`:** Lighter weight, no tokio dependency, built-in
  cookie jar. Both predecessors used it.
- **`EventHandler` thread pattern** (from RM): Dedicated background thread polls
  crossterm events + emits tick events via `mpsc`. Testable via `EventSource`
  trait with `FakeSource`.
- **`QuoteProvider` trait** (from RM): Abstracts Yahoo Finance for testability.
  Mock providers enable deterministic unit tests.
- **Model-View separation:** `App` struct (model) in `app.rs`, rendering in
  `ui/` modules. Event loop in `main.rs` acts as controller.
- **Edition 2024, MSRV 1.95.0, resolver 3:** Latest stable Rust features
  including let-chains.
- **Clippy pedantic** (`-D warnings`): Enforced project-wide.

### State machine

```
View modes:  Watchlist ←Tab→ Scanner ←Tab→ QualityControl
Per stock:   PENDING ──[all 5 QC items checked]──> EXECUTE
Header:      ANALYSIS IN PROGRESS ──[any 5/5]──> HIGH CONVICTION - READY
```

### What was unified

- **Domain types:** `Quote` (superset of both), `Watchlist`, `ScreenerResult`,
  `ScannerList`, `TopMovers`, `MarketStatus`, `ViewMode`, `SortMode`,
  `FilterMode`.
- **Config:** `Preferences` (theme, TOML), `Session` (symbols + modes, JSON),
  `QcSession` (per-stock QC state, JSON).
- **Themes:** 16 themes (8 dark + 8 light) with 18 semantic color fields.
- **QC system:** 5-point checklist per stock with 3 auto-check items from live
  data (insider ownership, sector heat, historical beats).

## Consequences

- Single binary provides complete workflow: monitor → screen → inspect → signal.
- 165 tests across all crates on initial commit.
- Clean dependency graph: binary depends on all 4 crates, crates depend only on
  `market-core`.
- Workspace crates can be tested independently (`cargo test -p finviz-scraper`).

---

## Fix: Code quality audit (`4d3794e`)

Post-merge quality sweep addressing four categories:

1. **Formatting drift** — `cargo fmt` across workspace.
2. **`.env*` not in `.gitignore`** — added to prevent accidental secret leaks.
3. **Subtraction underflow** — 4 sites used `len() - 1` on potentially empty
   collections. Replaced with `len().saturating_sub(1)`.
4. **Unstructured tracing** — 10 `tracing::warn!` calls converted from string
   interpolation (`"failed: {e}"`) to structured fields (`error = %e`) for
   log pipeline compatibility.
