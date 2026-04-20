//! Reusable bordered card — the building block of the conversation view.
//!
//! A `Card` is a titled, rounded-border frame holding a body of `Line`s. The
//! renderer computes its own height at a given width using Ratatui's own
//! `Paragraph::line_count`, which keeps the virtualized transcript honest
//! about wrapping.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct Card {
    pub icon: &'static str,
    pub title: String,
    pub right_title: Option<String>,
    pub body: Vec<Line<'static>>,
    pub border_color: Color,
    pub icon_color: Color,
    pub title_color: Color,
    pub focused: bool,
}

impl Card {
    /// How many terminal rows does this card occupy at the given outer width?
    /// Uses Ratatui's own `Paragraph::line_count` so wrap math matches render.
    pub fn height(&self, outer_width: u16) -> u16 {
        if outer_width < 4 {
            return 2;
        }
        let inner_w = outer_width.saturating_sub(4); // 2 borders + 2 pad
        let body_h = if self.body.is_empty() {
            1
        } else {
            Paragraph::new(self.body.clone())
                .wrap(Wrap { trim: false })
                .line_count(inner_w) as u16
        };
        body_h.saturating_add(2) // top + bottom border
    }

    pub fn render(&self, f: &mut Frame, area: Rect, _theme: &Theme) {
        let border_style = Style::default().fg(self.border_color);
        // Swap border weight based on focus so the focused card is obviously
        // different, without changing hue (role colors stay meaningful).
        let btype = if self.focused {
            BorderType::Double
        } else {
            BorderType::Rounded
        };

        // Leading marker + icon + title. The marker only appears when
        // focused so the focused card gets a very clear "you are here" cue.
        let focus_mark = if self.focused {
            vec![
                Span::styled(
                    " ▶",
                    Style::default()
                        .fg(self.border_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ]
        } else {
            vec![Span::raw(" ")]
        };
        let mut title_spans = focus_mark;
        title_spans.extend([
            Span::styled(
                self.icon.to_string(),
                Style::default()
                    .fg(self.icon_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                self.title.clone(),
                Style::default()
                    .fg(self.title_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);
        let title_line = Line::from(title_spans);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(btype)
            .border_style(border_style)
            .title(title_line);
        if let Some(r) = &self.right_title {
            block = block.title_top(
                Line::from(vec![Span::styled(
                    format!(" {r} "),
                    Style::default().fg(self.border_color),
                )])
                .right_aligned(),
            );
        }

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Body with one-column left padding for breathing room.
        let pad = Rect::new(
            inner.x.saturating_add(1),
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        f.render_widget(
            Paragraph::new(self.body.clone()).wrap(Wrap { trim: false }),
            pad,
        );
    }
}

/// Simple 1–N inline lines (no borders) used for Info / Warn / Error /
/// Compaction / Retry rows that read better flat.
#[derive(Debug, Clone)]
pub struct InlineRow {
    pub lines: Vec<Line<'static>>,
}

impl InlineRow {
    pub fn height(&self, _outer_width: u16) -> u16 {
        self.lines.len().max(1) as u16
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        f.render_widget(
            Paragraph::new(self.lines.clone()).wrap(Wrap { trim: false }),
            area,
        );
    }
}
