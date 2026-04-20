//! Built-in slash-command catalog.
//!
//! These are commands rata-pi handles *locally* — they're not part of pi's
//! `get_commands` response. We expose them as `CommandSource::Builtin` entries
//! so the Commands picker can mix them with pi's extensions / prompts / skills.
//!
//! Picking a built-in in the picker should *apply immediately* (not prefill
//! `/name ` into the composer); the app's modal Enter handler detects the
//! Builtin source and dispatches directly.

use crate::rpc::types::{CommandInfo, CommandSource};

/// Stable list of built-in commands. Names are shown with a leading `/`.
/// Descriptions fit one line in the picker's right-hand column.
pub fn builtins() -> Vec<CommandInfo> {
    const BUILTIN: CommandSource = CommandSource::Builtin;

    macro_rules! cmd {
        ($name:literal, $desc:literal) => {
            CommandInfo {
                name: $name.into(),
                description: Some($desc.into()),
                source: BUILTIN,
                location: None,
                path: None,
            }
        };
    }

    vec![
        // ── help / meta ───────────────────────────────────────────────────
        cmd!("help", "keybindings + commands cheat sheet"),
        cmd!("stats", "open the session stats modal"),
        cmd!("themes", "pick a color theme"),
        cmd!(
            "theme",
            "cycle (no arg) or apply by name: tokyo-night, dracula, …"
        ),
        // ── transcript / export ───────────────────────────────────────────
        cmd!("export", "write the transcript to a local markdown file"),
        cmd!("export-html", "ask pi to export the session to HTML"),
        cmd!("copy", "copy the last assistant message to the clipboard"),
        cmd!(
            "clear",
            "clear the transcript view (does not touch pi state)"
        ),
        // ── session ───────────────────────────────────────────────────────
        cmd!("rename", "set the display name: /rename my-feature"),
        cmd!("new", "start a fresh pi session"),
        cmd!(
            "switch",
            "load a session file: /switch /path/to/session.jsonl"
        ),
        cmd!("fork", "fork from an earlier user turn (picker)"),
        cmd!("compact", "compact context now (optional instructions arg)"),
        // ── model / thinking ──────────────────────────────────────────────
        cmd!("model", "open the model picker"),
        cmd!("think", "open the thinking-level picker"),
        cmd!("cycle-model", "advance to the next configured model"),
        cmd!("cycle-think", "advance to the next thinking level"),
        // ── pi runtime toggles ────────────────────────────────────────────
        cmd!("auto-compact", "toggle auto-compaction"),
        cmd!("auto-retry", "toggle auto-retry on transient errors"),
    ]
}

/// True if a command entry is one of ours (Builtin), not a pi command.
pub fn is_builtin(c: &CommandInfo) -> bool {
    c.source == CommandSource::Builtin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_non_empty_and_tagged() {
        let all = builtins();
        assert!(all.len() >= 15);
        assert!(all.iter().all(is_builtin));
        assert!(all.iter().any(|c| c.name == "theme"));
        assert!(all.iter().any(|c| c.name == "help"));
    }
}
