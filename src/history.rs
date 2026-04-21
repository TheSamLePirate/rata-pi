//! Prompt history — in-memory ring with JSONL persistence.
//!
//! One entry per submitted user prompt. Stored under the project data dir so
//! it survives across sessions. Excludes empty entries and duplicates of the
//! immediately-previous one.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub text: String,
    pub ts: u64,
}

#[derive(Debug)]
pub struct History {
    entries: Vec<HistoryEntry>,
    path: Option<PathBuf>,
    cursor: Option<usize>,
    /// Snapshot of the composer text at the moment the user started walking
    /// history, so we can restore it on the way back.
    stash: Option<String>,
}

impl History {
    pub fn load() -> Self {
        let path = resolve_path();
        let entries = path
            .as_ref()
            .and_then(|p| File::open(p).ok())
            .map(|f| {
                BufReader::new(f)
                    .lines()
                    .map_while(Result::ok)
                    .filter_map(|l| serde_json::from_str::<HistoryEntry>(&l).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Self {
            entries,
            path,
            cursor: None,
            stash: None,
        }
    }

    pub fn record(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if matches!(self.entries.last(), Some(e) if e.text == text) {
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = HistoryEntry {
            text: text.into(),
            ts,
        };
        // V2.11.2 · persist asynchronously so a slow or network FS can't
        // stall the submit() keypath. If we're not inside a tokio runtime
        // (tests), fall back to a synchronous write.
        let path = self.path.clone();
        let entry_for_disk = entry.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                h.spawn_blocking(move || {
                    if let Err(e) = append_file(path.as_deref(), &entry_for_disk) {
                        tracing::warn!(error = %e, "history append failed");
                    }
                });
            }
            Err(_) => {
                if let Err(e) = append_file(path.as_deref(), &entry_for_disk) {
                    tracing::warn!(error = %e, "history append failed");
                }
            }
        }
        self.entries.push(entry);
        // Walking is reset on new entry.
        self.cursor = None;
        self.stash = None;
    }

    /// Walk to an older entry. Returns the new composer text.
    pub fn prev(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        if self.cursor.is_none() {
            self.stash = Some(current.to_string());
            self.cursor = Some(self.entries.len() - 1);
            return Some(self.entries[self.entries.len() - 1].text.clone());
        }
        let i = self.cursor.unwrap();
        if i == 0 {
            return Some(self.entries[0].text.clone());
        }
        self.cursor = Some(i - 1);
        Some(self.entries[i - 1].text.clone())
    }

    /// Walk to a newer entry. Returns stashed text when we pass the end.
    pub fn next(&mut self) -> Option<String> {
        let i = self.cursor?;
        if i + 1 >= self.entries.len() {
            self.cursor = None;
            return Some(self.stash.take().unwrap_or_default());
        }
        self.cursor = Some(i + 1);
        Some(self.entries[i + 1].text.clone())
    }

    pub fn reset_walk(&mut self) {
        self.cursor = None;
        self.stash = None;
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    #[allow(dead_code)] // exposed for future `?`-help modal to show the file path
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }
}

fn append_file(path: Option<&std::path::Path>, entry: &HistoryEntry) -> std::io::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(entry).unwrap_or_default())?;
    Ok(())
}

fn resolve_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("dev", "olivvein", "rata-pi")?;
    Some(dirs.data_local_dir().join("history.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_history(entries: Vec<&str>) -> History {
        let now = 0;
        History {
            entries: entries
                .into_iter()
                .map(|t| HistoryEntry {
                    text: t.into(),
                    ts: now,
                })
                .collect(),
            path: None,
            cursor: None,
            stash: None,
        }
    }

    #[test]
    fn prev_starts_from_last_and_stashes_current() {
        let mut h = mem_history(vec!["a", "b", "c"]);
        assert_eq!(h.prev("draft").as_deref(), Some("c"));
        assert_eq!(h.prev("draft").as_deref(), Some("b"));
        assert_eq!(h.prev("draft").as_deref(), Some("a"));
        // already at oldest — stays put
        assert_eq!(h.prev("draft").as_deref(), Some("a"));
        // walk forward restores stash at the end
        assert_eq!(h.next().as_deref(), Some("b"));
        assert_eq!(h.next().as_deref(), Some("c"));
        assert_eq!(h.next().as_deref(), Some("draft"));
    }

    #[test]
    fn record_skips_empty_and_dupes() {
        let mut h = mem_history(vec!["a"]);
        h.record("   ");
        assert_eq!(h.entries.len(), 1);
        h.record("a");
        assert_eq!(h.entries.len(), 1);
        h.record("b");
        assert_eq!(h.entries.len(), 2);
    }
}
