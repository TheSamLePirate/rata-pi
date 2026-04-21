//! Global (non-modal) input handling — extracted in V3.d.4.
//!
//! Covers the four top-level keyboard / mouse paths that drive the app
//! while no modal is open:
//!
//! * [`handle_key`] — the main keypress dispatcher (Ctrl shortcuts, F-keys,
//!   composer editing, vim dispatch, history walk).
//! * [`handle_focus_key`] — focus-mode key handling (Ctrl+F transcript
//!   navigation).
//! * [`on_mouse_click`] — click hit-test: focus cards, expand tool output,
//!   re-pin live tail.
//! * [`handle_vim_normal`] — vim NORMAL-mode dispatcher.
//!
//! No behavior change. All helpers these functions rely on (`open_file_finder`,
//! `do_copy`, `entry_as_plain_text`, `merged_commands`, `last_tool_id`,
//! `submit`, `on_off`, `rect_contains`) remain in `mod.rs`, exposed as
//! `pub(super)` so this file can reach them.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::rpc::client::RpcClient;
use crate::rpc::commands::RpcCommand;
use crate::rpc::types::ThinkingLevel;
use crate::ui::modal::{ListModal, Modal, RadioModal};
use crate::ui::transcript::Entry;

use super::{
    App, do_copy, entry_as_plain_text, last_tool_id, merged_commands, on_off, open_file_finder,
    rect_contains, submit,
};

pub(super) fn handle_focus_key(code: KeyCode, _mods: KeyModifiers, app: &mut App) {
    let n = app.transcript.entries().len();
    if n == 0 {
        app.focus_idx = None;
        return;
    }
    let cur = app.focus_idx.unwrap_or(0);
    match code {
        KeyCode::Esc => {
            app.focus_idx = None;
            app.scroll = None;
            app.flash("focus mode off");
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let next = (cur + 1).min(n - 1);
            app.focus_idx = Some(next);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.focus_idx = Some(cur.saturating_sub(1));
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.focus_idx = Some(0);
        }
        KeyCode::Char('G') | KeyCode::End => {
            app.focus_idx = Some(n - 1);
        }
        KeyCode::PageDown => {
            app.focus_idx = Some((cur + 5).min(n - 1));
        }
        KeyCode::PageUp => {
            app.focus_idx = Some(cur.saturating_sub(5));
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            // Expand/collapse the focused tool card, if any.
            if let Some(Entry::ToolCall(tc)) = app.transcript.entries().get(cur) {
                let id = tc.id.clone();
                app.transcript.toggle_tool_expanded(&id);
            }
        }
        KeyCode::Char('y') | KeyCode::Char('c') => {
            // Copy the focused entry to the clipboard.
            if let Some(entry) = app.transcript.entries().get(cur) {
                let text = entry_as_plain_text(entry);
                do_copy(app, &text);
            }
        }
        KeyCode::Char('q') => {
            app.focus_idx = None;
        }
        _ => {}
    }
}

/// Mouse-click dispatcher: focus on transcript cards, toggle expand on a
/// tool card's header row (2nd click toggles), re-pin live tail when the
/// user clicks the ⬇ chip.
pub(super) fn on_mouse_click(x: u16, y: u16, app: &mut App) {
    // Live-tail chip first.
    if let Some(r) = app.mouse_map.live_tail_chip
        && rect_contains(r, x, y)
    {
        app.scroll = None;
        app.focus_idx = None;
        app.flash("re-pinned live tail");
        return;
    }
    // Transcript hit-test.
    let Some(idx) = app.mouse_map.entry_at(x, y) else {
        return;
    };
    // If the clicked entry is a tool call AND we're already focused on it
    // (second click), toggle its expanded state. Otherwise just focus.
    let is_tool = matches!(app.transcript.entries().get(idx), Some(Entry::ToolCall(_)));
    if app.focus_idx == Some(idx) && is_tool {
        if let Some(Entry::ToolCall(tc)) = app.transcript.entries().get(idx) {
            let id = tc.id.clone();
            app.transcript.toggle_tool_expanded(&id);
        }
    } else {
        app.focus_idx = Some(idx);
    }
}

pub(super) async fn handle_key(
    code: KeyCode,
    mods: KeyModifiers,
    app: &mut App,
    client: Option<&RpcClient>,
) {
    // Ctrl+C / Ctrl+D always quit, even in focus mode.
    if let (KeyCode::Char('c') | KeyCode::Char('d'), KeyModifiers::CONTROL) = (code, mods) {
        app.quit = true;
        return;
    }

    // Focus mode intercepts navigation keys. Esc exits focus mode.
    if app.focus_idx.is_some() {
        handle_focus_key(code, mods, app);
        return;
    }

    match (code, mods) {
        // Enter focus mode — navigate and expand cards with j/k/↑/↓.
        (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
            if !app.transcript.entries().is_empty() {
                app.focus_idx = Some(app.transcript.entries().len().saturating_sub(1));
                app.flash("focus mode · j/k nav · Enter expand · Esc exit");
            }
        }
        // Cycle theme. Several bindings so we work across terminals:
        //   • Alt+T         — reliable on macOS Terminal / iTerm / xterm
        //   • Ctrl+Shift+T  — works where Kitty keyboard protocol is active
        //   • F12           — pure function-key fallback
        // Matched BEFORE Ctrl+T so the more-specific combo wins.
        (KeyCode::Char('t') | KeyCode::Char('T'), m)
            if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
        {
            app.cycle_theme();
            app.flash(format!("theme → {}", app.theme.name));
        }
        (KeyCode::Char('t') | KeyCode::Char('T'), m) if m.contains(KeyModifiers::ALT) => {
            app.cycle_theme();
            app.flash(format!("theme → {}", app.theme.name));
        }
        (KeyCode::F(12), _) => {
            app.cycle_theme();
            app.flash(format!("theme → {}", app.theme.name));
        }
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
            app.show_thinking = !app.show_thinking;
        }
        (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
            if let Some(id) = last_tool_id(&app.transcript) {
                app.transcript.toggle_tool_expanded(&id);
            }
        }
        (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            open_file_finder(app, String::new(), crate::ui::modal::FilePickMode::Insert);
        }
        (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
            // Copy the most recent assistant message to the clipboard.
            let last_assistant = app.transcript.entries().iter().rev().find_map(|e| match e {
                Entry::Assistant(s) => Some(s.clone()),
                _ => None,
            });
            if let Some(text) = last_assistant {
                do_copy(app, &text);
            } else {
                app.flash_warn("nothing to copy yet");
            }
        }
        (KeyCode::Char(' '), KeyModifiers::CONTROL) => {
            if app.is_streaming {
                app.composer_mode = app.composer_mode.cycle_stream();
                app.flash(format!("composer: {:?}", app.composer_mode));
            }
        }
        (KeyCode::F(1), _) => {
            app.modal = Some(Modal::Commands(ListModal::new(
                "commands",
                "type to filter · Enter apply/insert · Esc close",
                merged_commands(&app.session.commands),
            )));
        }
        (KeyCode::F(5), _) => {
            app.modal = Some(Modal::Models(ListModal::new(
                "model",
                "↑↓ pick · Enter set · Esc close",
                app.session.available_models.clone(),
            )));
        }
        (KeyCode::F(6), _) => {
            let cur = app.session.thinking.unwrap_or(ThinkingLevel::Medium);
            let opts = [
                (ThinkingLevel::Off, "off"),
                (ThinkingLevel::Minimal, "minimal"),
                (ThinkingLevel::Low, "low"),
                (ThinkingLevel::Medium, "medium"),
                (ThinkingLevel::High, "high"),
                (ThinkingLevel::Xhigh, "xhigh"),
            ];
            let selected = opts.iter().position(|(l, _)| *l == cur).unwrap_or(3);
            app.modal = Some(Modal::Thinking(RadioModal::new(
                "thinking level",
                opts.to_vec(),
                selected,
            )));
        }
        (KeyCode::F(7), _) => {
            if let Some(stats) = &app.session.stats {
                app.modal = Some(Modal::Stats(Box::new(stats.clone())));
            } else {
                app.flash_warn("no stats yet");
            }
        }
        (KeyCode::F(8), _) => {
            if let Some(c) = client {
                app.flash("compacting…");
                let _ = c
                    .fire(RpcCommand::Compact {
                        custom_instructions: None,
                    })
                    .await;
            }
        }
        (KeyCode::F(9), _) => {
            let next = !app.session.auto_compaction.unwrap_or(true);
            app.session.auto_compaction = Some(next);
            if let Some(c) = client {
                let _ = c
                    .fire(RpcCommand::SetAutoCompaction { enabled: next })
                    .await;
            }
            app.flash(format!("auto-compact {}", on_off(next)));
        }
        (KeyCode::F(10), _) => {
            let next = !app.session.auto_retry.unwrap_or(true);
            app.session.auto_retry = Some(next);
            if let Some(c) = client {
                let _ = c.fire(RpcCommand::SetAutoRetry { enabled: next }).await;
            }
            app.flash(format!("auto-retry {}", on_off(next)));
        }
        (KeyCode::Char('?'), _) => {
            app.modal = Some(Modal::Help);
        }
        (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
            let entries = app.history.entries().to_vec();
            app.modal = Some(Modal::History(ListModal::new(
                "history",
                "type to filter · Enter restore · Esc close",
                entries,
            )));
        }
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
            match crate::ui::export::export(&app.transcript) {
                Ok(path) => app.flash_success(format!("exported → {}", path.display())),
                Err(e) => app.flash_error(format!("export failed: {e}")),
            }
        }
        (KeyCode::Up, KeyModifiers::NONE) => {
            if let Some(t) = app.history.prev(&app.composer.text()) {
                app.composer.set_text(&t);
            }
        }
        (KeyCode::Down, KeyModifiers::NONE) => {
            if let Some(t) = app.history.next() {
                app.composer.set_text(&t);
            }
        }
        (KeyCode::Char('/'), KeyModifiers::NONE) if app.composer.is_empty() => {
            app.modal = Some(Modal::Commands(ListModal::new(
                "/ commands",
                "type to filter · Enter apply/insert · Esc close",
                merged_commands(&app.session.commands),
            )));
        }
        (KeyCode::Esc, _)
            if app.vim_enabled
                && app.composer.mode == crate::composer::Mode::Insert
                && !app.composer.is_empty() =>
        {
            // Vim: Esc leaves insert mode. Only active when /vim is on.
            app.composer.mode = crate::composer::Mode::Normal;
        }
        (KeyCode::Esc, _) => {
            if app.is_streaming {
                if let Some(c) = client {
                    let _ = c.fire(RpcCommand::Abort).await;
                    app.transcript.push(Entry::Warn("aborted".into()));
                }
            } else if !app.composer.is_empty() {
                app.composer.clear();
            } else {
                app.quit = true;
            }
        }
        // Shift+Enter or Ctrl+J = newline. Plain Enter = submit.
        (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) => {
            app.composer.insert_newline();
        }
        (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
            app.composer.insert_newline();
        }
        // V3.e.7 · explicit Ctrl+Enter → submit. Already falls into the
        // catch-all Enter arm below, but pinning it makes the symmetry
        // with the Interview modal's explicit Ctrl+Enter binding
        // obvious and keeps the intent from being accidentally broken
        // by future arm reordering.
        (KeyCode::Enter, m) if m.contains(KeyModifiers::CONTROL) => submit(app, client).await,
        (KeyCode::Enter, _) => submit(app, client).await,

        // Cursor motion (work in both insert and normal mode).
        (KeyCode::Left, KeyModifiers::NONE) => app.composer.left(),
        (KeyCode::Right, KeyModifiers::NONE) => app.composer.right(),
        (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => app.composer.word_left(),
        (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => app.composer.word_right(),
        (KeyCode::Home, _) => app.composer.home(),
        (KeyCode::End, _) if app.composer.is_empty() => app.scroll = None,
        (KeyCode::End, _) => app.composer.end(),

        // Emacs-lite kill bindings in Insert mode.
        (KeyCode::Char('a'), KeyModifiers::CONTROL) => app.composer.home(),
        // (Ctrl+E is owned by the transcript expand-last-tool binding; use
        // `End` for composer end-of-line.)
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.composer.kill_line_back(),
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => app.composer.kill_line_forward(),
        (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
            // Kill word back.
            let before = app.composer.col;
            app.composer.word_left();
            let start = app.composer.col;
            let row = app.composer.row;
            let line = &mut app.composer.lines[row];
            line.drain(start..before);
        }
        (KeyCode::Backspace, _) => app.composer.backspace(),
        (KeyCode::Delete, _) => app.composer.delete_char_forward(),

        // Normal-mode dispatch before the default-insert arm so vim keys
        // take precedence over plain-char insertion.
        (KeyCode::Char(ch), m)
            if app.vim_enabled
                && app.composer.mode == crate::composer::Mode::Normal
                && !m.contains(KeyModifiers::CONTROL)
                && !m.contains(KeyModifiers::ALT) =>
        {
            handle_vim_normal(ch, app);
        }
        (KeyCode::Char(ch), m)
            if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
        {
            app.composer.insert_char(ch);
            app.history.reset_walk();
            let opens_picker = ch == '@' && {
                let text = app.composer.text();
                let before = text.len().saturating_sub(1);
                let prev = text.as_bytes().get(before.saturating_sub(1)).copied();
                before == 0 || matches!(prev, Some(b' ' | b'\t'))
            };
            if opens_picker {
                open_file_finder(app, String::new(), crate::ui::modal::FilePickMode::AtToken);
            }
        }
        (KeyCode::PageUp, _) => {
            let cur = app.scroll.unwrap_or(u16::MAX);
            app.scroll = Some(cur.saturating_sub(10));
        }
        (KeyCode::PageDown, _) => {
            let cur = app.scroll.unwrap_or(0);
            app.scroll = Some(cur.saturating_add(10));
        }
        _ => {}
    }
}

/// Vim normal-mode key dispatcher. Deliberately small subset — we rely
/// on the user falling back to insert mode for anything not covered.
pub(super) fn handle_vim_normal(ch: char, app: &mut App) {
    use crate::composer::Mode;
    let c = &mut app.composer;
    match ch {
        'h' => c.left(),
        'l' => c.right(),
        'j' => c.down(),
        'k' => c.up(),
        'w' => c.word_right(),
        'b' => c.word_left(),
        '0' => c.home(),
        '$' => c.end(),
        'i' => c.mode = Mode::Insert,
        'a' => {
            if c.col < c.lines[c.row].len() {
                c.right();
            }
            c.mode = Mode::Insert;
        }
        'A' => {
            c.end();
            c.mode = Mode::Insert;
        }
        'I' => {
            c.home();
            c.mode = Mode::Insert;
        }
        'o' => {
            c.end();
            c.insert_newline();
            c.mode = Mode::Insert;
        }
        'O' => {
            c.home();
            c.lines.insert(c.row, String::new());
            c.mode = Mode::Insert;
        }
        'x' => c.delete_char_forward(),
        'g' => {
            // Expect `gg` — use pending_op as the "g pressed" sentinel.
            if c.pending_op == Some('g') {
                c.top();
                c.pending_op = None;
            } else {
                c.pending_op = Some('g');
            }
        }
        'G' => c.bottom(),
        'd' => {
            if c.pending_op == Some('d') {
                c.delete_line();
                c.pending_op = None;
            } else {
                c.pending_op = Some('d');
            }
        }
        _ => {
            // Unknown key: drop any pending `g`/`d` so stray keys don't
            // activate double-letter ops.
            c.pending_op = None;
        }
    }
}
