# 0002 — Yahoo Finance Integration

**Status:** Implemented  
**Date:** 2026-04-19  
**Commits:** `dd0996c`, `2fbe52d`

## Problem

Yahoo Finance requires cookie+crumb authentication for all API requests. Two
issues prevented reliable access after the initial workspace merge:

1. **ureq v3 cookie bug** (`dd0996c`): The authentication flow hits
   `fc.yahoo.com/curveball` which returns a **404** with a `Set-Cookie`
   header. With ureq v3's default `http_status_as_error(true)`, the 404
   returns `Err(...)` before the cookie jar processes the `Set-Cookie` header.
   All subsequent requests fail with no session cookie.

2. **User-Agent blocking and rate-limiting** (`2fbe52d`): Yahoo actively blocks
   non-browser User-Agent strings with HTTP 429. The generic
   `pastel-market/0.1.0` UA was rejected. Additionally, the crumb endpoint
   (`/v1/test/getcrumb`) occasionally returns 429 on the first attempt.

## Decision

### Cookie fix: `http_status_as_error(false)`

Set `http_status_as_error(false)` on the `ureq::Agent` configuration. Non-2xx
responses are processed normally, allowing the cookie jar to store the
`Set-Cookie` header from the 404. Status codes are inspected manually via
`resp.status().as_u16()` where needed.

**Alternatives considered:**
- Extract headers from the ureq error type — v3 doesn't expose response headers
  on HTTP errors.
- Use a different cookie endpoint — no other Yahoo endpoint reliably sets the
  session cookie.

### Browser UA + retry with backoff

A dedicated `YAHOO_UA` constant uses a Chrome-like User-Agent string, applied
to all Yahoo Finance requests. This is separate from the generic
`market_core::http::USER_AGENT` used by other HTTP clients (Finviz).

The crumb fetch retries up to 3 times with exponential backoff (1s, 2s, 4s) on
429. This is a simple loop in `YahooClient::new()` rather than the generic
`call_with_retry`, because:

- It only runs once during client construction.
- It needs manual status code inspection (429 detection) under
  `http_status_as_error(false)` semantics.

## Consequences

- Yahoo Finance auth is reliable across sessions.
- The 1–7s startup delay from crumb retries is acceptable (one-time cost).
- The browser UA is scoped to `yahoo-provider` — other crates keep the generic
  UA.
- Manual status code checking is more verbose but explicit about what each
  endpoint expects.
