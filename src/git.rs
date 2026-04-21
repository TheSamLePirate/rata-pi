//! Thin wrapper over the `git` binary. Every call is async so the TUI tick
//! loop can await without blocking the Tokio executor.
//!
//! We intentionally skip `git2` (libgit2): shell-out is smaller, has no C
//! dependency, and `git` is on every dev machine.

use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

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

/// Run `git` with args, return stdout on success. Purely async — `git`
/// runs as a child via tokio's process module.
async fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Run `git` with a wall-clock ceiling. Used on the hot path (refresh ticker)
/// where a wedged repo would otherwise stall the UI.
async fn git_timeout(args: &[&str], dur: Duration) -> Result<String, String> {
    match timeout(dur, git(args)).await {
        Ok(r) => r,
        Err(_) => Err("git timed out".into()),
    }
}

/// Read a lightweight status summary. Uses `git status --porcelain=v2 --branch`.
pub async fn status() -> GitStatus {
    let mut st = GitStatus::default();
    let out = match git_timeout(
        &["status", "--porcelain=v2", "--branch"],
        Duration::from_millis(1000),
    )
    .await
    {
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
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    st.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    st.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(stripped) = line.strip_prefix("1 ") {
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

pub async fn diff(staged: bool) -> Result<String, String> {
    let mut args = vec!["--no-pager", "diff", "--no-color"];
    if staged {
        args.push("--cached");
    }
    git(&args).await
}

pub async fn log(n: u32) -> Result<Vec<Commit>, String> {
    let n_str = n.to_string();
    // Field separator is the ASCII unit separator 0x1F so message bodies
    // can't break it.
    let fmt = "%h\x1f%an\x1f%ar\x1f%s";
    let out = git(&[
        "--no-pager",
        "log",
        "-n",
        &n_str,
        "--pretty=format:",
        &format!("--pretty=format:{fmt}"),
    ])
    .await?;
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

pub async fn branches() -> Result<Vec<Branch>, String> {
    let out = git(&[
        "for-each-ref",
        "--format=%(HEAD) %(refname:short)",
        "refs/heads/",
    ])
    .await?;
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

pub async fn commit_all(msg: &str) -> Result<String, String> {
    git(&["commit", "-a", "-m", msg]).await
}

pub async fn stash() -> Result<String, String> {
    git(&["stash", "push"]).await
}

pub async fn switch(name: &str) -> Result<String, String> {
    git(&["switch", name]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_staged_and_unstaged() {
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
