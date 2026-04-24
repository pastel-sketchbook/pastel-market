# 0009: Embedded CIK Map

**Status**: Accepted  
**Date**: 2026-04-24  
**Context**: SEC EDGAR filing fetcher, CIK resolution performance

## Problem

SEC EDGAR requires a CIK (Central Index Key) number to fetch filings, not a
ticker symbol. The ticker→CIK mapping is published as a ~2 MB JSON file at
`https://www.sec.gov/files/company_tickers.json`.

The original implementation downloaded this file at runtime with a background
cache-warming thread. This caused two problems:

1. **Race condition** — Opening a chart before the background warm completed
   triggered a second parallel download of the same 2 MB file.
2. **Cold-start latency** — First chart open paid 1-3 seconds for the download
   even when the warm succeeded, because the submissions JSON (~4 MB for large
   companies like Apple) still had to be fetched.

## Decision

Embed the CIK map directly into the binary via `include_str!("../../../data/cik_map.ron")`.

The file `data/cik_map.ron` is a RON (Rusty Object Notation) map of
`{"TICKER": "0000000000"}` entries generated once from the SEC endpoint and
committed to the repository. RON is preferred over JSON for its native Rust
type system support and readability. It is parsed lazily on first use via
`OnceLock` — zero I/O, zero network, instant resolution.

## Consequences

### Positive

- **Zero network for CIK resolution** — no download, no race, no cache warming
- **Instant startup** — `Worker::new()` no longer spawns a background thread
- **Deterministic** — every build resolves the same tickers the same way
- **~224 KB binary increase** — negligible for a TUI application

### Negative

- **Stale data** — New tickers added to SEC after the snapshot won't resolve
  until `data/cik_map.json` is regenerated. CIK numbers never change, so
  existing mappings remain valid indefinitely.
- **Manual refresh** — Updating the map requires re-downloading from SEC and
  committing the new file.

### Mitigation

To refresh the map:

```sh
curl -sL -H "User-Agent: pastel-market/0.4.0 contact@example.com" \
  "https://www.sec.gov/files/company_tickers.json" | \
  python3 -c "
import json, sys
data = json.load(sys.stdin)
pairs = sorted((e['ticker'].upper(), str(e['cik_str']).zfill(10)) for e in data.values())
lines = [f'    \"{t}\": \"{c}\"' for t, c in pairs]
print('{')
print(',\n'.join(lines) + ',')
print('}')
" > data/cik_map.ron
```

## Alternatives Considered

| Approach | Rejected because |
|---|---|
| Runtime download + in-memory cache | Race condition, 2 MB cold-start latency |
| Hardcoded map of ~50 common tickers | Only covers a fraction of lookups |
| Disk-based cache (download once, persist) | Adds file I/O, cache invalidation logic, and platform-specific paths |
| JSON format | Already in workspace, but lacks Rust type system alignment; RON is idiomatic |
