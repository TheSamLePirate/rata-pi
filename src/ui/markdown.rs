//! Markdown → Ratatui `Line`s.
//!
//! Pragmatic, pulldown-cmark driven. Supports: headings, emphasis, strong,
//! strikethrough, inline code, fenced code blocks, block quotes, rules, lists
//! (unordered + ordered), soft/hard breaks, links (shown underlined; url appended
//! in parens if different from text), images (→ `[image]` placeholder).
//!
//! Syntax highlighting for code fences is deferred to M5 (syntect bring-up).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn render(md: &str) -> Vec<Line<'static>> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut r = Renderer::default();
    for ev in Parser::new_ext(md, opts) {
        r.ev(ev);
    }
    r.finish()
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    list_stack: Vec<ListState>,
    link_targets: Vec<String>,
    in_code_block: bool,
    /// Deferred: we suppress the blank line inserted after a block element when
    /// the next event is also a block-terminating one, to avoid double spacing.
    last_was_block_end: bool,
}

struct ListState {
    /// Current item number for ordered lists; `None` for bullets.
    next: Option<u64>,
}

impl Renderer {
    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_current();
        // Trim trailing blank lines.
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
                self.cur.push(Span::styled(
                    prefix.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
                self.push_style(Modifier::BOLD, Some(Color::Cyan));
            }
            Event::End(TagEnd::Heading(_)) => {
                self.pop_style();
                self.break_line();
                self.blank_line();
            }

            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_current();
                self.push_style(Modifier::ITALIC, Some(Color::DarkGray));
                self.cur.push(Span::styled(
                    "│ ".to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.pop_style();
                self.flush_current();
                self.blank_line();
            }

            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush_current();
                self.in_code_block = true;
                let lang = match kind {
                    CodeBlockKind::Fenced(l) if !l.is_empty() => format!("┄ {l} ┄"),
                    _ => "┄".to_string(),
                };
                self.lines
                    .push(Line::styled(lang, Style::default().fg(Color::DarkGray)));
                self.push_style(Modifier::empty(), Some(Color::Yellow));
            }
            Event::End(TagEnd::CodeBlock) => {
                self.pop_style();
                self.flush_current();
                self.in_code_block = false;
                self.lines.push(Line::styled(
                    "┄".to_string(),
                    Style::default().fg(Color::DarkGray),
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
                    .push(Span::styled(prefix, Style::default().fg(Color::DarkGray)));
            }
            Event::End(TagEnd::Item) => {
                self.break_line();
            }

            Event::Rule => {
                self.flush_current();
                self.lines.push(Line::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                ));
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
                self.push_style(Modifier::UNDERLINED, Some(Color::Blue));
            }
            Event::End(TagEnd::Link) => {
                self.pop_style();
                if let Some(url) = self.link_targets.pop() {
                    // If we still have current spans rendered (link text) and url
                    // differs meaningfully, append the URL in parens for discoverability.
                    let last_text = self
                        .cur
                        .last()
                        .map(|s| s.content.to_string())
                        .unwrap_or_default();
                    if !url.is_empty() && last_text != url {
                        self.cur.push(Span::styled(
                            format!(" ({url})"),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
            }

            Event::Start(Tag::Image { .. }) => {
                self.cur.push(Span::styled(
                    "[image]".to_string(),
                    Style::default().fg(Color::Magenta),
                ));
            }
            Event::End(TagEnd::Image) => {}

            Event::Code(text) => {
                self.cur.push(Span::styled(
                    format!("`{text}`"),
                    Style::default().fg(Color::Yellow),
                ));
            }

            Event::Text(t) => self.push_text(&t),

            Event::SoftBreak => {
                if self.in_code_block {
                    self.break_line();
                } else {
                    self.cur.push(Span::raw(" "));
                }
            }
            Event::HardBreak => self.break_line(),

            Event::TaskListMarker(done) => {
                let s = if done { "[x] " } else { "[ ] " };
                let style = if done {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
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

    fn collect(md: &str) -> String {
        render(md)
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
}
