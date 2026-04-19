# 0006 — Background Threaded Fetching

**Status:** Implemented  
**Date:** 2026-04-19  
**Commits:** uncommitted

## Problem

Every HTTP call (Yahoo Finance quotes, Finviz screener, Earnings Whispers) ran
synchronously on the main thread. During a full refresh cycle the UI froze for
1–5 seconds depending on network latency and the number of endpoints hit. This
violated the core architectural principle: *the event loop must never block*.

Specific blocking paths:

- `refresh_quotes()` — 3 sequential fetches (watchlist, indices, sectors) plus
  sparkline, news, and scanner
- `refresh_scanner()` — trending requires 2 serial calls; fundamentals calls
  Finviz (~2s)
- `refresh_qc_data()` — insider ownership, sector heat, and whisper data each
  block independently
- `refresh_added_symbol()` — blocks while the user waits after pressing Enter
- `on_tick()` — called `refresh_quotes()` directly, stalling the draw loop

## Decision

Introduce a `Worker` struct (`src/worker.rs`) that owns an
`Arc<dyn QuoteProvider>` and an `mpsc::channel<FetchResult>`. Each fetch is
submitted as a short-lived `std::thread::spawn` that performs the blocking HTTP
call and sends the result back. The main loop drains completed results via
`try_recv()` on every iteration before drawing.

### Why `std::thread` and not `tokio`/async

- The project uses `ureq` (synchronous HTTP) — no async runtime needed.
- `crossterm` event polling is synchronous.
- Spawning OS threads for 3–6 concurrent HTTP calls is simple and correct,
  avoiding a 200KB+ async runtime dependency.
- Each thread lives only for one HTTP call (~100ms–3s).

### Why `Arc<dyn QuoteProvider>` instead of cloning

- `ureq::Agent` shares its connection pool and cookie jar internally via `Arc`.
  Wrapping `YahooClient` in `Arc` for the trait object to be `Send + Sync` is
  a one-line trait bound change.

### Why a single `mpsc` channel for all result types

- A single `FetchResult` enum with 10 variants keeps the drain loop simple.
- Volume is low (< 10 results per refresh cycle).
- `try_recv()` processes all available results in one batch; ordering between
  types doesn't matter.

### Tick rate redesign

Decoupled UI tick rate from data refresh interval:

| Parameter | Value | Purpose |
|---|---|---|
| Tick rate | 250ms | UI redraw + result drain frequency |
| Active refresh | 120 ticks (30s) | Counted in `on_tick()` |
| Heartbeat refresh | 1200 ticks (5min) | Closed-market interval |

## Changes

| File | Change |
|---|---|
| `src/worker.rs` | New. `FetchResult` enum, `Worker` struct with `submit_*` methods |
| `src/app.rs` | `client` → `worker`. All `refresh_*` submit jobs. New `drain_results()`. `loading` flag. Tick counting in `on_tick()` |
| `src/main.rs` | Tick 30s → 250ms. `drain_results()` called before every `draw()` |
| `yahoo-provider/src/client.rs` | `QuoteProvider` trait gains `Send + Sync` bound |

## Consequences

### Positive

- UI never freezes during HTTP calls.
- Multiple endpoints fetched concurrently (watchlist, indices, sectors in
  parallel).
- Sparkline fetch triggered reactively when quotes arrive.
- Adding a symbol returns to normal mode immediately.

### Negative

- Brief window of stale data while fetches are in-flight. Mitigated by
  `loading` flag.
- Tests require `sleep` + `drain_results` to synchronize with background
  threads. Acceptable for mock providers completing in < 1ms.
- Each refresh cycle spawns 3–6 OS threads. Negligible on developer
  workstations.

## Test impact

- 166 tests passing, 0 failures, 0 clippy warnings.
- `with_provider()` takes `Arc<dyn QuoteProvider>` instead of `Box`.
- `refresh_and_drain()` test helper for deterministic async testing.
- 3 tick tests updated for new counting model.
