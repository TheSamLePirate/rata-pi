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

    // StatusWidget takes a 3-row strip between the editor and footer
    // (V2.13.d · was 4; top-only rule instead of a full-box border saves
    // one row). Hidden on short terminals.
    let status_h: u16 = if area.height >= 20 { 3 } else { 0 };

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
        c.push(Constraint::Length(1)); // footer (V2.13.c · was 2 rows)
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
        plan_card(&app.plan, &app.theme).render(
            f,
            r,
            &app.theme,
            crate::ui::cards::FocusMode::None,
        );
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
        draw_modal(f, area, modal, app, mm);
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
    // V2.13.d · top-only rule with the title inline, saving one row
    // compared to a full bordered box. Border color still encodes live
    // state so the status strip is visually distinct when pi is busy.
    let block = Block::default()
        .borders(Borders::TOP)
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
    // V3.i.1 · sparkline tint tracks live state. During error the whole
    // status widget's border already goes red; matching the sparkline
    // makes the "something's broken" cue unmistakable.
    let tp_color = match app.live {
        LiveState::Error => t.error,
        LiveState::Retrying { .. } => t.warning,
        _ => t.accent,
    };
    f.render_widget(
        Sparkline::default()
            .data(&throughput_data)
            .style(Style::default().fg(tp_color)),
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
    let cost_color = match app.live {
        LiveState::Error => t.error,
        LiveState::Retrying { .. } => t.warning,
        _ => t.success,
    };
    f.render_widget(
        Sparkline::default()
            .data(&cost_data)
            .style(Style::default().fg(cost_color)),
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

pub(super) fn fmt_elapsed(d: Duration) -> String {
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

/// V2.13.c · slimmed header. One tight row with:
///   rata-pi · heartbeat · spinner · model · state · (queue if nonzero)
///   · (git chip if in a repo) · right: turn counter
/// Thinking / session-name / session-file / notify-backend / etc. moved
/// to /settings.
fn draw_header(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Heartbeat: pulse shape via the triangle easer; color = heartbeat color
    // which already encodes liveness (green recent / yellow quiet / red stale).
    // When offline, force red so the user sees it without reading the state.
    let heartbeat_pct = crate::anim::ease::triangle(((app.ticks % 10) as f64) / 10.0);
    let heartbeat_sym = if heartbeat_pct > 0.5 { "●" } else { "○" };
    let heartbeat_color = if app.spawn_error.is_some() {
        t.error
    } else {
        app.heartbeat_color()
    };

    let spinner = if app.is_streaming {
        Span::styled(
            format!(" {}", SPINNER[(app.ticks as usize) % SPINNER.len()]),
            Style::default().fg(t.accent_strong),
        )
    } else {
        Span::raw("  ")
    };

    let (state_label, state_color) = match (app.is_streaming, app.spawn_error.is_some()) {
        (true, _) => (app.live.label(), t.accent_strong),
        (_, true) => ("offline", t.error),
        _ => (app.live.label(), t.muted),
    };

    let mut left: Vec<Span<'static>> = vec![
        Span::styled(
            " rata-pi ",
            Style::default()
                .fg(t.text)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw("  "),
        Span::styled(
            heartbeat_sym.to_string(),
            Style::default().fg(heartbeat_color),
        ),
        spinner,
        Span::raw(" "),
        Span::styled(
            short_model_label(&app.session.model_label),
            Style::default().fg(t.accent),
        ),
        Span::raw("  "),
        Span::styled(format!("· {state_label}"), Style::default().fg(state_color)),
    ];

    // Queue chips only when something's pending.
    let steer_n = app.session.queue_steering.len();
    let fu_n = app.session.queue_follow_up.len();
    if steer_n > 0 {
        left.push(Span::styled(
            format!("  ↻{}", steer_n),
            Style::default().fg(t.warning).add_modifier(Modifier::BOLD),
        ));
    }
    if fu_n > 0 {
        left.push(Span::styled(
            format!("  ▸{}", fu_n),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ));
    }

    // Git chip only when inside a repo.
    if let Some(g) = &app.git_status
        && g.is_repo
    {
        let branch = g.branch.as_deref().unwrap_or("DETACHED");
        let dirty = g.dirty();
        left.extend([
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
            left.push(Span::styled(
                format!(" ↑{} ↓{}", g.ahead, g.behind),
                Style::default().fg(t.dim),
            ));
        }
    }

    // Turn counter only once at least one turn has started.
    if app.turn_count > 0 {
        left.push(Span::styled(
            format!("  t{}", app.turn_count),
            Style::default().fg(t.dim),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(left)), area);
}

/// Shorten a model label for the header. Providers usually surface
/// `<provider>/<id>` — keep the id tail (`claude-sonnet-4-6`) and
/// drop the provider when the full label is long.
fn short_model_label(full: &str) -> String {
    if full.len() <= 24 {
        return full.to_string();
    }
    match full.rsplit_once('/') {
        Some((_, id)) => id.to_string(),
        None => full.to_string(),
    }
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

/// V2.13.c · single-row footer. Context gauge + a right-aligned pointer
/// to `/settings` and `/shortcuts`. Flash messages and ext-UI statuses
/// become transient toasts rather than hijacking a permanent row
/// (existing toast widget handles those).
fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Context gauge (or placeholder).
    let (pct, gauge_label) = if let Some(ctx) = app
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
        (0.0, "context —".to_string())
    };

    // Right-aligned chip. When a flash message is active, it replaces
    // the hint chip; otherwise the stable `? help · /settings · /shortcuts`
    // pointer stays put. Flashes expire naturally after 15 ticks.
    // V3.e.3 · color per FlashKind.
    // V3.i.1 · glyph prefix per kind (info ℹ · success ✓ · warn ⚠ ·
    // error ✗). Under 60 cols the glyph collapses to a plain bullet to
    // avoid burning too much hint width on narrow terminals.
    let hint_spans: Vec<Span<'static>> = if let Some((msg, _, kind)) = &app.flash {
        let color = match kind {
            super::FlashKind::Success => t.success,
            super::FlashKind::Warn => t.warning,
            super::FlashKind::Error => t.error,
            super::FlashKind::Info => t.accent,
        };
        let glyph = if area.width < 60 {
            "• "
        } else {
            match kind {
                super::FlashKind::Success => "✓ ",
                super::FlashKind::Warn => "⚠ ",
                super::FlashKind::Error => "✗ ",
                super::FlashKind::Info => "ℹ ",
            }
        };
        vec![
            Span::raw(" "),
            Span::styled(
                glyph.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                msg.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]
    } else {
        vec![
            Span::styled(" ", Style::default()),
            kb("?", t),
            Span::styled(" help ", Style::default().fg(t.dim)),
            Span::styled("·", Style::default().fg(t.dim)),
            Span::styled(" ", Style::default()),
            kb("/settings", t),
            Span::styled(" ", Style::default()),
            kb("/shortcuts", t),
            Span::raw(" "),
        ]
    };
    let hint_line = Line::from(hint_spans);
    // Estimate hint width from the plain text (Ratatui doesn't expose a
    // per-span width helper we can rely on on every Line — count chars).
    let hint_text: String = hint_line
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    let hint_w = hint_text.chars().count() as u16;

    let gauge_w = area.width.saturating_sub(hint_w.min(area.width));
    let gauge_area = Rect::new(area.x, area.y, gauge_w, area.height.min(1));
    let hint_area = Rect::new(
        area.x + gauge_w,
        area.y,
        hint_w.min(area.width.saturating_sub(gauge_w)),
        area.height.min(1),
    );

    let gauge_color = theme::gauge_color(&app.theme, pct);
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color).bg(Color::Reset))
        .ratio(pct)
        .label(gauge_label);
    f.render_widget(gauge, gauge_area);
    f.render_widget(Paragraph::new(hint_line), hint_area);
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
