//! Project file walker + fuzzy filter for the Ctrl+P / @path picker.
//!
//! Honors `.gitignore` via the `ignore` crate; caps the walk at a reasonable
//! file count so huge monorepos don't hang the TUI. Paths are stored as
//! strings relative to the walk root for stable filter matching.

use std::path::PathBuf;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ignore::WalkBuilder;

/// Soft cap on the file count. If the walk exceeds this we stop and flag
/// the result as truncated.
pub const MAX_FILES: usize = 20_000;

#[derive(Debug, Clone)]
pub struct FileList {
    pub root: PathBuf,
    /// Relative paths as strings (forward-slash separated even on Windows
    /// for filter determinism).
    pub files: Vec<String>,
    pub truncated: bool,
}

impl FileList {
    pub fn empty() -> Self {
        Self {
            root: PathBuf::from("."),
            files: Vec::new(),
            truncated: false,
        }
    }
}

/// Walk the current working directory, respecting `.gitignore` and friends.
pub fn walk_cwd() -> FileList {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    walk(&root)
}

pub fn walk(root: &std::path::Path) -> FileList {
    let mut files: Vec<String> = Vec::new();
    let mut truncated = false;

    let walker = WalkBuilder::new(root)
        .hidden(false) // show dotfiles (but .gitignore still applies)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .parents(true)
        .follow_links(false)
        .build();

    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        let s: String = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        if s.is_empty() {
            continue;
        }
        files.push(s);
        if files.len() >= MAX_FILES {
            truncated = true;
            break;
        }
    }

    // Short-over-long, alphabetical for ties — gives a stable deterministic
    // baseline when the fuzzy query is empty.
    files.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));

    FileList {
        root: root.to_path_buf(),
        files,
        truncated,
    }
}

/// Apply a fuzzy filter. Returns `(path, score)` pairs sorted descending by
/// score. Empty query returns `min(limit, files.len())` items in walk order.
pub fn filter(files: &[String], query: &str, limit: usize) -> Vec<(String, i64)> {
    if query.is_empty() {
        return files.iter().take(limit).map(|p| (p.clone(), 0)).collect();
    }
    let matcher = SkimMatcherV2::default().ignore_case();
    let mut scored: Vec<(String, i64)> = files
        .iter()
        .filter_map(|p| matcher.fuzzy_match(p, query).map(|s| (p.clone(), s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.len().cmp(&b.0.len())));
    scored.truncate(limit);
    scored
}

/// Read the first ~40 lines or ~8 KiB of a file (whichever first) for the
/// preview pane. Returns `None` if the file can't be read or looks binary.
///
/// V3.b · bounded reads: `metadata()` is consulted first so we can bail on
/// large files *before* reading any bytes, and the actual read is a
/// `BufReader::take(MAX_BYTES)` instead of the whole-file `std::fs::read`
/// it used to be. Previously we'd pull a 100 MiB log into memory just to
/// reject it — no longer.
pub fn preview(root: &std::path::Path, rel: &str) -> Option<(String, String)> {
    use std::io::Read;

    const MAX_BYTES: usize = 8 * 1024;
    const MAX_LINES: usize = 40;
    const SIZE_CAP: u64 = 50 * 1024 * 1024;

    let path = root.join(rel);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > SIZE_CAP {
        return None;
    }
    let file = std::fs::File::open(&path).ok()?;
    let mut reader = std::io::BufReader::new(file).take(MAX_BYTES as u64);
    let mut bytes: Vec<u8> = Vec::with_capacity(MAX_BYTES);
    reader.read_to_end(&mut bytes).ok()?;
    // Binary-file heuristic: null byte in the sampled prefix.
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let clipped: String = text.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
    let lang = rel.rsplit('.').next().unwrap_or("").to_string();
    Some((clipped, lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_returns_prefix() {
        let files: Vec<String> = (0..200).map(|i| format!("f{i}.rs")).collect();
        let out = filter(&files, "", 50);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn fuzzy_matches_substrings() {
        let files = vec![
            "src/app.rs".to_string(),
            "src/rpc/codec.rs".to_string(),
            "Cargo.toml".to_string(),
            "README.md".to_string(),
        ];
        let out = filter(&files, "app", 10);
        assert!(!out.is_empty());
        assert_eq!(out[0].0, "src/app.rs");
    }

    #[test]
    fn fuzzy_is_case_insensitive() {
        let files = vec!["README.md".into()];
        let out = filter(&files, "readme", 10);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn empty_filter_returns_empty_when_no_files() {
        let out = filter(&[], "", 10);
        assert!(out.is_empty());
    }

    /// V3.b regression: a >50 MiB file must be rejected *before* any bytes
    /// are read. We create the file via `set_len` so the filesystem marks
    /// it sparse — if `preview` ever regresses to `std::fs::read` the test
    /// will still pass on a sparse FS, but the `metadata()` guard should
    /// fire regardless of how the file got that size.
    #[test]
    fn preview_rejects_files_over_size_cap_without_reading() {
        let dir = std::env::temp_dir().join(format!("tau-v3b-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("huge.txt");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(51 * 1024 * 1024).unwrap(); // 51 MiB, sparse
        drop(f);
        assert!(preview(&dir, "huge.txt").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// V3.b: a reasonably-sized file reads successfully and returns a
    /// bounded sample (no more than the documented MAX_LINES).
    #[test]
    fn preview_clips_to_max_lines() {
        let dir = std::env::temp_dir().join(format!("tau-v3b-clip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("many.txt");
        let content: String = (0..200).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, content).unwrap();
        let (sample, _lang) = preview(&dir, "many.txt").expect("preview should succeed");
        assert!(
            sample.lines().count() <= 40,
            "expected ≤40 lines, got {}",
            sample.lines().count()
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
