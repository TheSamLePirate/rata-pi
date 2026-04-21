//! `/settings` modal — key handling, action dispatch, row model, rendering.
//!
//! Extracted from `app/mod.rs` in V3.d.2. No behavior change. The public
//! surface is `pub(crate)` so existing call sites in `mod.rs` (draw_modal,
//! handle_modal_key, and reducer tests) keep working unchanged.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::rpc::client::RpcClient;
use crate::rpc::commands::RpcCommand;
use crate::rpc::types::{FollowUpMode, SteeringMode};
use crate::theme::Theme;
use crate::ui::modal::Modal;

use super::super::draw::fmt_elapsed;
use super::super::{App, on_off};

/// One key applied to the Settings modal. Returns `(Some(action), _)`
/// when the user pressed Enter/Space/←/→ on an interactive row — the
/// caller dispatches it. `(_, true)` means close the modal (Esc).
pub(crate) fn settings_modal_key(
    code: KeyCode,
    _mods: KeyModifiers,
    app: &mut App,
) -> (Option<SettingsAction>, bool) {
    // Build the row list against an immutable view of app first, then
    // pull the mutable Settings state borrow. The row contents aren't
    // needed to live past the mutation step — we just read indices.
    let rows = build_settings_rows(app);
    let Some(Modal::Settings(state)) = app.modal.as_mut() else {
        return (None, false);
    };

    // Clamp initial selection onto a selectable row (the first one is
    // usually a Header so the default selected=0 needs correcting).
    if !rows
        .get(state.selected)
        .map(|r| r.is_selectable())
        .unwrap_or(false)
        && let Some(first) = rows.iter().position(|r| r.is_selectable())
    {
        state.selected = first;
    }

    let n = rows.len();
    let step_by = |state: &mut crate::ui::modal::SettingsState, delta: i32| {
        if n == 0 {
            return;
        }
        let mut i = state.selected as i32;
        let step = if delta >= 0 { 1 } else { -1 };
        for _ in 0..n {
            i += step;
            if i < 0 {
                i = n as i32 - 1;
            } else if i >= n as i32 {
                i = 0;
            }
            if rows[i as usize].is_selectable() {
                state.selected = i as usize;
                state.user_scrolled = false;
                return;
            }
        }
    };

    match code {
        KeyCode::Esc => return (None, true),
        // V3.e.2 · Tab / Shift+Tab navigate between selectable rows,
        // symmetric with every other modal. Without this binding users
        // who reflexively press Tab from the Interview modal got
        // nothing.
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => step_by(state, 1),
        KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => step_by(state, -1),
        KeyCode::Home | KeyCode::Char('g') => {
            if let Some(first) = rows.iter().position(|r| r.is_selectable()) {
                state.selected = first;
                state.user_scrolled = false;
            }
        }
        KeyCode::End | KeyCode::Char('G') => {
            if let Some(last) = rows.iter().rposition(|r| r.is_selectable()) {
                state.selected = last;
                state.user_scrolled = false;
            }
        }
        KeyCode::PageUp => {
            for _ in 0..5 {
                step_by(state, -1);
            }
            state.user_scrolled = true;
        }
        KeyCode::PageDown => {
            for _ in 0..5 {
                step_by(state, 1);
            }
            state.user_scrolled = true;
        }
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => {
            if let Some(row) = rows.get(state.selected) {
                return (row_to_action(row, CycleDir::Forward), false);
            }
        }
        KeyCode::Left => {
            if let Some(row) = rows.get(state.selected) {
                return (row_to_action(row, CycleDir::Backward), false);
            }
        }
        _ => {}
    }
    (None, false)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CycleDir {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SettingsAction {
    Toggle(ToggleAction),
    // The direction field is carried for API symmetry; currently all
    // cycle actions advance forward regardless (pi's RPC surface has
    // no `previous_model` / `previous_thinking_level` endpoints).
    Cycle(CycleAction, #[allow(dead_code)] CycleDir),
}

fn row_to_action(row: &SettingsRow, dir: CycleDir) -> Option<SettingsAction> {
    match row {
        SettingsRow::Toggle { action, .. } => Some(SettingsAction::Toggle(*action)),
        SettingsRow::Cycle { action, .. } => Some(SettingsAction::Cycle(*action, dir)),
        _ => None,
    }
}

/// Apply the action: mutate App state locally and, when needed, fire
/// the RPC that reflects it to pi.
pub(crate) async fn dispatch_settings_action(
    app: &mut App,
    client: Option<&RpcClient>,
    action: SettingsAction,
) {
    match action {
        SettingsAction::Toggle(ToggleAction::ShowThinking) => {
            app.show_thinking = !app.show_thinking;
        }
        SettingsAction::Toggle(ToggleAction::Notify) => {
            app.notify_enabled = !app.notify_enabled;
            app.flash(format!(
                "notifications {}",
                if app.notify_enabled { "on" } else { "off" }
            ));
        }
        SettingsAction::Toggle(ToggleAction::Vim) => {
            app.vim_enabled = !app.vim_enabled;
            app.composer.mode = crate::composer::Mode::Insert;
            app.flash(format!(
                "vim mode {}",
                if app.vim_enabled { "on" } else { "off" }
            ));
        }
        SettingsAction::Toggle(ToggleAction::AutoCompact) => {
            let next = !app.session.auto_compaction.unwrap_or(true);
            app.session.auto_compaction = Some(next);
            if let Some(c) = client {
                let _ = c
                    .fire(RpcCommand::SetAutoCompaction { enabled: next })
                    .await;
                app.flash_success(format!("auto-compact {}", on_off(next)));
            } else {
                // V3.a · local flag flipped but there's no pi to persist it.
                // Warn the user so they don't think the setting stuck.
                app.flash_warn(format!(
                    "auto-compact {} — offline, applies next session",
                    on_off(next)
                ));
            }
        }
        SettingsAction::Toggle(ToggleAction::AutoRetry) => {
            let next = !app.session.auto_retry.unwrap_or(true);
            app.session.auto_retry = Some(next);
            if let Some(c) = client {
                let _ = c.fire(RpcCommand::SetAutoRetry { enabled: next }).await;
                app.flash_success(format!("auto-retry {}", on_off(next)));
            } else {
                app.flash_warn(format!(
                    "auto-retry {} — offline, applies next session",
                    on_off(next)
                ));
            }
        }
        SettingsAction::Toggle(ToggleAction::PlanAutoRun) => {
            app.plan.auto_run = !app.plan.auto_run;
            app.flash(format!(
                "plan auto-run {}",
                if app.plan.auto_run { "on" } else { "off" }
            ));
        }
        SettingsAction::Toggle(ToggleAction::ShowRawMarkers) => {
            app.show_raw_markers = !app.show_raw_markers;
            app.flash(format!(
                "raw markers {}",
                if app.show_raw_markers {
                    "visible"
                } else {
                    "hidden"
                }
            ));
        }

        SettingsAction::Cycle(CycleAction::Theme, _) => {
            app.cycle_theme();
            app.flash(format!("theme → {}", app.theme.name));
        }
        SettingsAction::Cycle(CycleAction::ThinkingLevel, _) => {
            if let Some(c) = client {
                let _ = c.fire(RpcCommand::CycleThinkingLevel).await;
            }
        }
        SettingsAction::Cycle(CycleAction::Model, _) => {
            if let Some(c) = client {
                let _ = c.fire(RpcCommand::CycleModel).await;
            }
        }
        SettingsAction::Cycle(CycleAction::SteeringMode, _) => {
            let cur = app.session.steering_mode.unwrap_or(SteeringMode::All);
            let next = match cur {
                SteeringMode::All => SteeringMode::OneAtATime,
                SteeringMode::OneAtATime => SteeringMode::All,
            };
            app.session.steering_mode = Some(next);
            if let Some(c) = client {
                let _ = c.fire(RpcCommand::SetSteeringMode { mode: next }).await;
            }
        }
        SettingsAction::Cycle(CycleAction::FollowUpMode, _) => {
            let cur = app.session.follow_up_mode.unwrap_or(FollowUpMode::All);
            let next = match cur {
                FollowUpMode::All => FollowUpMode::OneAtATime,
                FollowUpMode::OneAtATime => FollowUpMode::All,
            };
            app.session.follow_up_mode = Some(next);
            if let Some(c) = client {
                let _ = c.fire(RpcCommand::SetFollowUpMode { mode: next }).await;
            }
        }
    }
}

/// A single row in the `/settings` modal. Rebuilt fresh from `&App` on
/// every draw so the live values stay accurate.
#[derive(Debug, Clone)]
pub(crate) enum SettingsRow {
    Header(&'static str),
    Info {
        label: String,
        value: String,
    },
    Toggle {
        label: String,
        value: bool,
        action: ToggleAction,
    },
    Cycle {
        label: String,
        display: String,
        action: CycleAction,
    },
}

impl SettingsRow {
    fn is_selectable(&self) -> bool {
        matches!(self, SettingsRow::Toggle { .. } | SettingsRow::Cycle { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ToggleAction {
    ShowThinking,
    Notify,
    Vim,
    AutoCompact,
    AutoRetry,
    PlanAutoRun,
    /// V3.f.3 · leave protocol markers (plan + interview) visible in
    /// the transcript for debugging.
    ShowRawMarkers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CycleAction {
    Theme,
    ThinkingLevel,
    SteeringMode,
    FollowUpMode,
    Model,
}

pub(crate) fn build_settings_rows(app: &App) -> Vec<SettingsRow> {
    use SettingsRow as R;
    let mut rows = Vec::with_capacity(64);

    rows.push(R::Header("Session"));
    rows.push(R::Info {
        label: "name".into(),
        value: app
            .session
            .session_name
            .clone()
            .unwrap_or_else(|| "(unset · /rename to set)".into()),
    });
    rows.push(R::Info {
        label: "connection".into(),
        value: if app.spawn_error.is_some() {
            format!(
                "offline · {}",
                app.spawn_error.as_deref().unwrap_or("pi not available")
            )
        } else {
            "connected".into()
        },
    });
    rows.push(R::Info {
        label: "pi binary".into(),
        value: app
            .caps
            .pi_binary
            .clone()
            .unwrap_or_else(|| "not on PATH".into()),
    });

    rows.push(R::Header("Model"));
    rows.push(R::Cycle {
        label: "active model".into(),
        display: app.session.model_label.clone(),
        action: CycleAction::Model,
    });
    rows.push(R::Cycle {
        label: "thinking level".into(),
        display: app
            .session
            .thinking
            .map(|t| format!("{t:?}").to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".into()),
        action: CycleAction::ThinkingLevel,
    });
    rows.push(R::Cycle {
        label: "steering mode".into(),
        display: mode_display(app.session.steering_mode.map(|m| format!("{m:?}"))),
        action: CycleAction::SteeringMode,
    });
    rows.push(R::Cycle {
        label: "follow-up mode".into(),
        display: mode_display(app.session.follow_up_mode.map(|m| format!("{m:?}"))),
        action: CycleAction::FollowUpMode,
    });

    rows.push(R::Header("Behavior"));
    rows.push(R::Toggle {
        label: "show thinking".into(),
        value: app.show_thinking,
        action: ToggleAction::ShowThinking,
    });
    rows.push(R::Toggle {
        label: "notifications".into(),
        value: app.notify_enabled,
        action: ToggleAction::Notify,
    });
    rows.push(R::Toggle {
        label: "auto-compaction".into(),
        value: app.session.auto_compaction.unwrap_or(true),
        action: ToggleAction::AutoCompact,
    });
    rows.push(R::Toggle {
        label: "auto-retry".into(),
        value: app.session.auto_retry.unwrap_or(true),
        action: ToggleAction::AutoRetry,
    });
    rows.push(R::Toggle {
        label: "plan auto-run".into(),
        value: app.plan.auto_run,
        action: ToggleAction::PlanAutoRun,
    });

    rows.push(R::Header("Appearance"));
    rows.push(R::Cycle {
        label: "theme".into(),
        display: app.theme.name.to_string(),
        action: CycleAction::Theme,
    });
    rows.push(R::Toggle {
        label: "vim mode".into(),
        value: app.vim_enabled,
        action: ToggleAction::Vim,
    });
    // V3.f.3 · leave plan / interview markers visible in the transcript
    // tail. Off by default; flipping it re-renders the strip pass as a
    // no-op so future agent_ends show raw brackets through.
    rows.push(R::Toggle {
        label: "show raw markers".into(),
        value: app.show_raw_markers,
        action: ToggleAction::ShowRawMarkers,
    });

    rows.push(R::Header("Live state"));
    let elapsed = fmt_elapsed(app.elapsed_since_live());
    rows.push(R::Info {
        label: "live".into(),
        value: format!("{} · {elapsed}", app.live.label()),
    });
    rows.push(R::Info {
        label: "turn".into(),
        value: format!("{}", app.turn_count),
    });
    rows.push(R::Info {
        label: "tools".into(),
        value: format!("{} running · {} done", app.tool_running, app.tool_done),
    });
    rows.push(R::Info {
        label: "queue".into(),
        value: format!(
            "steer {} · follow-up {}",
            app.session.queue_steering.len(),
            app.session.queue_follow_up.len()
        ),
    });
    if let Some(stats) = &app.session.stats {
        let ctx = stats
            .context_usage
            .as_ref()
            .map(|c| {
                let pct = c.percent.unwrap_or(0.0);
                let tokens = c.tokens.unwrap_or(0);
                format!(
                    "{:.0}% · {}k / {}k tok",
                    pct,
                    tokens / 1000,
                    c.context_window / 1000
                )
            })
            .unwrap_or_else(|| "—".into());
        rows.push(R::Info {
            label: "context".into(),
            value: ctx,
        });
        rows.push(R::Info {
            label: "session cost".into(),
            value: format!("${:.4}", stats.cost),
        });
    }

    rows.push(R::Header("Capabilities"));
    rows.push(R::Info {
        label: "terminal".into(),
        value: format!("{:?}", app.caps.term.kind),
    });
    rows.push(R::Info {
        label: "kitty keyboard".into(),
        value: if app.caps.term.kitty_keyboard {
            "advertised".into()
        } else {
            "not advertised (Alt+T / F12 fallback)".into()
        },
    });
    rows.push(R::Info {
        label: "graphics".into(),
        value: if app.caps.term.graphics {
            "supported".into()
        } else {
            "no image protocol detected".into()
        },
    });
    rows.push(R::Info {
        label: "clipboard".into(),
        value: if app.caps.clipboard_native {
            "arboard (native)".into()
        } else {
            "OSC 52 fallback".into()
        },
    });
    rows.push(R::Info {
        label: "notify-rust feature".into(),
        value: if cfg!(feature = "notify") {
            "compiled in".into()
        } else {
            "off (OSC 777 only)".into()
        },
    });

    rows.push(R::Header("Paths"));
    rows.push(R::Info {
        label: "history file".into(),
        value: app
            .caps
            .history_path
            .clone()
            .unwrap_or_else(|| "(no history path)".into()),
    });
    rows.push(R::Info {
        label: "crash dumps".into(),
        value: app.caps.state_dir.clone(),
    });

    rows.push(R::Header("Build"));
    rows.push(R::Info {
        label: "rata-pi".into(),
        value: app.caps.package_version.into(),
    });
    rows.push(R::Info {
        label: "platform".into(),
        value: app.caps.platform.clone(),
    });

    rows
}

fn mode_display(m: Option<String>) -> String {
    m.unwrap_or_else(|| "default".into())
        .to_ascii_lowercase()
        .replace('_', "-")
}

pub(crate) fn settings_row_source_line(rows: &[SettingsRow], selected: usize) -> Option<u16> {
    let mut line_idx = 1u16; // leading blank
    for (i, r) in rows.iter().enumerate() {
        if i == selected {
            return Some(line_idx);
        }
        // Header = blank spacer + heading line = 2 rows.
        if matches!(r, SettingsRow::Header(_)) {
            line_idx = line_idx.saturating_add(2);
        } else {
            line_idx = line_idx.saturating_add(1);
        }
    }
    None
}

pub(crate) fn settings_body(
    app: &App,
    state: &crate::ui::modal::SettingsState,
    t: &Theme,
) -> Vec<Line<'static>> {
    let rows = build_settings_rows(app);
    let mut out: Vec<Line<'static>> = Vec::with_capacity(rows.len() * 2 + 4);
    out.push(Line::default());

    for (i, r) in rows.iter().enumerate() {
        let focused = i == state.selected && r.is_selectable();
        let marker = if focused { "▶" } else { " " };
        let marker_style = if focused {
            Style::default()
                .fg(t.accent_strong)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.dim)
        };
        match r {
            SettingsRow::Header(title) => {
                out.push(Line::default());
                out.push(Line::from(vec![
                    Span::styled(
                        format!("  {title}  "),
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("─".repeat(70), Style::default().fg(t.dim)),
                ]));
            }
            SettingsRow::Info { label, value } => {
                out.push(Line::from(vec![
                    Span::styled(format!("  {marker} "), marker_style),
                    Span::styled(format!("{:<22}", label), Style::default().fg(t.muted)),
                    Span::styled(value.clone(), Style::default().fg(t.text)),
                ]));
            }
            SettingsRow::Toggle { label, value, .. } => {
                let glyph = if *value { "[x]" } else { "[ ]" };
                let glyph_color = if *value {
                    if focused { t.accent_strong } else { t.success }
                } else if focused {
                    t.accent
                } else {
                    t.dim
                };
                let yn = if *value { "yes" } else { "no" };
                let label_style = if focused {
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.muted)
                };
                out.push(Line::from(vec![
                    Span::styled(format!("  {marker} "), marker_style),
                    Span::styled(format!("{:<22}", label), label_style),
                    Span::styled(
                        format!("{glyph} "),
                        Style::default()
                            .fg(glyph_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(yn.to_string(), Style::default().fg(t.text)),
                ]));
            }
            SettingsRow::Cycle {
                label,
                display,
                action,
            } => {
                let arrow_color = if focused { t.accent_strong } else { t.dim };
                let label_style = if focused {
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.muted)
                };
                // V3.e.5 · only Theme has a real backward cycle — the
                // pi-backed cycles (Model, ThinkingLevel, SteeringMode,
                // FollowUpMode) don't have a `previous_*` RPC, so ← is a
                // no-op there. Hide the ◂ affordance to stop implying
                // bidirectional control.
                let bidirectional = matches!(action, CycleAction::Theme);
                let mut spans = vec![
                    Span::styled(format!("  {marker} "), marker_style),
                    Span::styled(format!("{:<22}", label), label_style),
                ];
                if bidirectional {
                    spans.push(Span::styled(
                        "◂ ",
                        Style::default()
                            .fg(arrow_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    // Placeholder spaces so forward-only rows still align
                    // visually with the theme row's ◂ affordance.
                    spans.push(Span::raw("  "));
                }
                spans.push(Span::styled(
                    display.clone(),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    " ▸",
                    Style::default()
                        .fg(arrow_color)
                        .add_modifier(Modifier::BOLD),
                ));
                out.push(Line::from(spans));
            }
        }
    }

    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  Enter/Space — toggle or cycle · ←/→ — cycle step · PgUp/PgDn — scroll · Esc — close",
        Style::default().fg(t.dim),
    )));

    out
}
