# 0004 — Sector Column and Accent Titles

**Status:** Implemented  
**Date:** 2026-04-19  
**Commit:** `815eaf3`

## Problem

Two UI gaps after the initial merge:

1. **No sector visibility:** The watchlist and scanner tables showed 6 columns
   (Symbol, Name, Price, Change, Change%, Volume) but not the stock's sector.
   Sector context is important for the QC checklist's "Sector Heat > SPY"
   auto-check item and for identifying sector rotation.

2. **Muted section titles:** All section titles (Watchlist, Scanner, QC panels,
   etc.) used the theme's border color, making them visually indistinguishable
   from panel borders. They should stand out as headings.

## Decision

### Sector column

- Added `sector: Option<String>` to the `Quote` struct.
- Yahoo Finance API now requests the `sector` field via the `fields=...`
  query parameter.
- Finviz `screener_result_to_quote()` maps the existing `sector` field.
- Watchlist and scanner tables now show 7 columns: Symbol, Name, **Sector**,
  Price, Change, Change%, Volume.

### Accent titles

All section titles across every UI module (`header.rs`, `watchlist.rs`,
`scanner.rs`, `qc.rs`, `detail.rs`) use `theme.accent` with `BOLD` modifier
instead of the muted border color.

## Consequences

- Sector is visible at a glance in both watchlist and scanner views.
- Section titles are visually distinct from panel borders across all 16 themes.
- The `Quote` struct grew by one `Option<String>` field — negligible memory
  impact.
