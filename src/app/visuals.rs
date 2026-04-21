//! Per-entry transcript rendering cache.
//!
//! The `draw_body` hot path reads from this cache rather than calling
//! `markdown::render` / `syntax::highlight` every frame. The cache is
//! rebuilt by `update_visuals_cache` (invoked once per frame from
//! `prepare_frame_caches`) and keyed on:
//!
//! * the active theme name (theme switch wipes everything);
//! * a per-entry fingerprint over the mutable fields; and
//! * the content width — height is recomputed when the terminal resizes,
//!   but `visual` itself is reused.
//!
//! The `Visual` enum is the render-ready product of the cache: either a
//! `Card` with a pre-baked body + chrome, or an `InlineRow` for single-
//! line entries. It owns its line data so `draw_body` can render without
//! any further allocations per visible card.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::ui::cards::{Card, InlineRow};
use crate::ui::transcript::{CompactionState, Entry, RetryState};

use super::{App, LiveState, build_one_visual};

/// One rendered Entry. Sized (`height()`) and drawn (`render()`) without
/// any allocation beyond what the cached body already holds.
pub(super) enum Visual {
    Card(Card),
    Inline(InlineRow),
}

impl Visual {
    pub(super) fn height(&self, outer_w: u16) -> u16 {
        match self {
            Self::Card(c) => c.height(outer_w),
            Self::Inline(r) => r.height(outer_w),
        }
    }

    pub(super) fn render(&self, f: &mut ratatui::Frame, area: Rect, idx: usize, app: &App) {
        match self {
            Self::Card(c) => {
                // V2.11.2 · focus is an out-of-band lookup (no clone of the
                // Card just to flip a flag). The renderer swaps to
                // BorderType::Double + prepends a ▶ marker when focused.
                let focused = app.focus_idx == Some(idx);
                c.render(f, area, &app.theme, focused);
            }
            Self::Inline(r) => r.render(f, area),
        }
    }

    /// Render a visual into `target` when part of it is scrolled off the
    /// top (`skip > 0`) and/or the bottom extends below the viewport.
    ///
    /// Allocation-free: we use `Block::borders` to opt in/out of each
    /// border edge (top border omitted when clipped from above, bottom
    /// when clipped from below) and `Paragraph::scroll` on the body to
    /// shift it by the appropriate wrapped-row count. This is what makes
    /// long streaming assistant cards cheap to render — no per-frame
    /// scratch `Buffer` sized to the full card height.
    pub(super) fn render_clipped(
        &self,
        f: &mut ratatui::Frame,
        target: Rect,
        idx: usize,
        app: &App,
        skip: u16,
        full_h: u16,
    ) {
        match self {
            Self::Card(c) => render_card_clipped(f, target, c, app, idx, skip, full_h),
            Self::Inline(r) => {
                f.render_widget(
                    Paragraph::new(r.lines.clone())
                        .wrap(Wrap { trim: false })
                        .scroll((skip, 0)),
                    target,
                );
            }
        }
    }
}

/// Partial-card renderer: selectively omits the top/bottom border edges
/// when the card is clipped and uses `Paragraph::scroll` to skip body rows.
fn render_card_clipped(
    f: &mut ratatui::Frame,
    target: Rect,
    card: &Card,
    app: &App,
    idx: usize,
    skip: u16,
    full_h: u16,
) {
    let focused = app.focus_idx == Some(idx);
    let show_top = skip == 0;
    let show_bottom = skip.saturating_add(target.height) >= full_h;

    let mut borders = Borders::LEFT | Borders::RIGHT;
    if show_top {
        borders |= Borders::TOP;
    }
    if show_bottom {
        borders |= Borders::BOTTOM;
    }

    let btype = if focused {
        BorderType::Double
    } else {
        BorderType::Rounded
    };
    let border_style = Style::default().fg(card.border_color);

    let focus_mark = if focused && show_top {
        vec![
            Span::styled(
                " ▶",
                Style::default()
                    .fg(card.border_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]
    } else if show_top {
        vec![Span::raw(" ")]
    } else {
        Vec::new()
    };

    let mut block = Block::default()
        .borders(borders)
        .border_type(btype)
        .border_style(border_style);
    if show_top {
        let mut title_spans = focus_mark;
        title_spans.extend([
            Span::styled(
                card.icon.to_string(),
                Style::default()
                    .fg(card.icon_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                card.title.clone(),
                Style::default()
                    .fg(card.title_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]);
        block = block.title(Line::from(title_spans));
        if let Some(r) = &card.right_title {
            block = block.title_top(
                Line::from(vec![Span::styled(
                    format!(" {r} "),
                    Style::default().fg(card.border_color),
                )])
                .right_aligned(),
            );
        }
    }

    let inner = block.inner(target);
    f.render_widget(block, target);

    // body_skip: if we drew a top border (row 0 of the card), body starts
    // at inner.y. If we didn't, we already ate the top border's 1 row in
    // `skip`, so body rows to skip = skip - 1.
    let body_skip = if show_top { 0 } else { skip.saturating_sub(1) };

    let pad = Rect::new(
        inner.x.saturating_add(1),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    f.render_widget(
        Paragraph::new(card.body.clone())
            .wrap(Wrap { trim: false })
            .scroll((body_skip, 0)),
        pad,
    );
}

/// Fingerprint the mutable bits of an Entry. Two entries with equal
/// fingerprints produce equal `Visual`s under the same theme +
/// show_thinking setting, so cached slots can be reused.
///
/// For append-only text (User/Thinking/Assistant/Info/Warn/Error), the
/// `len()` is a strict monotonic proxy for content identity in our model:
/// these entries either grow at the tail or don't change. For structured
/// entries (ToolCall, BashExec, Compaction, Retry) we hash the specific
/// fields that can mutate after creation.
pub(super) fn fingerprint_entry(entry: &Entry, show_thinking: bool) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    std::mem::discriminant(entry).hash(&mut h);
    match entry {
        Entry::User(s) => s.len().hash(&mut h),
        Entry::Thinking(s) => {
            s.len().hash(&mut h);
            show_thinking.hash(&mut h);
        }
        Entry::Assistant(s) => s.len().hash(&mut h),
        Entry::ToolCall(tc) => {
            tc.name.len().hash(&mut h);
            tc.output.len().hash(&mut h);
            (tc.status as u8).hash(&mut h);
            tc.is_error.hash(&mut h);
            tc.expanded.hash(&mut h);
        }
        Entry::BashExec(bx) => {
            bx.command.len().hash(&mut h);
            bx.output.len().hash(&mut h);
            bx.exit_code.hash(&mut h);
            bx.cancelled.hash(&mut h);
            bx.truncated.hash(&mut h);
        }
        Entry::Info(s) | Entry::Warn(s) | Entry::Error(s) => s.len().hash(&mut h),
        Entry::Compaction(c) => {
            c.reason.len().hash(&mut h);
            match &c.state {
                CompactionState::Running => 0u8.hash(&mut h),
                CompactionState::Done { summary } => {
                    1u8.hash(&mut h);
                    summary.as_deref().map(str::len).unwrap_or(0).hash(&mut h);
                }
                CompactionState::Aborted => 2u8.hash(&mut h),
                CompactionState::Failed(e) => {
                    3u8.hash(&mut h);
                    e.len().hash(&mut h);
                }
            }
        }
        Entry::Retry(r) => {
            r.attempt.hash(&mut h);
            r.max_attempts.hash(&mut h);
            match &r.state {
                RetryState::Waiting { delay_ms, error } => {
                    0u8.hash(&mut h);
                    delay_ms.hash(&mut h);
                    error.len().hash(&mut h);
                }
                RetryState::Succeeded => 1u8.hash(&mut h),
                RetryState::Exhausted(e) => {
                    2u8.hash(&mut h);
                    e.len().hash(&mut h);
                }
            }
        }
        Entry::TurnMarker { number } => number.hash(&mut h),
    }
    h.finish()
}

/// Per-entry cache slot.
pub(super) struct CachedVisual {
    pub(super) fingerprint: u64,
    pub(super) visual: Visual,
    /// Width at which `height` was last computed. A width change is the
    /// only reason we recompute `height` without also rebuilding `visual`.
    pub(super) width: u16,
    pub(super) height: u16,
}

/// Transcript rendering cache. Keyed on theme change and on each entry's
/// fingerprint.
#[derive(Default)]
pub(super) struct VisualsCache {
    pub(super) theme_key: String,
    pub(super) entries: Vec<CachedVisual>,
    /// V3.b · last transcript mutation epoch observed. When the current
    /// `Transcript::mutation_epoch` matches this, the cache is known to
    /// be in sync with the transcript and the fingerprint walk can be
    /// skipped entirely. The live-tail path still forces a rebuild of
    /// the streaming Assistant entry.
    pub(super) last_seen_epoch: u64,
    /// V3.b · content-width captured on the last walk. A mismatch means
    /// heights need to be refreshed (but visuals can be reused).
    pub(super) last_seen_width: u16,
}

/// Rebuild stale cache slots against the current transcript, and refresh
/// heights if `content_w` has changed. Idempotent when nothing changed.
///
/// V3.b · the walk is now incremental: when `Transcript::mutation_epoch`
/// matches the epoch we saw on the previous frame AND the theme + width
/// haven't changed AND no live streaming tail is present, we return
/// early. Previously every frame paid an O(n) hash over every entry
/// just to discover nothing changed.
///
/// The live-streaming tail (last Assistant entry while streaming) is
/// still forced to rebuild every frame so the cursor blink + "pi ·
/// streaming" title stay live without a dedicated invalidation channel.
pub(super) fn update_visuals_cache(app: &mut App, content_w: u16) {
    let current_epoch = app.transcript.mutation_epoch();

    // Theme change → wipe everything and force a full walk.
    let theme_changed = app.visuals_cache.theme_key != app.theme.name;
    if theme_changed {
        app.visuals_cache.theme_key = app.theme.name.to_string();
        app.visuals_cache.entries.clear();
    }

    let width_changed = app.visuals_cache.last_seen_width != content_w;

    let entries_len = app.transcript.entries().len();
    // Shrinkage (e.g. /clear, /switch session).
    if app.visuals_cache.entries.len() > entries_len {
        app.visuals_cache.entries.truncate(entries_len);
    }

    // Which entry (if any) is the live-streaming tail this frame.
    let live_tail_idx: Option<usize> = if app.is_streaming
        && matches!(app.live, LiveState::Streaming | LiveState::Llm)
        && matches!(app.transcript.entries().last(), Some(Entry::Assistant(_)))
    {
        Some(entries_len - 1)
    } else {
        None
    };

    let epoch_stale = current_epoch != app.visuals_cache.last_seen_epoch;

    // Fast path: no mutations, no theme change, no width change, no live
    // tail. Skip the fingerprint loop — everything in the cache is already
    // correct for this frame.
    if !theme_changed
        && !width_changed
        && !epoch_stale
        && live_tail_idx.is_none()
        && app.visuals_cache.entries.len() == entries_len
    {
        return;
    }

    for i in 0..entries_len {
        let is_live = live_tail_idx == Some(i);
        let entry_fp = {
            let entry = &app.transcript.entries()[i];
            fingerprint_entry(entry, app.show_thinking)
        };
        // Live-tail's fingerprint is ignored — we always rebuild it.
        let target_fp = if is_live { u64::MAX } else { entry_fp };

        let must_rebuild = match app.visuals_cache.entries.get(i) {
            None => true,
            Some(_) if is_live => true,
            Some(c) => c.fingerprint != target_fp,
        };

        if must_rebuild {
            // Snapshot Entry by index; `build_one_visual` needs a &App
            // read-only, but we can't borrow app.transcript and &app
            // together. Clone the entry — cheap for small variants.
            let entry = app.transcript.entries()[i].clone();
            let visual = build_one_visual(&entry, app, is_live);
            let height = visual.height(content_w);
            let slot = CachedVisual {
                fingerprint: target_fp,
                visual,
                width: content_w,
                height,
            };
            if i < app.visuals_cache.entries.len() {
                app.visuals_cache.entries[i] = slot;
            } else {
                app.visuals_cache.entries.push(slot);
            }
        } else if let Some(c) = app.visuals_cache.entries.get_mut(i)
            && c.width != content_w
        {
            // Width changed but content didn't — recompute height only.
            c.height = c.visual.height(content_w);
            c.width = content_w;
        }
    }

    app.visuals_cache.last_seen_epoch = current_epoch;
    app.visuals_cache.last_seen_width = content_w;
}
