//! Terminal chrome drawing: header, body, editor, footer, status,
//! widget strips, toasts, scrollbar, and the `kb()` keybinding chip.
//!
//! Modal drawing (`draw_modal` and its per-modal body builders) stays
//! in `super` for now — it's a big tangle that will get its own
//! extraction in a follow-up.
//!
//! These helpers read from `super::VisualsCache` and `super::App`; they
//! mutate only the frame-scoped `MouseMap` out-arg.

use std::time::Duration;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph, Sparkline, Wrap};

use crate::theme::{self, Theme};
use crate::ui::ext_ui::NotifyKind;

use super::{App, ComposerMode, LiveState, MouseMap, WidgetPlacement, draw_modal, plan_card};

// ───────────────────────────────────────────────────────── drawing ──

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub(super) fn draw(f: &mut ratatui::Frame, app: &App, mm: &mut MouseMap) {
    let area = f.area();

    // Ext-ui widgets above/below the editor get their own strips.
    let above_widgets = app.ext_ui.widgets_at(WidgetPlacement::AboveEditor);
    let below_widgets = app.ext_ui.widgets_at(WidgetPlacement::BelowEditor);
    let above_h: u16 = above_widgets
        .iter()
        .map(|w| w.lines.len() as u16)
        .sum::<u16>()
        .min(8);
    let below_h: u16 = below_widgets
        .iter()
        .map(|w| w.lines.len() as u16)
        .sum::<u16>()
        .min(8);

    // StatusWidget takes a 4-row strip between the editor and footer, unless
    // the terminal is too short.
    let status_h: u16 = if area.height >= 20 { 4 } else { 0 };

    // Plan card above the editor when a plan is active. Height = items + 2
    // borders, capped so a long plan doesn't crowd out the transcript.
    let plan_h: u16 = if app.plan.is_active() || app.plan.all_done() {
        let items = app.plan.total().min(6) as u16;
        items.saturating_add(2).min(8)
    } else {
        0
    };

    let constraints: Vec<Constraint> = {
        let mut c = vec![
            Constraint::Length(1), // header
            Constraint::Min(3),    // body
        ];
        if above_h > 0 {
            c.push(Constraint::Length(above_h));
        }
        if plan_h > 0 {
            c.push(Constraint::Length(plan_h));
        }
        // Editor height grows with the composer content — min 3, max 10.
        let composer_rows = app.composer.desired_rows(8);
        c.push(Constraint::Length(composer_rows.saturating_add(2))); // +2 borders
        if below_h > 0 {
            c.push(Constraint::Length(below_h));
        }
        if status_h > 0 {
            c.push(Constraint::Length(status_h));
        }
        c.push(Constraint::Length(2)); // footer
        c
    };
    let rects = Layout::vertical(constraints).split(area);
    let mut idx = 0;
    let header = rects[idx];
    idx += 1;
    let body = rects[idx];
    idx += 1;
    let above_rect = if above_h > 0 {
        let r = rects[idx];
        idx += 1;
        Some(r)
    } else {
        None
    };
    let plan_rect = if plan_h > 0 {
        let r = rects[idx];
        idx += 1;
        Some(r)
    } else {
        None
    };
    let editor_area = rects[idx];
    idx += 1;
    let below_rect = if below_h > 0 {
        let r = rects[idx];
        idx += 1;
        Some(r)
    } else {
        None
    };
    let status_rect = if status_h > 0 {
        let r = rects[idx];
        idx += 1;
        Some(r)
    } else {
        None
    };
    let footer = rects[idx];

    draw_header(f, header, app);
    draw_body(f, body, app, mm);
    if let Some(r) = above_rect {
        draw_widgets(f, r, &above_widgets);
    }
    if let Some(r) = plan_rect {
        plan_card(&app.plan, &app.theme).render(f, r, &app.theme, false);
    }
    draw_editor(f, editor_area, app);
    if let Some(r) = below_rect {
        draw_widgets(f, r, &below_widgets);
    }
    if let Some(r) = status_rect {
        draw_status(f, r, app);
    }
    draw_footer(f, footer, app);

    draw_toasts(f, area, &app.ext_ui.toasts, &app.theme);

    if let Some(modal) = &app.modal {
        draw_modal(f, area, modal, app);
    }
}

fn draw_widgets(f: &mut ratatui::Frame, area: Rect, widgets: &[&crate::ui::ext_ui::Widget]) {
    use crate::ui::ext_ui::Widget as ExtWidget;
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, w) in widgets.iter().enumerate() {
        if i > 0 {
            lines.push(Line::default());
        }
        let w: &ExtWidget = w;
        for ln in &w.lines {
            lines.push(Line::from(Span::raw(ln.clone())));
        }
    }
    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_toasts(
    f: &mut ratatui::Frame,
    area: Rect,
    toasts: &std::collections::VecDeque<crate::ui::ext_ui::Toast>,
    theme: &Theme,
) {
    if toasts.is_empty() {
        return;
    }
    let max_w = area.width.saturating_mul(5) / 10;
    let width = max_w.min(area.width);
    let n = toasts.len() as u16;
    let height = n.min(area.height);
    if width < 10 || height == 0 {
        return;
    }
    let x = area.x + area.width.saturating_sub(width);
    let y = area.y + area.height.saturating_sub(height + 3);
    let rect = Rect::new(x, y, width, height);
    f.render_widget(Clear, rect);
    let lines: Vec<Line<'static>> = toasts
        .iter()
        .map(|t| {
            let color = match t.kind {
                NotifyKind::Info => theme.accent_strong,
                NotifyKind::Warning => theme.warning,
                NotifyKind::Error => theme.error,
            };
            Line::from(vec![
                Span::styled(
                    format!(
                        " {} ",
                        match t.kind {
                            NotifyKind::Info => "ℹ",
                            NotifyKind::Warning => "⚠",
                            NotifyKind::Error => "✗",
                        }
                    ),
                    Style::default()
                        .bg(color)
                        .fg(Color::Rgb(0, 0, 0))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(t.text.clone(), Style::default().fg(color)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(Text::from(lines)), rect);
}

fn draw_status(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let border_color = match app.live {
        LiveState::Idle => t.border_idle,
        LiveState::Error => t.error,
        LiveState::Retrying { .. } => t.warning,
        LiveState::Tool => t.role_tool,
        LiveState::Thinking => t.role_thinking,
        LiveState::Streaming | LiveState::Llm | LiveState::Sending => t.border_active,
        LiveState::Compacting => t.accent_strong,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::styled(" status ", Style::default().fg(t.muted)),
            Span::styled(
                format!("· {}", fmt_elapsed(app.elapsed_since_live())),
                Style::default().fg(t.dim),
            ),
            Span::raw(" "),
        ]))
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 2 || inner.width < 20 {
        return;
    }

    // Row 1: spinner + state label + sub-info · throughput label
    // Row 2: throughput sparkline · tok/s · cost sparkline · $ turn/session
    let [row1, row2] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
        .flex(ratatui::layout::Flex::Start)
        .areas(inner);

    // ── row 1 — state line ──────────────────────────────────────────────
    let spinner = if app.live.wants_spinner() {
        const S: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let c = S[(app.ticks as usize) % S.len()];
        Span::styled(
            format!("{c} "),
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "· ",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )
    };

    let state_main = match app.live {
        LiveState::Retrying {
            attempt,
            max_attempts,
            delay_ms,
        } => format!("retry {attempt}/{max_attempts} in {}ms", delay_ms),
        LiveState::Llm => format!("llm · {}", app.session.model_label),
        other => other.label().to_string(),
    };

    let tools_chip = if app.tool_running > 0 {
        format!("  {} running · {} done", app.tool_running, app.tool_done)
    } else if app.tool_done > 0 {
        format!("  tools: {} done", app.tool_done)
    } else {
        String::new()
    };

    let turn_chip = if app.turn_count > 0 {
        format!("  turn {}", app.turn_count)
    } else {
        String::new()
    };

    let spans = vec![
        spinner,
        Span::styled(state_main, Style::default().fg(border_color)),
        Span::styled(turn_chip, Style::default().fg(t.dim)),
        Span::styled(tools_chip, Style::default().fg(t.muted)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), row1);

    // ── row 2 — sparklines + numeric chips ──────────────────────────────
    // Split the row into three regions: throughput label+spark, cost
    // label+spark, session cost.
    let cols = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(35),
        Constraint::Percentage(15),
    ])
    .split(row2);
    let [tp_col, cost_col, total_col] = [cols[0], cols[1], cols[2]];

    // Throughput subsection: "throughput  ▂▅▇  82 t/s"
    let throughput_data: Vec<u64> = app.throughput.iter().copied().map(u64::from).collect();
    let tp_label = Span::styled("throughput  ", Style::default().fg(t.dim));
    let tp_rate = format!("  {} t/s", app.recent_avg_throughput());
    let tp_rate_span = Span::styled(
        tp_rate,
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    );

    // Allocate inside tp_col: label (12) | spark (flex) | rate (10)
    let tp_inner = Layout::horizontal([
        Constraint::Length(13),
        Constraint::Min(5),
        Constraint::Length(10),
    ])
    .split(tp_col);
    f.render_widget(Paragraph::new(Line::from(vec![tp_label])), tp_inner[0]);
    f.render_widget(
        Sparkline::default()
            .data(&throughput_data)
            .style(Style::default().fg(t.accent)),
        tp_inner[1],
    );
    f.render_widget(Paragraph::new(Line::from(vec![tp_rate_span])), tp_inner[2]);

    // Cost subsection.
    let cost_data: Vec<u64> = app
        .cost_series
        .iter()
        .map(|c| (c * 100_000.0) as u64)
        .collect();
    let last_turn_cost = app.cost_series.back().copied().unwrap_or(0.0);
    let cost_inner = Layout::horizontal([
        Constraint::Length(7),
        Constraint::Min(5),
        Constraint::Length(12),
    ])
    .split(cost_col);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "cost  ",
            Style::default().fg(t.dim),
        )])),
        cost_inner[0],
    );
    f.render_widget(
        Sparkline::default()
            .data(&cost_data)
            .style(Style::default().fg(t.success)),
        cost_inner[1],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!("  ${last_turn_cost:.4}"),
            Style::default().fg(t.success).add_modifier(Modifier::BOLD),
        )])),
        cost_inner[2],
    );

    // Session total cost.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" session ", Style::default().fg(t.dim)),
            Span::styled(
                format!("${:.4}", app.cost_session),
                Style::default()
                    .fg(t.accent_strong)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        total_col,
    );
}

fn fmt_elapsed(d: Duration) -> String {
    let s = d.as_secs();
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let spinner = if app.is_streaming {
        Span::styled(
            format!("{} ", SPINNER[(app.ticks as usize) % SPINNER.len()]),
            Style::default().fg(t.accent_strong),
        )
    } else {
        Span::raw("  ")
    };
    let status = if app.is_streaming {
        Span::styled("streaming", Style::default().fg(t.accent_strong))
    } else if app.spawn_error.is_some() {
        Span::styled("pi offline", Style::default().fg(t.error))
    } else {
        Span::styled("idle", Style::default().fg(t.muted))
    };
    let thinking_badge = if app.show_thinking {
        Span::styled("  think ●", Style::default().fg(t.role_thinking))
    } else {
        Span::styled("  think ○", Style::default().fg(t.dim))
    };
    let queue = format!(
        "  steer:{} · follow-up:{}",
        app.session.queue_steering.len(),
        app.session.queue_follow_up.len(),
    );
    let session_name = app
        .session
        .session_name
        .as_deref()
        .map(|n| format!("  [{n}]"))
        .unwrap_or_default();

    // Heartbeat dot: pulse shape via the triangle easer on the 10-tick loop
    // of `ticks`. Color comes from heartbeat_color (maps recent-event time).
    let heartbeat_pct = crate::anim::ease::triangle(((app.ticks % 10) as f64) / 10.0);
    let heartbeat_sym = if heartbeat_pct > 0.5 { "●" } else { "○" };

    let mut line_spans = vec![
        Span::styled(
            " rata-pi ",
            Style::default()
                .fg(t.text)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{heartbeat_sym} "),
            Style::default().fg(app.heartbeat_color()),
        ),
        spinner,
        Span::styled(&app.session.model_label, Style::default().fg(t.accent)),
        Span::styled(session_name, Style::default().fg(t.dim)),
        Span::raw("  ·  "),
        status,
        thinking_badge,
        Span::styled(queue, Style::default().fg(t.dim)),
    ];
    // Git chip: only when inside a repo. Shows `  ⎇ branch●` where the dot
    // is warning-colored when dirty, success-colored when clean.
    if let Some(g) = &app.git_status
        && g.is_repo
    {
        let branch = g.branch.as_deref().unwrap_or("DETACHED");
        let dirty = g.dirty();
        line_spans.extend([
            Span::raw("  "),
            Span::styled(
                "⎇ ",
                Style::default().fg(t.muted).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                branch.to_string(),
                Style::default()
                    .fg(if dirty { t.warning } else { t.success })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if dirty { "●" } else { "○" },
                Style::default().fg(if dirty { t.warning } else { t.success }),
            ),
        ]);
        if g.ahead > 0 || g.behind > 0 {
            line_spans.push(Span::styled(
                format!(" ↑{} ↓{}", g.ahead, g.behind),
                Style::default().fg(t.dim),
            ));
        }
    }
    let line = Line::from(line_spans);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &App, mm: &mut MouseMap) {
    let t = &app.theme;
    let border_color = if app.focus_idx.is_some() {
        t.border_active
    } else {
        t.border_idle
    };
    let title = if app.focus_idx.is_some() {
        " transcript · focus ".to_string()
    } else {
        " transcript ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(err) = &app.spawn_error {
        let msg = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                "⚠ pi is not available",
                Style::default().fg(t.error).add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(err.clone()),
            Line::default(),
            Line::from(Span::styled(
                "press q or Ctrl+C to quit",
                Style::default().fg(t.dim),
            )),
        ]))
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        return;
    }

    // Leave one column on the right for the scrollbar indicator.
    let content_w = inner.width.saturating_sub(1);
    let sbar_col = Rect::new(inner.x + content_w, inner.y, 1, inner.height);
    let content = Rect::new(inner.x, inner.y, content_w, inner.height);

    // V2.11.2 · read from the visuals cache populated in `prepare_frame_
    // caches`. No markdown / syntect / line_count runs inside this function.
    let cache = &app.visuals_cache.entries;
    if cache.is_empty() {
        let hint = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                "welcome to rata-pi",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(Span::styled(
                "type a prompt and press Enter · Ctrl+F focus mode · Alt+T theme · ?  help",
                Style::default().fg(t.dim),
            )),
        ]))
        .wrap(Wrap { trim: false });
        f.render_widget(hint, inner);
        return;
    }

    // Heights are cached at `content_w` by the prepare step. If the width
    // this draw sees disagrees (a resize landed between prepare and draw),
    // fall back to per-entry height() — correctness wins over the cache
    // for this one frame.
    let total_h: u16 = cache
        .iter()
        .map(|c| {
            if c.width == content_w {
                c.height
            } else {
                c.visual.height(content_w)
            }
        })
        .fold(0u16, |a, h| a.saturating_add(h));
    let viewport = content.height;
    let max_offset = total_h.saturating_sub(viewport);

    // Resolve the scroll offset: auto-follow bottom OR follow focus OR user.
    let offset: u16 = if let Some(idx) = app.focus_idx {
        // Center the focused card in the viewport.
        let mut prefix: u16 = 0;
        for c in &cache[..idx.min(cache.len())] {
            let h = if c.width == content_w {
                c.height
            } else {
                c.visual.height(content_w)
            };
            prefix = prefix.saturating_add(h);
        }
        let focused_h = cache
            .get(idx)
            .map(|c| {
                if c.width == content_w {
                    c.height
                } else {
                    c.visual.height(content_w)
                }
            })
            .unwrap_or(0);
        let mid = prefix.saturating_add(focused_h / 2);
        mid.saturating_sub(viewport / 2).min(max_offset)
    } else {
        match app.scroll {
            None => max_offset,
            Some(v) => v.min(max_offset),
        }
    };

    // Reset hit-test map for this frame.
    mm.clear();
    mm.body_rect = content;

    // Render visuals that intersect [offset .. offset+viewport).
    let mut y_cursor: u16 = 0;
    let mut draw_y: u16 = content.y;
    let end_y = content.y + viewport;
    for (i, c) in cache.iter().enumerate() {
        let h = if c.width == content_w {
            c.height
        } else {
            c.visual.height(content_w)
        };
        let top = y_cursor;
        let bottom = y_cursor.saturating_add(h);
        y_cursor = bottom;
        if bottom <= offset {
            continue;
        }
        if top >= offset + viewport {
            break;
        }
        let v = &c.visual;
        // How much of this visual's top is already scrolled off?
        let skip = offset.saturating_sub(top);
        let remaining_vertical = end_y.saturating_sub(draw_y);
        let draw_h = h.saturating_sub(skip).min(remaining_vertical);
        if draw_h == 0 {
            break;
        }
        let target = Rect::new(content.x, draw_y, content.width, draw_h);
        if skip == 0 && draw_h >= h {
            v.render(f, target, i, app);
        } else {
            v.render_clipped(f, target, i, app, skip, h);
            if skip > 0 {
                render_cutoff_hint(f, Rect::new(target.x, target.y, target.width, 1), t);
            }
        }
        // Record the on-screen rect of this visual for mouse hit-testing.
        mm.visible.push((target.y, target.y + target.height, i));
        draw_y = draw_y.saturating_add(draw_h);
        if draw_y >= end_y {
            break;
        }
    }

    // Sticky live-tail chip when the user scrolled up.
    let live_tail = app.focus_idx.is_none() && app.scroll.is_none();
    if !live_tail && max_offset > 0 && offset < max_offset {
        let chip = " ⬇ live tail (End) ";
        let w = chip.chars().count() as u16;
        if content.width > w + 2 {
            let cx = content.x + (content.width - w) / 2;
            let cy = content.y + content.height.saturating_sub(1);
            let rect = Rect::new(cx, cy, w, 1);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    chip,
                    Style::default()
                        .bg(t.accent)
                        .fg(Color::Rgb(0, 0, 0))
                        .add_modifier(Modifier::BOLD),
                ))),
                rect,
            );
            mm.live_tail_chip = Some(rect);
        }
    }

    // Scrollbar column.
    draw_scrollbar(f, sbar_col, offset, viewport, total_h, t);
}

fn render_cutoff_hint(f: &mut ratatui::Frame, area: Rect, t: &Theme) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "   ⋯  (card continues above)",
            Style::default().fg(t.dim),
        ))),
        area,
    );
}

pub(super) fn draw_scrollbar(
    f: &mut ratatui::Frame,
    area: Rect,
    offset: u16,
    viewport: u16,
    total: u16,
    t: &Theme,
) {
    if area.height == 0 || total <= viewport {
        return;
    }
    // Thumb size proportional to visible/total, min 1 row.
    let thumb_size = ((viewport as u32 * area.height as u32) / total.max(1) as u32) as u16;
    let thumb_size = thumb_size.max(1).min(area.height);
    let track_range = area.height.saturating_sub(thumb_size);
    let thumb_pos = if total > viewport {
        (offset as u32 * track_range as u32 / (total - viewport).max(1) as u32) as u16
    } else {
        0
    };
    for row in 0..area.height {
        let y = area.y + row;
        let ch = if row >= thumb_pos && row < thumb_pos + thumb_size {
            "│"
        } else {
            "·"
        };
        let style = if row >= thumb_pos && row < thumb_pos + thumb_size {
            Style::default().fg(t.accent)
        } else {
            Style::default().fg(t.dim)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(ch, style))),
            Rect::new(area.x, y, 1, 1),
        );
    }
}

fn draw_editor(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let text = app.composer.text();
    let is_bash = text.trim_start().starts_with('!');

    let mode_chip = if app.vim_enabled {
        match app.composer.mode {
            crate::composer::Mode::Insert => " · INS",
            crate::composer::Mode::Normal => " · NORM",
        }
    } else {
        ""
    };

    let (color, title) = if is_bash {
        (t.role_bash, " bash (! prefix · Enter run) ".to_string())
    } else if app.is_streaming {
        let label = match app.composer_mode {
            ComposerMode::Steer | ComposerMode::Prompt => "steer",
            ComposerMode::FollowUp => "follow-up",
        };
        (
            t.border_active,
            format!(" {label}{mode_chip} (Ctrl+Space cycle · Esc abort) "),
        )
    } else {
        (
            t.border_idle,
            format!(" prompt{mode_chip} (Enter send · Shift+Enter/Ctrl+J newline · Esc clear) "),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Multi-line render via Composer::render — the cursor is baked into
    // the line using Modifier::REVERSED on the current char/column.
    let lines = app.composer.render(t, true);
    f.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let [bar, hints] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

    // Context gauge row.
    let (pct, label) = if let Some(ctx) = app
        .session
        .stats
        .as_ref()
        .and_then(|s| s.context_usage.as_ref())
    {
        let pct = ctx.percent.unwrap_or(0.0).clamp(0.0, 100.0);
        let tokens = ctx.tokens.unwrap_or(0);
        (
            pct / 100.0,
            format!(
                "{:>3.0}% · {}k / {}k tok · ${:.2}",
                pct,
                tokens / 1000,
                ctx.context_window / 1000,
                app.session.stats.as_ref().map(|s| s.cost).unwrap_or(0.0),
            ),
        )
    } else {
        (0.0, "context: —".to_string())
    };
    let gauge_color = theme::gauge_color(&app.theme, pct);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color).bg(Color::Reset))
        .ratio(pct)
        .label(label);
    f.render_widget(gauge, bar);

    // Hints + flash message row.
    let mut spans = if app.is_streaming {
        vec![
            kb("Esc", t),
            Span::raw(" abort · "),
            kb("Ctrl+Space", t),
            Span::raw(" cycle · "),
            kb("F7", t),
            Span::raw(" stats · "),
            kb("Ctrl+T", t),
            Span::raw(" thinking · "),
            kb("PgUp/PgDn", t),
            Span::raw(" scroll · "),
            kb("Ctrl+C", t),
            Span::raw(" quit"),
        ]
    } else if app.focus_idx.is_some() {
        vec![
            kb("j/k", t),
            Span::raw(" nav · "),
            kb("Enter", t),
            Span::raw(" expand · "),
            kb("g/G", t),
            Span::raw(" top/bot · "),
            kb("Esc", t),
            Span::raw(" exit · "),
            kb("Ctrl+C", t),
            Span::raw(" quit"),
        ]
    } else {
        vec![
            kb("Enter", t),
            Span::raw(" send · "),
            kb("/", t),
            Span::raw(" cmds · "),
            kb("Ctrl+F", t),
            Span::raw(" focus · "),
            kb("F5", t),
            Span::raw(" model · "),
            kb("F6", t),
            Span::raw(" think · "),
            kb("F7", t),
            Span::raw(" stats · "),
            kb("Alt+T", t),
            Span::raw(" theme · "),
            kb("?", t),
            Span::raw(" help · "),
            kb("Ctrl+C", t),
            Span::raw(" quit"),
        ]
    };
    if let Some((msg, _)) = &app.flash {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            format!("• {msg}"),
            Style::default()
                .fg(app.theme.warning)
                .add_modifier(Modifier::BOLD),
        ));
    }
    for (k, v) in &app.ext_ui.statuses {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            format!("[{k}] "),
            Style::default().fg(app.theme.role_thinking),
        ));
        spans.push(Span::styled(v.clone(), Style::default().fg(app.theme.text)));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().fg(app.theme.muted)),
        hints,
    );
}

/// Render a keybinding chip. Uses the active theme's `accent_strong` so the
/// hints match whichever theme is in effect (was hardcoded to `Color::Cyan`
/// before V2.11.3 — broke light themes).
pub(super) fn kb(s: &str, t: &Theme) -> Span<'static> {
    Span::styled(
        s.to_string(),
        Style::default()
            .fg(t.accent_strong)
            .add_modifier(Modifier::BOLD),
    )
}

// ───────────────────────────────────────────────────────── modals ──
