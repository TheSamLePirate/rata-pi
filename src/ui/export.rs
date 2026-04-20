//! Transcript → Markdown export.
//!
//! Writes a timestamped `.md` file under the project data dir, capturing the
//! session as you'd skim it — user/assistant turns, thinking, tool calls with
//! final output, bash, info/warn/error rows, retries, and compaction.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;

use crate::ui::transcript::{
    BashExec, Compaction, CompactionState, Entry, Retry, RetryState, ToolCall, ToolStatus,
    Transcript,
};

pub fn export(transcript: &Transcript) -> std::io::Result<PathBuf> {
    let dir = resolve_dir();
    fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("session-{stamp}.md"));
    let md = render(transcript);
    fs::write(&path, md)?;
    Ok(path)
}

fn resolve_dir() -> PathBuf {
    ProjectDirs::from("dev", "olivvein", "rata-pi")
        .map(|d| d.data_local_dir().join("exports"))
        .unwrap_or_else(|| std::env::temp_dir().join("rata-pi-exports"))
}

pub fn render(transcript: &Transcript) -> String {
    let mut out = String::new();
    out.push_str("# rata-pi session\n\n");
    for e in transcript.entries() {
        match e {
            Entry::User(s) => {
                out.push_str("## User\n\n");
                out.push_str(s);
                out.push_str("\n\n");
            }
            Entry::Thinking(s) => {
                out.push_str("### _thinking_\n\n");
                for line in s.lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }
            Entry::Assistant(s) => {
                out.push_str("## Assistant\n\n");
                out.push_str(s);
                out.push_str("\n\n");
            }
            Entry::ToolCall(tc) => render_tool(&mut out, tc),
            Entry::BashExec(bx) => render_bash(&mut out, bx),
            Entry::Info(s) => {
                out.push_str(&format!("_· {s}_\n\n"));
            }
            Entry::Warn(s) => {
                out.push_str(&format!("**⚠ {s}**\n\n"));
            }
            Entry::Error(s) => {
                out.push_str(&format!("**✗ {s}**\n\n"));
            }
            Entry::Compaction(c) => render_compaction(&mut out, c),
            Entry::Retry(r) => render_retry(&mut out, r),
            Entry::TurnMarker { number } => {
                out.push_str(&format!("\n---\n\n**turn {number}**\n\n"));
            }
        }
    }
    out
}

fn render_tool(out: &mut String, tc: &ToolCall) {
    let sym = match tc.status {
        ToolStatus::Running => "…",
        ToolStatus::Ok => "✓",
        ToolStatus::Err => "✗",
    };
    out.push_str(&format!("### Tool {sym} `{}`\n\n", tc.name));
    if !tc.args.is_null() {
        out.push_str("```json\n");
        out.push_str(
            &serde_json::to_string_pretty(&tc.args).unwrap_or_else(|_| tc.args.to_string()),
        );
        out.push_str("\n```\n\n");
    }
    if !tc.output.trim().is_empty() {
        out.push_str("```\n");
        out.push_str(&crate::ui::ansi::strip(&tc.output));
        if !tc.output.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
}

fn render_bash(out: &mut String, bx: &BashExec) {
    let status = if bx.cancelled {
        "cancelled".to_string()
    } else {
        format!("exit {}", bx.exit_code)
    };
    out.push_str(&format!("### `$ {}` — {status}\n\n", bx.command));
    if !bx.output.is_empty() {
        out.push_str("```\n");
        out.push_str(&crate::ui::ansi::strip(&bx.output));
        if !bx.output.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    if bx.truncated {
        if let Some(p) = &bx.full_output_path {
            out.push_str(&format!("_(output truncated — full log: `{p}`)_\n\n"));
        } else {
            out.push_str("_(output truncated)_\n\n");
        }
    }
}

fn render_compaction(out: &mut String, c: &Compaction) {
    match &c.state {
        CompactionState::Running => {
            out.push_str(&format!("_compacting ({})…_\n\n", c.reason));
        }
        CompactionState::Done { summary } => {
            out.push_str(&format!("_compacted ({})_\n\n", c.reason));
            if let Some(s) = summary {
                out.push_str("> ");
                out.push_str(s);
                out.push_str("\n\n");
            }
        }
        CompactionState::Aborted => {
            out.push_str(&format!("_compaction aborted ({})_\n\n", c.reason));
        }
        CompactionState::Failed(msg) => {
            out.push_str(&format!("**compaction failed ({}): {msg}**\n\n", c.reason));
        }
    }
}

fn render_retry(out: &mut String, r: &Retry) {
    match &r.state {
        RetryState::Waiting { delay_ms, error } => {
            out.push_str(&format!(
                "_retry {}/{} in {}ms — {error}_\n\n",
                r.attempt, r.max_attempts, delay_ms
            ));
        }
        RetryState::Succeeded => {
            out.push_str(&format!("_retry {} succeeded_\n\n", r.attempt));
        }
        RetryState::Exhausted(msg) => {
            out.push_str(&format!("**retry exhausted at {}: {msg}**\n\n", r.attempt));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_basic_turn() {
        let mut t = Transcript::default();
        t.push(Entry::User("hi".into()));
        t.push(Entry::Assistant("hello **world**".into()));
        let md = render(&t);
        assert!(md.contains("## User"));
        assert!(md.contains("hi"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("**world**"));
    }

    #[test]
    fn renders_bash_with_exit_code() {
        let mut t = Transcript::default();
        t.push(Entry::BashExec(BashExec {
            command: "ls".into(),
            output: "a\nb\n".into(),
            exit_code: 0,
            cancelled: false,
            truncated: false,
            full_output_path: None,
        }));
        let md = render(&t);
        assert!(md.contains("$ ls"));
        assert!(md.contains("exit 0"));
        assert!(md.contains("a\nb"));
    }
}
