//! Command catalog for the Commands picker (F1 / `/`).
//!
//! Two kinds of entries:
//!
//! * **built-ins** — Tau's own slash commands; pushed eagerly so the
//!   picker shows more than just pi's skill list.
//! * **pi commands** — fetched at bootstrap via `get_commands`, wrapped in
//!   `MenuItem` at picker-open time.
//!
//! Each `MenuItem` carries a category for grouping, an optional argument
//! hint (`"<name>"`), and an example line that the detail pane on the right
//! of the modal renders.

use crate::rpc::types::{CommandInfo, CommandSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Session,
    Transcript,
    Model,
    PiRuntime,
    View,
    Git,
    Debug,
    Extension,
    Prompt,
    Skill,
    Theme,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Transcript => "transcript",
            Self::Model => "model",
            Self::PiRuntime => "pi runtime",
            Self::View => "view",
            Self::Git => "git",
            Self::Debug => "debug",
            Self::Extension => "extension",
            Self::Prompt => "prompt",
            Self::Skill => "skill",
            Self::Theme => "theme",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Session => "◆",
            Self::Transcript => "▤",
            Self::Model => "✦",
            Self::PiRuntime => "⚙",
            Self::View => "◉",
            Self::Git => "⎇",
            Self::Debug => "⌕",
            Self::Extension => "●",
            Self::Prompt => "▸",
            Self::Skill => "✧",
            Self::Theme => "◈",
        }
    }

    /// Sort order for grouping. Built-ins first, pi next.
    fn sort_key(self) -> u8 {
        match self {
            Self::Session => 0,
            Self::Transcript => 1,
            Self::Model => 2,
            Self::PiRuntime => 3,
            Self::View => 4,
            Self::Git => 5,
            Self::Theme => 6,
            Self::Debug => 7,
            Self::Extension => 8,
            Self::Prompt => 9,
            Self::Skill => 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: String,
    pub description: String,
    pub category: Category,
    pub args: &'static str,
    pub example: &'static str,
    pub source: CommandSource,
}

impl MenuItem {
    pub fn is_builtin(&self) -> bool {
        self.source == CommandSource::Builtin
    }

    pub fn is_theme(&self) -> bool {
        self.name.starts_with("theme ")
    }
}

/// Built-in slash-command catalog. Returns `MenuItem`s tagged Builtin.
pub fn builtins() -> Vec<MenuItem> {
    macro_rules! b {
        ($cat:expr, $name:literal, $desc:literal, $args:literal, $example:literal) => {
            MenuItem {
                name: $name.into(),
                description: $desc.into(),
                category: $cat,
                args: $args,
                example: $example,
                source: CommandSource::Builtin,
            }
        };
    }

    use Category::*;

    vec![
        // ── session ──────────────────────────────────────────────────────
        b!(
            Session,
            "help",
            "keybindings + commands cheat sheet",
            "",
            "/help"
        ),
        b!(
            Session,
            "stats",
            "open the session stats modal",
            "",
            "/stats"
        ),
        b!(
            Session,
            "rename",
            "set the session display name",
            "<name>",
            "/rename add-oauth"
        ),
        b!(Session, "new", "start a fresh pi session", "", "/new"),
        b!(
            Session,
            "switch",
            "load a session from a jsonl path",
            "<path>",
            "/switch ~/.pi/…/a.jsonl"
        ),
        b!(
            Session,
            "fork",
            "fork from an earlier user turn (picker)",
            "",
            "/fork"
        ),
        b!(
            Session,
            "snapshots",
            "list saved transcript snapshots (V2.9)",
            "",
            "/snapshots"
        ),
        // ── transcript / export ──────────────────────────────────────────
        b!(
            Transcript,
            "export",
            "write the transcript to a local markdown file",
            "",
            "/export"
        ),
        b!(
            Transcript,
            "export-html",
            "ask pi to export the session to HTML",
            "",
            "/export-html"
        ),
        b!(
            Transcript,
            "copy",
            "copy the last assistant message to the clipboard",
            "",
            "/copy"
        ),
        b!(
            Transcript,
            "clear",
            "clear the transcript view only (pi state intact)",
            "",
            "/clear"
        ),
        b!(
            Transcript,
            "retry",
            "re-submit the last user prompt",
            "",
            "/retry"
        ),
        // ── model / thinking ─────────────────────────────────────────────
        b!(Model, "model", "open the model picker", "", "/model"),
        b!(
            Model,
            "think",
            "open the thinking-level picker",
            "",
            "/think"
        ),
        b!(
            Model,
            "cycle-model",
            "advance to the next configured model",
            "",
            "/cycle-model"
        ),
        b!(
            Model,
            "cycle-think",
            "advance to the next thinking level",
            "",
            "/cycle-think"
        ),
        // ── pi runtime toggles / controls ────────────────────────────────
        b!(
            PiRuntime,
            "compact",
            "compact context now (optional instructions)",
            "[instructions]",
            "/compact focus on code"
        ),
        b!(
            PiRuntime,
            "auto-compact",
            "toggle auto-compaction",
            "",
            "/auto-compact"
        ),
        b!(
            PiRuntime,
            "auto-retry",
            "toggle auto-retry on transient errors",
            "",
            "/auto-retry"
        ),
        b!(
            PiRuntime,
            "abort",
            "abort the current streaming run",
            "",
            "/abort"
        ),
        b!(
            PiRuntime,
            "abort-bash",
            "abort the currently running bash command",
            "",
            "/abort-bash"
        ),
        b!(
            PiRuntime,
            "abort-retry",
            "cancel an in-progress auto-retry",
            "",
            "/abort-retry"
        ),
        b!(
            PiRuntime,
            "steer-mode",
            "set steering delivery mode",
            "<all | one-at-a-time>",
            "/steer-mode all"
        ),
        b!(
            PiRuntime,
            "follow-up-mode",
            "set follow-up delivery mode",
            "<all | one-at-a-time>",
            "/follow-up-mode one-at-a-time"
        ),
        // ── view / ui ────────────────────────────────────────────────────
        b!(
            View,
            "theme",
            "cycle (no arg) or apply by name",
            "[tokyo-night|dracula|…]",
            "/theme dracula"
        ),
        b!(View, "themes", "pick a color theme (picker)", "", "/themes"),
        b!(
            View,
            "find",
            "fuzzy file finder (Ctrl+P) — inserts @path",
            "[query]",
            "/find app"
        ),
        b!(
            View,
            "vim",
            "enable vim normal/insert composer mode",
            "",
            "/vim"
        ),
        b!(
            View,
            "default",
            "disable vim mode (Esc clears composer again)",
            "",
            "/default"
        ),
        b!(
            View,
            "plan",
            "open the plan view (no-arg) or manage steps",
            "[set|add|done|fail|next|clear|auto]",
            "/plan set fetch repo | parse output | write tests"
        ),
        // ── git ──────────────────────────────────────────────────────────
        b!(Git, "status", "git status summary", "", "/status"),
        b!(
            Git,
            "diff",
            "diff viewer (unstaged, or --staged)",
            "[--staged]",
            "/diff --staged"
        ),
        b!(
            Git,
            "git-log",
            "recent commits picker",
            "[n]",
            "/git-log 20"
        ),
        b!(Git, "branch", "list + switch branches", "", "/branch"),
        b!(
            Git,
            "switch-branch",
            "checkout a branch by name",
            "<name>",
            "/switch-branch feat/x"
        ),
        b!(
            Git,
            "commit",
            "commit tracked changes with message",
            "<message>",
            "/commit wire it up"
        ),
        b!(Git, "stash", "git stash push", "", "/stash"),
        // ── session · settings/shortcuts (V2.13) ─────────────────────────
        // V3.e.4 · aliases baked into the description so the picker's
        // substring-match filter surfaces the row when a user types
        // `/prefs` or `/keys` without remembering the canonical name.
        b!(
            Session,
            "settings",
            "every tunable setting + live state (aliases: /prefs · /preferences)",
            "",
            "/settings"
        ),
        b!(
            Session,
            "shortcuts",
            "read-only keybinding reference (aliases: /keys · /hotkeys)",
            "",
            "/shortcuts"
        ),
        // V3.j.4 · composer templates.
        b!(
            Session,
            "template",
            "save / use / list / delete composer templates (alias: /tpl)",
            "<save|use|list|delete> [name]",
            "/template save review-pr"
        ),
        // V3.j.3 · transcript search (MVP: jump-to-latest-match).
        b!(
            Transcript,
            "search",
            "find text in the transcript and focus the most recent match",
            "<text>",
            "/search insufficient credits"
        ),
        // ── debug ────────────────────────────────────────────────────────
        b!(
            Debug,
            "doctor",
            "readiness modal (pi, terminal, clipboard, git, notifications)",
            "",
            "/doctor"
        ),
        b!(
            Debug,
            "mcp",
            "list MCP servers (if pi exposes them)",
            "",
            "/mcp"
        ),
        b!(
            Debug,
            "notify",
            "toggle desktop notifications (OSC 777 + native if built with `notify`)",
            "",
            "/notify"
        ),
        b!(Debug, "version", "show Tau version", "", "/version"),
        b!(Debug, "log", "show the log file path", "", "/log"),
        b!(
            Debug,
            "env",
            "show relevant environment variables",
            "",
            "/env"
        ),
    ]
}

/// Wrap pi-fetched commands in `MenuItem`s for mixing with built-ins.
pub fn wrap_pi(cmds: &[CommandInfo]) -> Vec<MenuItem> {
    cmds.iter()
        .map(|c| {
            let category = match c.source {
                CommandSource::Extension => Category::Extension,
                CommandSource::Prompt => Category::Prompt,
                CommandSource::Skill => Category::Skill,
                CommandSource::Builtin => Category::Extension, // shouldn't happen
            };
            MenuItem {
                name: c.name.clone(),
                description: c.description.clone().unwrap_or_default(),
                category,
                args: "",
                example: "",
                source: c.source,
            }
        })
        .collect()
}

/// Merge built-ins + pi commands for the picker. Built-ins first (by
/// `Category::sort_key`), pi commands second.
pub fn merged_menu(pi_cmds: &[CommandInfo]) -> Vec<MenuItem> {
    let mut items = builtins();
    items.extend(wrap_pi(pi_cmds));
    items.sort_by(|a, b| {
        a.category
            .sort_key()
            .cmp(&b.category.sort_key())
            .then_with(|| a.name.cmp(&b.name))
    });
    items
}

/// Menu items built for the /themes picker — not mixed with commands.
pub fn theme_items<I>(themes: I) -> Vec<MenuItem>
where
    I: IntoIterator<Item = &'static str>,
{
    themes
        .into_iter()
        .map(|name| MenuItem {
            name: format!("theme {name}"),
            description: format!("switch to {name}"),
            category: Category::Theme,
            args: "",
            example: "",
            source: CommandSource::Builtin,
        })
        .collect()
}

/// Case-insensitive match against both name and description.
pub fn matches(item: &MenuItem, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_ascii_lowercase();
    item.name.to_ascii_lowercase().contains(&q)
        || item.description.to_ascii_lowercase().contains(&q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_non_empty_and_all_marked_builtin() {
        let all = builtins();
        assert!(all.len() >= 25);
        for it in &all {
            assert!(it.is_builtin(), "{} should be builtin", it.name);
        }
    }

    #[test]
    fn merged_menu_keeps_builtins_first() {
        let pi = vec![CommandInfo {
            name: "skill:x".into(),
            description: Some("from pi".into()),
            source: CommandSource::Skill,
            location: None,
            path: None,
        }];
        let m = merged_menu(&pi);
        assert!(m.iter().any(|i| i.name == "help"));
        assert!(m.iter().any(|i| i.name == "skill:x"));
        // first entry should be Session / Transcript category (not Skill).
        let first_cat = m.first().unwrap().category;
        assert_ne!(first_cat, Category::Skill);
    }

    #[test]
    fn matches_name_and_description() {
        let it = MenuItem {
            name: "rename".into(),
            description: "set the session display name".into(),
            category: Category::Session,
            args: "<name>",
            example: "/rename x",
            source: CommandSource::Builtin,
        };
        assert!(matches(&it, "rename"));
        assert!(matches(&it, "display"));
        assert!(matches(&it, "SESSION")); // case-insensitive
        assert!(!matches(&it, "xyz"));
    }

    /// V3.e.4 · users typing `/keys` or `/prefs` in the picker must see
    /// the /shortcuts / /settings rows surface, even though those are the
    /// canonical names. Aliases are embedded in each description so the
    /// substring filter catches them.
    #[test]
    fn picker_surfaces_rows_via_alias_hints() {
        let all = builtins();
        let shortcuts = all
            .iter()
            .find(|i| i.name == "shortcuts")
            .expect("shortcuts row should exist");
        let settings = all
            .iter()
            .find(|i| i.name == "settings")
            .expect("settings row should exist");
        assert!(
            matches(shortcuts, "keys"),
            "filter `keys` misses shortcuts row"
        );
        assert!(
            matches(shortcuts, "hotkeys"),
            "filter `hotkeys` misses shortcuts row"
        );
        assert!(
            matches(settings, "prefs"),
            "filter `prefs` misses settings row"
        );
        assert!(
            matches(settings, "preferences"),
            "filter `preferences` misses settings row"
        );
    }

    #[test]
    fn theme_items_are_themes() {
        let it = theme_items(["tokyo-night", "dracula"]);
        assert_eq!(it.len(), 2);
        assert!(it.iter().all(|i| i.is_theme()));
        assert!(it[0].name.starts_with("theme "));
    }
}
