# 0003 — Dynamic Watchlist Seeding

**Status:** Implemented  
**Date:** 2026-04-19  
**Commit:** `567bf77`

## Problem

The watchlist was initialized with a hardcoded `DEFAULT_SYMBOLS` array
(`AAPL`, `MSFT`, `GOOGL`, etc.). A fixed list doesn't reflect current market
conditions — a first-time user sees stale tickers regardless of what's moving.

## Decision

On first launch (no persisted session), seed the watchlist dynamically from
Yahoo Finance predefined screeners with cascade fallback:

1. `day_gainers` — top 20 by daily gain
2. `day_losers` — fallback if gainers fails
3. `most_actives` — final fallback
4. Empty watchlist if all fail

On subsequent launches, persisted session symbols are restored.

`App::new()` takes no arguments — symbols are determined internally from
session or screener.

### Why `day_gainers` first

The QC checklist is designed for bullish setups. Stocks making significant
upward moves are more relevant for screening than a static blue-chip list or
losers.

## Consequences

- First launch shows currently active stocks.
- `DEFAULT_SYMBOLS` constant removed entirely.
- If Yahoo is unreachable on first launch with no session, watchlist starts
  empty (user adds symbols with `a`).
- Seed limit capped at 20 symbols for fast initial fetch.
