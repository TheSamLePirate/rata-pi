//! Transcript card builders — extracted in V4.d.2.
//!
//! Builds the `Card` / `Line` payloads that the visuals cache then
//! renders. `mod.rs` still owns `build_one_visual` (the per-entry
//! dispatcher); everything it calls for tool / bash / compaction /
//! retry card bodies lives here.
//!
//! Pure functions — no `App` state, no mutation. The theme-aware
//! syntect highlighter + diff body builder also sit here because
//! they share the render context.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use crate::ui::cards::Card;
use crate::ui::transcript::{
    BashExec, Compaction, CompactionState, Retry, RetryState, ToolCall, ToolStatus,
};

use super::helpers::{args_preview, truncate_preview};

pub(super) fn plain_paragraph(s: &str, color: Color) -> Vec<Line<'static>> {
    s.split('\n')
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(color))))
        .collect()
}

pub(super) fn thinking_body(s: &str, t: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for l in s.split('\n') {
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(t.role_thinking)),
            Span::styled(
                l.to_string(),
                Style::default().fg(t.muted).add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
    lines
}

pub(super) fn tool_card(tc: &ToolCall, t: &Theme) -> Card {
    let (status_icon, color, status_label) = match tc.status {
        ToolStatus::Running => ("⚙", t.role_tool, "running"),
        ToolStatus::Ok => ("✓", t.success, "ok"),
        ToolStatus::Err => ("✗", t.error, "error"),
    };
    let color = if tc.is_error { t.error } else { color };

    let body = build_tool_body(tc, t);

    let expand = if tc.expanded { "▾" } else { "▸" };
    // Right-title shows the primary arg summary when we can find one,
    // falling back to the status label.
    let right = primary_arg_chip(tc)
        .map(|s| truncate_preview(&s, 60))
        .unwrap_or_else(|| format!("{status_icon} {status_label}"));

    Card {
        icon: tool_family_icon(&tc.name),
        title: format!("{expand} {}", tc.name),
        right_title: Some(right),
        body,
        border_color: color,
        icon_color: color,
        title_color: color,
    }
}

/// Dispatch tool body rendering by tool-family. Unknown tools fall back to
/// the generic args+out layout.
pub(super) fn build_tool_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    match tool_family(&tc.name) {
        ToolFamily::Edit => build_edit_body(tc, t),
        ToolFamily::ReadFile => build_read_body(tc, t),
        ToolFamily::Grep => build_grep_body(tc, t),
        ToolFamily::Write => build_write_body(tc, t),
        ToolFamily::Bash => build_generic_body(tc, t),
        ToolFamily::Todo => build_todo_body(tc, t),
        ToolFamily::Generic => build_generic_body(tc, t),
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ToolFamily {
    Bash,
    Edit,
    ReadFile,
    Grep,
    Write,
    Todo,
    Generic,
}

pub(super) fn tool_family(name: &str) -> ToolFamily {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        "bash" | "run" | "shell" | "exec" | "command" => ToolFamily::Bash,
        "edit" | "apply_patch" | "str_replace" | "str_replace_editor" | "multi_edit" | "patch" => {
            ToolFamily::Edit
        }
        "read" | "read_file" | "readfile" | "view" | "cat" => ToolFamily::ReadFile,
        "grep" | "search" | "rg" | "ripgrep" => ToolFamily::Grep,
        "write" | "write_file" | "create" | "create_file" => ToolFamily::Write,
        "todo" | "todowrite" | "tasks" => ToolFamily::Todo,
        _ => ToolFamily::Generic,
    }
}

pub(super) fn tool_family_icon(name: &str) -> &'static str {
    match tool_family(name) {
        ToolFamily::Bash => "$",
        ToolFamily::Edit => "±",
        ToolFamily::ReadFile => "▤",
        ToolFamily::Grep => "⌕",
        ToolFamily::Write => "✎",
        ToolFamily::Todo => "☐",
        _ => "⚙",
    }
}

/// Extract a human-readable chip for the card right-title from common args.
pub(super) fn primary_arg_chip(tc: &ToolCall) -> Option<String> {
    let obj = tc.args.as_object()?;
    for k in ["file_path", "path", "filename", "file", "target"] {
        if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    for k in ["pattern", "query", "q"] {
        if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(format!("\"{}\"", truncate_preview(v, 40)));
        }
    }
    for k in ["command", "cmd"] {
        if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(truncate_preview(v, 40));
        }
    }
    None
}

pub(super) fn build_generic_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let mut body: Vec<Line<'static>> = Vec::new();
    let arg_preview = args_preview(&tc.args);
    if !arg_preview.is_empty() {
        body.push(Line::from(vec![
            Span::styled("args  ", Style::default().fg(t.dim)),
            Span::styled(
                truncate_preview(&arg_preview, 300),
                Style::default().fg(t.muted),
            ),
        ]));
    }
    add_output_body(&mut body, tc, t);
    body_or_ellipsis(body, t)
}

pub(super) fn build_edit_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let file_path = obj
        .and_then(|o| o.get("file_path").or_else(|| o.get("path")))
        .and_then(|v| v.as_str());
    let old_s = obj
        .and_then(|o| o.get("old_string").or_else(|| o.get("old")))
        .and_then(|v| v.as_str());
    let new_s = obj
        .and_then(|o| o.get("new_string").or_else(|| o.get("new")))
        .and_then(|v| v.as_str());

    let mut body: Vec<Line<'static>> = Vec::new();

    // Show a synthetic diff when we have old+new (common for str_replace).
    if let (Some(old_s), Some(new_s)) = (old_s, new_s) {
        let lang = file_path
            .and_then(|p| p.rsplit('.').next())
            .unwrap_or("")
            .to_string();
        body.push(Line::from(vec![
            Span::styled("file  ", Style::default().fg(t.dim)),
            Span::styled(
                file_path.unwrap_or("(inline)").to_string(),
                Style::default().fg(t.muted),
            ),
        ]));
        body.push(Line::default());
        for l in old_s.lines() {
            body.push(diff_body_line("-", l, &lang, t.diff_remove, t));
        }
        for l in new_s.lines() {
            body.push(diff_body_line("+", l, &lang, t.diff_add, t));
        }
        body.push(Line::default());
    } else if let Some(p) = file_path {
        body.push(Line::from(vec![
            Span::styled("file  ", Style::default().fg(t.dim)),
            Span::styled(p.to_string(), Style::default().fg(t.muted)),
        ]));
    }

    // Output: if it's a proper unified diff, use the diff widget. Else raw.
    let output = crate::ui::ansi::strip(&tc.output);
    if !output.trim().is_empty() {
        let show = tc.expanded || body.len() < 6;
        if show {
            body.push(Line::default());
            if crate::ui::diff::is_unified_diff(&output) {
                body.extend(crate::ui::diff::render(&output, t));
            } else {
                for part in output.split('\n').take(if tc.expanded { 400 } else { 8 }) {
                    body.push(Line::from(Span::styled(
                        part.to_string(),
                        Style::default().fg(t.muted),
                    )));
                }
            }
        } else {
            body.push(Line::from(Span::styled(
                "(Enter / Ctrl+E to see result)",
                Style::default().fg(t.dim),
            )));
        }
    }

    body_or_ellipsis(body, t)
}

pub(super) fn build_read_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let file_path = obj
        .and_then(|o| o.get("file_path").or_else(|| o.get("path")))
        .and_then(|v| v.as_str());
    let mut body: Vec<Line<'static>> = Vec::new();
    if let Some(p) = file_path {
        body.push(Line::from(vec![
            Span::styled("path  ", Style::default().fg(t.dim)),
            Span::styled(p.to_string(), Style::default().fg(t.muted)),
        ]));
    }
    let output = crate::ui::ansi::strip(&tc.output);
    if tc.expanded && !output.trim().is_empty() {
        let lang = file_path
            .and_then(|p| p.rsplit('.').next())
            .unwrap_or("")
            .to_string();
        body.push(Line::default());
        let content = strip_line_numbers(&output);
        for l in crate::ui::syntax::highlight(&content, &lang, t)
            .into_iter()
            .take(400)
        {
            body.push(l);
        }
    } else if !output.is_empty() {
        let lines_total = output.lines().count();
        body.push(Line::from(Span::styled(
            format!("{lines_total} lines — Enter / Ctrl+E to view"),
            Style::default().fg(t.dim),
        )));
    }
    body_or_ellipsis(body, t)
}

pub(super) fn build_grep_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let pattern = obj
        .and_then(|o| o.get("pattern").or_else(|| o.get("query")))
        .and_then(|v| v.as_str());
    let mut body: Vec<Line<'static>> = Vec::new();
    if let Some(p) = pattern {
        body.push(Line::from(vec![
            Span::styled("query  ", Style::default().fg(t.dim)),
            Span::styled(
                format!("\"{p}\""),
                Style::default()
                    .fg(t.role_tool)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let output = crate::ui::ansi::strip(&tc.output);
    if !output.trim().is_empty() {
        body.push(Line::default());
        let max = if tc.expanded { 400 } else { 8 };
        let mut last_file = String::new();
        for part in output.lines().take(max) {
            if let Some((file, rest)) = part.split_once(':') {
                if file != last_file {
                    body.push(Line::from(Span::styled(
                        file.to_string(),
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    )));
                    last_file = file.to_string();
                }
                body.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(rest.to_string(), Style::default().fg(t.muted)),
                ]));
            } else {
                body.push(Line::from(Span::styled(
                    part.to_string(),
                    Style::default().fg(t.muted),
                )));
            }
        }
    }
    body_or_ellipsis(body, t)
}

pub(super) fn build_write_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let file_path = obj
        .and_then(|o| o.get("file_path").or_else(|| o.get("path")))
        .and_then(|v| v.as_str());
    let content = obj
        .and_then(|o| o.get("content").or_else(|| o.get("file_text")))
        .and_then(|v| v.as_str());

    let mut body: Vec<Line<'static>> = Vec::new();
    if let Some(p) = file_path {
        body.push(Line::from(vec![
            Span::styled("path  ", Style::default().fg(t.dim)),
            Span::styled(p.to_string(), Style::default().fg(t.muted)),
        ]));
    }
    if let Some(c) = content {
        let lang = file_path
            .and_then(|p| p.rsplit('.').next())
            .unwrap_or("")
            .to_string();
        body.push(Line::default());
        let max = if tc.expanded { 400 } else { 6 };
        for l in crate::ui::syntax::highlight(c, &lang, t)
            .into_iter()
            .take(max)
        {
            body.push(l);
        }
        if !tc.expanded && c.lines().count() > 6 {
            body.push(Line::from(Span::styled(
                format!("(+{} more lines — Enter to expand)", c.lines().count() - 6),
                Style::default().fg(t.dim),
            )));
        }
    }
    body_or_ellipsis(body, t)
}

pub(super) fn build_todo_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let mut body: Vec<Line<'static>> = Vec::new();
    let items = tc
        .args
        .as_object()
        .and_then(|o| o.get("todos").or_else(|| o.get("items")))
        .and_then(|v| v.as_array());
    if let Some(items) = items {
        for item in items.iter().take(20) {
            let text = item
                .get("content")
                .or_else(|| item.get("text"))
                .or_else(|| item.get("task"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no text)");
            let status = item
                .get("status")
                .or_else(|| item.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let (mark, color) = match status {
                "completed" | "done" => ("☑", t.success),
                "in_progress" | "active" => ("◐", t.warning),
                _ => ("☐", t.dim),
            };
            body.push(Line::from(vec![
                Span::styled(
                    format!("{mark} "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(text.to_string(), Style::default().fg(t.text)),
            ]));
        }
    } else {
        body.push(Line::from(Span::styled(
            "(no todo items)",
            Style::default().fg(t.dim),
        )));
    }
    body_or_ellipsis(body, t)
}

pub(super) fn add_output_body(body: &mut Vec<Line<'static>>, tc: &ToolCall, t: &Theme) {
    if tc.expanded {
        let output = crate::ui::ansi::strip(&tc.output);
        if output.trim().is_empty() {
            body.push(Line::from(Span::styled(
                "(no output yet)",
                Style::default().fg(t.dim),
            )));
        } else if crate::ui::diff::is_unified_diff(&output) {
            body.push(Line::default());
            body.extend(crate::ui::diff::render(&output, t));
        } else {
            for part in output.split('\n').take(400) {
                body.push(Line::from(Span::styled(
                    part.to_string(),
                    Style::default().fg(t.muted),
                )));
            }
        }
    } else if !tc.output.trim().is_empty() {
        let preview = tc
            .output
            .lines()
            .next()
            .map(|s| truncate_preview(s, 200))
            .unwrap_or_default();
        body.push(Line::from(vec![
            Span::styled("out  ", Style::default().fg(t.dim)),
            Span::styled(preview, Style::default().fg(t.muted)),
        ]));
        if tc.output.lines().count() > 1 {
            body.push(Line::from(Span::styled(
                format!(
                    "(+{} more — Enter / Ctrl+E to expand)",
                    tc.output.lines().count() - 1
                ),
                Style::default().fg(t.dim),
            )));
        }
    }
}

pub(super) fn body_or_ellipsis(body: Vec<Line<'static>>, t: &Theme) -> Vec<Line<'static>> {
    if body.is_empty() {
        vec![Line::from(Span::styled("…", Style::default().fg(t.dim)))]
    } else {
        body
    }
}

pub(super) fn diff_body_line(
    prefix: &str,
    text: &str,
    lang: &str,
    color: Color,
    t: &Theme,
) -> Line<'static> {
    let spans = if lang.is_empty() {
        vec![Span::styled(text.to_string(), Style::default().fg(color))]
    } else {
        crate::ui::syntax::highlight(text, lang, t)
            .into_iter()
            .next()
            .map(|l| l.spans)
            .unwrap_or_else(|| vec![Span::styled(text.to_string(), Style::default().fg(color))])
    };
    let mut line_spans = vec![Span::styled(
        format!(" {prefix} "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];
    line_spans.extend(spans);
    let _ = t;
    Line::from(line_spans)
}

/// Drop common "  N→content" or "  N\tcontent" line-number prefixes that many
/// read_file tools emit, so syntect can tokenize clean source.
pub(super) fn strip_line_numbers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let stripped = line
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ')
            .trim_start_matches(['\t', '→', '|', ':']);
        // If that ate everything, keep original.
        let keep = if stripped.is_empty() { line } else { stripped };
        out.push_str(keep);
        out.push('\n');
    }
    out
}

pub(super) fn bash_card(bx: &BashExec, t: &Theme) -> Card {
    let (status_color, status_txt) = if bx.cancelled {
        (t.warning, "cancelled".to_string())
    } else if bx.exit_code == 0 {
        (t.success, format!("exit {}", bx.exit_code))
    } else {
        (t.error, format!("exit {}", bx.exit_code))
    };
    let body_text = crate::ui::ansi::strip(&bx.output);
    let mut body: Vec<Line<'static>> = Vec::new();
    for part in body_text.split('\n').take(300) {
        body.push(Line::from(Span::styled(
            part.to_string(),
            Style::default().fg(t.muted),
        )));
    }
    if body.is_empty() {
        body.push(Line::from(Span::styled(
            "(no output)",
            Style::default().fg(t.dim),
        )));
    }
    if bx.truncated {
        let path = bx
            .full_output_path
            .as_deref()
            .unwrap_or("(path not provided)");
        body.push(Line::from(Span::styled(
            format!("… truncated — full log: {path}"),
            Style::default().fg(t.warning),
        )));
    }

    Card {
        icon: "$",
        title: format!("$ {}", bx.command),
        right_title: Some(status_txt),
        body,
        border_color: status_color,
        icon_color: t.role_bash,
        title_color: t.role_bash,
    }
}

pub(super) fn compaction_lines(c: &Compaction, t: &Theme) -> Vec<Line<'static>> {
    let (sym, color, label) = match &c.state {
        CompactionState::Running => ("⟲", t.accent_strong, "compacting".to_string()),
        CompactionState::Done { summary } => {
            let s = summary
                .as_deref()
                .map(|s| truncate_preview(s, 100))
                .unwrap_or_default();
            (
                "⟲",
                t.success,
                if s.is_empty() {
                    "compaction complete".to_string()
                } else {
                    format!("compaction: {s}")
                },
            )
        }
        CompactionState::Aborted => ("⟲", t.warning, "compaction aborted".into()),
        CompactionState::Failed(msg) => ("⟲", t.error, format!("compaction failed: {msg}")),
    };
    vec![Line::from(vec![
        Span::styled(
            format!("  {sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
        Span::raw(" "),
        Span::styled(format!("({})", c.reason), Style::default().fg(t.dim)),
    ])]
}

pub(super) fn retry_lines(r: &Retry, t: &Theme) -> Vec<Line<'static>> {
    let (sym, color, label) = match &r.state {
        RetryState::Waiting { delay_ms, error } => (
            "↻",
            t.warning,
            format!(
                "retry {}/{} in {}ms — {}",
                r.attempt,
                r.max_attempts,
                delay_ms,
                truncate_preview(error, 80),
            ),
        ),
        RetryState::Succeeded => ("↻", t.success, format!("retry {} succeeded", r.attempt)),
        RetryState::Exhausted(msg) => (
            "↻",
            t.error,
            format!(
                "retry exhausted at {}: {}",
                r.attempt,
                truncate_preview(msg, 80)
            ),
        ),
    };
    vec![Line::from(vec![
        Span::styled(
            format!("  {sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
    ])]
}
