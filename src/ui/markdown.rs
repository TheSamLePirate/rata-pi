//! Markdown → Ratatui `Line`s.
//!
//! Pragmatic, pulldown-cmark driven. Supports: headings, emphasis, strong,
//! strikethrough, inline code, fenced code blocks, block quotes, rules, lists
//! (unordered + ordered), soft/hard breaks, links (shown underlined; url appended
//! in parens if different from text), images (→ `[image]` placeholder).
//!
//! V3.g.1 · takes `&Theme` and renders every colour through semantic
//! slots (accent / muted / dim / warning / success / error / …). The
//! fenced-code-block syntect pass also threads the theme through so
//! highlight colours match the active palette.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

pub fn render(md: &str, theme: &Theme) -> Vec<Line<'static>> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut r = Renderer::new(theme);
    for ev in Parser::new_ext(md, opts) {
        r.ev(ev);
    }
    r.finish()
}

struct Renderer<'a> {
    theme: &'a Theme,
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<ListState>,
    link_targets: Vec<String>,
    in_code_block: bool,
    code_buf: String,
    code_lang: String,
    last_was_block_end: bool,
}

struct ListState {
    /// Current item number for ordered lists; `None` for bullets.
    next: Option<u64>,
}

impl<'a> Renderer<'a> {
    fn new(theme: &'a Theme) -> Self {
        Self {
            theme,
            lines: Vec::new(),
            cur: Vec::new(),
            style_stack: Vec::new(),
            list_stack: Vec::new(),
            link_targets: Vec::new(),
            in_code_block: false,
            code_buf: String::new(),
            code_lang: String::new(),
            last_was_block_end: false,
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_current();
        while matches!(self.lines.last(), Some(l) if l.spans.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }

    fn flush_current(&mut self) {
        if !self.cur.is_empty() {
            let spans = std::mem::take(&mut self.cur);
            self.lines.push(Line::from(spans));
        }
    }

    fn break_line(&mut self) {
        let spans = std::mem::take(&mut self.cur);
        self.lines.push(Line::from(spans));
    }

    fn blank_line(&mut self) {
        if !matches!(self.lines.last(), Some(l) if l.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }

    fn cur_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, add_mod: Modifier, fg: Option<Color>) {
        let mut s = self.cur_style().add_modifier(add_mod);
        if let Some(c) = fg {
            s = s.fg(c);
        }
        self.style_stack.push(s);
    }

    fn pop_style(&mut self) {
        self.style_stack.pop();
    }

    fn push_text(&mut self, text: &str) {
        let style = self.cur_style();
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                self.break_line();
            }
            if !part.is_empty() {
                self.cur.push(Span::styled(part.to_string(), style));
            }
        }
    }

    fn list_prefix(&mut self) -> String {
        let depth = self.list_stack.len();
        let indent = "  ".repeat(depth.saturating_sub(1));
        let Some(top) = self.list_stack.last_mut() else {
            return indent;
        };
        if let Some(n) = top.next.as_mut() {
            let s = format!("{indent}{n}. ");
            *n += 1;
            s
        } else {
            format!("{indent}• ")
        }
    }

    fn ev(&mut self, ev: Event<'_>) {
        let t = self.theme;
        match ev {
            // ── block boundaries ─────────────────────────────────────────
            Event::Start(Tag::Paragraph) => {
                self.flush_current();
            }
            Event::End(TagEnd::Paragraph) => {
                self.flush_current();
                self.blank_line();
                self.last_was_block_end = true;
            }

            Event::Start(Tag::Heading { level, .. }) => {
                self.flush_current();
                let prefix = match level {
                    HeadingLevel::H1 => "# ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "### ",
                    HeadingLevel::H4 => "#### ",
                    HeadingLevel::H5 => "##### ",
                    HeadingLevel::H6 => "###### ",
                };
                self.cur
                    .push(Span::styled(prefix.to_string(), Style::default().fg(t.dim)));
                self.push_style(Modifier::BOLD, Some(t.accent));
            }
            Event::End(TagEnd::Heading(_)) => {
                self.pop_style();
                self.break_line();
                self.blank_line();
            }

            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_current();
                self.push_style(Modifier::ITALIC, Some(t.muted));
                self.cur
                    .push(Span::styled("│ ".to_string(), Style::default().fg(t.dim)));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.pop_style();
                self.flush_current();
                self.blank_line();
            }

            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush_current();
                self.in_code_block = true;
                self.code_buf.clear();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    _ => String::new(),
                };
                let lang_chip = if self.code_lang.is_empty() {
                    "┄─────".to_string()
                } else {
                    format!("┄─── {} ───┄", self.code_lang)
                };
                self.lines
                    .push(Line::styled(lang_chip, Style::default().fg(t.dim)));
            }
            Event::End(TagEnd::CodeBlock) => {
                let code = std::mem::take(&mut self.code_buf);
                let lang = std::mem::take(&mut self.code_lang);
                for l in crate::ui::syntax::highlight(&code, &lang, t) {
                    self.lines.push(l);
                }
                self.in_code_block = false;
                self.lines.push(Line::styled(
                    "└──────".to_string(),
                    Style::default().fg(t.dim),
                ));
                self.blank_line();
            }

            Event::Start(Tag::List(start)) => {
                self.flush_current();
                self.list_stack.push(ListState { next: start });
            }
            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.blank_line();
                }
            }
            Event::Start(Tag::Item) => {
                self.flush_current();
                let prefix = self.list_prefix();
                self.cur
                    .push(Span::styled(prefix, Style::default().fg(t.dim)));
            }
            Event::End(TagEnd::Item) => {
                self.break_line();
            }

            Event::Rule => {
                self.flush_current();
                self.lines
                    .push(Line::styled("─".repeat(40), Style::default().fg(t.dim)));
                self.blank_line();
            }

            // ── inline formatting ────────────────────────────────────────
            Event::Start(Tag::Emphasis) => self.push_style(Modifier::ITALIC, None),
            Event::End(TagEnd::Emphasis) => self.pop_style(),
            Event::Start(Tag::Strong) => self.push_style(Modifier::BOLD, None),
            Event::End(TagEnd::Strong) => self.pop_style(),
            Event::Start(Tag::Strikethrough) => self.push_style(Modifier::CROSSED_OUT, None),
            Event::End(TagEnd::Strikethrough) => self.pop_style(),

            Event::Start(Tag::Link { dest_url, .. }) => {
                self.link_targets.push(dest_url.to_string());
                self.push_style(Modifier::UNDERLINED, Some(t.accent_strong));
            }
            Event::End(TagEnd::Link) => {
                self.pop_style();
                if let Some(url) = self.link_targets.pop() {
                    let last_text = self
                        .cur
                        .last()
                        .map(|s| s.content.to_string())
                        .unwrap_or_default();
                    if !url.is_empty() && last_text != url {
                        self.cur.push(Span::styled(
                            format!(" ({url})"),
                            Style::default().fg(t.dim),
                        ));
                    }
                }
            }

            Event::Start(Tag::Image { .. }) => {
                self.cur.push(Span::styled(
                    "[image]".to_string(),
                    Style::default().fg(t.accent),
                ));
            }
            Event::End(TagEnd::Image) => {}

            Event::Code(text) => {
                self.cur.push(Span::styled(
                    format!("`{text}`"),
                    Style::default().fg(t.warning),
                ));
            }

            Event::Text(t_ev) => {
                if self.in_code_block {
                    self.code_buf.push_str(&t_ev);
                } else {
                    self.push_text(&t_ev);
                }
            }

            Event::SoftBreak => {
                if self.in_code_block {
                    self.code_buf.push('\n');
                } else {
                    self.cur.push(Span::raw(" "));
                }
            }
            Event::HardBreak => {
                if self.in_code_block {
                    self.code_buf.push('\n');
                } else {
                    self.break_line();
                }
            }

            Event::TaskListMarker(done) => {
                let s = if done { "[x] " } else { "[ ] " };
                let style = if done {
                    Style::default().fg(t.success)
                } else {
                    Style::default().fg(t.dim)
                };
                self.cur.push(Span::styled(s.to_string(), style));
            }

            // Ignore FootnoteReference, Html, etc. for now.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> &'static Theme {
        crate::theme::default_theme()
    }

    fn collect(md: &str) -> String {
        render(md, theme())
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn headings_and_paragraph() {
        let out = collect("# Title\n\nhello **world**");
        assert!(out.contains("# Title"));
        assert!(out.contains("hello world"));
    }

    #[test]
    fn inline_code() {
        assert!(collect("use `cargo`").contains("`cargo`"));
    }

    #[test]
    fn fenced_code_block() {
        let out = collect("```rust\nlet x = 1;\n```");
        assert!(out.contains("let x = 1;"));
        assert!(out.contains("┄"));
    }

    #[test]
    fn unordered_list() {
        let out = collect("- one\n- two");
        assert!(out.contains("• one"));
        assert!(out.contains("• two"));
    }

    #[test]
    fn ordered_list() {
        let out = collect("1. first\n2. second");
        assert!(out.contains("1. first"));
        assert!(out.contains("2. second"));
    }

    #[test]
    fn link_shows_url_when_different() {
        let out = collect("[docs](https://ratatui.rs)");
        assert!(out.contains("docs"));
        assert!(out.contains("(https://ratatui.rs)"));
    }

    #[test]
    fn task_list() {
        let out = collect("- [x] done\n- [ ] todo");
        assert!(out.contains("[x] done"));
        assert!(out.contains("[ ] todo"));
    }

    /// V3.g.1 · swapping the theme MUST change at least one rendered
    /// colour — the renderer is no longer locked to the V2.x hardcoded
    /// DarkGray/Cyan/Blue/Yellow palette.
    #[test]
    fn heading_color_tracks_theme_accent() {
        let tokyo = crate::theme::find("tokyo-night").unwrap();
        let dracula = crate::theme::find("dracula").unwrap();
        let a = render("# Heading\n\nbody", tokyo);
        let b = render("# Heading\n\nbody", dracula);
        // First line's 2nd span is the heading text styled with accent.
        let a_fg = a[0].spans[1].style.fg;
        let b_fg = b[0].spans[1].style.fg;
        assert_eq!(a_fg, Some(tokyo.accent));
        assert_eq!(b_fg, Some(dracula.accent));
        assert_ne!(a_fg, b_fg);
    }
}
