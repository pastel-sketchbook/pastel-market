# 0008: Google News RSS Limitations

**Status**: Active limitation — revisit when alternatives mature  
**Date**: 2026-04-24  
**Context**: Chart overlay news panel, Google News RSS integration

## Decision

Use Google News RSS (`news.google.com/rss/search`) as a keyless, paywall-free
news source, merged with Yahoo Finance search headlines.

## Limitations

### 1. Summaries are often empty

Google News RSS `<description>` typically contains only the headline and
publisher wrapped in HTML tags:

```xml
<description>
  &lt;a href="..."&gt;Headline&lt;/a&gt;&amp;nbsp;
  &lt;font color="#6f6f6f"&gt;Publisher&lt;/font&gt;
</description>
```

After stripping HTML and removing the duplicate headline/publisher text,
nothing remains. The summary panel (`Enter` key) only opens when there is
actual summary content beyond the headline — for most items it will not
open at all.

**Impact**: The inline summary feature is useful for ~30-40% of articles
(those with a `<br>` followed by extra text). The rest are headline-only.

### 2. Entity-encoded HTML in descriptions

Google News encodes HTML inside `<description>` using XML entities
(`&lt;`, `&gt;`). quick-xml 0.39 decodes these during parsing and then
interprets the resulting `<font>`, `</font>`, `<br>` as real XML tags,
splitting content into fragments and losing angle brackets.

**Workaround**: We extract `<description>` content via raw string search
before quick-xml processes it (`extract_raw_descriptions`), then decode
entities and strip HTML ourselves (`clean_html`).

### 3. No full article text

Google News RSS only provides headlines and links. The actual article
content lives behind publisher paywalls (WSJ, Bloomberg, etc.) or requires
JavaScript rendering. We cannot fetch full article text without:

- An API key (Finnhub, NewsAPI, etc.)
- A headless browser for JS-rendered pages
- Paying for a news aggregation service

### 4. Title deduplication is fuzzy

Yahoo and Google News return overlapping headlines. Deduplication uses
normalized word overlap (>60% of shorter title's words). This may
occasionally merge distinct articles with similar titles or fail to
catch paraphrased duplicates.

### 5. Google News rate limiting

Google News RSS has no documented rate limit but may throttle or block
requests from a single IP if called too frequently. We mitigate this
with single-attempt fast retry (no backoff) and only fetching on
chart open (not on auto-refresh ticks).

## Alternatives to revisit

| Source | Pros | Cons |
|---|---|---|
| **Finnhub** (free tier) | Headline + summary, 60 req/min | Requires API key |
| **NewsAPI** | Full search, summary, content snippet | Requires API key, free tier limited |
| **Seeking Alpha RSS** | Good stock analysis content | May block non-browser UAs |
| **SEC EDGAR full-text search** | Regulatory filings with full text | Not news, already integrated separately |
| **Newspaper3k / readability** | Extract article text from URL | Requires fetching full page, JS-heavy sites fail |
| **GDELT Project** | Global news, no key, JSON API | Noisy, not stock-focused |

## Files

- `crates/market-core/src/news.rs` — Google News RSS fetcher + parser
- `src/worker.rs` — parallel fetch (Yahoo + Google), merge + dedup
- `src/ui/chart.rs` — news panel rendering, summary toggle guard
- `src/app.rs` — `Enter` only opens summary when content exists
