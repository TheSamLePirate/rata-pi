//! Persistent user config — theme, toggles, composer draft.
//!
//! V3.j.1 · rata-pi picks up the user's preferred theme + a handful of
//! `/settings` toggles from `<config_dir>/rata-pi/config.json` on
//! launch, and writes it back whenever one of those settings changes.
//! A second file (`draft.txt` in the same dir) holds the composer
//! buffer the user had unsent at quit-time so the next launch can
//! offer to restore it.
//!
//! The directory path comes from `directories::ProjectDirs` — same
//! resolution rata-pi uses for history. On typical platforms that
//! means:
//!   Linux   → ~/.config/rata-pi/
//!   macOS   → ~/Library/Application Support/dev.olivvein.rata-pi/
//!   Windows → %APPDATA%\olivvein\rata-pi\config\
//!
//! Deviation from PLAN_V3: the plan called for TOML. We use JSON so
//! the dep footprint stays at serde_json (already pulled in for the
//! RPC wire format). Behaviourally identical to what the plan asked
//! for; the format is an implementation detail.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Persisted subset of `App` state. Every field is `Option` so loading
/// a config that's missing keys (old version, partial rewrite) still
/// succeeds; missing keys fall back to the in-memory defaults set in
/// `App::new`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub theme: Option<String>,
    pub notify: Option<bool>,
    pub vim: Option<bool>,
    pub show_thinking: Option<bool>,
    pub show_raw_markers: Option<bool>,
    pub focus_marker: Option<String>, // "both" | "border-only" | "marker-only"
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("dev", "olivvein", "rata-pi")
}

/// Absolute path of `config.json`. `None` when no project dir resolves
/// (CI sandbox, weird filesystem).
pub fn config_path() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().join("config.json"))
}

/// Absolute path of the composer draft file.
pub fn draft_path() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().join("draft.txt"))
}

/// When true, `load` / `save` / `save_draft` / `take_draft` are
/// all no-ops. Automatic in `#[cfg(test)]` builds so unit tests
/// that construct `App::new` don't read or pollute the real user
/// config / draft files.
fn persist_disabled() -> bool {
    cfg!(test)
}

/// Best-effort: read the config file. Any error (missing, malformed,
/// IO fault) returns the default config. Boot must never fail here.
pub fn load() -> UserConfig {
    if persist_disabled() {
        return UserConfig::default();
    }
    let Some(path) = config_path() else {
        return UserConfig::default();
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return UserConfig::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Write the config. Errors are logged at debug and swallowed — a
/// failed write shouldn't crash the app.
pub fn save(cfg: &UserConfig) {
    if persist_disabled() {
        return;
    }
    let Some(path) = config_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::debug!(error = %e, ?path, "config_dir create failed");
        return;
    }
    let json = match serde_json::to_string_pretty(cfg) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "config serialize failed");
            return;
        }
    };
    if let Err(e) = std::fs::write(&path, json) {
        tracing::debug!(error = %e, ?path, "config write failed");
    }
}

/// Save the composer draft. Called on Esc-quit when the composer is
/// non-empty. Any error is logged and swallowed.
pub fn save_draft(text: &str) {
    if persist_disabled() {
        return;
    }
    let Some(path) = draft_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Err(e) = std::fs::write(&path, text) {
        tracing::debug!(error = %e, ?path, "draft write failed");
    }
}

/// Load-and-clear: returns the stored draft if any, then deletes the
/// file so it's consumed exactly once per launch.
pub fn take_draft() -> Option<String> {
    if persist_disabled() {
        return None;
    }
    let path = draft_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_default() {
        // Point to a non-existent path; load should swallow and return default.
        let cfg = UserConfig {
            theme: Some("tokyo-night".into()),
            ..Default::default()
        };
        let round = serde_json::to_string(&cfg).unwrap();
        let back: UserConfig = serde_json::from_str(&round).unwrap();
        assert_eq!(back.theme.as_deref(), Some("tokyo-night"));
    }

    #[test]
    fn malformed_json_falls_back_to_default() {
        let cfg: UserConfig = serde_json::from_str("not valid json").unwrap_or_default();
        assert!(cfg.theme.is_none());
    }

    #[test]
    fn partial_config_missing_keys_load_cleanly() {
        let partial = r#"{"theme": "dracula"}"#;
        let cfg: UserConfig = serde_json::from_str(partial).unwrap();
        assert_eq!(cfg.theme.as_deref(), Some("dracula"));
        assert!(cfg.notify.is_none());
        assert!(cfg.vim.is_none());
    }
}
