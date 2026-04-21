//! Modal system — overlays that temporarily steal keyboard focus.
//!
//! The app holds at most one `Modal` at a time. When present it paints a
//! centered, bordered frame over the transcript area and routes input to
//! modal-specific handlers. `Modal::handle_key` returns a `ModalOutcome`
//! instructing the app whether to close, keep, or fire an RPC as the result.

use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::files::FileList;
use crate::git::{Branch, Commit, GitStatus};
use crate::history::HistoryEntry;
use crate::rpc::types::{ForkMessage, Model, SessionStats, ThinkingLevel};
use crate::ui::commands::MenuItem;

/// Fuzzy file finder state. The `files` list is captured at modal-open
/// time; `query` is typed live; `selected` indexes into the fuzzy-filtered
/// subset.
///
/// V2.11.1 caches both the filtered path list and the rendered preview so
/// neither the fuzzy matcher nor syntect runs in the draw closure. The
/// caches are keyed on the inputs that can invalidate them (query string
/// for the filter; selected index + active query for the preview) and are
/// invalidated eagerly in the key handler.
#[derive(Debug)]
pub struct FileFinder {
    pub title: String,
    pub hint: String,
    pub files: FileList,
    pub query: String,
    pub selected: usize,
    pub mode: FilePickMode,
    /// V2.11.1 · background walk has not finished yet; the modal shows a
    /// "indexing files…" placeholder until the FileList arrives.
    pub loading: bool,
    /// V2.11.1 · cached fuzzy-filtered list — a snapshot of `filter(files,
    /// query, cap)`. Rebuilt in `refresh_filter` whenever the query string
    /// or the underlying file list changes.
    pub filtered: Vec<(String, i64)>,
    /// V2.11.1 · the query the `filtered` cache was built against. `None`
    /// means the cache is empty and must be rebuilt on next access.
    pub filter_query: Option<String>,
    /// V2.11.1 · cached highlighted preview for the `selected` path. The
    /// `Vec<Line<'static>>` is produced once (file read + syntect) and
    /// reused every frame until the selection moves.
    pub preview_cache: Option<PreviewCache>,
}

#[derive(Debug, Clone)]
pub struct PreviewCache {
    pub path: String,
    pub lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone, Copy)]
pub enum FilePickMode {
    /// Replace the composer entirely with `@<path>`.
    Insert,
    /// Replace the final `@<incomplete>` token with `@<path>`.
    AtToken,
}

impl FileFinder {
    /// Rebuild the filtered list when the query or the underlying list
    /// changes. Cheap if nothing moved (the keys match and we short-circuit).
    pub fn refresh_filter(&mut self, cap: usize) {
        let needs = !matches!(&self.filter_query, Some(q) if q == &self.query);
        if !needs {
            return;
        }
        self.filtered = crate::files::filter(&self.files.files, &self.query, cap);
        self.filter_query = Some(self.query.clone());
        // Clamp selection — a fresh filter may have shrunk the list.
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
        // Selection's content probably changed; drop the preview cache.
        self.preview_cache = None;
    }

    /// The path currently selected in the cached list, if any.
    pub fn current_path(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(|(p, _)| p.as_str())
    }

    /// Invalidate the preview cache — called when the selection moves.
    pub fn invalidate_preview(&mut self) {
        self.preview_cache = None;
    }

    /// Swap in a freshly-walked file list (from the background `walk_cwd`).
    pub fn set_files(&mut self, files: FileList) {
        self.files = files;
        self.loading = false;
        self.filter_query = None;
        self.filtered.clear();
        self.preview_cache = None;
    }
}

#[derive(Debug)]
pub enum Modal {
    Stats(Box<SessionStats>),
    /// Two-pane command menu: categorized list left, detail right.
    Commands(ListModal<MenuItem>),
    Models(ListModal<Model>),
    Thinking(RadioModal<ThinkingLevel>),
    History(ListModal<HistoryEntry>),
    Forks(ListModal<ForkMessage>),
    /// Fuzzy file finder — Ctrl+P, /find, @path.
    Files(FileFinder),

    // ── V2.7: git ────────────────────────────────────────────────────────
    /// Full-screen diff viewer using the V2.3 diff widget.
    Diff(DiffView),
    /// Status summary.
    GitStatus(Box<GitStatus>),
    /// Commit log picker.
    GitLog(GitLogState),
    /// Branch picker.
    GitBranch(GitBranchState),

    /// Plan view: full list of plan items with status.
    PlanView,

    /// V2.11 · readiness modal. One row per check.
    Doctor(Vec<DoctorCheck>),
    /// V2.11 · MCP servers (if pi exposes any).
    Mcp(Vec<McpRow>),

    /// V2.12.g · interview mode — agent-authored structured form.
    /// Boxed because `InterviewState` is the largest modal payload.
    Interview(Box<crate::interview::InterviewState>),

    Help,

    /// Extension UI dialog: select from a list of strings.
    ExtSelect {
        request_id: String,
        title: String,
        options: Vec<String>,
        selected: usize,
    },
    /// Extension UI dialog: yes/no confirmation.
    ExtConfirm {
        request_id: String,
        title: String,
        message: Option<String>,
        selected: usize, // 0 = No, 1 = Yes (safer default)
    },
    /// Extension UI dialog: free-form single-line input.
    ExtInput {
        request_id: String,
        title: String,
        placeholder: Option<String>,
        value: String,
    },
    /// Extension UI dialog: editor. Treated as single-line until the
    /// multi-line composer lands in M5.
    ExtEditor {
        request_id: String,
        title: String,
        value: String,
    },
}

#[derive(Debug)]
pub struct DiffView {
    pub title: String,
    pub staged: bool,
    pub diff: String,
    /// Line-granular scroll offset into the rendered diff lines.
    pub scroll: u16,
}

/// A single row in the `/doctor` readiness modal.
#[derive(Debug, Clone)]
pub struct DoctorCheck {
    pub label: &'static str,
    pub status: DoctorStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
    Info,
}

/// A row in the `/mcp` modal. pi doesn't currently expose MCP state over
/// RPC, so we surface one informational row explaining that. When/if pi
/// adds `get_mcp_servers` (or similar), each server becomes its own row.
#[derive(Debug, Clone)]
pub struct McpRow {
    pub name: String,
    pub status: McpStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpStatus {
    /// Reserved for when pi starts reporting MCP server state.
    #[allow(dead_code)]
    Connected,
    #[allow(dead_code)]
    Disconnected,
    Unknown,
}

#[derive(Debug)]
pub struct GitLogState {
    pub commits: Vec<Commit>,
    pub selected: usize,
}

#[derive(Debug)]
pub struct GitBranchState {
    pub branches: Vec<Branch>,
    pub query: String,
    pub selected: usize,
}

/// A scrollable, filterable list of items.
#[derive(Debug)]
pub struct ListModal<T> {
    pub title: String,
    pub hint: String,
    pub query: String,
    pub items: Vec<T>,
    pub selected: usize,
}

impl<T> ListModal<T> {
    pub fn new(title: impl Into<String>, hint: impl Into<String>, items: Vec<T>) -> Self {
        Self {
            title: title.into(),
            hint: hint.into(),
            query: String::new(),
            items,
            selected: 0,
        }
    }
}

/// Radio-button modal for a fixed enum set.
#[derive(Debug)]
pub struct RadioModal<T> {
    pub title: String,
    pub options: Vec<(T, &'static str)>,
    pub selected: usize,
}

impl<T> RadioModal<T> {
    pub fn new(title: impl Into<String>, options: Vec<(T, &'static str)>, selected: usize) -> Self {
        Self {
            title: title.into(),
            options,
            selected,
        }
    }
}

/// Returns a rect centered in `area`, capped at `max_w x max_h` but never
/// exceeding 90 % of either dimension. For tiny terminals we just use what
/// fits.
pub fn centered(area: Rect, max_w: u16, max_h: u16) -> Rect {
    let w = max_w.min(area.width.saturating_mul(9) / 10).min(area.width);
    let h = max_h
        .min(area.height.saturating_mul(9) / 10)
        .min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Case-insensitive substring match for filter queries.
pub fn matches_query(haystack: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    haystack.to_lowercase().contains(&query.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::FileList;
    use std::path::PathBuf;

    fn finder_with(files: Vec<&str>) -> FileFinder {
        FileFinder {
            title: "t".into(),
            hint: "h".into(),
            files: FileList {
                root: PathBuf::from("."),
                files: files.into_iter().map(|s| s.to_string()).collect(),
                truncated: false,
            },
            query: String::new(),
            selected: 0,
            mode: FilePickMode::Insert,
            loading: false,
            filtered: Vec::new(),
            filter_query: None,
            preview_cache: None,
        }
    }

    #[test]
    fn refresh_filter_caches_by_query() {
        let mut ff = finder_with(vec!["a.rs", "b.rs", "c.rs"]);
        ff.refresh_filter(10);
        assert_eq!(ff.filter_query.as_deref(), Some(""));
        assert_eq!(ff.filtered.len(), 3);

        // Changing the query rebuilds.
        ff.query = "b".into();
        ff.refresh_filter(10);
        assert_eq!(ff.filter_query.as_deref(), Some("b"));
        assert!(ff.filtered.iter().any(|(p, _)| p == "b.rs"));
    }

    #[test]
    fn refresh_filter_short_circuits_when_query_unchanged() {
        let mut ff = finder_with(vec!["a.rs"]);
        ff.refresh_filter(10);
        let first = ff.filtered.clone();
        ff.refresh_filter(10);
        assert_eq!(first, ff.filtered);
    }

    #[test]
    fn current_path_follows_selection() {
        let mut ff = finder_with(vec!["a.rs", "b.rs"]);
        ff.refresh_filter(10);
        ff.selected = 1;
        assert_eq!(ff.current_path(), Some("b.rs"));
    }

    #[test]
    fn set_files_clears_caches() {
        let mut ff = finder_with(vec!["a.rs"]);
        ff.refresh_filter(10);
        ff.preview_cache = Some(PreviewCache {
            path: "a.rs".into(),
            lines: vec![],
        });
        ff.set_files(FileList {
            root: PathBuf::from("."),
            files: vec!["x.rs".into()],
            truncated: false,
        });
        assert!(ff.filter_query.is_none());
        assert!(ff.filtered.is_empty());
        assert!(ff.preview_cache.is_none());
        assert!(!ff.loading);
    }

    #[test]
    fn invalidate_preview_drops_cache() {
        let mut ff = finder_with(vec!["a.rs"]);
        ff.preview_cache = Some(PreviewCache {
            path: "a.rs".into(),
            lines: vec![],
        });
        ff.invalidate_preview();
        assert!(ff.preview_cache.is_none());
    }
}
