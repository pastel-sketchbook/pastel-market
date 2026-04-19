# 0005 — Live Data Wiring

**Status:** Implemented  
**Date:** 2026-04-19  
**Commit:** `a0daa0c`

## Problem

After the merge, the UI components existed but several data flows were
disconnected — the app had the infrastructure to display data but wasn't
fetching or routing it in response to user actions:

1. **Sparkline didn't refresh on navigation:** Moving up/down (`j`/`k`) in the
   watchlist kept showing the sparkline for the previously selected stock.
2. **Adding a symbol showed no quote:** Pressing `a` to add a symbol returned
   to normal mode but the new row showed blank price data until the next
   auto-refresh cycle (30s–5min).
3. **QC data never fetched:** Pressing `r` in QC view didn't trigger any
   Finviz or Whisper data fetches. The auto-check items (insider ownership,
   sector heat, historical beats) stayed empty.
4. **Top movers panels empty:** The gainers/losers panels in the detail view
   were never populated from scanner results.
5. **News not fetched on toggle:** Pressing `n` toggled the news panel but
   didn't trigger a fetch for the selected stock.

## Decision

Wire up each data flow to the appropriate user action:

| Action | Fetch triggered |
|---|---|
| `j`/`k` navigation (table focus) | `refresh_sparkline()` for new selection |
| `j`/`k` navigation (news visible) | `refresh_news()` for new selection |
| `a` + Enter (add symbol) | `refresh_added_symbol()` via `Watchlist::set_quote()` |
| `r` in QC view | `refresh_qc_data()`: insider ownership (Finviz), sector heat (Finviz ETFs vs SPY), whisper data + historical beats (Earnings Whispers) |
| `n` toggle on | `refresh_news()` for selected stock |
| Scanner data arrives | `update_top_movers_from_scanner()` populates gainers/losers |

### `Watchlist::set_quote()`

Added a new method to update a single quote by index, avoiding a full
`update_quotes()` call for the one-symbol case. Used after adding a symbol to
immediately show its price data.

## Consequences

- The UI is reactive to all user actions — no stale data visible after
  navigation or symbol addition.
- QC auto-check items populate on first `r` press in QC view, enabling the
  conviction signal workflow.
- Top movers panels show actual data from the most recent scanner fetch.
- All new fetches were synchronous (blocking the UI thread) — addressed in
  0006.
