//! Syntax highlighting via syntect.
//!
//! Loads the bundled syntax + theme sets exactly once (OnceLock) and exposes
//! a simple `highlight(code, lang_hint) -> Vec<Line<'static>>` entry point.
//!
//! Language detection:
//!   1. `lang_hint` treated as a markdown fence tag (rust, py, sh, …).
//!   2. Fall back to extension lookup (rs, py, …).
//!   3. Fall back to case-insensitive full-name lookup (Rust, Python, …).
//!   4. Plain-text.
//!
//! Theme: `base16-ocean.dark` — bundled with syntect, good contrast on every
//! rata-pi theme.

use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme as SynTheme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<SynTheme> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static SynTheme {
    THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes
            .get("base16-ocean.dark")
            .or_else(|| ts.themes.values().next())
            .cloned()
            .expect("syntect ships at least one theme")
    })
}

fn detect<'a>(ss: &'a SyntaxSet, hint: &str) -> &'a SyntaxReference {
    let hint = hint.trim();
    if hint.is_empty() {
        return ss.find_syntax_plain_text();
    }
    ss.find_syntax_by_token(hint)
        .or_else(|| ss.find_syntax_by_extension(hint))
        .or_else(|| {
            ss.syntaxes()
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(hint))
        })
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// Tokenize `code` as `lang_hint` and return pre-wrapped Ratatui Lines.
/// Each output `Line` corresponds to one source line (with newlines stripped).
pub fn highlight(code: &str, lang_hint: &str) -> Vec<Line<'static>> {
    let ss = syntax_set();
    let syn = detect(ss, lang_hint);
    let mut h = HighlightLines::new(syn, theme());
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, ss).unwrap_or_default();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(ranges.len());
        for (style, text) in ranges {
            let text = text.trim_end_matches('\n').to_string();
            if text.is_empty() {
                continue;
            }
            let c = style.foreground;
            spans.push(Span::styled(
                text,
                Style::default().fg(Color::Rgb(c.r, c.g, c.b)),
            ));
        }
        out.push(Line::from(spans));
    }
    // `LinesWithEndings` of trailing-newline-terminated text yields a final
    // empty line we don't need — drop it.
    if matches!(out.last(), Some(l) if l.spans.is_empty()) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_highlightable_without_crash() {
        let lines = highlight("hello world\nfoo", "");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn rust_snippet_produces_colored_spans() {
        let lines = highlight("fn main() { let x = 1; }\n", "rust");
        assert_eq!(lines.len(), 1);
        // Should have more than one span because syntax tokenizes.
        assert!(lines[0].spans.len() >= 2);
    }

    #[test]
    fn unknown_lang_falls_back_to_plain() {
        let lines = highlight("x = 1", "gibberish-not-a-language");
        assert_eq!(lines.len(), 1);
    }
}
