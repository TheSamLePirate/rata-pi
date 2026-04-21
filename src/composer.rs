//! Multi-line composer with cursor + optional vim-style normal mode.
//!
//! We roll our own instead of pulling `tui-textarea` — smaller dep tree,
//! exact control over the theme-aware render, and a matched vim-subset
//! that only does what we actually need.
//!
//! Character indexing is byte-based. Word motions respect UTF-8 char
//! boundaries via `char_indices`. The cursor column is clamped when
//! navigating rows so moving up/down never lands in the middle of a
//! multi-byte code point.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Insert,
    Normal,
}

impl Mode {
    #[allow(dead_code)] // exposed for future UI callers
    pub fn label(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Normal => "NORMAL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Composer {
    pub lines: Vec<String>,
    /// Row index into `lines`.
    pub row: usize,
    /// Byte offset into `lines[row]` (always on a char boundary).
    pub col: usize,
    pub mode: Mode,
    /// Pending operator for vim-style `dd` / `yy` etc.
    pub pending_op: Option<char>,
}

impl Default for Composer {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            mode: Mode::Insert,
            pending_op: None,
        }
    }
}

impl Composer {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(String::is_empty)
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_text(&mut self, s: &str) {
        self.lines = s.split('\n').map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].len();
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.row = 0;
        self.col = 0;
        self.pending_op = None;
    }

    // ── mutate (insert-mode primitives) ──────────────────────────────────

    pub fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.row];
        line.insert(self.col, c);
        self.col += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(ch);
            }
        }
    }

    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.row];
        let rest = line[self.col..].to_string();
        line.truncate(self.col);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            // Remove preceding char.
            let line = &mut self.lines[self.row];
            let prev = prev_char_boundary(line, self.col);
            line.drain(prev..self.col);
            self.col = prev;
        } else if self.row > 0 {
            // Join with previous line.
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].push_str(&current);
        }
    }

    pub fn delete_char_forward(&mut self) {
        let line_len = self.lines[self.row].len();
        if self.col < line_len {
            let line = &mut self.lines[self.row];
            let next = next_char_boundary(line, self.col);
            line.drain(self.col..next);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    // ── navigation ───────────────────────────────────────────────────────

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col = prev_char_boundary(&self.lines[self.row], self.col);
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].len();
        }
    }

    pub fn right(&mut self) {
        let len = self.lines[self.row].len();
        if self.col < len {
            self.col = next_char_boundary(&self.lines[self.row], self.col);
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = clamp_to_char_boundary(&self.lines[self.row], self.col);
        }
    }

    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = clamp_to_char_boundary(&self.lines[self.row], self.col);
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.lines[self.row].len();
    }

    pub fn top(&mut self) {
        self.row = 0;
        self.col = 0;
    }

    pub fn bottom(&mut self) {
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].len();
    }

    /// Jump to the start of the previous word (vim `b`).
    pub fn word_left(&mut self) {
        loop {
            if self.col == 0 {
                if self.row == 0 {
                    return;
                }
                self.row -= 1;
                self.col = self.lines[self.row].len();
                continue;
            }
            let line = &self.lines[self.row];
            // Step back one char.
            let prev = prev_char_boundary(line, self.col);
            let ch = line[prev..self.col].chars().next();
            self.col = prev;
            if matches!(ch, Some(c) if c.is_alphanumeric() || c == '_') {
                // Continue to the start of the current word-ish run.
                while self.col > 0 {
                    let p = prev_char_boundary(line, self.col);
                    let c = line[p..self.col].chars().next();
                    if matches!(c, Some(c) if c.is_alphanumeric() || c == '_') {
                        self.col = p;
                    } else {
                        break;
                    }
                }
                return;
            }
            // Else keep skipping whitespace/punctuation.
        }
    }

    /// Jump to the start of the next word (vim `w`).
    pub fn word_right(&mut self) {
        // Skip current word if we're on one.
        while self.col < self.lines[self.row].len() {
            let c = self.lines[self.row][self.col..].chars().next().unwrap();
            if !(c.is_alphanumeric() || c == '_') {
                break;
            }
            self.col = next_char_boundary(&self.lines[self.row], self.col);
        }
        // Then skip whitespace / punctuation until the next word start or
        // the next line.
        loop {
            let line = &self.lines[self.row];
            if self.col >= line.len() {
                if self.row + 1 >= self.lines.len() {
                    return;
                }
                self.row += 1;
                self.col = 0;
                continue;
            }
            let c = line[self.col..].chars().next().unwrap();
            if c.is_alphanumeric() || c == '_' {
                return;
            }
            self.col = next_char_boundary(line, self.col);
        }
    }

    pub fn kill_line_forward(&mut self) {
        let line = &mut self.lines[self.row];
        line.truncate(self.col);
    }

    pub fn kill_line_back(&mut self) {
        let line = &mut self.lines[self.row];
        line.drain(..self.col);
        self.col = 0;
    }

    /// Delete the current row. If it's the only row, leave an empty row.
    pub fn delete_line(&mut self) {
        if self.lines.len() > 1 {
            self.lines.remove(self.row);
            if self.row >= self.lines.len() {
                self.row = self.lines.len() - 1;
            }
        } else {
            self.lines[0].clear();
        }
        self.col = clamp_to_char_boundary(&self.lines[self.row], self.col);
    }

    /// Number of rows the composer wants to render at (≥1, ≤max).
    pub fn desired_rows(&self, max: u16) -> u16 {
        (self.lines.len() as u16).clamp(1, max)
    }

    // ── render ───────────────────────────────────────────────────────────

    /// Produce a ready-to-render `Vec<Line>`. The cursor cell is styled
    /// reversed on the current column (replacing the glyph beneath) so the
    /// position is visible without hardware cursor plumbing.
    pub fn render(&self, theme: &Theme, focus: bool) -> Vec<Line<'static>> {
        let mut out = Vec::with_capacity(self.lines.len());
        for (i, line) in self.lines.iter().enumerate() {
            if i == self.row && focus {
                out.push(render_with_cursor(line, self.col, theme, self.mode));
            } else {
                out.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default().fg(theme.text),
                )));
            }
        }
        out
    }
}

fn render_with_cursor(line: &str, col: usize, theme: &Theme, mode: Mode) -> Line<'static> {
    let cursor_style = Style::default()
        .fg(theme.text)
        .add_modifier(Modifier::REVERSED | Modifier::BOLD);
    // Insert-mode cursor is a bar BETWEEN chars; emulate with a reversed
    // space. Normal-mode cursor highlights the glyph UNDER it.
    match mode {
        Mode::Insert => {
            let before: String = line[..col].to_string();
            let after: String = line[col..].to_string();
            Line::from(vec![
                Span::styled(before, Style::default().fg(theme.text)),
                Span::styled(" ", cursor_style),
                Span::styled(after, Style::default().fg(theme.text)),
            ])
        }
        Mode::Normal => {
            if col >= line.len() {
                // Cursor past end of line — show a reversed block at end.
                Line::from(vec![
                    Span::styled(line.to_string(), Style::default().fg(theme.text)),
                    Span::styled(" ", cursor_style),
                ])
            } else {
                let before: String = line[..col].to_string();
                let next = next_char_boundary(line, col);
                let under: String = line[col..next].to_string();
                let after: String = line[next..].to_string();
                Line::from(vec![
                    Span::styled(before, Style::default().fg(theme.text)),
                    Span::styled(under, cursor_style),
                    Span::styled(after, Style::default().fg(theme.text)),
                ])
            }
        }
    }
}

// ── char-boundary helpers ────────────────────────────────────────────────

fn prev_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i.saturating_sub(1);
    while j > 0 && !s.is_char_boundary(j) {
        j -= 1;
    }
    j
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = (i + 1).min(s.len());
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

fn clamp_to_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i.min(s.len());
    while j > 0 && !s.is_char_boundary(j) {
        j -= 1;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_backspace() {
        let mut c = Composer::default();
        for ch in "hello".chars() {
            c.insert_char(ch);
        }
        assert_eq!(c.text(), "hello");
        assert_eq!(c.col, 5);
        c.backspace();
        assert_eq!(c.text(), "hell");
        c.home();
        c.end();
        assert_eq!(c.col, 4);
    }

    #[test]
    fn newline_splits_lines() {
        let mut c = Composer::default();
        for ch in "hello".chars() {
            c.insert_char(ch);
        }
        c.insert_newline();
        for ch in "world".chars() {
            c.insert_char(ch);
        }
        assert_eq!(c.text(), "hello\nworld");
        assert_eq!(c.lines.len(), 2);
        assert_eq!(c.row, 1);
        c.up();
        assert_eq!(c.row, 0);
    }

    #[test]
    fn backspace_joins_lines() {
        let mut c = Composer::default();
        c.set_text("ab\ncd");
        c.top();
        c.down();
        c.home();
        c.backspace();
        assert_eq!(c.text(), "abcd");
        assert_eq!(c.row, 0);
        assert_eq!(c.col, 2);
    }

    #[test]
    fn word_right_skips_spaces() {
        let mut c = Composer::default();
        c.set_text("foo bar baz");
        c.top();
        c.word_right();
        assert_eq!(c.col, 4); // on 'b' of "bar"
        c.word_right();
        assert_eq!(c.col, 8); // on 'b' of "baz"
    }

    #[test]
    fn word_left_lands_on_word_start() {
        let mut c = Composer::default();
        c.set_text("foo bar baz");
        c.end();
        c.word_left();
        assert_eq!(c.col, 8);
        c.word_left();
        assert_eq!(c.col, 4);
        c.word_left();
        assert_eq!(c.col, 0);
    }

    #[test]
    fn utf8_aware_nav() {
        let mut c = Composer::default();
        c.set_text("café");
        c.top();
        // é is a multi-byte char
        c.right();
        c.right();
        c.right();
        c.right();
        assert_eq!(c.col, c.lines[0].len());
    }

    #[test]
    fn delete_line_reduces_rows() {
        let mut c = Composer::default();
        c.set_text("a\nb\nc");
        c.row = 1;
        c.delete_line();
        assert_eq!(c.text(), "a\nc");
        assert_eq!(c.row, 1);
    }

    #[test]
    fn desired_rows_capped() {
        let mut c = Composer::default();
        c.set_text("1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
        assert_eq!(c.desired_rows(5), 5);
        assert_eq!(c.desired_rows(20), 10);
    }
}
