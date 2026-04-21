//! Syntax highlighting via syntect.
//!
//! Loads the bundled syntax set exactly once (OnceLock) and keeps a tiny
//! cache of syntect themes keyed by name — the first call for a given
//! palette name clones and stores it, subsequent calls reuse. V3.g.1
//! adds the `theme` parameter so fenced-code colouring tracks the
//! active rata-pi theme instead of being locked to `base16-ocean.dark`.
//!
//! Language detection:
//!   1. `lang_hint` treated as a markdown fence tag (rust, py, sh, …).
//!   2. Fall back to extension lookup (rs, py, …).
//!   3. Fall back to case-insensitive full-name lookup (Rust, Python, …).
//!   4. Plain-text.

use std::sync::{Mutex, OnceLock};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme as SynTheme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::theme::Theme;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
/// Cache of resolved syntect `SynTheme`s keyed by name. Populated lazily
/// on first call for each name. Tiny — ~6 entries at most (one per
/// built-in rata-pi theme).
static PALETTE_CACHE: OnceLock<Mutex<std::collections::HashMap<&'static str, SynTheme>>> =
    OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn palette(name: &'static str) -> SynTheme {
    let cache = PALETTE_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache.lock().expect("palette cache mutex poisoned");
    if let Some(p) = guard.get(name) {
        return p.clone();
    }
    let ts = theme_set();
    // Fallback chain: requested name → base16-ocean.dark → first.
    let p = ts
        .themes
        .get(name)
        .or_else(|| ts.themes.get("base16-ocean.dark"))
        .or_else(|| ts.themes.values().next())
        .cloned()
        .expect("syntect ships at least one theme");
    guard.insert(name, p.clone());
    p
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

/// Tokenize `code` as `lang_hint` with the palette from `theme` and
/// return pre-wrapped Ratatui Lines. Each output `Line` corresponds to
/// one source line (with newlines stripped).
pub fn highlight(code: &str, lang_hint: &str, theme: &Theme) -> Vec<Line<'static>> {
    let ss = syntax_set();
    let syn = detect(ss, lang_hint);
    let pal = palette(theme.syntect_name);
    let mut h = HighlightLines::new(syn, &pal);
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

    fn dark() -> &'static Theme {
        crate::theme::find("tokyo-night").unwrap()
    }

    #[test]
    fn plain_text_is_highlightable_without_crash() {
        let lines = highlight("hello world\nfoo", "", dark());
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn rust_snippet_produces_colored_spans() {
        let lines = highlight("fn main() { let x = 1; }\n", "rust", dark());
        assert_eq!(lines.len(), 1);
        // Should have more than one span because syntax tokenizes.
        assert!(lines[0].spans.len() >= 2);
    }

    #[test]
    fn unknown_lang_falls_back_to_plain() {
        let lines = highlight("x = 1", "gibberish-not-a-language", dark());
        assert_eq!(lines.len(), 1);
    }

    /// V3.g.1 · the chosen syntect palette differs across built-in
    /// rata-pi themes. Swapping theme must change the colouring so
    /// fenced code stops looking out-of-place on a themed transcript.
    #[test]
    fn different_themes_produce_different_palettes() {
        let tokyo = crate::theme::find("tokyo-night").unwrap();
        let solarized = crate::theme::find("solarized-dark").unwrap();
        assert_ne!(tokyo.syntect_name, solarized.syntect_name);
        let a = highlight("fn main() { let x = 1; }", "rust", tokyo);
        let b = highlight("fn main() { let x = 1; }", "rust", solarized);
        // Same token count, different colours on at least one span.
        let a_colors: Vec<_> = a[0].spans.iter().map(|s| s.style.fg).collect();
        let b_colors: Vec<_> = b[0].spans.iter().map(|s| s.style.fg).collect();
        assert_eq!(a_colors.len(), b_colors.len());
        assert!(
            a_colors != b_colors,
            "expected palette swap to change at least one colour"
        );
    }

    /// V3.g.1 · a theme asking for an unknown syntect palette doesn't
    /// crash — the resolver falls back to `base16-ocean.dark` (or the
    /// first bundled palette) so highlighting stays functional.
    #[test]
    fn unknown_palette_falls_back_gracefully() {
        let mut bogus = *crate::theme::find("tokyo-night").unwrap();
        bogus.syntect_name = "not-a-real-palette";
        let lines = highlight("fn main() {}", "rust", &bogus);
        assert!(!lines.is_empty());
    }
}
