//! News fetching via Yahoo Finance search endpoint.
//!
//! Re-exports the parser from [`crate::quotes`] and provides a convenience
//! type alias. The actual parsing lives in `quotes.rs` alongside all other
//! Yahoo JSON parsers.

pub use crate::quotes::parse_news_response;
