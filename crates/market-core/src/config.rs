//! User configuration persistence.
//!
//! Combines Reins Market's Preferences/Session with Pastel Picker's `QcSession`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::domain::{FilterMode, SortMode, ViewMode};
use crate::theme;

/// Application name used for directory paths.
const APP_NAME: &str = "pastel-market";

/// Persistent user preferences (TOML).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Preferences {
    /// Theme name (must match a name in `theme::THEMES`).
    pub theme: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: theme::THEMES[0].name.to_string(),
        }
    }
}

/// Ephemeral session state (JSON).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    /// Watchlist symbols in order.
    pub symbols: Vec<String>,
    /// Current sort mode name.
    pub sort_mode: String,
    /// Current filter mode name.
    pub filter_mode: String,
    /// Current view mode name.
    pub view_mode: String,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            symbols: Vec::new(),
            sort_mode: String::from("Default"),
            filter_mode: String::from("All"),
            view_mode: String::from("Watchlist"),
        }
    }
}

/// QC session state persisted between runs.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct QcSession {
    /// Per-stock QC checklist state: ticker -> Vec<bool>.
    pub qc_state: HashMap<String, Vec<bool>>,
}

impl QcSession {
    /// Load QC session from disk. Returns empty session on any failure.
    #[must_use]
    pub fn load() -> Self {
        qc_session_path()
            .and_then(|p| fs::read_to_string(&p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save QC session to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file
    /// cannot be written.
    pub fn save(qc_state: &HashMap<String, Vec<bool>>) -> Result<()> {
        let path = qc_session_path().context("could not determine data directory")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("could not create data directory")?;
        }
        let session = Self {
            qc_state: qc_state.clone(),
        };
        let content =
            serde_json::to_string_pretty(&session).context("could not serialize QC session")?;
        fs::write(&path, content).context("could not write QC session file")?;
        Ok(())
    }

    /// Check whether any QC state has been recorded.
    #[must_use]
    pub fn has_state(&self) -> bool {
        self.qc_state.values().any(|items| items.iter().any(|&b| b))
    }
}

// --- Path resolution ---

/// Resolve the preferences file path.
#[must_use]
pub fn preferences_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", APP_NAME)
        .map(|dirs| dirs.config_dir().join("preferences.toml"))
}

/// Resolve the session file path.
#[must_use]
pub fn session_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", APP_NAME)
        .map(|dirs| dirs.data_dir().join("session.json"))
}

/// Resolve the QC session file path.
#[must_use]
pub fn qc_session_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", APP_NAME)
        .map(|dirs| dirs.data_dir().join("qc_session.json"))
}

// --- Load / Save ---

/// Load preferences from disk, falling back to defaults on any error.
#[must_use]
pub fn load_preferences() -> Preferences {
    preferences_path()
        .and_then(|p| fs::read_to_string(&p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save preferences to disk.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file
/// cannot be written.
pub fn save_preferences(prefs: &Preferences) -> Result<()> {
    let path = preferences_path().context("could not determine config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("could not create config directory")?;
    }
    let content = toml::to_string_pretty(prefs).context("could not serialize preferences")?;
    fs::write(&path, content).context("could not write preferences file")?;
    Ok(())
}

/// Load session from disk, falling back to defaults on any error.
#[must_use]
pub fn load_session() -> Session {
    session_path()
        .and_then(|p| fs::read_to_string(&p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save session to disk.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file
/// cannot be written.
pub fn save_session(session: &Session) -> Result<()> {
    let path = session_path().context("could not determine data directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("could not create data directory")?;
    }
    let content = serde_json::to_string_pretty(session).context("could not serialize session")?;
    fs::write(&path, content).context("could not write session file")?;
    Ok(())
}

// --- Mode string conversions ---

/// Convert a `SortMode` to its display name for persistence.
#[must_use]
pub fn sort_mode_to_string(mode: SortMode) -> String {
    mode.to_string()
}

/// Parse a sort mode name back into a `SortMode`.
#[must_use]
pub fn sort_mode_from_string(s: &str) -> SortMode {
    match s {
        "Change% \u{2193}" => SortMode::ChangeDesc,
        "Change% \u{2191}" => SortMode::ChangeAsc,
        "Price \u{2193}" => SortMode::PriceDesc,
        "Volume \u{2193}" => SortMode::VolumeDesc,
        "Symbol" => SortMode::Symbol,
        _ => SortMode::Default,
    }
}

/// Convert a `FilterMode` to its display name for persistence.
#[must_use]
pub fn filter_mode_to_string(mode: FilterMode) -> String {
    mode.to_string()
}

/// Parse a filter mode name back into a `FilterMode`.
#[must_use]
pub fn filter_mode_from_string(s: &str) -> FilterMode {
    match s {
        "Gainers" => FilterMode::Gainers,
        "Losers" => FilterMode::Losers,
        "Big Movers" => FilterMode::BigMovers,
        "High Vol" => FilterMode::HighVolume,
        "Near 52W High" => FilterMode::Near52WkHigh,
        _ => FilterMode::All,
    }
}

/// Convert a `ViewMode` to its display name for persistence.
#[must_use]
pub fn view_mode_to_string(mode: ViewMode) -> String {
    mode.to_string()
}

/// Parse a view mode name back into a `ViewMode`.
#[must_use]
pub fn view_mode_from_string(s: &str) -> ViewMode {
    match s {
        "Scanner" => ViewMode::Scanner,
        "Quality Control" => ViewMode::QualityControl,
        _ => ViewMode::Watchlist,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_default_uses_first_theme() {
        let prefs = Preferences::default();
        assert_eq!(prefs.theme, theme::THEMES[0].name);
    }

    #[test]
    fn preferences_roundtrip_toml() {
        let prefs = Preferences {
            theme: "Dracula".to_string(),
        };
        let toml_str = toml::to_string_pretty(&prefs).expect("serialize");
        let parsed: Preferences = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(prefs, parsed);
    }

    #[test]
    fn session_default_has_empty_symbols() {
        let session = Session::default();
        assert!(session.symbols.is_empty());
        assert_eq!(session.sort_mode, "Default");
        assert_eq!(session.filter_mode, "All");
        assert_eq!(session.view_mode, "Watchlist");
    }

    #[test]
    fn session_roundtrip_json() {
        let session = Session {
            symbols: vec!["AAPL".into(), "MSFT".into()],
            sort_mode: "Change% \u{2193}".into(),
            filter_mode: "Gainers".into(),
            view_mode: "Scanner".into(),
        };
        let json = serde_json::to_string_pretty(&session).expect("serialize");
        let parsed: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(session, parsed);
    }

    #[test]
    fn sort_mode_roundtrip_all_variants() {
        let modes = [
            SortMode::Default,
            SortMode::ChangeDesc,
            SortMode::ChangeAsc,
            SortMode::PriceDesc,
            SortMode::VolumeDesc,
            SortMode::Symbol,
        ];
        for mode in modes {
            let s = sort_mode_to_string(mode);
            let back = sort_mode_from_string(&s);
            assert_eq!(mode, back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn filter_mode_roundtrip_all_variants() {
        let modes = [
            FilterMode::All,
            FilterMode::Gainers,
            FilterMode::Losers,
            FilterMode::BigMovers,
            FilterMode::HighVolume,
            FilterMode::Near52WkHigh,
        ];
        for mode in modes {
            let s = filter_mode_to_string(mode);
            let back = filter_mode_from_string(&s);
            assert_eq!(mode, back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn view_mode_roundtrip_all_variants() {
        let modes = [
            ViewMode::Watchlist,
            ViewMode::Scanner,
            ViewMode::QualityControl,
        ];
        for mode in modes {
            let s = view_mode_to_string(mode);
            let back = view_mode_from_string(&s);
            assert_eq!(mode, back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn qc_session_default_is_empty() {
        let session = QcSession::default();
        assert!(session.qc_state.is_empty());
        assert!(!session.has_state());
    }

    #[test]
    fn qc_session_has_state_detects_checked_items() {
        let mut session = QcSession::default();
        session
            .qc_state
            .insert("AAPL".to_string(), vec![false, false, false]);
        assert!(!session.has_state());

        session
            .qc_state
            .insert("AAPL".to_string(), vec![false, true, false]);
        assert!(session.has_state());
    }

    #[test]
    fn qc_session_roundtrip_json() {
        let mut qc_state = HashMap::new();
        qc_state.insert("AAPL".to_string(), vec![true, false, true, false, true]);
        let session = QcSession {
            qc_state: qc_state.clone(),
        };
        let json = serde_json::to_string(&session).expect("serialize");
        let loaded: QcSession = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(loaded.qc_state, qc_state);
    }

    #[test]
    fn preferences_path_is_some() {
        assert!(preferences_path().is_some());
    }

    #[test]
    fn session_path_is_some() {
        assert!(session_path().is_some());
    }

    #[test]
    fn qc_session_path_is_some() {
        assert!(qc_session_path().is_some());
    }
}
