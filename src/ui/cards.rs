//! Reusable bordered card — the building block of the conversation view.
//!
//! A `Card` is a titled, rounded-border frame holding a body of `Line`s. The
//! renderer computes its own height at a given width using Ratatui's own
//! `Paragraph::line_count`, which keeps the virtualized transcript honest
//! about wrapping.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

use crate::theme::Theme;

/// V3.i.2 · how a focused card renders. `None` skips both focus cues;
/// the other variants toggle the Double border and the ▶ marker
/// independently so users can drop either encoding from `/settings →
/// Appearance → focus marker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusMode {
    None,
    Both,
    BorderOnly,
    MarkerOnly,
}

impl FocusMode {
    pub fn show_double_border(self) -> bool {
        matches!(self, Self::Both | Self::BorderOnly)
    }
    pub fn show_marker(self) -> bool {
        matches!(self, Self::Both | Self::MarkerOnly)
    }
}

#[derive(Debug, Clone)]
pub struct Card {
    pub icon: &'static str,
    pub title: String,
    pub right_title: Option<String>,
    pub body: Vec<Line<'static>>,
    pub border_color: Color,
    pub icon_color: Color,
    pub title_color: Color,
}

impl Card {
    /// How many terminal rows does this card occupy at the given outer width?
    /// Uses Ratatui's own `Paragraph::line_count` so wrap math matches render.
    ///
    /// Focus state does not affect height (the title row is one row whether
    /// focused or not), so the caller doesn't need to pass it.
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

    pub fn render(&self, f: &mut Frame, area: Rect, theme: &Theme, focus: FocusMode) {
        self.render_to_buffer(area, f.buffer_mut(), theme, focus);
    }

    /// Render into any buffer at `area`. Used directly by `render` and also
    /// by the transcript virtualization path when it needs a scratch buffer
    /// to clip a tall card to the visible window.
    ///
    /// `focus` is passed in (not stored on `Card`) so the cached body and
    /// chrome can be reused across focus toggles without rebuild.
    pub fn render_to_buffer(&self, area: Rect, buf: &mut Buffer, _theme: &Theme, focus: FocusMode) {
        let border_style = Style::default().fg(self.border_color);
        // Swap border weight based on focus so the focused card is obviously
        // different, without changing hue (role colors stay meaningful).
        // V3.i.2 · the focus-marker knob lets users drop one of the two
        // encodings (border / marker) when both feel too loud.
        let btype = if focus.show_double_border() {
            BorderType::Double
        } else {
            BorderType::Rounded
        };

        let focus_mark = if focus.show_marker() {
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
        block.render(area, buf);

        // Body with one-column left padding for breathing room.
        let pad = Rect::new(
            inner.x.saturating_add(1),
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        Paragraph::new(self.body.clone())
            .wrap(Wrap { trim: false })
            .render(pad, buf);
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
