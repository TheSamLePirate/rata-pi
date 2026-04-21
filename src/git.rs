//! Thin wrapper over the `git` binary. All calls are synchronous but fast —
//! the caller decides whether to invoke them off a user gesture (modal open)
//! or on a low-rate timer (header chip refresh).
//!
//! We intentionally skip `git2` (libgit2): shell-out is smaller, has no C
//! dependency, and `git` is on every dev machine.

use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
}

impl GitStatus {
    pub fn dirty(&self) -> bool {
        self.staged > 0 || self.unstaged > 0 || self.untracked > 0
    }
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub name: String,
    pub current: bool,
}

/// Run `git` with args, return stdout on success.
fn git(args: &[&str]) -> Result<String, String> {
    match Command::new("git").args(args).output() {
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Run `git` with args but give up after `timeout` (kept small for the
/// refresh path so a wedged repo can't freeze the UI tick).
fn git_timeout(args: &[&str], timeout: Duration) -> Result<String, String> {
    use std::sync::mpsc;
    use std::thread;
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let r = git(&args.iter().map(String::as_str).collect::<Vec<_>>());
        let _ = tx.send(r);
    });
    rx.recv_timeout(timeout)
        .map_err(|_| "git timed out".to_string())
        .and_then(|r| r)
}

#[allow(dead_code)] // exposed for future callers / tests
pub fn is_repo() -> bool {
    git_timeout(
        &["rev-parse", "--is-inside-work-tree"],
        Duration::from_millis(500),
    )
    .map(|s| s.trim() == "true")
    .unwrap_or(false)
}

/// Read a lightweight status summary. Uses `git status --porcelain=v2 --branch`.
pub fn status() -> GitStatus {
    let mut st = GitStatus::default();
    let out = match git_timeout(
        &["status", "--porcelain=v2", "--branch"],
        Duration::from_millis(1000),
    ) {
        Ok(o) => o,
        Err(_) => return st,
    };
    st.is_repo = true;

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest != "(detached)" {
                st.branch = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // "+<ahead> -<behind>"
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    st.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    st.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(stripped) = line.strip_prefix("1 ") {
            // Ordinary changed entry: `1 XY <...>`.
            classify_file_entry(&mut st, stripped);
        } else if let Some(stripped) = line.strip_prefix("2 ") {
            classify_file_entry(&mut st, stripped);
        } else if line.starts_with("? ") {
            st.untracked = st.untracked.saturating_add(1);
        }
    }
    st
}

fn classify_file_entry(st: &mut GitStatus, rest: &str) {
    // First two chars are XY — X is index (staged), Y is worktree (unstaged).
    let xy = rest.as_bytes();
    if xy.len() < 2 {
        return;
    }
    let (x, y) = (xy[0] as char, xy[1] as char);
    if x != '.' && x != ' ' {
        st.staged = st.staged.saturating_add(1);
    }
    if y != '.' && y != ' ' {
        st.unstaged = st.unstaged.saturating_add(1);
    }
}

/// `git diff` or `git diff --cached`.
pub fn diff(staged: bool) -> Result<String, String> {
    let mut args = vec!["--no-pager", "diff", "--no-color"];
    if staged {
        args.push("--cached");
    }
    git(&args)
}

pub fn log(n: u32) -> Result<Vec<Commit>, String> {
    let n_str = n.to_string();
    // Use ISO-like date + compact author via pretty format. Field separator
    // is the ASCII unit separator 0x1F so message bodies can't break it.
    let fmt = "%h\x1f%an\x1f%ar\x1f%s";
    let out = git(&[
        "--no-pager",
        "log",
        "-n",
        &n_str,
        "--pretty=format:",
        &format!("--pretty=format:{fmt}"),
    ])?;
    let mut commits = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split('\x1f').collect();
        if parts.len() == 4 {
            commits.push(Commit {
                hash: parts[0].to_string(),
                author: parts[1].to_string(),
                date: parts[2].to_string(),
                subject: parts[3].to_string(),
            });
        }
    }
    Ok(commits)
}

pub fn branches() -> Result<Vec<Branch>, String> {
    let out = git(&[
        "for-each-ref",
        "--format=%(HEAD) %(refname:short)",
        "refs/heads/",
    ])?;
    let mut list = Vec::new();
    for line in out.lines() {
        let line = line.trim_end();
        let (marker, name) = line.split_at(1);
        list.push(Branch {
            name: name.trim().to_string(),
            current: marker.trim() == "*",
        });
    }
    Ok(list)
}

pub fn commit_all(msg: &str) -> Result<String, String> {
    git(&["commit", "-a", "-m", msg])
}

pub fn stash() -> Result<String, String> {
    git(&["stash", "push"])
}

pub fn switch(name: &str) -> Result<String, String> {
    git(&["switch", name])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_staged_and_unstaged() {
        // Porcelain v2 entry: after stripping the "1 " / "2 " prefix, the
        // two leading chars ARE the XY status code (no space between).
        let mut s = GitStatus::default();
        classify_file_entry(&mut s, "MM N... 100644 100644 100644 <h1> <h2> path");
        assert_eq!(s.staged, 1);
        assert_eq!(s.unstaged, 1);
        classify_file_entry(&mut s, ".M N... 100644 100644 100644 <h1> <h2> path");
        assert_eq!(s.staged, 1);
        assert_eq!(s.unstaged, 2);
        classify_file_entry(&mut s, "M. N... 100644 100644 100644 <h1> <h2> path");
        assert_eq!(s.staged, 2);
    }

    #[test]
    fn dirty_reflects_counts() {
        let mut s = GitStatus::default();
        assert!(!s.dirty());
        s.untracked = 1;
        assert!(s.dirty());
    }
}
