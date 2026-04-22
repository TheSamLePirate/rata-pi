//! Modal-key dispatch — extracted from `app/mod.rs` in V4.d.3.
//!
//! One big per-Modal-variant match that routes keypresses to the
//! right state mutation. Every arm ends in one of three outcomes:
//! mutate state in-place (scroll, toggle, navigate), dispatch an
//! async action (slash command, RPC fire, settings dispatch), or
//! close the modal.
//!
//! The dispatcher lives here in one file so the per-variant logic
//! is in one place — the per-modal modules (`modals::interview`,
//! `modals::settings`) own only their own state + internal helpers.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::rpc::client::RpcClient;
use crate::rpc::commands::{ExtensionUiResponse, RpcCommand};
use crate::ui::modal::Modal;
use crate::ui::transcript::Entry;

use super::helpers::truncate_preview;
use super::modals::interview::{dispatch_interview_response, interview_key};
use super::modals::settings::{dispatch_settings_action, settings_modal_key};
use super::{
    App, FILES_CAP, bootstrap, filtered_commands, filtered_count_commands, filtered_count_forks,
    filtered_count_history, filtered_count_models, filtered_forks, filtered_history,
    filtered_models, handle_list_keys, insert_file_ref, next_char_boundary_str,
    prev_char_boundary_str, transcript_hits, try_local_slash, try_pi_slash,
};

pub(super) async fn handle_modal_key(
    code: KeyCode,
    mods: KeyModifiers,
    app: &mut App,
    client: Option<&RpcClient>,
) {
    let Some(modal) = app.modal.as_mut() else {
        return;
    };
    match modal {
        Modal::Stats(_)
        | Modal::Help
        | Modal::GitStatus(_)
        | Modal::PlanView
        | Modal::Doctor(_)
        | Modal::Mcp(_) => match code {
            // V3.e.6 · read-only viewers accept Esc, Enter, AND q (less-
            // style) so every dismissal keystroke the user might try
            // works. Interactive modals (Settings, Commands, Interview,
            // GitLog, Files, …) keep Esc-only to avoid eating `q`.
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => app.modal = None,
            _ => {}
        },
        Modal::Diff(d) => match code {
            KeyCode::Esc | KeyCode::Char('q') => app.modal = None,
            KeyCode::Char('j') | KeyCode::Down => {
                d.scroll = d.scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                d.scroll = d.scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                d.scroll = d.scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                d.scroll = d.scroll.saturating_sub(10);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                d.scroll = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                d.scroll = u16::MAX;
            }
            _ => {}
        },
        // V2.13.a · read-only shortcut reference. Scroll-only.
        Modal::Shortcuts { scroll } => match code {
            KeyCode::Esc | KeyCode::Char('q') => app.modal = None,
            KeyCode::Char('j') | KeyCode::Down => {
                *scroll = scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *scroll = scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                *scroll = scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                *scroll = scroll.saturating_sub(10);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                *scroll = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                *scroll = u16::MAX;
            }
            _ => {}
        },
        // V2.13.b · settings modal. Navigate with ↑↓/j/k (skipping
        // Headers). Enter / Space toggles booleans or advances cycles.
        // ← / → steps cycle rows. PgUp/PgDn scroll the viewport.
        Modal::Settings(_) => {
            let (maybe_action, should_close) = settings_modal_key(code, mods, app);
            if should_close {
                app.modal = None;
            } else if let Some(action) = maybe_action {
                dispatch_settings_action(app, client, action).await;
            }
        }
        // V4.c · template picker.
        //   ↑↓ / j / k  nav
        //   Enter       load body into composer, close modal
        //   d / Del     delete focused template + refresh the list
        //   Esc         close
        Modal::Templates(list) => match code {
            KeyCode::Esc => app.modal = None,
            KeyCode::Char('j') | KeyCode::Down => {
                if !list.items.is_empty() {
                    list.selected = (list.selected + 1).min(list.items.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                list.selected = list.selected.saturating_sub(1);
            }
            KeyCode::Home | KeyCode::Char('g') => list.selected = 0,
            KeyCode::End | KeyCode::Char('G') => {
                list.selected = list.items.len().saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(t) = list.items.get(list.selected) {
                    let body = t.body.clone();
                    let name = t.name.clone();
                    app.composer.set_text(&body);
                    app.modal = None;
                    app.flash_success(format!("loaded template {name:?}"));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(t) = list.items.get(list.selected) {
                    let name = t.name.clone();
                    crate::templates::delete(&name);
                    list.items.remove(list.selected);
                    if list.selected >= list.items.len() && !list.items.is_empty() {
                        list.selected = list.items.len() - 1;
                    }
                    if list.items.is_empty() {
                        app.modal = None;
                        app.flash_info(format!("deleted {name:?} — no templates left"));
                    } else {
                        app.flash_success(format!("deleted template {name:?}"));
                    }
                }
            }
            _ => {}
        },
        // V4.b · transcript-search overlay.
        //   printable / Backspace / Delete / ←→/Home/End — edit query
        //   n / Down / Tab     — next hit
        //   N / Up / BackTab   — prev hit
        //   Enter              — focus the current hit, close modal
        //   Esc                — close modal without focusing
        Modal::Search(state) => {
            let mut query_changed = false;
            match code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Enter => {
                    if let Some(&idx) = state.hits.get(state.hit_idx) {
                        app.focus_idx = Some(idx);
                        app.scroll = None;
                    }
                    app.modal = None;
                }
                KeyCode::Char('n') | KeyCode::Down | KeyCode::Tab
                    if !state.hits.is_empty()
                        && !mods.contains(KeyModifiers::CONTROL)
                        && !mods.contains(KeyModifiers::ALT) =>
                {
                    state.hit_idx = (state.hit_idx + 1) % state.hits.len();
                }
                KeyCode::Char('N') | KeyCode::Up | KeyCode::BackTab
                    if !state.hits.is_empty()
                        && !mods.contains(KeyModifiers::CONTROL)
                        && !mods.contains(KeyModifiers::ALT) =>
                {
                    state.hit_idx = if state.hit_idx == 0 {
                        state.hits.len() - 1
                    } else {
                        state.hit_idx - 1
                    };
                }
                KeyCode::Backspace => {
                    if state.query_cursor > 0 {
                        let prev = prev_char_boundary_str(&state.query, state.query_cursor);
                        state.query.drain(prev..state.query_cursor);
                        state.query_cursor = prev;
                        query_changed = true;
                    }
                }
                KeyCode::Delete => {
                    if state.query_cursor < state.query.len() {
                        let next = next_char_boundary_str(&state.query, state.query_cursor);
                        state.query.drain(state.query_cursor..next);
                        query_changed = true;
                    }
                }
                KeyCode::Left => {
                    if state.query_cursor > 0 {
                        state.query_cursor =
                            prev_char_boundary_str(&state.query, state.query_cursor);
                    }
                }
                KeyCode::Right => {
                    if state.query_cursor < state.query.len() {
                        state.query_cursor =
                            next_char_boundary_str(&state.query, state.query_cursor);
                    }
                }
                KeyCode::Home => state.query_cursor = 0,
                KeyCode::End => state.query_cursor = state.query.len(),
                KeyCode::Char(ch)
                    if !mods.contains(KeyModifiers::CONTROL)
                        && !mods.contains(KeyModifiers::ALT) =>
                {
                    // Lowercase `n` / `N` handled above as navigation
                    // when hits exist. When the query is still growing
                    // (no hits yet, or user is typing `naive`, etc.)
                    // we get here because the navigation arm is gated
                    // on `!state.hits.is_empty()`.
                    let mut buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut buf);
                    state.query.insert_str(state.query_cursor, s);
                    state.query_cursor += s.len();
                    query_changed = true;
                }
                _ => {}
            }
            if query_changed && let Some(Modal::Search(state)) = app.modal.as_mut() {
                state.hits = transcript_hits(&app.transcript, &state.query);
                // Clamp hit_idx into the new hit range.
                if state.hit_idx >= state.hits.len() {
                    state.hit_idx = state.hits.len().saturating_sub(1);
                }
            }
        }
        Modal::GitLog(state) => {
            let n = state.commits.len();
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 {
                        state.selected = (state.selected + 1).min(n - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    if n > 0 {
                        state.selected = (state.selected + 10).min(n - 1);
                    }
                }
                KeyCode::PageUp => {
                    state.selected = state.selected.saturating_sub(10);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    state.selected = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    if n > 0 {
                        state.selected = n - 1;
                    }
                }
                _ => {}
            }
        }
        Modal::GitBranch(state) => {
            let n = state.branches.len();
            if handle_list_keys(&mut state.query, &mut state.selected, code, n) {
                return;
            }
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    let pick = state
                        .branches
                        .iter()
                        .filter(|b| {
                            state.query.is_empty()
                                || b.name
                                    .to_ascii_lowercase()
                                    .contains(&state.query.to_ascii_lowercase())
                        })
                        .nth(state.selected)
                        .map(|b| b.name.clone());
                    app.modal = None;
                    if let Some(name) = pick {
                        match crate::git::switch(&name).await {
                            Ok(_) => app.flash_success(format!("switched to {name}")),
                            Err(e) => app.flash_error(format!("switch failed: {e}")),
                        }
                    }
                }
                _ => {}
            }
        }
        Modal::Commands(list) => {
            let n = filtered_count_commands(&list.items, &list.query);
            if handle_list_keys(&mut list.query, &mut list.selected, code, n) {
                return;
            }
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    let Some(cmd) = filtered_commands(&list.items, &list.query)
                        .nth(list.selected)
                        .cloned()
                    else {
                        return;
                    };
                    // `/themes` reuses the Commands modal. Entries look like
                    // "theme <name>" — apply inline.
                    if cmd.is_theme() {
                        let theme_name = cmd.name.strip_prefix("theme ").unwrap_or("");
                        if app.set_theme_by_name(theme_name) {
                            app.flash(format!("theme → {}", app.theme.name));
                        }
                        app.modal = None;
                        return;
                    }
                    if cmd.is_builtin() {
                        let needs_arg = !cmd.args.is_empty();
                        if needs_arg {
                            app.composer.clear();
                            app.composer.insert_char('/');
                            app.composer.insert_str(&cmd.name);
                            app.composer.insert_char(' ');
                        } else {
                            app.modal = None;
                            let name = cmd.name.clone();
                            if !try_local_slash(app, &name, "").await {
                                if let Some(c) = client {
                                    try_pi_slash(app, c, &name, "").await;
                                } else {
                                    app.flash(format!("/{name} needs pi (offline)"));
                                }
                            }
                            return;
                        }
                        app.modal = None;
                        return;
                    }
                    // Pi command — prefill the composer with /name so the
                    // user can type arguments and submit.
                    app.composer.clear();
                    app.composer.insert_char('/');
                    app.composer.insert_str(&cmd.name);
                    app.composer.insert_char(' ');
                    app.modal = None;
                }
                _ => {}
            }
        }
        Modal::Models(list) => {
            let n = filtered_count_models(&list.items, &list.query);
            if handle_list_keys(&mut list.query, &mut list.selected, code, n) {
                return;
            }
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    let Some(m) = filtered_models(&list.items, &list.query)
                        .nth(list.selected)
                        .cloned()
                    else {
                        return;
                    };
                    if let Some(c) = client {
                        match c
                            .call(RpcCommand::SetModel {
                                provider: m.provider.clone(),
                                model_id: m.id.clone(),
                            })
                            .await
                        {
                            Ok(_) => {
                                app.session.model_label = format!("{}/{}", m.provider, m.id);
                                app.flash(format!("model → {}/{}", m.provider, m.id));
                            }
                            Err(e) => app.flash_error(format!("set_model failed: {e}")),
                        }
                    }
                    app.modal = None;
                }
                _ => {}
            }
        }
        Modal::History(list) => {
            let n = filtered_count_history(&list.items, &list.query);
            if handle_list_keys(&mut list.query, &mut list.selected, code, n) {
                return;
            }
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    if let Some(entry) = filtered_history(&list.items, &list.query)
                        .nth(list.selected)
                        .cloned()
                    {
                        app.composer.set_text(&entry.text);
                    }
                    app.modal = None;
                }
                _ => {}
            }
        }
        Modal::Forks(list) => {
            let n = filtered_count_forks(&list.items, &list.query);
            if handle_list_keys(&mut list.query, &mut list.selected, code, n) {
                return;
            }
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    let pick = filtered_forks(&list.items, &list.query)
                        .nth(list.selected)
                        .cloned();
                    app.modal = None;
                    if let (Some(c), Some(f)) = (client, pick) {
                        match c
                            .call(RpcCommand::Fork {
                                entry_id: f.entry_id.clone(),
                            })
                            .await
                        {
                            Ok(_) => {
                                app.flash_success(format!(
                                    "forked at {}",
                                    truncate_preview(&f.text, 40)
                                ));
                                bootstrap(c, app).await;
                            }
                            Err(e) => app.flash_error(format!("fork failed: {e}")),
                        }
                    }
                }
                _ => {}
            }
        }
        Modal::Files(ff) => {
            // V2.11.1 · use the cached filter + track selection changes
            // so the fuzzy matcher never runs inside a key-press handler
            // and the preview cache drops when the selection moves.
            ff.refresh_filter(FILES_CAP);
            let n = ff.filtered.len();
            let prev_sel = ff.selected;
            let prev_query = ff.query.clone();
            if handle_list_keys(&mut ff.query, &mut ff.selected, code, n) {
                if ff.query != prev_query {
                    ff.refresh_filter(FILES_CAP);
                }
                if ff.selected != prev_sel {
                    ff.invalidate_preview();
                }
                return;
            }
            match code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Enter => {
                    let Some(path) = ff.current_path().map(str::to_string) else {
                        return;
                    };
                    let mode = ff.mode;
                    app.modal = None;
                    insert_file_ref(app, &path, mode);
                }
                _ => {}
            }
        }
        Modal::Thinking(radio) => match code {
            KeyCode::Esc => app.modal = None,
            KeyCode::Up => {
                if radio.selected > 0 {
                    radio.selected -= 1;
                }
            }
            KeyCode::Down => {
                if radio.selected + 1 < radio.options.len() {
                    radio.selected += 1;
                }
            }
            KeyCode::Enter => {
                let (level, _label) = radio.options[radio.selected];
                if let Some(c) = client {
                    match c.call(RpcCommand::SetThinkingLevel { level }).await {
                        Ok(_) => {
                            app.session.thinking = Some(level);
                            app.flash(format!("thinking → {level:?}"));
                        }
                        Err(e) => app.flash_error(format!("set_thinking_level failed: {e}")),
                    }
                }
                app.modal = None;
            }
            _ => {}
        },
        Modal::ExtSelect {
            request_id,
            options,
            selected,
            ..
        } => match code {
            KeyCode::Up => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down => {
                if !options.is_empty() && *selected + 1 < options.len() {
                    *selected += 1;
                }
            }
            KeyCode::Enter => {
                let value = options.get(*selected).cloned();
                let req_id = request_id.clone();
                app.modal = None;
                if let (Some(c), Some(v)) = (client, value) {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::value(req_id, v))
                        .await;
                }
            }
            KeyCode::Esc => {
                let req_id = request_id.clone();
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::cancelled(req_id))
                        .await;
                }
            }
            _ => {}
        },
        Modal::ExtConfirm {
            request_id,
            selected,
            ..
        } => match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                *selected = 1 - *selected;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let req_id = request_id.clone();
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::confirmed(req_id, true))
                        .await;
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let req_id = request_id.clone();
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::confirmed(req_id, false))
                        .await;
                }
            }
            KeyCode::Enter => {
                let confirmed = *selected == 1;
                let req_id = request_id.clone();
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::confirmed(req_id, confirmed))
                        .await;
                }
            }
            KeyCode::Esc => {
                let req_id = request_id.clone();
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::cancelled(req_id))
                        .await;
                }
            }
            _ => {}
        },
        // V2.12.g · interview modal — see `interview_key` below.
        Modal::Interview(state) => {
            // Esc always cancels. We intercept it here so it never
            // reaches the per-field key handler (a text field would
            // otherwise swallow it silently).
            if code == KeyCode::Esc {
                app.modal = None;
                app.transcript
                    .push(Entry::Info("interview cancelled by user".to_string()));
                app.flash("interview cancelled");
                return;
            }
            let submit = interview_key(state.as_mut(), code, mods);
            if submit {
                // Valid form + submit trigger: serialise the response,
                // close the modal, dispatch to pi.
                let response = state.as_response();
                let summary = state.human_summary();
                app.modal = None;
                if let Some(c) = client {
                    dispatch_interview_response(app, c, response, summary).await;
                } else {
                    app.flash("interview needs pi (offline) — response discarded");
                }
            }
        }
        Modal::ExtInput {
            request_id, value, ..
        }
        | Modal::ExtEditor {
            request_id, value, ..
        } => match code {
            KeyCode::Char(ch) => {
                value.push(ch);
            }
            KeyCode::Backspace => {
                value.pop();
            }
            KeyCode::Enter => {
                let req_id = request_id.clone();
                let v = std::mem::take(value);
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::value(req_id, v))
                        .await;
                }
            }
            KeyCode::Esc => {
                let req_id = request_id.clone();
                app.modal = None;
                if let Some(c) = client {
                    let _ = c
                        .send_ext_ui_response(ExtensionUiResponse::cancelled(req_id))
                        .await;
                }
            }
            _ => {}
        },
        // V3.f · Plan Review modal. Three sub-modes:
        //   * Review (action chips + step list, read-only)
        //   * Edit (step list focus; add / delete / enter-to-edit)
        //   * Edit + text-entry (a single step under cursor)
        Modal::PlanReview(state) => {
            use crate::ui::modal::{EditingStep, PlanReviewMode};

            // ── Text-entry sub-mode: raw keys edit the buffer. ────────
            if let Some(edit) = state.editing.as_mut() {
                match code {
                    KeyCode::Enter => {
                        // Commit buffer → items[index]; return to Edit.
                        if let Some(slot) = state.items.get_mut(edit.index) {
                            *slot = std::mem::take(&mut edit.buffer);
                        }
                        state.editing = None;
                    }
                    KeyCode::Esc => {
                        // Drop edits, return to Edit list.
                        state.editing = None;
                    }
                    KeyCode::Backspace => {
                        if edit.cursor > 0 {
                            let prev = prev_char_boundary_str(&edit.buffer, edit.cursor);
                            edit.buffer.drain(prev..edit.cursor);
                            edit.cursor = prev;
                        }
                    }
                    KeyCode::Delete => {
                        if edit.cursor < edit.buffer.len() {
                            let next = next_char_boundary_str(&edit.buffer, edit.cursor);
                            edit.buffer.drain(edit.cursor..next);
                        }
                    }
                    KeyCode::Left => {
                        if edit.cursor > 0 {
                            edit.cursor = prev_char_boundary_str(&edit.buffer, edit.cursor);
                        }
                    }
                    KeyCode::Right => {
                        if edit.cursor < edit.buffer.len() {
                            edit.cursor = next_char_boundary_str(&edit.buffer, edit.cursor);
                        }
                    }
                    KeyCode::Home => edit.cursor = 0,
                    KeyCode::End => edit.cursor = edit.buffer.len(),
                    KeyCode::Char(ch)
                        if !mods.contains(KeyModifiers::CONTROL)
                            && !mods.contains(KeyModifiers::ALT) =>
                    {
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        edit.buffer.insert_str(edit.cursor, s);
                        edit.cursor += s.len();
                    }
                    _ => {}
                }
                return;
            }

            match state.mode {
                // ── Review mode: action chips drive Accept / Edit / Deny. ──
                PlanReviewMode::Review => match code {
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        app.modal = None;
                        app.accept_proposed_plan();
                    }
                    KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Esc => {
                        app.modal = None;
                        app.deny_proposed_plan();
                    }
                    KeyCode::Char('e') | KeyCode::Char('E') => {
                        state.mode = PlanReviewMode::Edit;
                        state.selected = 0;
                    }
                    KeyCode::Char('t') | KeyCode::Char('T') => {
                        state.auto_run_pref = !state.auto_run_pref;
                        if let Some(p) = app.proposed_plan.as_mut() {
                            p.suggested_auto_run = state.auto_run_pref;
                        }
                    }
                    KeyCode::Enter => {
                        // Focused chip: 0 Accept · 1 Edit · 2 Deny.
                        let sel = state.selected;
                        match sel {
                            0 => {
                                app.modal = None;
                                app.accept_proposed_plan();
                            }
                            1 => {
                                state.mode = PlanReviewMode::Edit;
                                state.selected = 0;
                            }
                            _ => {
                                app.modal = None;
                                app.deny_proposed_plan();
                            }
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        state.selected = (state.selected + 1).min(2);
                    }
                    KeyCode::PageDown => state.scroll = state.scroll.saturating_add(10),
                    KeyCode::PageUp => state.scroll = state.scroll.saturating_sub(10),
                    KeyCode::Home | KeyCode::Char('g') => state.scroll = 0,
                    KeyCode::End | KeyCode::Char('G') => state.scroll = u16::MAX,
                    KeyCode::Down | KeyCode::Char('j') => {
                        state.scroll = state.scroll.saturating_add(1)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.scroll = state.scroll.saturating_sub(1)
                    }
                    _ => {}
                },
                // ── Edit mode: navigate steps, add / delete / enter-to-edit. ──
                PlanReviewMode::Edit => match code {
                    KeyCode::Esc => {
                        // Back to Review without committing anything (the
                        // list itself is already committed; edits to the
                        // list mutate items directly).
                        state.mode = PlanReviewMode::Review;
                        state.selected = 1; // keep focus on Edit chip
                    }
                    KeyCode::Char('s') | KeyCode::Char('S')
                        if mods.contains(KeyModifiers::CONTROL) =>
                    {
                        // Ctrl+S commits the edited list as the accepted plan.
                        if let Some(p) = app.proposed_plan.as_mut() {
                            p.items = state.items.clone();
                            p.suggested_auto_run = state.auto_run_pref;
                        }
                        app.modal = None;
                        app.accept_proposed_plan();
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !state.items.is_empty() {
                            state.selected = (state.selected + 1).min(state.items.len() - 1);
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    KeyCode::Enter | KeyCode::Char('i') => {
                        if let Some(cur) = state.items.get(state.selected) {
                            let buffer = cur.clone();
                            let cursor = buffer.len();
                            state.editing = Some(EditingStep {
                                index: state.selected,
                                buffer,
                                cursor,
                            });
                        }
                    }
                    KeyCode::Char('a') => {
                        // Add blank step below the focused row.
                        let at = state.selected.saturating_add(1).min(state.items.len());
                        state.items.insert(at, String::new());
                        state.selected = at;
                        if let Some(p) = app.proposed_plan.as_mut() {
                            p.items = state.items.clone();
                        }
                        // Jump straight into text-entry for the new row.
                        state.editing = Some(EditingStep {
                            index: at,
                            buffer: String::new(),
                            cursor: 0,
                        });
                    }
                    KeyCode::Delete | KeyCode::Char('x') => {
                        if state.selected < state.items.len() {
                            state.items.remove(state.selected);
                            if state.selected >= state.items.len() && !state.items.is_empty() {
                                state.selected = state.items.len() - 1;
                            }
                            if let Some(p) = app.proposed_plan.as_mut() {
                                p.items = state.items.clone();
                            }
                        }
                    }
                    KeyCode::Char('t') | KeyCode::Char('T') => {
                        state.auto_run_pref = !state.auto_run_pref;
                        if let Some(p) = app.proposed_plan.as_mut() {
                            p.suggested_auto_run = state.auto_run_pref;
                        }
                    }
                    _ => {}
                },
            }
        }
    }
}
