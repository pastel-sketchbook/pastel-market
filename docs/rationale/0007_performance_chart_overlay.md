# 0007 — Performance Chart Overlay

**Status:** Implemented  
**Date:** 2026-04-19  
**Commits:** c2782ff

## Problem

Users could see sparklines (tiny inline price history) in the watchlist table,
but had no way to inspect detailed price action over multiple time horizons.
Pastel Picker's predecessor had no charting at all; Reins Market had sparkline
data but no interactive chart view. To evaluate a stock's technical setup before
committing to a QC checklist, users need a proper price chart with selectable
time ranges.

## Decision

Add a modal chart overlay that opens on `Enter` for the selected stock. The
overlay renders a ratatui `Chart` widget with Braille markers over a centered
popup covering ~90% of the terminal. Eight time ranges (1D, 5D, 1M, 3M, 6M,
YTD, 1Y, 5Y) are selectable via number keys `1`–`8` or left/right arrows.

Key design choices:

1. **Modal overlay, not a new view mode** — The chart sits on top of the current
   view (watchlist or scanner). This avoids adding a fourth `ViewMode` variant
   and keeps the Tab cycle simple. `Esc` or `q` dismisses it.

2. **`ChartRange` enum in `market-core`** — Each variant carries its Yahoo
   Finance `range` and `interval` query parameters. This keeps Yahoo-specific
   URL logic out of the UI layer while letting the domain model own the range
   semantics (`next()`, `prev()`, `label()`, `ALL`).

3. **`PricePoint` gains `timestamp: Option<i64>`** — The sparkline parser now
   zips Yahoo's timestamp array with close prices, enabling the chart widget to
   use real time-based X-axis labels instead of simple indices.

4. **`fetch_sparkline` takes `ChartRange`** — Previously hardcoded to 1-day.
   Now the `QuoteProvider` trait method accepts a range, and the Yahoo client
   maps it to the correct `range=` and `interval=` query params.

5. **Background fetching via `Worker::submit_chart()`** — Chart data loads
   asynchronously. The UI shows a "Loading chart..." state while waiting, then
   `drain_results()` picks up `FetchResult::Chart` and populates `app.chart_data`.

6. **Chart keys intercepted first** — When `chart_open` is true, all key events
   route through `handle_chart_key()` before the normal key handler. This
   prevents range-selection keys (1–8) from leaking into scanner list selection.

## Rendering

- Green line when last price >= first price, red otherwise
- Y-axis shows low/mid/high labels
- Title bar shows symbol, range label, and price change summary
- Range tab bar at top highlights the active range
- Help bar at bottom shows available keybindings

## Alternatives Considered

- **Separate view mode**: Would complicate Tab cycling and require a back-stack.
  Modal overlay is simpler and matches how trading terminals work (popup charts).
- **ASCII art instead of Braille**: Lower resolution. Braille markers give 2x4
  sub-character resolution which is important for price charts in small terminals.
- **Inline chart below the table**: Wastes vertical space permanently. A modal
  overlay uses the full terminal area only when needed.

## Testing

- `enter_opens_chart_in_watchlist_view` — verifies `chart_open` + symbol + range
- `esc_closes_chart` — verifies dismissal resets state
- `chart_range_switch` — verifies number key changes range and sets loading
- `chart_left_right_navigation` — verifies arrow key range cycling
- `chart_keys_do_not_leak` — verifies scanner list index unchanged during chart
- `close_chart_clears_data` — verifies full state cleanup
- 4 `ChartRange` unit tests in `domain.rs` (next, prev, label, yahoo params)
