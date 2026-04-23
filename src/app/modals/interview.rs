//! Interview modal — key handling, rendering, and dispatch.
//!
//! Extracted from `app/mod.rs` in V3.d.1. No behavior change: this module
//! is a straight lift-and-shift of the `interview_*`, `text_field_*`,
//! `try_submit_interview`, and `dispatch_interview_response` functions
//! that previously lived in the monolithic `app/mod.rs`.
//!
//! Public surface (via `pub(super)`) matches exactly what `mod.rs` still
//! calls — `interview_key`, `dispatch_interview_response`, and
//! `interview_body` / `interview_body_and_focus_rows` for the draw path.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::interview::{FieldValue, InterviewState};
use crate::rpc::client::RpcClient;
use crate::rpc::commands::RpcCommand;
use crate::theme::Theme;
use crate::ui::transcript::Entry;

use super::super::{App, ComposerMode, LiveState};

/// Handle a keystroke for the open interview modal. Returns `true` when
/// the user submitted the form (caller then finalizes the response and
/// closes the modal). The focus chrome, field editing, and submit-slot
/// shortcut live here.
pub(crate) fn interview_key(state: &mut InterviewState, code: KeyCode, mods: KeyModifiers) -> bool {
    // Global: Tab / Shift+Tab navigate between interactive fields AND
    // the virtual Submit slot.
    match (code, mods) {
        (KeyCode::Tab, _) => {
            state.focus_next();
            return false;
        }
        (KeyCode::BackTab, _) => {
            state.focus_prev();
            return false;
        }
        (KeyCode::Down, m) if !m.contains(KeyModifiers::ALT) => {
            state.focus_next();
            return false;
        }
        (KeyCode::Up, m) if !m.contains(KeyModifiers::ALT) => {
            state.focus_prev();
            return false;
        }
        // Ctrl+Enter / Ctrl+S are kept as power-user shortcuts. They
        // work reliably on kitty-protocol terminals; on others the user
        // should Tab to the Submit button and press Enter.
        (KeyCode::Enter, m) if m.contains(KeyModifiers::CONTROL) => {
            return try_submit_interview(state);
        }
        (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => {
            return try_submit_interview(state);
        }
        // Explicit scroll: PgUp/PgDn move the viewport by ~10 rows.
        // Ctrl+Home/Ctrl+End jump to top/bottom. Marks `user_scrolled`
        // so focus-follow pauses until the user Tabs again.
        (KeyCode::PageDown, _) => {
            state.scroll = state.scroll.saturating_add(10);
            state.user_scrolled = true;
            return false;
        }
        (KeyCode::PageUp, _) => {
            state.scroll = state.scroll.saturating_sub(10);
            state.user_scrolled = true;
            return false;
        }
        (KeyCode::Home, m) if m.contains(KeyModifiers::CONTROL) => {
            state.scroll = 0;
            state.user_scrolled = true;
            return false;
        }
        (KeyCode::End, m) if m.contains(KeyModifiers::CONTROL) => {
            state.scroll = u16::MAX;
            state.user_scrolled = true;
            return false;
        }
        _ => {}
    }

    // Focus on the submit slot (past the last field): the only key we
    // handle is plain Enter / Space → submit. Everything else is a no-op.
    if state.focus_on_submit() {
        return matches!(code, KeyCode::Enter | KeyCode::Char(' ')) && try_submit_interview(state);
    }

    // Field-specific dispatch on the focused field. Some field handlers
    // want to advance focus after editing (e.g. plain Enter on a
    // single-line text/number/select acts like Tab), but we can't touch
    // `state` while holding the mutable borrow on `state.fields[…]`. So
    // we set a flag inside the match and act on it after it drops.
    let mut advance_focus_after = false;
    match &mut state.fields[state.focus] {
        FieldValue::Section { .. } | FieldValue::Info { .. } => {
            // Non-interactive shouldn't be focused; guard anyway.
        }
        FieldValue::Toggle { value, .. } => match code {
            KeyCode::Char(' ') | KeyCode::Char('x') => *value = !*value,
            KeyCode::Left if *value => *value = false,
            KeyCode::Right if !*value => *value = true,
            KeyCode::Enter => *value = !*value,
            _ => {}
        },
        FieldValue::Select {
            options, selected, ..
        } => match code {
            KeyCode::Left => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Right => {
                if *selected + 1 < options.len() {
                    *selected += 1;
                }
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                // Numeric shortcut: 1..9 picks the Nth option.
                let idx = ch.to_digit(10).unwrap_or(0) as usize;
                if idx >= 1 && idx <= options.len() {
                    *selected = idx - 1;
                }
            }
            // Plain Enter on a select advances focus — consistent with
            // text fields and helps users who haven't discovered Tab.
            KeyCode::Enter => {
                advance_focus_after = true;
            }
            _ => {}
        },
        FieldValue::Multiselect {
            options,
            checked,
            cursor,
            ..
        } => match code {
            KeyCode::Left => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
            }
            KeyCode::Right => {
                if *cursor + 1 < options.len() {
                    *cursor += 1;
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Char('x') => {
                if let Some(c) = checked.get_mut(*cursor) {
                    *c = !*c;
                }
            }
            _ => {}
        },
        FieldValue::Text {
            value,
            cursor,
            multiline,
            ..
        } => {
            // Plain Enter on a single-line text field advances to the
            // next focus (HTML-form convention). In multiline fields
            // Enter is still needed to insert a newline, so skip that.
            if matches!(code, KeyCode::Enter)
                && !mods.contains(KeyModifiers::SHIFT)
                && !mods.contains(KeyModifiers::CONTROL)
                && !*multiline
            {
                advance_focus_after = true;
            } else {
                text_field_key(value, cursor, *multiline, code, mods);
            }
        }
        FieldValue::Number {
            value,
            cursor,
            min,
            max,
            ..
        } => {
            // Plain Enter advances focus.
            if matches!(code, KeyCode::Enter) && mods == KeyModifiers::NONE {
                advance_focus_after = true;
                // Fall through to the match's end so we don't also
                // edit the buffer.
                // (We don't short-circuit via return so the advance
                // happens in the post-match block below.)
            } else {
                // Gate non-numeric chars so the user can only type digits /
                // sign / decimal point.
                if let KeyCode::Char(ch) = code {
                    let allowed = ch.is_ascii_digit()
                        || ch == '.'
                        || ch == '-'
                        || ch == 'e'
                        || ch == 'E'
                        || ch == '+';
                    if !allowed {
                        return false;
                    }
                }
                text_field_key(value, cursor, false, code, mods);
                // Clamp on every edit so the user can't exceed bounds.
                if let (Ok(n), _) = (value.parse::<f64>(), ()) {
                    let mut fixed = n;
                    if let Some(lo) = min {
                        fixed = fixed.max(*lo);
                    }
                    if let Some(hi) = max {
                        fixed = fixed.min(*hi);
                    }
                    if (fixed - n).abs() > f64::EPSILON {
                        *value = if fixed.fract() == 0.0 {
                            format!("{:.0}", fixed)
                        } else {
                            format!("{fixed}")
                        };
                        *cursor = value.len();
                    }
                }
            }
        }
    }
    if advance_focus_after {
        state.focus_next();
    }
    false
}

/// Apply a keystroke to a plain-String text buffer. Handles insert,
/// backspace, delete, cursor motion (including word motion via Alt+←/→
/// and line kill via Ctrl+U/K/W), and multiline newlines.
fn text_field_key(
    value: &mut String,
    cursor: &mut usize,
    multiline: bool,
    code: KeyCode,
    mods: KeyModifiers,
) {
    match (code, mods) {
        (KeyCode::Char(ch), m)
            if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            value.insert_str(*cursor, s);
            *cursor += s.len();
        }
        (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) && multiline => {
            value.insert(*cursor, '\n');
            *cursor += 1;
        }
        (KeyCode::Backspace, _) => {
            if *cursor > 0 {
                let prev = prev_char_boundary(value, *cursor);
                value.drain(prev..*cursor);
                *cursor = prev;
            }
        }
        (KeyCode::Delete, _) => {
            if *cursor < value.len() {
                let next = next_char_boundary(value, *cursor);
                value.drain(*cursor..next);
            }
        }
        (KeyCode::Left, m) if !m.contains(KeyModifiers::ALT) => {
            if *cursor > 0 {
                *cursor = prev_char_boundary(value, *cursor);
            }
        }
        (KeyCode::Right, m) if !m.contains(KeyModifiers::ALT) && *cursor < value.len() => {
            *cursor = next_char_boundary(value, *cursor);
        }
        (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
            *cursor = word_left_index(value, *cursor);
        }
        (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
            *cursor = word_right_index(value, *cursor);
        }
        (KeyCode::Home, _) => {
            *cursor = 0;
        }
        (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
            *cursor = 0;
        }
        (KeyCode::End, _) => {
            *cursor = value.len();
        }
        (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
            *cursor = value.len();
        }
        (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
            value.drain(..*cursor);
            *cursor = 0;
        }
        (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
            value.truncate(*cursor);
        }
        (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
            let start = word_left_index(value, *cursor);
            value.drain(start..*cursor);
            *cursor = start;
        }
        _ => {}
    }
}

fn prev_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i.saturating_sub(1);
    while j > 0 && !s.is_char_boundary(j) {
        j -= 1;
    }
    j
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i.saturating_add(1).min(s.len());
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

fn word_left_index(s: &str, mut i: usize) -> usize {
    // Skip any trailing non-word chars, then the word.
    while i > 0 {
        let p = prev_char_boundary(s, i);
        let c = s[p..i].chars().next();
        if matches!(c, Some(c) if c.is_alphanumeric() || c == '_') {
            break;
        }
        i = p;
    }
    while i > 0 {
        let p = prev_char_boundary(s, i);
        let c = s[p..i].chars().next();
        if !matches!(c, Some(c) if c.is_alphanumeric() || c == '_') {
            break;
        }
        i = p;
    }
    i
}

fn word_right_index(s: &str, mut i: usize) -> usize {
    // Skip current word, then leading non-word chars.
    while i < s.len() {
        let c = s[i..].chars().next();
        if !matches!(c, Some(c) if c.is_alphanumeric() || c == '_') {
            break;
        }
        i = next_char_boundary(s, i);
    }
    while i < s.len() {
        let c = s[i..].chars().next();
        if matches!(c, Some(c) if c.is_alphanumeric() || c == '_') {
            break;
        }
        i = next_char_boundary(s, i);
    }
    i
}

fn try_submit_interview(state: &mut InterviewState) -> bool {
    match state.first_missing_required() {
        None => {
            state.validation_error = None;
            true
        }
        Some(label) => {
            state.validation_error = Some(format!("required: {label}"));
            // Jump focus to the first missing interactive field for the user.
            for i in 0..state.fields.len() {
                if state.fields[i].missing_required() {
                    state.focus = i;
                    break;
                }
            }
            false
        }
    }
}

/// Send the interview's serialized response to pi and record a
/// transcript entry so the user has a trail.
pub(crate) async fn dispatch_interview_response(
    app: &mut App,
    client: &RpcClient,
    response: String,
    summary: String,
) {
    // Show the submission in the transcript first.
    let line = if summary.is_empty() {
        "interview submitted".to_string()
    } else {
        format!("interview submitted · {summary}")
    };
    app.transcript
        .push(Entry::User(format!("(interview) {line}")));
    app.history.record(&response);

    let rpc = if app.is_streaming {
        match app.composer_mode {
            ComposerMode::FollowUp => RpcCommand::FollowUp {
                message: response,
                images: vec![],
            },
            _ => RpcCommand::Steer {
                message: response,
                images: vec![],
            },
        }
    } else {
        app.set_live(LiveState::Sending);
        RpcCommand::Prompt {
            message: response,
            images: vec![],
            streaming_behavior: None,
        }
    };
    if let Err(e) = client.fire(rpc).await {
        app.transcript
            .push(Entry::Error(format!("interview dispatch failed: {e}")));
    }
}

pub(crate) fn interview_body(state: &InterviewState, t: &Theme) -> Vec<Line<'static>> {
    interview_body_and_focus_rows(state, t).0
}

/// Build the modal body AND a parallel vector mapping each focus slot
/// (0..=fields.len()) to its starting source-line index. The draw path
/// uses that mapping to compute scroll offsets that keep the focused
/// field visible. Position `fields.len()` is the submit button.
pub(crate) fn interview_body_and_focus_rows(
    state: &InterviewState,
    t: &Theme,
) -> (Vec<Line<'static>>, Vec<u16>) {
    let mut out: Vec<Line<'static>> = Vec::new();
    // focus_rows[i] = source-line index where field i starts.
    // focus_rows[fields.len()] = source-line of the submit button.
    let mut focus_rows: Vec<u16> = vec![0; state.fields.len() + 1];

    // Top-matter: description + validation error.
    if let Some(desc) = &state.description {
        out.push(Line::from(Span::styled(
            format!("  {desc}"),
            Style::default().fg(t.muted),
        )));
        out.push(Line::default());
    }
    if let Some(err) = &state.validation_error {
        out.push(Line::from(vec![
            Span::styled(
                "  ✗ ",
                Style::default().fg(t.error).add_modifier(Modifier::BOLD),
            ),
            Span::styled(err.clone(), Style::default().fg(t.error)),
        ]));
        out.push(Line::default());
    }

    // Fields.
    for (i, f) in state.fields.iter().enumerate() {
        focus_rows[i] = out.len() as u16;
        let focused = i == state.focus && f.is_interactive();
        let marker = if focused { "▶" } else { " " };
        let marker_style = if focused {
            Style::default()
                .fg(t.accent_strong)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.dim)
        };
        let label_color = if focused { t.accent_strong } else { t.text };

        match f {
            FieldValue::Section { title, description } => {
                // Blank line above for visual spacing, then a heading.
                if i > 0 {
                    out.push(Line::default());
                }
                let rule = "─".repeat(60);
                out.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("{title}  "),
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(rule, Style::default().fg(t.dim)),
                ]));
                if let Some(d) = description {
                    out.push(Line::from(Span::styled(
                        format!("  {d}"),
                        Style::default().fg(t.muted),
                    )));
                }
                out.push(Line::default());
            }
            FieldValue::Info { text } => {
                out.push(Line::from(vec![
                    Span::styled("  ℹ  ", Style::default().fg(t.accent_strong)),
                    Span::styled(text.clone(), Style::default().fg(t.muted)),
                ]));
                out.push(Line::default());
            }
            FieldValue::Text {
                label,
                description,
                placeholder,
                value,
                cursor,
                required,
                multiline,
                ..
            } => {
                out.push(interview_label_line(
                    marker,
                    marker_style,
                    label,
                    *required,
                    label_color,
                    t,
                ));
                if let Some(d) = description {
                    out.push(interview_desc_line(d, t));
                }
                out.extend(interview_text_display(
                    value,
                    *cursor,
                    focused,
                    placeholder.as_deref(),
                    *multiline,
                    t,
                ));
                out.push(Line::default());
            }
            FieldValue::Toggle {
                label,
                description,
                value,
                ..
            } => {
                out.push(interview_label_line(
                    marker,
                    marker_style,
                    label,
                    false,
                    label_color,
                    t,
                ));
                if let Some(d) = description {
                    out.push(interview_desc_line(d, t));
                }
                let box_color = if focused { t.accent_strong } else { t.text };
                let glyph = if *value { "[x]" } else { "[ ]" };
                let yn = if *value { "yes" } else { "no" };
                out.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        format!("{glyph} "),
                        Style::default().fg(box_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(yn.to_string(), Style::default().fg(t.muted)),
                ]));
                out.push(Line::default());
            }
            FieldValue::Select {
                label,
                description,
                options,
                selected,
                required,
                ..
            } => {
                out.push(interview_label_line(
                    marker,
                    marker_style,
                    label,
                    *required,
                    label_color,
                    t,
                ));
                if let Some(d) = description {
                    out.push(interview_desc_line(d, t));
                }
                let mut row: Vec<Span<'static>> = vec![Span::raw("     ")];
                for (j, opt) in options.iter().enumerate() {
                    let is_sel = j == *selected;
                    let glyph = if is_sel { "(●)" } else { "( )" };
                    let glyph_color = if is_sel {
                        if focused { t.accent_strong } else { t.accent }
                    } else {
                        t.dim
                    };
                    let text_color = if is_sel { t.text } else { t.muted };
                    row.push(Span::styled(
                        format!("{glyph} "),
                        Style::default()
                            .fg(glyph_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                    row.push(Span::styled(opt.clone(), Style::default().fg(text_color)));
                    if j + 1 < options.len() {
                        row.push(Span::raw("   "));
                    }
                }
                out.push(Line::from(row));
                out.push(Line::default());
            }
            FieldValue::Multiselect {
                label,
                description,
                options,
                checked,
                cursor,
                ..
            } => {
                out.push(interview_label_line(
                    marker,
                    marker_style,
                    label,
                    false,
                    label_color,
                    t,
                ));
                if let Some(d) = description {
                    out.push(interview_desc_line(d, t));
                }
                let mut row: Vec<Span<'static>> = vec![Span::raw("     ")];
                for (j, opt) in options.iter().enumerate() {
                    let c = checked.get(j).copied().unwrap_or(false);
                    let is_cur = focused && j == *cursor;
                    let glyph = if c { "[x]" } else { "[ ]" };
                    let glyph_color = if c {
                        t.accent_strong
                    } else if is_cur {
                        t.accent
                    } else {
                        t.dim
                    };
                    let text_style = if is_cur {
                        Style::default()
                            .fg(t.text)
                            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
                    } else if c {
                        Style::default().fg(t.text)
                    } else {
                        Style::default().fg(t.muted)
                    };
                    row.push(Span::styled(
                        format!("{glyph} "),
                        Style::default()
                            .fg(glyph_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                    row.push(Span::styled(opt.clone(), text_style));
                    if j + 1 < options.len() {
                        row.push(Span::raw("   "));
                    }
                }
                out.push(Line::from(row));
                out.push(Line::default());
            }
            FieldValue::Number {
                label,
                description,
                min,
                max,
                value,
                cursor,
                required,
                ..
            } => {
                out.push(interview_label_line(
                    marker,
                    marker_style,
                    label,
                    *required,
                    label_color,
                    t,
                ));
                let range = match (min, max) {
                    (Some(lo), Some(hi)) => Some(format!(
                        "{}–{}",
                        format_number_for_hint(*lo),
                        format_number_for_hint(*hi)
                    )),
                    (Some(lo), None) => Some(format!("≥ {}", format_number_for_hint(*lo))),
                    (None, Some(hi)) => Some(format!("≤ {}", format_number_for_hint(*hi))),
                    (None, None) => None,
                };
                if let Some(d) = description {
                    out.push(interview_desc_line(d, t));
                }
                if let Some(r) = range {
                    out.push(interview_desc_line(&format!("range: {r}"), t));
                }
                out.extend(interview_text_display(
                    value, *cursor, focused, None, false, t,
                ));
                out.push(Line::default());
            }
        }
    }

    // Submit button row — focusable via Tab. When focused, show the
    // ▶ cursor marker + invert the button label so the user sees it
    // is the current target.
    // Record its source-line position for the focus-tracker.
    focus_rows[state.fields.len()] = out.len() as u16 + 1; // +1 for the blank line pushed just below

    let can_submit = state.first_missing_required().is_none();
    let submit_focused = state.focus_on_submit();
    let submit_bg = if can_submit { t.success } else { t.dim };
    let marker = if submit_focused { "▶" } else { " " };
    let marker_style = if submit_focused {
        Style::default()
            .fg(t.accent_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.dim)
    };
    let button_style = if submit_focused {
        // Focused: full reverse-video button with bright border feel.
        Style::default()
            .fg(Color::Rgb(0, 0, 0))
            .bg(submit_bg)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
            .fg(Color::Rgb(0, 0, 0))
            .bg(submit_bg)
            .add_modifier(Modifier::BOLD)
    };
    let button_label = if submit_focused {
        format!(" ▶ {} ◀ ", state.submit_label)
    } else {
        format!("   {}   ", state.submit_label)
    };
    out.push(Line::default());
    out.push(Line::from(vec![
        Span::styled(format!("  {marker} "), marker_style),
        Span::styled(button_label, button_style),
        Span::raw("   "),
        Span::styled(
            if !can_submit {
                "fill required fields, then Enter"
            } else if submit_focused {
                "press Enter to send · Esc to cancel"
            } else {
                "Tab here · Ctrl+S / Ctrl+Enter anywhere · Esc to cancel"
            }
            .to_string(),
            Style::default().fg(t.muted),
        ),
    ]));

    (out, focus_rows)
}

fn interview_label_line(
    marker: &'static str,
    marker_style: Style,
    label: &str,
    required: bool,
    label_color: Color,
    t: &Theme,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("  {marker} "), marker_style),
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(label_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if required {
        spans.push(Span::styled(
            " *",
            Style::default().fg(t.error).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn interview_desc_line(desc: &str, t: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("     {desc}"),
        Style::default().fg(t.dim),
    ))
}

fn interview_text_display(
    value: &str,
    cursor: usize,
    focused: bool,
    placeholder: Option<&str>,
    multiline: bool,
    t: &Theme,
) -> Vec<Line<'static>> {
    // Split into display lines; when cursor is in the buffer and focus
    // is active, overlay a reversed cell at the cursor column on the
    // target row.
    let mut out: Vec<Line<'static>> = Vec::new();
    if value.is_empty() {
        let shown = placeholder.map(|p| format!("({p})"));
        let ph_style = Style::default().fg(t.dim);
        let base = shown.unwrap_or_else(|| " ".to_string());
        if focused {
            out.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    " ",
                    Style::default().fg(t.text).add_modifier(Modifier::REVERSED),
                ),
                Span::styled(base, ph_style),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(base, ph_style),
            ]));
        }
        return out;
    }

    // Compute the row / column of the cursor.
    if multiline {
        // Find which line the cursor is on.
        let mut prefix_len = 0usize;
        let mut row_idx = 0usize;
        let mut col_in_row = cursor;
        for (i, line) in value.split_inclusive('\n').enumerate() {
            let line_end = prefix_len + line.len();
            // The '\n' itself lives at line_end-1 if present; cursor may
            // legitimately sit at line_end (end of the inclusive range).
            if cursor <= line_end {
                row_idx = i;
                col_in_row = cursor - prefix_len;
                break;
            }
            prefix_len = line_end;
            row_idx = i + 1;
            col_in_row = cursor - prefix_len;
        }
        for (i, line) in value.split('\n').enumerate() {
            let line_str = line.to_string();
            if focused && i == row_idx {
                out.push(line_with_cursor(
                    "     ",
                    &line_str,
                    col_in_row.min(line_str.len()),
                    t,
                ));
            } else {
                out.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(line_str, Style::default().fg(t.text)),
                ]));
            }
        }
    } else if focused {
        out.push(line_with_cursor("     ", value, cursor.min(value.len()), t));
    } else {
        out.push(Line::from(vec![
            Span::raw("     "),
            Span::styled(value.to_string(), Style::default().fg(t.text)),
        ]));
    }
    out
}

fn line_with_cursor(prefix: &'static str, line: &str, col: usize, t: &Theme) -> Line<'static> {
    let before = line[..col.min(line.len())].to_string();
    let rest = &line[col.min(line.len())..];
    let (under, after) = match rest.chars().next() {
        Some(c) => {
            let cb = c.len_utf8();
            (rest[..cb].to_string(), rest[cb..].to_string())
        }
        None => (" ".to_string(), String::new()),
    };
    let cursor_style = Style::default()
        .fg(t.text)
        .add_modifier(Modifier::REVERSED | Modifier::BOLD);
    Line::from(vec![
        Span::raw(prefix),
        Span::styled(before, Style::default().fg(t.text)),
        Span::styled(under, cursor_style),
        Span::styled(after, Style::default().fg(t.text)),
    ])
}

fn format_number_for_hint(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{:.0}", n)
    } else {
        format!("{n}")
    }
}
