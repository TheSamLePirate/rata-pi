//! Inline unified-diff renderer.
//!
//! Parses a single-hunk or multi-hunk unified diff and produces Ratatui lines
//! with:
//!   * file-header rows in `diff_file` color
//!   * `@@` hunk-header rows in `diff_hunk` color
//!   * a 2-column gutter (`old_ln new_ln`) followed by a prefix chip
//!     (`+`/`-`/` `) and the line text in `diff_add`/`diff_remove`/`muted`.
//!
//! Languages detected from a file-header path (e.g. `+++ b/src/app.rs`) get
//! context-line syntax highlighting via `ui::syntax::highlight`.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

pub fn is_unified_diff(s: &str) -> bool {
    for line in s.lines().take(30) {
        if line.starts_with("@@ ") || line.starts_with("+++ ") || line.starts_with("--- ") {
            return true;
        }
    }
    false
}

pub fn render(diff: &str, t: &Theme) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut old_ln: u64 = 0;
    let mut new_ln: u64 = 0;
    let mut lang: String = String::new();

    for raw in diff.lines() {
        if let Some(rest) = raw.strip_prefix("+++ ") {
            lang = infer_lang(rest);
            out.push(header_line(raw, t.diff_file));
            continue;
        }
        if raw.starts_with("--- ") {
            out.push(header_line(raw, t.diff_file));
            continue;
        }
        if raw.starts_with("diff ") || raw.starts_with("index ") {
            out.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(t.dim),
            )));
            continue;
        }
        if let Some((o, n)) = parse_hunk(raw) {
            old_ln = o;
            new_ln = n;
            out.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(t.diff_hunk),
            )));
            continue;
        }
        // Body line inside a hunk.
        let (prefix, sign_color, text_style, bumps) = match raw.as_bytes().first().copied() {
            Some(b'+') => (
                "+",
                t.diff_add,
                Style::default().fg(t.diff_add),
                (false, true),
            ),
            Some(b'-') => (
                "-",
                t.diff_remove,
                Style::default().fg(t.diff_remove),
                (true, false),
            ),
            Some(b' ') => (" ", t.dim, Style::default().fg(t.muted), (true, true)),
            _ => {
                out.push(Line::from(Span::styled(
                    raw.to_string(),
                    Style::default().fg(t.dim),
                )));
                continue;
            }
        };
        let body = if raw.is_empty() { "" } else { &raw[1..] };

        let ln_left = if bumps.0 {
            old_ln.to_string()
        } else {
            "·".into()
        };
        let ln_right = if bumps.1 {
            new_ln.to_string()
        } else {
            "·".into()
        };

        let body_spans = highlight_body(body, &lang, text_style, t);
        let mut spans = vec![
            Span::styled(format!(" {ln_left:>4} "), Style::default().fg(t.dim)),
            Span::styled(format!("{ln_right:>4} "), Style::default().fg(t.dim)),
            Span::styled(
                format!("{prefix} "),
                Style::default().fg(sign_color).add_modifier(Modifier::BOLD),
            ),
        ];
        spans.extend(body_spans);
        out.push(Line::from(spans));

        if bumps.0 {
            old_ln = old_ln.saturating_add(1);
        }
        if bumps.1 {
            new_ln = new_ln.saturating_add(1);
        }
    }
    out
}

fn highlight_body(body: &str, lang: &str, fallback: Style, theme: &Theme) -> Vec<Span<'static>> {
    if body.is_empty() {
        return vec![Span::raw("")];
    }
    if lang.is_empty() {
        return vec![Span::styled(body.to_string(), fallback)];
    }
    let highlighted = crate::ui::syntax::highlight(body, lang, theme);
    match highlighted.into_iter().next() {
        Some(line) if !line.spans.is_empty() => line.spans,
        _ => vec![Span::styled(body.to_string(), fallback)],
    }
}

fn header_line(raw: &str, color: ratatui::style::Color) -> Line<'static> {
    Line::from(Span::styled(
        raw.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn parse_hunk(s: &str) -> Option<(u64, u64)> {
    let s = s.strip_prefix("@@ ")?;
    let (ranges, _) = s.split_once(" @@").unwrap_or((s, ""));
    let mut old_ = None;
    let mut new_ = None;
    for tok in ranges.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('-') {
            let n = rest.split_once(',').map(|(a, _)| a).unwrap_or(rest);
            old_ = n.parse::<u64>().ok();
        } else if let Some(rest) = tok.strip_prefix('+') {
            let n = rest.split_once(',').map(|(a, _)| a).unwrap_or(rest);
            new_ = n.parse::<u64>().ok();
        }
    }
    match (old_, new_) {
        (Some(o), Some(n)) => Some((o, n)),
        _ => None,
    }
}

fn infer_lang(path: &str) -> String {
    let p = path
        .trim()
        .trim_start_matches("a/")
        .trim_start_matches("b/");
    let ext = p.rsplit('.').next().unwrap_or("").trim();
    ext.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unified_diff() {
        assert!(is_unified_diff("--- a\n+++ b\n@@ -1 +1 @@\n-x\n+y\n"));
        assert!(!is_unified_diff("hello"));
    }

    #[test]
    fn parse_hunk_header() {
        let h = parse_hunk("@@ -10,5 +20,4 @@ fn main").unwrap();
        assert_eq!(h, (10, 20));
        assert_eq!(parse_hunk("@@ -1 +1 @@").unwrap(), (1, 1));
        assert!(parse_hunk("not a hunk").is_none());
    }

    #[test]
    fn infer_lang_from_path() {
        assert_eq!(infer_lang("a/src/app.rs"), "rs");
        assert_eq!(infer_lang("b/main.py"), "py");
        assert_eq!(infer_lang("Makefile"), "Makefile");
    }

    #[test]
    fn renders_minimal_diff_without_panic() {
        use crate::theme;
        let diff = "--- a/x.rs\n+++ b/x.rs\n@@ -1,3 +1,3 @@\n fn one() {}\n-old line\n+new line\n";
        let lines = render(diff, theme::default_theme());
        assert!(lines.len() >= 5);
    }
}
