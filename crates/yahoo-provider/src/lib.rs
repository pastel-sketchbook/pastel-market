//! Yahoo Finance real-time data provider.
//!
//! Provides the [`QuoteProvider`] trait for abstracting stock data sources
//! and [`YahooClient`] as the production implementation using Yahoo Finance
//! APIs with cookie+crumb authentication.

pub mod client;
pub mod news;
pub mod quotes;

pub use client::{QuoteProvider, YahooClient};
