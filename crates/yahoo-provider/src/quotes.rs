//! Yahoo Finance JSON response parsers for quotes, sparklines, screeners, and trending tickers.

use std::collections::HashMap;

use market_core::domain::{NewsItem, PricePoint, Quote};

/// Extract ordered quote results from the Yahoo Finance v7 response body.
///
/// Returns one `Option<Quote>` per input symbol, in the same order as
/// `symbols`. Unrecognised symbols map to `None`.
#[must_use]
pub fn parse_quotes_response(body: &serde_json::Value, symbols: &[String]) -> Vec<Option<Quote>> {
    let results = body["quoteResponse"]["result"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut map: HashMap<String, Quote> = HashMap::new();
    for item in results {
        if let Some(q) = parse_quote(&item) {
            map.insert(q.symbol.clone(), q);
        }
    }

    symbols.iter().map(|s| map.remove(s)).collect()
}

/// Extract intraday price points from the Yahoo Finance v8 spark response.
///
/// Null close values (pre-market gaps) are silently skipped.
#[must_use]
pub fn parse_sparkline_response(body: &serde_json::Value) -> Vec<PricePoint> {
    let closes = body["spark"]["result"][0]["response"][0]["indicators"]["quote"][0]["close"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    closes
        .iter()
        .filter_map(|v| v.as_f64().map(|close| PricePoint { close }))
        .collect()
}

/// Extract quotes from a Yahoo Finance screener response.
///
/// The screener endpoint (`/v1/finance/screener/predefined/saved`)
/// returns a structure like:
/// ```json
/// { "finance": { "result": [{ "quotes": [...] }] } }
/// ```
#[must_use]
pub fn parse_screener_response(body: &serde_json::Value) -> Vec<Quote> {
    let items = body["finance"]["result"][0]["quotes"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    items.iter().filter_map(parse_quote).collect()
}

/// Extract trending ticker symbols from a Yahoo Finance trending response.
#[must_use]
pub fn parse_trending_response(body: &serde_json::Value) -> Vec<String> {
    let items = body["finance"]["result"][0]["quotes"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    items
        .iter()
        .filter_map(|v| v["symbol"].as_str().map(String::from))
        .collect()
}

/// Extract news items from a Yahoo Finance search response.
#[must_use]
pub fn parse_news_response(body: &serde_json::Value) -> Vec<NewsItem> {
    let items = body["news"].as_array().cloned().unwrap_or_default();

    items
        .iter()
        .filter_map(|v| {
            let title = v["title"].as_str()?;
            if title.is_empty() {
                return None;
            }
            Some(NewsItem {
                title: title.to_string(),
                publisher: v["publisher"].as_str().unwrap_or("Unknown").to_string(),
                link: v["link"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Parse a single quote JSON object into our domain type.
#[must_use]
pub fn parse_quote(value: &serde_json::Value) -> Option<Quote> {
    let symbol = value["symbol"].as_str()?;
    Some(Quote {
        symbol: symbol.to_string(),
        short_name: value["shortName"].as_str().map(String::from),
        sector: value["sector"].as_str().map(String::from),
        market_state: value["marketState"].as_str().map(String::from),
        regular_market_price: value["regularMarketPrice"].as_f64().unwrap_or(0.0),
        regular_market_change: value["regularMarketChange"].as_f64().unwrap_or(0.0),
        regular_market_change_percent: value["regularMarketChangePercent"].as_f64().unwrap_or(0.0),
        regular_market_volume: value["regularMarketVolume"].as_u64().unwrap_or(0),
        regular_market_previous_close: value["regularMarketPreviousClose"].as_f64().unwrap_or(0.0),
        regular_market_open: value["regularMarketOpen"].as_f64().unwrap_or(0.0),
        regular_market_day_high: value["regularMarketDayHigh"].as_f64().unwrap_or(0.0),
        regular_market_day_low: value["regularMarketDayLow"].as_f64().unwrap_or(0.0),
        fifty_two_week_high: value["fiftyTwoWeekHigh"].as_f64().unwrap_or(0.0),
        fifty_two_week_low: value["fiftyTwoWeekLow"].as_f64().unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_quote ---

    #[test]
    fn parse_quote_extracts_fields_from_json() {
        let json = serde_json::json!({
            "symbol": "AAPL",
            "shortName": "Apple Inc.",
            "marketState": "REGULAR",
            "regularMarketPrice": 175.50,
            "regularMarketChange": 2.30,
            "regularMarketChangePercent": 1.33,
            "regularMarketVolume": 55_000_000_u64,
            "regularMarketPreviousClose": 173.20,
            "regularMarketOpen": 173.50,
            "regularMarketDayHigh": 176.00,
            "regularMarketDayLow": 173.00,
            "fiftyTwoWeekHigh": 199.62,
            "fiftyTwoWeekLow": 124.17,
        });

        let q = parse_quote(&json).expect("should parse");
        assert_eq!(q.symbol, "AAPL");
        assert_eq!(q.market_state.as_deref(), Some("REGULAR"));
        assert!((q.regular_market_price - 175.50).abs() < f64::EPSILON);
        assert_eq!(q.regular_market_volume, 55_000_000);
    }

    #[test]
    fn parse_quote_returns_none_without_symbol() {
        let json = serde_json::json!({ "regularMarketPrice": 100.0 });
        assert!(parse_quote(&json).is_none());
    }

    #[test]
    fn parse_quote_returns_none_for_empty_object() {
        assert!(parse_quote(&serde_json::json!({})).is_none());
    }

    #[test]
    fn parse_quote_returns_none_for_null_symbol() {
        assert!(parse_quote(&serde_json::json!({ "symbol": null })).is_none());
    }

    #[test]
    fn parse_quote_missing_short_name_yields_none_field() {
        let json = serde_json::json!({
            "symbol": "TSLA",
            "regularMarketPrice": 250.0,
        });
        let q = parse_quote(&json).expect("should parse");
        assert!(q.short_name.is_none());
    }

    #[test]
    fn parse_quote_missing_numerics_default_to_zero() {
        let json = serde_json::json!({ "symbol": "XYZ" });
        let q = parse_quote(&json).expect("should parse with only symbol");
        assert!((q.regular_market_price).abs() < f64::EPSILON);
        assert!((q.regular_market_change).abs() < f64::EPSILON);
        assert_eq!(q.regular_market_volume, 0);
        assert!(q.market_state.is_none());
    }

    #[test]
    fn parse_quote_extracts_market_state() {
        let json = serde_json::json!({ "symbol": "AAPL", "marketState": "POST" });
        let q = parse_quote(&json).expect("should parse");
        assert_eq!(q.market_state.as_deref(), Some("POST"));
    }

    // --- parse_quotes_response ---

    fn make_v7_body(quotes: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({ "quoteResponse": { "result": quotes, "error": null } })
    }

    #[test]
    fn quotes_response_returns_in_symbol_order() {
        let body = make_v7_body(&[
            serde_json::json!({ "symbol": "MSFT", "regularMarketPrice": 400.0 }),
            serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 175.0 }),
        ]);
        let symbols = vec!["AAPL".into(), "MSFT".into()];
        let result = parse_quotes_response(&body, &symbols);
        assert_eq!(result[0].as_ref().unwrap().symbol, "AAPL");
        assert_eq!(result[1].as_ref().unwrap().symbol, "MSFT");
    }

    #[test]
    fn quotes_response_unknown_symbol_maps_to_none() {
        let body =
            make_v7_body(&[serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 175.0 })]);
        let symbols = vec!["AAPL".into(), "FAKE".into()];
        let result = parse_quotes_response(&body, &symbols);
        assert!(result[0].is_some());
        assert!(result[1].is_none());
    }

    #[test]
    fn quotes_response_empty_result_array() {
        let body = make_v7_body(&[]);
        let symbols = vec!["AAPL".into()];
        let result = parse_quotes_response(&body, &symbols);
        assert!(result[0].is_none());
    }

    #[test]
    fn quotes_response_empty_symbols_returns_empty() {
        let body =
            make_v7_body(&[serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 175.0 })]);
        let result = parse_quotes_response(&body, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn quotes_response_missing_key() {
        let body = serde_json::json!({ "error": "something" });
        let result = parse_quotes_response(&body, &["AAPL".into()]);
        assert!(result[0].is_none());
    }

    #[test]
    fn quotes_response_duplicate_symbols_uses_last() {
        let body = make_v7_body(&[
            serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 100.0 }),
            serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 200.0 }),
        ]);
        let result = parse_quotes_response(&body, &["AAPL".into()]);
        let price = result[0].as_ref().unwrap().regular_market_price;
        assert!((price - 200.0).abs() < f64::EPSILON);
    }

    // --- parse_sparkline_response ---

    fn make_v8_body(closes: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({
            "spark": { "result": [{ "response": [{ "indicators": { "quote": [{ "close": closes }] } }] }] }
        })
    }

    #[test]
    fn sparkline_extracts_close_prices() {
        let body = make_v8_body(&[
            serde_json::json!(100.0),
            serde_json::json!(101.5),
            serde_json::json!(102.0),
        ]);
        let points = parse_sparkline_response(&body);
        assert_eq!(points.len(), 3);
        assert!((points[0].close - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn sparkline_filters_null_closes() {
        let body = make_v8_body(&[
            serde_json::json!(100.0),
            serde_json::json!(null),
            serde_json::json!(102.0),
        ]);
        let points = parse_sparkline_response(&body);
        assert_eq!(points.len(), 2);
    }

    #[test]
    fn sparkline_empty_returns_empty() {
        assert!(parse_sparkline_response(&make_v8_body(&[])).is_empty());
    }

    #[test]
    fn sparkline_missing_key_returns_empty() {
        assert!(parse_sparkline_response(&serde_json::json!({})).is_empty());
    }

    // --- parse_screener_response ---

    fn make_screener_body(quotes: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({
            "finance": { "result": [{ "id": "day_gainers", "quotes": quotes }], "error": null }
        })
    }

    #[test]
    fn screener_extracts_quotes() {
        let body = make_screener_body(&[
            serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 175.50 }),
            serde_json::json!({ "symbol": "TSLA", "regularMarketPrice": 250.0 }),
        ]);
        let result = parse_screener_response(&body);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol, "AAPL");
        assert_eq!(result[1].symbol, "TSLA");
    }

    #[test]
    fn screener_skips_items_without_symbol() {
        let body = make_screener_body(&[
            serde_json::json!({ "symbol": "AAPL", "regularMarketPrice": 175.0 }),
            serde_json::json!({ "regularMarketPrice": 999.0 }),
        ]);
        assert_eq!(parse_screener_response(&body).len(), 1);
    }

    #[test]
    fn screener_empty_returns_empty() {
        assert!(parse_screener_response(&make_screener_body(&[])).is_empty());
    }

    #[test]
    fn screener_missing_finance_key() {
        assert!(parse_screener_response(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn screener_missing_result_array() {
        let body = serde_json::json!({ "finance": { "result": [] } });
        assert!(parse_screener_response(&body).is_empty());
    }

    #[test]
    fn screener_non_array_quotes_returns_empty() {
        let body = serde_json::json!({ "finance": { "result": [{ "quotes": "invalid" }] } });
        assert!(parse_screener_response(&body).is_empty());
    }

    // --- parse_trending_response ---

    fn make_trending_body(symbols: &[&str]) -> serde_json::Value {
        let quotes: Vec<serde_json::Value> = symbols
            .iter()
            .map(|s| serde_json::json!({ "symbol": *s }))
            .collect();
        serde_json::json!({ "finance": { "result": [{ "quotes": quotes }], "error": null } })
    }

    #[test]
    fn trending_extracts_symbols() {
        let result = parse_trending_response(&make_trending_body(&["AAPL", "TSLA", "NVDA"]));
        assert_eq!(result, vec!["AAPL", "TSLA", "NVDA"]);
    }

    #[test]
    fn trending_empty_returns_empty() {
        assert!(parse_trending_response(&make_trending_body(&[])).is_empty());
    }

    #[test]
    fn trending_missing_finance_key() {
        assert!(parse_trending_response(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn trending_skips_items_without_symbol() {
        let body = serde_json::json!({
            "finance": { "result": [{ "quotes": [
                { "symbol": "AAPL" },
                { "name": "no_symbol" },
                { "symbol": "MSFT" },
            ] }] }
        });
        assert_eq!(parse_trending_response(&body), vec!["AAPL", "MSFT"]);
    }

    #[test]
    fn trending_duplicate_symbols_pass_through() {
        let result = parse_trending_response(&make_trending_body(&["AAPL", "AAPL", "MSFT"]));
        assert_eq!(result, vec!["AAPL", "AAPL", "MSFT"]);
    }

    #[test]
    fn trending_non_array_quotes_returns_empty() {
        let body = serde_json::json!({ "finance": { "result": [{ "quotes": 42 }] } });
        assert!(parse_trending_response(&body).is_empty());
    }

    // --- parse_news_response ---

    fn make_news_body(items: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({ "news": items })
    }

    #[test]
    fn news_extracts_headlines() {
        let body = make_news_body(&[
            serde_json::json!({ "title": "Apple beats earnings", "publisher": "Reuters", "link": "https://example.com/1" }),
            serde_json::json!({ "title": "MSFT rallies", "publisher": "Bloomberg", "link": "https://example.com/2" }),
        ]);
        let result = parse_news_response(&body);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "Apple beats earnings");
        assert_eq!(result[0].publisher, "Reuters");
    }

    #[test]
    fn news_empty_array() {
        assert!(parse_news_response(&make_news_body(&[])).is_empty());
    }

    #[test]
    fn news_missing_key() {
        assert!(parse_news_response(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn news_skips_empty_titles() {
        let body = make_news_body(&[
            serde_json::json!({ "title": "", "publisher": "Reuters" }),
            serde_json::json!({ "title": "Real headline", "publisher": "AP" }),
        ]);
        assert_eq!(parse_news_response(&body).len(), 1);
    }

    #[test]
    fn news_missing_publisher_defaults_to_unknown() {
        let body = make_news_body(&[serde_json::json!({ "title": "Breaking", "link": "x" })]);
        assert_eq!(parse_news_response(&body)[0].publisher, "Unknown");
    }

    #[test]
    fn news_missing_link_defaults_to_empty() {
        let body = make_news_body(&[serde_json::json!({ "title": "No link", "publisher": "CNN" })]);
        assert!(parse_news_response(&body)[0].link.is_empty());
    }

    #[test]
    fn news_missing_title_skips_item() {
        let body = make_news_body(&[serde_json::json!({ "publisher": "Reuters" })]);
        assert!(parse_news_response(&body).is_empty());
    }

    #[test]
    fn news_non_array_returns_empty() {
        assert!(parse_news_response(&serde_json::json!({ "news": "invalid" })).is_empty());
    }
}
