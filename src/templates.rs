//! Reusable composer templates — V3.j.4.
//!
//! Stored as a flat JSON map `{ name: body }` under
//! `<config_dir>/tau/templates.json`. The usual user commands:
//!
//! * `/template save <name>`   — snapshot the composer into `<name>`
//! * `/template list`          — open a picker (V3.j.4+: for now flashes)
//! * `/template use <name>`    — replace the composer with `<name>`
//! * `/template delete <name>` — drop a template
//!
//! Same `cfg!(test)` persistence guard as `config::*` so tests don't
//! pollute real user state.

use std::collections::BTreeMap;
use std::path::PathBuf;

use directories::ProjectDirs;

fn persist_disabled() -> bool {
    cfg!(test)
}

fn templates_path() -> Option<PathBuf> {
    ProjectDirs::from("dev", "olivvein", "tau").map(|d| d.config_dir().join("templates.json"))
}

pub fn load() -> BTreeMap<String, String> {
    if persist_disabled() {
        return BTreeMap::new();
    }
    let Some(path) = templates_path() else {
        return BTreeMap::new();
    };
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save(map: &BTreeMap<String, String>) {
    if persist_disabled() {
        return;
    }
    let Some(path) = templates_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(&path, json);
    }
}

/// Save `body` under `name`. Overwrites any existing entry with the
/// same name.
pub fn put(name: &str, body: &str) {
    let mut map = load();
    map.insert(name.to_string(), body.to_string());
    save(&map);
}

/// Retrieve the body for `name`, if stored.
pub fn get(name: &str) -> Option<String> {
    load().get(name).cloned()
}

/// Remove `name`. Returns true if a template was actually removed.
pub fn delete(name: &str) -> bool {
    let mut map = load();
    let removed = map.remove(name).is_some();
    if removed {
        save(&map);
    }
    removed
}

/// Names sorted alphabetically.
///
/// V3.j.4 used this for the flash-based listing; V4.c moved to the
/// picker modal which consumes the full `load()` map. Kept in case
/// a future slash subcommand wants a quick listing without loading
/// bodies.
#[allow(dead_code)]
pub fn list_names() -> Vec<String> {
    load().into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V3.j.4 · serde round-trip of the templates map. (Live filesystem
    /// access is gated off under cfg(test).)
    #[test]
    fn roundtrip_templates_map() {
        let mut m: BTreeMap<String, String> = BTreeMap::new();
        m.insert("debug-bug".into(), "Please investigate...".into());
        m.insert("review-pr".into(), "Review PR #".into());
        let s = serde_json::to_string(&m).unwrap();
        let back: BTreeMap<String, String> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back.get("debug-bug").unwrap(), "Please investigate...");
    }

    #[test]
    fn load_is_empty_under_test() {
        let m = load();
        assert!(m.is_empty(), "tests must not read real templates file");
    }

    #[test]
    fn put_and_delete_under_test_are_noops() {
        // The cfg(test) guard means put/delete can't actually persist;
        // behaviour-wise, this test exists so a future someone who drops
        // the guard doesn't accidentally pollute real state undetected.
        put("tmp", "value");
        assert!(load().is_empty());
        assert!(!delete("tmp"));
    }
}
