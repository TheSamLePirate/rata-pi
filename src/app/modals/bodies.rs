//! Modal body renderers — extracted from `app/mod.rs` in V4.d.1.
//!
//! Every `Modal::*` variant's body-building function lives here.
//! `draw_modal` in `mod.rs` calls into this module for each variant,
//! reusing the shared `centered` popup + scrollbar + two-pane layout
//! plumbing that already lived there.
//!
//! Helpers that exist solely to support one body (`split_cursor`,
//! `context_snippet`, `commands_selected_line`, `status_row`,
//! `which_pi`, `doctor_checks`, `mcp_rows`) move with their consumers
//! so `mod.rs` only imports what it actually still calls.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::history::HistoryEntry;
use crate::rpc::types::{ForkMessage, Model, SessionStats, ThinkingLevel};
use crate::theme::Theme;
use crate::ui::modal::{ListModal, RadioModal};
use crate::ui::transcript::{Entry, Transcript};

use super::super::draw::kb;
use super::super::helpers::truncate_preview;
use super::super::{App, filtered_commands, filtered_forks, filtered_history, filtered_models};

/// Compute the terminal-row index of the selected item in `commands_text`.
/// Mirrors the category-grouping + description-line layout so the scroll
/// computation can keep the selected item centered.
/// Left-pane body for the FileFinder modal. Width-aware truncation so the
/// scroll math remains one-row-per-item.
pub(crate) fn file_finder_text(
    ff: &crate::ui::modal::FileFinder,
    t: &Theme,
    list_width: u16,
) -> Text<'static> {
    // Reads directly from `ff.filtered`, which `prepare_frame_caches`
    // keeps in sync with the query. No matcher runs inside this function.
    let mut lines: Vec<Line<'static>> = Vec::new();
    let scored = &ff.filtered;

    let hint_bits = if ff.loading {
        "   (indexing files…)".to_string()
    } else if ff.files.truncated {
        format!(
            "   ({} / {}+ files truncated)",
            scored.len(),
            crate::files::MAX_FILES
        )
    } else {
        format!("   ({} items)", scored.len())
    };

    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            ff.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(hint_bits, Style::default().fg(t.dim)),
    ]));
    lines.push(Line::default());

    if ff.loading && scored.is_empty() {
        lines.push(Line::from(Span::styled(
            "walking repo (respects .gitignore)…",
            Style::default().fg(t.dim),
        )));
        return Text::from(lines);
    }

    if scored.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
        return Text::from(lines);
    }

    let max = list_width as usize;
    for (i, (path, _score)) in scored.iter().enumerate() {
        let is_sel = i == ff.selected;
        let marker = if is_sel { "▸" } else { " " };
        let style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let display = if path.chars().count() + 4 > max {
            // Prefer showing the filename over the path prefix.
            let name = path.rsplit('/').next().unwrap_or(path);
            let prefix_len = max.saturating_sub(name.chars().count() + 5);
            if prefix_len >= 3 {
                let prefix: String = path.chars().take(prefix_len.saturating_sub(2)).collect();
                format!("{prefix}…/{name}")
            } else {
                truncate_preview(name, max.saturating_sub(2))
            }
        } else {
            path.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {marker} "), style),
            Span::styled(display, style),
        ]));
    }
    Text::from(lines)
}

/// Right-pane preview of the selected file: first ~40 lines, syntect-
/// highlighted by extension. Reads purely from `ff.preview_cache`, which
/// V4.c · left-pane body for the template picker. One row per saved
/// template, `▶` marker on the selected row.
pub(crate) fn templates_text(
    list: &crate::ui::modal::ListModal<crate::ui::modal::Template>,
    t: &Theme,
) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(" templates · {} saved ", list.items.len()),
        Style::default().fg(t.muted),
    )));
    lines.push(Line::default());
    for (i, tpl) in list.items.iter().enumerate() {
        let focused = i == list.selected;
        let marker = if focused { "▶" } else { " " };
        let marker_style = if focused {
            Style::default()
                .fg(t.accent_strong)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.dim)
        };
        let name_style = if focused {
            Style::default().fg(t.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        // Preview: first non-blank line of the body, truncated.
        let first = tpl
            .body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .chars()
            .take(40)
            .collect::<String>();
        lines.push(Line::from(vec![
            Span::styled(format!("  {marker} "), marker_style),
            Span::styled(format!("{:<18}", tpl.name), name_style),
            Span::styled(format!(" · {first}"), Style::default().fg(t.muted)),
        ]));
    }
    Text::from(lines)
}

/// V4.c · right-pane body preview for the focused template.
pub(crate) fn template_preview_lines(
    list: &crate::ui::modal::ListModal<crate::ui::modal::Template>,
    t: &Theme,
) -> Vec<Line<'static>> {
    let Some(tpl) = list.items.get(list.selected) else {
        return vec![Line::from(Span::styled(
            "(no selection)",
            Style::default().fg(t.dim),
        ))];
    };
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        Span::styled("  ◇ ", Style::default().fg(t.accent)),
        Span::styled(
            tpl.name.clone(),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
    ]));
    out.push(Line::default());
    for ln in tpl.body.lines().take(40) {
        out.push(Line::from(Span::styled(
            ln.to_string(),
            Style::default().fg(t.text),
        )));
    }
    if tpl.body.lines().count() > 40 {
        out.push(Line::default());
        out.push(Line::from(Span::styled(
            format!("…({} more lines)", tpl.body.lines().count() - 40),
            Style::default().fg(t.dim),
        )));
    }
    out
}

/// `prepare_frame_caches` populates on selection changes. The file system
/// is NOT touched inside this function.
pub(crate) fn file_preview_lines(
    ff: &crate::ui::modal::FileFinder,
    t: &Theme,
) -> Vec<Line<'static>> {
    let Some(cache) = ff.preview_cache.as_ref() else {
        let msg = if ff.loading {
            "(waiting for index…)"
        } else {
            "(no selection)"
        };
        return vec![Line::from(Span::styled(msg, Style::default().fg(t.dim)))];
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  ▤ ", Style::default().fg(t.accent)),
        Span::styled(
            cache.path.clone(),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    lines.extend(cache.lines.iter().cloned());
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "  Enter inserts @path · Esc closes",
        Style::default().fg(t.dim),
    )));
    lines
}

// ─────────────────────────────────────────────────────────── git bodies ──

pub(crate) fn diff_body_lines(d: &crate::ui::modal::DiffView, t: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(vec![Span::styled(
        format!("  {}", d.title),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )]));
    out.push(Line::default());
    if d.diff.trim().is_empty() {
        out.push(Line::from(Span::styled(
            "  (no changes)",
            Style::default().fg(t.dim),
        )));
        return out;
    }
    out.extend(crate::ui::diff::render(&d.diff, t));
    out
}

pub(crate) fn git_status_body(s: &crate::git::GitStatus, t: &Theme) -> Vec<Line<'static>> {
    if !s.is_repo {
        return vec![Line::from(Span::styled(
            "  (not a git repository)",
            Style::default().fg(t.dim),
        ))];
    }
    let mut out = Vec::new();
    let dirty_dot = if s.dirty() {
        Span::styled(
            " ●",
            Style::default().fg(t.warning).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" ○", Style::default().fg(t.success))
    };
    out.push(Line::from(vec![
        Span::styled(
            "  branch  ",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            s.branch.clone().unwrap_or_else(|| "(detached)".into()),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        dirty_dot,
    ]));
    out.push(Line::from(vec![Span::styled(
        format!("          ahead {} · behind {}", s.ahead, s.behind,),
        Style::default().fg(t.dim),
    )]));
    out.push(Line::default());
    out.push(status_row("staged", s.staged, t.diff_add, t));
    out.push(status_row("unstaged", s.unstaged, t.diff_remove, t));
    out.push(status_row("untracked", s.untracked, t.warning, t));
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  hint: /diff · /diff --staged · /log · /branch · /commit <msg> · /stash",
        Style::default().fg(t.dim),
    )));
    out
}

/// Build the readiness-check rows for the /doctor modal.
pub(crate) fn doctor_checks(app: &App) -> Vec<crate::ui::modal::DoctorCheck> {
    use crate::ui::modal::{DoctorCheck, DoctorStatus};
    // V3.b · read cached values from AppCaps instead of re-probing each
    // time /doctor opens (clipboard handle creation in particular was
    // needlessly expensive on some platforms).
    let caps = app.caps.term;
    let mut rows = Vec::new();

    // pi on PATH
    rows.push(DoctorCheck {
        label: "pi binary",
        status: if app.caps.pi_binary.is_some() {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Fail
        },
        detail: app
            .caps
            .pi_binary
            .clone()
            .unwrap_or_else(|| "not found on PATH".into()),
    });

    // pi connection live
    rows.push(DoctorCheck {
        label: "pi connection",
        status: if app.spawn_error.is_some() {
            DoctorStatus::Fail
        } else {
            DoctorStatus::Pass
        },
        detail: app
            .spawn_error
            .clone()
            .unwrap_or_else(|| "connected".into()),
    });

    // terminal
    rows.push(DoctorCheck {
        label: "terminal",
        status: DoctorStatus::Info,
        detail: format!("{:?}", caps.kind),
    });
    rows.push(DoctorCheck {
        label: "kitty keyboard",
        status: if caps.kitty_keyboard {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Warn
        },
        detail: if caps.kitty_keyboard {
            "enabled (Ctrl+Shift+T disambiguated)".into()
        } else {
            "not advertised (Alt+T / F12 fallbacks)".into()
        },
    });
    rows.push(DoctorCheck {
        label: "graphics",
        status: if caps.graphics {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Info
        },
        detail: if caps.graphics {
            "supported".into()
        } else {
            "no image protocol detected".into()
        },
    });

    // clipboard (cached in AppCaps — no re-probe per /doctor open)
    rows.push(DoctorCheck {
        label: "clipboard",
        status: if app.caps.clipboard_native {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Warn
        },
        detail: if app.caps.clipboard_native {
            "arboard (native)".into()
        } else {
            "OSC 52 fallback".into()
        },
    });

    // git
    let in_repo = app.git_status.as_ref().map(|s| s.is_repo).unwrap_or(false);
    rows.push(DoctorCheck {
        label: "git",
        status: if in_repo {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Info
        },
        detail: if in_repo {
            app.git_status
                .as_ref()
                .and_then(|s| s.branch.clone())
                .unwrap_or_else(|| "(detached)".into())
        } else {
            "not a git repository".into()
        },
    });

    // theme
    rows.push(DoctorCheck {
        label: "theme",
        status: DoctorStatus::Info,
        detail: app.theme.name.to_string(),
    });

    // notifications
    let notify_feat = cfg!(feature = "notify");
    rows.push(DoctorCheck {
        label: "notifications",
        status: if app.notify_enabled {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Info
        },
        detail: format!(
            "{} · osc777 always · native {}",
            if app.notify_enabled { "on" } else { "off" },
            if notify_feat {
                "feature enabled"
            } else {
                "feature disabled"
            }
        ),
    });

    rows
}

/// Check whether `pi` is resolvable on PATH; return the first match path.
pub(crate) fn which_pi() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("pi");
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Render doctor rows as styled lines.
pub(crate) fn doctor_body(
    checks: &[crate::ui::modal::DoctorCheck],
    t: &Theme,
) -> Vec<Line<'static>> {
    use crate::ui::modal::DoctorStatus;
    let mut out = Vec::with_capacity(checks.len() + 2);
    out.push(Line::default());
    for c in checks {
        let (glyph, color) = match c.status {
            DoctorStatus::Pass => ("✓", t.success),
            DoctorStatus::Warn => ("▲", t.warning),
            DoctorStatus::Fail => ("✗", t.error),
            DoctorStatus::Info => ("·", t.dim),
        };
        out.push(Line::from(vec![
            Span::styled(
                format!("  {glyph}  "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<16}", c.label),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(c.detail.clone(), Style::default().fg(t.muted)),
        ]));
    }
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  hint: /notify toggles · /mcp shows MCP servers · Esc closes",
        Style::default().fg(t.dim),
    )));
    out
}

/// Build MCP rows. pi 0.x does not currently expose MCP servers via the
/// JSONL RPC, so we ship a single informational row. Future-proof: when pi
/// adds `get_mcp_servers`, populate this list from its response.
pub(crate) fn mcp_rows(_app: &App) -> Vec<crate::ui::modal::McpRow> {
    use crate::ui::modal::{McpRow, McpStatus};
    vec![McpRow {
        name: "mcp info".into(),
        status: McpStatus::Unknown,
        detail: "pi does not expose MCP server state over RPC yet".into(),
    }]
}

pub(crate) fn mcp_body(rows: &[crate::ui::modal::McpRow], t: &Theme) -> Vec<Line<'static>> {
    use crate::ui::modal::McpStatus;
    let mut out = Vec::with_capacity(rows.len() + 2);
    out.push(Line::default());
    for r in rows {
        let (glyph, color) = match r.status {
            McpStatus::Connected => ("●", t.success),
            McpStatus::Disconnected => ("○", t.error),
            McpStatus::Unknown => ("·", t.dim),
        };
        out.push(Line::from(vec![
            Span::styled(
                format!("  {glyph}  "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<18}", r.name),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(r.detail.clone(), Style::default().fg(t.muted)),
        ]));
    }
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  hint: MCP servers are configured in pi's settings, not Tau",
        Style::default().fg(t.dim),
    )));
    out
}

/// Render the Interview modal body: header with title / description,
/// then one block per field. Focused interactive field gets a `▶`
/// marker + accent color; required-but-empty fields get a red chip.
/// V2.13.a · read-only keybinding reference. One entry per key; grouped
/// into sections that match the app's input surfaces.
pub(crate) fn shortcuts_body(t: &Theme) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    let section = |out: &mut Vec<Line<'static>>, title: &str| {
        out.push(Line::default());
        out.push(Line::from(vec![
            Span::styled(
                format!("  {title}  "),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(60), Style::default().fg(t.dim)),
        ]));
    };
    let row = |out: &mut Vec<Line<'static>>, keys: &str, action: &str| {
        out.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}  ", keys),
                Style::default()
                    .fg(t.accent_strong)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(action.to_string(), Style::default().fg(t.text)),
        ]));
    };

    out.push(Line::from(Span::styled(
        "  Every keyboard action Tau responds to.",
        Style::default().fg(t.muted),
    )));

    section(&mut out, "Global");
    row(&mut out, "Ctrl+C", "quit");
    row(&mut out, "Ctrl+D", "quit");

    section(&mut out, "Editor (idle — no modal)");
    row(&mut out, "Enter", "submit prompt");
    row(&mut out, "Shift+Enter / Ctrl+J", "insert newline");
    row(&mut out, "Esc (streaming)", "abort current run");
    row(&mut out, "Esc (empty composer)", "quit");
    row(&mut out, "Esc (composer has text)", "clear composer");
    row(&mut out, "Ctrl+F", "enter focus mode");
    row(&mut out, "Ctrl+T", "toggle thinking visibility");
    row(&mut out, "Alt+T / Ctrl+Shift+T / F12", "cycle theme");
    row(&mut out, "Ctrl+E", "toggle expand on last tool card");
    row(&mut out, "Ctrl+P", "fuzzy file finder");
    row(&mut out, "Ctrl+Y", "copy last assistant message");
    row(&mut out, "Ctrl+R", "prompt-history picker");
    row(&mut out, "Ctrl+S", "export transcript to markdown");
    row(&mut out, "Ctrl+Space (streaming)", "cycle composer intent");
    row(&mut out, "F1 / /", "commands modal");
    row(&mut out, "F5", "model picker");
    row(&mut out, "F6", "thinking-level picker");
    row(&mut out, "F7", "stats modal");
    row(&mut out, "F8", "compact context now");
    row(&mut out, "F9", "toggle auto-compaction");
    row(&mut out, "F10", "toggle auto-retry");
    row(&mut out, "?", "help modal");
    row(&mut out, "↑ / ↓", "prompt history prev / next");
    row(&mut out, "PgUp / PgDn", "scroll transcript ±5");
    row(&mut out, "End (empty composer)", "re-pin live tail");

    section(&mut out, "Composer editing");
    row(&mut out, "← / →", "cursor left / right");
    row(&mut out, "Alt+← / Alt+→", "word left / word right");
    row(&mut out, "Home / Ctrl+A", "start of line");
    row(&mut out, "End", "end of line (composer non-empty)");
    row(&mut out, "Backspace", "delete char before cursor");
    row(&mut out, "Delete", "delete char under cursor");
    row(&mut out, "Ctrl+U", "kill to start of line");
    row(&mut out, "Ctrl+K", "kill to end of line");
    row(&mut out, "Ctrl+W", "kill word back");

    section(&mut out, "Focus mode (Ctrl+F)");
    row(&mut out, "j / ↓", "next card");
    row(&mut out, "k / ↑", "previous card");
    row(&mut out, "g / Home", "first card");
    row(&mut out, "G / End", "last card");
    row(&mut out, "PgUp / PgDn", "±5 cards");
    row(&mut out, "Enter / Space", "toggle expand on tool card");
    row(&mut out, "y / c / Ctrl+Y", "copy focused card to clipboard");
    row(&mut out, "Esc / q", "exit focus mode");

    section(&mut out, "Modal — any");
    row(&mut out, "↑ / ↓", "move selection");
    row(&mut out, "PgUp / PgDn", "page ±5");
    row(&mut out, "Home / End", "first / last");
    row(&mut out, "Enter", "apply selection");
    row(&mut out, "Esc", "close modal");
    row(&mut out, "(printable key)", "append to filter query");
    row(&mut out, "Backspace", "delete from filter query");

    section(&mut out, "Vim mode (opt-in via /vim)");
    row(&mut out, "Esc", "Normal mode");
    row(
        &mut out,
        "i / a / I / A",
        "Insert at cursor / after / start / end",
    );
    row(&mut out, "o / O", "new line below / above + Insert");
    row(&mut out, "h j k l", "left / down / up / right");
    row(&mut out, "w / b", "word right / left");
    row(&mut out, "0 / $", "start / end of line");
    row(&mut out, "x", "delete char under cursor");
    row(&mut out, "dd", "delete line");
    row(&mut out, "gg / G", "top / bottom");

    section(&mut out, "Interview modal");
    row(&mut out, "Tab / ↓", "next field (incl. Submit button)");
    row(&mut out, "Shift+Tab / ↑", "previous field");
    row(&mut out, "← / →", "cycle select / move multiselect cursor");
    row(&mut out, "Alt+← / Alt+→", "word motion in text / number");
    row(&mut out, "Space", "toggle boolean / multiselect option");
    row(&mut out, "1..9", "on select, pick Nth option");
    row(&mut out, "Shift+Enter", "newline (multiline text only)");
    row(&mut out, "Enter (text / select)", "advance focus");
    row(&mut out, "Enter (Submit button)", "submit the form");
    row(&mut out, "Ctrl+S / Ctrl+Enter", "submit from anywhere");
    row(&mut out, "PgUp / PgDn", "scroll viewport ±10");
    row(&mut out, "Ctrl+Home / Ctrl+End", "scroll to top / bottom");
    row(&mut out, "Esc", "cancel interview");

    section(&mut out, "Mouse");
    row(&mut out, "wheel up / down", "scroll transcript");
    row(&mut out, "left click on card", "focus card");
    row(&mut out, "double-click tool card", "toggle expand");
    row(&mut out, "click ⬇ live-tail chip", "re-pin live tail");

    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  See /settings for runtime flags and state. See /help for a quick summary.",
        Style::default().fg(t.dim),
    )));

    out
}

pub(crate) fn status_row(label: &str, count: u32, color: Color, t: &Theme) -> Line<'static> {
    let dot = if count > 0 { "●" } else { "○" };
    Line::from(vec![
        Span::styled(
            format!("  {:>10}  ", label),
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{dot} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{count}"), Style::default().fg(t.text)),
    ])
}

pub(crate) fn git_log_body(state: &crate::ui::modal::GitLogState, t: &Theme) -> Vec<Line<'static>> {
    if state.commits.is_empty() {
        return vec![Line::from(Span::styled(
            "  (no commits)",
            Style::default().fg(t.dim),
        ))];
    }
    let mut out: Vec<Line<'static>> = Vec::with_capacity(state.commits.len() + 2);
    out.push(Line::from(vec![Span::styled(
        format!("  {} commits", state.commits.len()),
        Style::default().fg(t.dim),
    )]));
    out.push(Line::default());
    for (i, c) in state.commits.iter().enumerate() {
        let is_sel = i == state.selected;
        let marker = if is_sel { "▸" } else { " " };
        let subject_style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        out.push(Line::from(vec![
            Span::styled(
                format!(" {marker} "),
                Style::default()
                    .fg(if is_sel { t.accent } else { t.dim })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<8}  ", c.hash),
                Style::default().fg(t.warning).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<20}  ", truncate_preview(&c.author, 20)),
                Style::default().fg(t.accent_strong),
            ),
            Span::styled(
                format!("{:<14}  ", truncate_preview(&c.date, 14)),
                Style::default().fg(t.dim),
            ),
            Span::styled(c.subject.clone(), subject_style),
        ]));
    }
    out
}

/// V4.b · body of the transcript-search overlay. Shows the query
/// (with an inline cursor) followed by a preview of each hit: which
/// transcript row it's on, a short label, and a snippet centred on
/// the match. `hit_idx` gets a `▶` focus marker so the user sees
/// which card Enter would jump to.
pub(crate) fn search_body(
    state: &crate::ui::modal::SearchState,
    transcript: &Transcript,
    t: &Theme,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    // Query input row with inline cursor overlay.
    let (before, under, after) = split_cursor(&state.query, state.query_cursor);
    let q_style = Style::default().fg(t.text);
    let cursor_style = Style::default()
        .fg(t.text)
        .add_modifier(Modifier::REVERSED | Modifier::BOLD);
    out.push(Line::from(vec![
        Span::styled("  query: ", Style::default().fg(t.muted)),
        Span::styled(before, q_style),
        Span::styled(under, cursor_style),
        Span::styled(after, q_style),
    ]));
    out.push(Line::default());

    if state.query.is_empty() {
        out.push(Line::from(Span::styled(
            "  start typing — every transcript row is scanned case-insensitively",
            Style::default().fg(t.dim),
        )));
        return out;
    }

    if state.hits.is_empty() {
        out.push(Line::from(Span::styled(
            format!("  no matches for {:?}", state.query),
            Style::default().fg(t.warning),
        )));
        return out;
    }

    // Preview each hit: "[N] kind · snippet" where kind is user /
    // assistant / thinking / tool output / etc., and snippet is ~60
    // chars of context around the first occurrence of the query.
    let needle = state.query.to_lowercase();
    for (i, &idx) in state.hits.iter().enumerate() {
        let focused = i == state.hit_idx;
        let marker = if focused { "▶" } else { " " };
        let marker_style = if focused {
            Style::default()
                .fg(t.accent_strong)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.dim)
        };
        let (kind, text) = match transcript.entries().get(idx) {
            Some(Entry::User(s)) => ("user", s.as_str()),
            Some(Entry::Assistant(s)) => ("assistant", s.as_str()),
            Some(Entry::Thinking(s)) => ("thinking", s.as_str()),
            Some(Entry::Info(s)) => ("info", s.as_str()),
            Some(Entry::Warn(s)) => ("warn", s.as_str()),
            Some(Entry::Error(s)) => ("error", s.as_str()),
            Some(Entry::ToolCall(tc)) => ("tool", tc.output.as_str()),
            Some(Entry::BashExec(bx)) => ("bash", bx.output.as_str()),
            _ => continue,
        };
        let snippet = context_snippet(text, &needle, 60);
        out.push(Line::from(vec![
            Span::styled(format!("  {marker} "), marker_style),
            Span::styled(
                format!("[#{}]", idx + 1),
                Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {kind:>9} · "), Style::default().fg(t.muted)),
            Span::styled(snippet, Style::default().fg(t.text)),
        ]));
    }

    out
}

/// V4.b · extract ~`max` chars of context around the first occurrence
/// of `needle` (lowercase) inside `text`. Newlines collapse to single
/// spaces so the preview stays on one line.
pub(crate) fn context_snippet(text: &str, needle_lower: &str, max: usize) -> String {
    let haystack = text.to_lowercase();
    let pos = haystack.find(needle_lower).unwrap_or(0);
    let half = max / 2;
    let start = pos.saturating_sub(half);
    let end = (pos + needle_lower.len() + half).min(text.len());
    // Walk to the nearest char boundary so we don't slice mid-utf8.
    let mut s = start;
    while s < text.len() && !text.is_char_boundary(s) {
        s += 1;
    }
    let mut e = end;
    while e < text.len() && !text.is_char_boundary(e) {
        e += 1;
    }
    let slice = &text[s..e];
    let flat: String = slice
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .collect();
    let prefix = if s > 0 { "…" } else { "" };
    let suffix = if e < text.len() { "…" } else { "" };
    format!("{prefix}{flat}{suffix}")
}

/// V3.f · body of the Plan Review modal. Review mode renders the action
/// chips (Accept / Edit / Deny); Edit mode renders a focused step list
/// with mutation hints. A third "Edit + text entry" sub-state inlines a
/// cursor into the current row.
pub(crate) fn plan_review_body(
    state: &crate::ui::modal::PlanReviewState,
    t: &Theme,
) -> Vec<Line<'static>> {
    use crate::ui::modal::{PlanReviewMode, PlanReviewPurpose};
    let mut out: Vec<Line<'static>> = Vec::new();
    let in_edit = state.mode == PlanReviewMode::Edit;

    let intro = match (state.purpose, in_edit) {
        (PlanReviewPurpose::NewPlan, false) => format!(
            "The agent proposed a {}-step plan. Review before execution.",
            state.items.len()
        ),
        (PlanReviewPurpose::Amendment, false) => format!(
            "The agent wants to amend the current plan ({} steps after amendment).",
            state.items.len()
        ),
        (_, true) => {
            "Edit mode · ↑↓ nav · Enter edit · a add · x delete · Ctrl+S accept · Esc back"
                .to_string()
        }
    };
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        format!("  {intro}"),
        Style::default().fg(t.muted),
    )));
    out.push(Line::default());

    for (i, step) in state.items.iter().enumerate() {
        let focused = in_edit && i == state.selected;
        let editing = state.editing.as_ref().filter(|e| e.index == i);

        let marker = if focused { "▶" } else { " " };
        let marker_style = if focused {
            Style::default()
                .fg(t.accent_strong)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.dim)
        };
        let num_style = Style::default().fg(t.dim).add_modifier(Modifier::BOLD);
        let prefix_num = format!("[ ] {:>2}. ", i + 1);

        if let Some(e) = editing {
            let (before, under, after) = split_cursor(&e.buffer, e.cursor);
            out.push(Line::from(vec![
                Span::styled(format!("  {marker} "), marker_style),
                Span::styled(prefix_num, num_style),
                Span::styled(before, Style::default().fg(t.text)),
                Span::styled(
                    under,
                    Style::default()
                        .fg(t.text)
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                ),
                Span::styled(after, Style::default().fg(t.text)),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::styled(format!("  {marker} "), marker_style),
                Span::styled(prefix_num, num_style),
                Span::styled(step.clone(), Style::default().fg(t.text)),
            ]));
        }
    }

    if in_edit && state.items.is_empty() {
        out.push(Line::from(Span::styled(
            "  (empty — press `a` to add the first step)",
            Style::default().fg(t.dim),
        )));
    }

    out.push(Line::default());
    out.push(Line::from(vec![
        Span::raw("  auto-run after accept: "),
        Span::styled(
            if state.auto_run_pref { "ON" } else { "OFF" },
            Style::default()
                .fg(if state.auto_run_pref {
                    t.success
                } else {
                    t.dim
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("    (press ", Style::default().fg(t.dim)),
        kb("t", t),
        Span::styled(" to toggle)", Style::default().fg(t.dim)),
    ]));
    out.push(Line::default());

    if !in_edit {
        // Action chips — Review mode only.
        let chip = |label: &str, idx: usize, color: Color| -> Span<'static> {
            let focused = state.selected == idx;
            let style = if focused {
                Style::default()
                    .fg(Color::Rgb(0, 0, 0))
                    .bg(color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            };
            Span::styled(format!("  {label}  "), style)
        };
        out.push(Line::from(vec![
            Span::raw("  "),
            chip("Accept", 0, t.success),
            Span::raw("  "),
            chip("Edit", 1, t.accent),
            Span::raw("  "),
            chip("Deny", 2, t.error),
        ]));
        out.push(Line::default());
        out.push(Line::from(Span::styled(
            "  a Accept · e Edit · d Deny · t toggle auto-run · Esc / d to cancel",
            Style::default().fg(t.dim),
        )));
    } else {
        out.push(Line::from(Span::styled(
            "  ↑↓ nav · Enter edit · a add · x delete · t toggle auto-run · Ctrl+S accept · Esc back",
            Style::default().fg(t.dim),
        )));
    }

    out
}

/// Split a buffer at `cursor` into `(before, under, after)` where `under`
/// is the single char at the cursor (or " " when at end-of-buffer).
pub(crate) fn split_cursor(s: &str, cursor: usize) -> (String, String, String) {
    let cursor = cursor.min(s.len());
    let before = s[..cursor].to_string();
    let rest = &s[cursor..];
    let (under, after) = match rest.chars().next() {
        Some(c) => {
            let cb = c.len_utf8();
            (rest[..cb].to_string(), rest[cb..].to_string())
        }
        None => (" ".to_string(), String::new()),
    };
    (before, under, after)
}

pub(crate) fn plan_full_lines(plan: &crate::plan::Plan, t: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if !plan.is_active() && !plan.all_done() && plan.total() == 0 {
        out.push(Line::from(Span::styled(
            "  (no plan)",
            Style::default().fg(t.dim),
        )));
        out.push(Line::default());
        out.push(Line::from(Span::styled(
            "  Tell the agent what to do and let it propose a plan, or run:",
            Style::default().fg(t.dim),
        )));
        out.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                "/plan set step 1 | step 2 | step 3",
                Style::default().fg(t.accent_strong),
            ),
        ]));
        return out;
    }

    out.push(Line::from(vec![
        Span::styled(
            "  ▸ progress  ",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} / {}", plan.count_done(), plan.total()),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if plan.auto_run {
                "   auto-run ●"
            } else {
                "   auto-run ○"
            },
            Style::default().fg(if plan.auto_run { t.success } else { t.dim }),
        ),
    ]));
    out.push(Line::default());
    for (i, it) in plan.items.iter().enumerate() {
        let (color, bold) = match it.status {
            crate::plan::Status::Done => (t.success, false),
            crate::plan::Status::Active => (t.accent, true),
            crate::plan::Status::Pending => (t.dim, false),
            crate::plan::Status::Failed => (t.error, true),
        };
        let mut style = Style::default().fg(color);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        out.push(Line::from(vec![
            Span::styled(format!("  {} {:>2}. ", it.status.marker(), i + 1), style),
            Span::styled(it.text.clone(), style),
        ]));
        if it.attempts > 0 && it.status == crate::plan::Status::Active {
            out.push(Line::from(Span::styled(
                format!(
                    "       attempts: {}/{}",
                    it.attempts,
                    crate::plan::MAX_ATTEMPTS
                ),
                Style::default().fg(t.dim),
            )));
        }
    }
    if let Some(r) = plan.fail_reason.as_deref() {
        out.push(Line::default());
        out.push(Line::from(vec![
            Span::styled(
                "  failure: ",
                Style::default().fg(t.error).add_modifier(Modifier::BOLD),
            ),
            Span::styled(r.to_string(), Style::default().fg(t.error)),
        ]));
    }
    out
}

/// Compact plan card shown above the editor while a plan is active.
pub(crate) fn plan_card(plan: &crate::plan::Plan, t: &Theme) -> crate::ui::cards::Card {
    use crate::ui::cards::Card;
    let mut body = Vec::new();
    // Show a focused window around the active step for the compact card.
    let active = plan.current_idx();
    for (i, it) in plan.items.iter().enumerate() {
        let (color, bold) = match it.status {
            crate::plan::Status::Done => (t.success, false),
            crate::plan::Status::Active => (t.accent, true),
            crate::plan::Status::Pending => (t.dim, false),
            crate::plan::Status::Failed => (t.error, true),
        };
        let mut style = Style::default().fg(color);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        let marker = it.status.marker();
        body.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(it.text.clone(), style),
        ]));
        // Cap compact card height — if plan is long, show only 6 items
        // centered around active.
        if plan.items.len() > 6
            && let Some(a) = active
            && (i + 3 < a || i > a + 3)
        {
            body.pop();
        }
    }
    let right = format!(
        "{}/{} {}",
        plan.count_done(),
        plan.total(),
        if plan.auto_run { "·auto" } else { "" }
    );
    Card {
        icon: "◆",
        title: "plan".into(),
        right_title: Some(right),
        body,
        border_color: if plan.fail_reason.is_some() {
            t.error
        } else {
            t.accent
        },
        icon_color: t.accent,
        title_color: t.accent,
    }
}

pub(crate) fn git_branch_body(
    state: &crate::ui::modal::GitBranchState,
    t: &Theme,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            state.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    out.push(Line::default());
    let q = state.query.to_ascii_lowercase();
    let filtered: Vec<&crate::git::Branch> = state
        .branches
        .iter()
        .filter(|b| q.is_empty() || b.name.to_ascii_lowercase().contains(&q))
        .collect();
    if filtered.is_empty() {
        out.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
        return out;
    }
    for (i, b) in filtered.iter().enumerate() {
        let is_sel = i == state.selected;
        let marker = if is_sel { "▸" } else { " " };
        let chip = if b.current {
            Span::styled(
                " (current) ",
                Style::default().fg(t.success).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        };
        let style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        out.push(Line::from(vec![
            Span::styled(format!("  {marker} "), style),
            Span::styled(b.name.clone(), style),
            chip,
        ]));
    }
    out
}

pub(crate) fn commands_selected_line(list: &ListModal<crate::ui::commands::MenuItem>) -> u16 {
    let filtered: Vec<&crate::ui::commands::MenuItem> =
        filtered_commands(&list.items, &list.query).collect();
    // filter + blank line above the list.
    let mut line: u16 = 2;
    let mut last_cat: Option<crate::ui::commands::Category> = None;
    for (i, it) in filtered.iter().enumerate() {
        if last_cat != Some(it.category) {
            if last_cat.is_some() {
                line = line.saturating_add(1); // inter-group blank
            }
            line = line.saturating_add(1); // header row
            last_cat = Some(it.category);
        }
        if i == list.selected {
            return line;
        }
        line = line.saturating_add(1); // item row
        if !it.description.is_empty() {
            line = line.saturating_add(1); // description row
        }
    }
    line
}

pub(crate) fn help_text(t: &Theme) -> Text<'static> {
    // V3.e.1 · previous body listed 10 commands out of 40+ and omitted
    // /settings and /shortcuts entirely. New body points at the two
    // authoritative references and keeps only the daily-use keys so a
    // first-time user isn't drowned.
    let heading = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))
    };
    Text::from(vec![
        Line::from(Span::styled(
            "Welcome to Tau (2*PI).",
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        heading("Full reference"),
        Line::from(vec![
            Span::raw("  "),
            kb("/shortcuts", t),
            Span::raw("  every keybinding, grouped by context (aliases: /keys · /hotkeys)"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("/settings", t),
            Span::raw("  every tunable + live state (aliases: /prefs · /preferences)"),
        ]),
        Line::default(),
        heading("Essentials"),
        Line::from(vec![
            Span::raw("  "),
            kb("Enter", t),
            Span::raw("          submit prompt"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("Shift+Enter", t),
            Span::raw("    newline in composer"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("Esc", t),
            Span::raw("            abort streaming · clear composer · quit"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("Ctrl+F", t),
            Span::raw("         focus mode (navigate transcript cards)"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("Ctrl+C", t),
            Span::raw("         quit"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("/", t),
            Span::raw("              slash-command picker"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            kb("?", t),
            Span::raw("              this screen"),
        ]),
        Line::default(),
        Line::from(Span::styled(
            "  Press / to browse commands · /shortcuts for the full keymap.",
            Style::default().fg(t.dim),
        )),
    ])
}

pub(crate) fn stats_text(s: &SessionStats, t: &Theme) -> Text<'static> {
    let mut lines = vec![
        label_value("session", s.session_name_opt(), t),
        label_value(
            "messages",
            format!(
                "{} user · {} assistant · {} tools",
                s.user_messages, s.assistant_messages, s.tool_calls
            ),
            t,
        ),
        label_value(
            "tokens",
            format!(
                "in {} · out {} · cache R {} · cache W {} · total {}",
                s.tokens.input,
                s.tokens.output,
                s.tokens.cache_read,
                s.tokens.cache_write,
                s.tokens.total
            ),
            t,
        ),
        label_value("cost", format!("${:.4}", s.cost), t),
    ];
    if let Some(ctx) = &s.context_usage {
        lines.push(label_value(
            "context",
            format!(
                "{} / {} tokens ({}%)",
                ctx.tokens.unwrap_or(0),
                ctx.context_window,
                ctx.percent.map(|p| format!("{p:.0}")).unwrap_or_default()
            ),
            t,
        ));
    }
    if let Some(file) = &s.session_file {
        lines.push(label_value("file", file.clone(), t));
    }
    if let Some(id) = &s.session_id {
        lines.push(label_value("id", id.clone(), t));
    }
    Text::from(lines)
}

trait SessionStatsExt {
    fn session_name_opt(&self) -> String;
}
impl SessionStatsExt for SessionStats {
    fn session_name_opt(&self) -> String {
        self.session_id.clone().unwrap_or_else(|| "—".into())
    }
}

pub(crate) fn label_value(k: &str, v: impl Into<String>, t: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:>12}  ", k), Style::default().fg(t.accent)),
        Span::styled(v.into(), Style::default().fg(t.text)),
    ])
}

/// Categorized, two-pane body text for the Commands modal.
///
/// Line layout:
///   line 0:  "filter: <query>    (N items)"
///   line 1:  blank
///   line 2+: list — category headers (not selectable; dim) and items.
///            Item lines include source badge + icon + name + args + description.
///
/// The caller (draw_modal) renders this `Text` in the LEFT pane and uses
/// `command_detail_lines` to populate the RIGHT pane with the focused
/// item's description + argument + example.
pub(crate) fn commands_text(
    list: &ListModal<crate::ui::commands::MenuItem>,
    t: &Theme,
    list_width: u16,
) -> Text<'static> {
    use crate::ui::commands::Category;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let filtered: Vec<&crate::ui::commands::MenuItem> =
        filtered_commands(&list.items, &list.query).collect();

    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   ({} items)", filtered.len()),
            Style::default().fg(t.dim),
        ),
    ]));
    lines.push(Line::default());

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
        return Text::from(lines);
    }

    let mut last_cat: Option<Category> = None;
    for (i, it) in filtered.iter().enumerate() {
        // Category divider on boundary.
        if last_cat != Some(it.category) {
            if last_cat.is_some() {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", it.category.icon()),
                    Style::default().fg(t.accent),
                ),
                Span::styled(
                    it.category.label().to_string(),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
            ]));
            last_cat = Some(it.category);
        }

        let is_sel = i == list.selected;
        let marker = if is_sel { "▸" } else { " " };
        let name_style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let badge = match it.source {
            crate::rpc::types::CommandSource::Extension => "ext",
            crate::rpc::types::CommandSource::Prompt => "prompt",
            crate::rpc::types::CommandSource::Skill => "skill",
            crate::rpc::types::CommandSource::Builtin => "builtin",
        };
        // Build the primary item row, truncated to the list pane width so
        // it never wraps — the scroll math needs rendered rows == source
        // rows for the scroll cap to land on the last item.
        let raw_main = format!("  {marker} /{}", it.name);
        let args_piece = if it.args.is_empty() {
            String::new()
        } else {
            format!(" {}", it.args)
        };
        let badge_piece = format!("   [{badge}]");
        // Budget: list_width cells total. The badge sits at the end; the
        // name + args occupy the rest. Truncate the name if necessary.
        let available = list_width as usize;
        let badge_len = badge_piece.chars().count();
        let args_len = args_piece.chars().count();
        let name_budget = available.saturating_sub(badge_len + args_len);
        let name_trunc = if raw_main.chars().count() > name_budget {
            truncate_preview(&raw_main, name_budget)
        } else {
            raw_main
        };
        let mut spans = vec![Span::styled(name_trunc, name_style)];
        if !args_piece.is_empty() {
            spans.push(Span::styled(args_piece, Style::default().fg(t.warning)));
        }
        spans.push(Span::styled(badge_piece, Style::default().fg(t.dim)));
        lines.push(Line::from(spans));
        // Description: indent 6, truncate to fit so it never wraps.
        if !it.description.is_empty() {
            let desc_w = (list_width as usize).saturating_sub(6);
            let desc_trunc = truncate_preview(&it.description, desc_w);
            lines.push(Line::from(Span::styled(
                format!("      {desc_trunc}"),
                Style::default().fg(t.dim),
            )));
        }
    }
    Text::from(lines)
}

/// Right-pane detail lines for the currently-selected item.
pub(crate) fn command_detail_lines(
    list: &ListModal<crate::ui::commands::MenuItem>,
    t: &Theme,
) -> Vec<Line<'static>> {
    let filtered: Vec<&crate::ui::commands::MenuItem> =
        filtered_commands(&list.items, &list.query).collect();
    let Some(it) = filtered.get(list.selected) else {
        return vec![Line::from(Span::styled(
            "(no selection)",
            Style::default().fg(t.dim),
        ))];
    };

    let mut lines = Vec::new();
    // Title row.
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", it.category.icon()),
            Style::default().fg(t.accent),
        ),
        Span::styled(
            format!("/{}", it.name),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if it.args.is_empty() {
                String::new()
            } else {
                format!(" {}", it.args)
            },
            Style::default().fg(t.warning),
        ),
    ]));
    // Category + source chip.
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {}", it.category.label()),
            Style::default().fg(t.dim),
        ),
        Span::styled(
            format!(
                " · {}",
                match it.source {
                    crate::rpc::types::CommandSource::Extension => "extension",
                    crate::rpc::types::CommandSource::Prompt => "prompt template",
                    crate::rpc::types::CommandSource::Skill => "skill",
                    crate::rpc::types::CommandSource::Builtin => "built-in",
                }
            ),
            Style::default().fg(t.muted),
        ),
    ]));
    lines.push(Line::default());

    // Description (wrapped).
    if !it.description.is_empty() {
        lines.push(Line::from(Span::styled(
            "description",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(it.description.clone(), Style::default().fg(t.text)),
        ]));
        lines.push(Line::default());
    }

    if !it.args.is_empty() {
        lines.push(Line::from(Span::styled(
            "arguments",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(it.args.to_string(), Style::default().fg(t.warning)),
        ]));
        lines.push(Line::default());
    }

    if !it.example.is_empty() {
        lines.push(Line::from(Span::styled(
            "example",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(it.example.to_string(), Style::default().fg(t.accent_strong)),
        ]));
        lines.push(Line::default());
    }

    let action_hint = if it.is_theme() {
        "Enter applies the theme · Esc closes"
    } else if it.is_builtin() && it.args.is_empty() {
        "Enter runs it · Esc closes"
    } else if it.is_builtin() {
        "Enter prefills the composer · Esc closes"
    } else {
        "Enter prefills /name · Esc closes"
    };
    lines.push(Line::from(Span::styled(
        format!("  {action_hint}"),
        Style::default().fg(t.dim),
    )));

    lines
}

pub(crate) fn models_text(list: &ListModal<Model>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&Model> = filtered_models(&list.items, &list.query).collect();
    for (i, m) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let cw = m
            .context_window
            .map(|c| format!(" · {}k ctx", c / 1000))
            .unwrap_or_default();
        let reasoning = if m.reasoning { " · reasoning" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(format!("{}/{}", m.provider, m.id), style),
            Span::styled(format!("{cw}{reasoning}"), Style::default().fg(t.dim)),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

pub(crate) fn thinking_text(radio: &RadioModal<ThinkingLevel>, t: &Theme) -> Text<'static> {
    let lines: Vec<Line<'static>> = radio
        .options
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let (marker, style) = if i == radio.selected {
                (
                    "◉",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                ("○", Style::default().fg(t.dim))
            };
            Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled((*label).to_string(), style),
            ])
        })
        .collect();
    Text::from(lines)
}

pub(crate) fn forks_text(list: &ListModal<ForkMessage>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&ForkMessage> = filtered_forks(&list.items, &list.query).collect();
    for (i, f) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(
                truncate_preview(&f.entry_id, 10),
                Style::default().fg(t.warning),
            ),
            Span::raw("  "),
            Span::styled(truncate_preview(&f.text.replace('\n', " ⏎ "), 200), style),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no fork candidates)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

pub(crate) fn history_text(list: &ListModal<HistoryEntry>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&HistoryEntry> = filtered_history(&list.items, &list.query).collect();
    for (i, e) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let preview = truncate_preview(&e.text.replace('\n', " ⏎ "), 200);
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(preview, style),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

pub(crate) fn ext_select_text(options: &[String], selected: usize, t: &Theme) -> Text<'static> {
    let lines: Vec<Line<'static>> = options
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let marker = if i == selected { "▸" } else { " " };
            let style = if i == selected {
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(o.clone(), style),
            ])
        })
        .collect();
    Text::from(lines)
}

pub(crate) fn ext_confirm_text(message: Option<&str>, selected: usize, t: &Theme) -> Text<'static> {
    let mut lines = Vec::new();
    if let Some(m) = message {
        lines.push(Line::from(Span::styled(
            m.to_string(),
            Style::default().fg(t.text),
        )));
        lines.push(Line::default());
    }
    let sel_yes = selected == 1;
    let yes_style = if sel_yes {
        Style::default()
            .bg(t.success)
            .fg(Color::Rgb(0, 0, 0))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.success).add_modifier(Modifier::DIM)
    };
    let no_style = if !sel_yes {
        Style::default()
            .bg(t.error)
            .fg(Color::Rgb(0, 0, 0))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.error).add_modifier(Modifier::DIM)
    };
    lines.push(Line::from(vec![
        Span::styled("  [ No ]  ", no_style),
        Span::raw("     "),
        Span::styled("  [ Yes ]  ", yes_style),
    ]));
    Text::from(lines)
}

pub(crate) fn ext_input_text(placeholder: Option<&str>, value: &str, t: &Theme) -> Text<'static> {
    let mut lines = Vec::new();
    if let Some(p) = placeholder {
        lines.push(Line::from(Span::styled(
            format!("({p})"),
            Style::default().fg(t.dim),
        )));
        lines.push(Line::default());
    }
    let display = if value.is_empty() {
        Line::from(vec![Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        )])
    } else {
        Line::from(vec![
            Span::styled(value.to_string(), Style::default().fg(t.text)),
            Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)),
        ])
    };
    lines.push(display);
    Text::from(lines)
}
