//! M3 app: chat + modals + session state.
//!
//! Adds on top of M2:
//! - Transcript bootstrap from `get_messages`
//! - Modal system: Commands (F1), Models (F5), Thinking (F6), Stats (F7), Help (?)
//! - Slash-trigger (typing `/` in an empty composer) opens the Commands modal
//!   with a live filter preloaded from the first char
//! - Periodic `get_session_stats` polling drives a context-window gauge in the
//!   header and populates the Stats modal
//! - Queue state from `queue_update` appears as a header chip; `Ctrl+Space`
//!   cycles the composer between steer / follow-up intent during streaming
//! - F8 = compact now, F9 = toggle auto-compaction, F10 = toggle auto-retry

use std::io::{Stdout, stdout};
use std::panic;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};

use crate::cli::Args;
use crate::history::{History, HistoryEntry};
use crate::rpc::client::{self, RpcClient, RpcError};
use crate::rpc::commands::{ExtensionUiResponse, RpcCommand};
use crate::rpc::events::{AssistantEvent, Incoming};
use crate::rpc::types::{
    AgentMessage, AssistantBlock, CommandInfo, ContentBlock, FollowUpMode, ForkMessage, Model,
    SessionStats, State, SteeringMode, ThinkingLevel, ToolResultPayload, UserContent,
};
use crate::theme::{self, Theme};
use crate::ui::ext_ui::{ExtReq, ExtUiState, NotifyKind, WidgetPlacement, parse as parse_ext};
use crate::ui::markdown;
use crate::ui::modal::{ListModal, Modal, RadioModal, centered, matches_query};
use crate::ui::transcript::{
    BashExec, Compaction, CompactionState, Entry, Retry, RetryState, ToolCall, ToolStatus,
    Transcript,
};

pub async fn run(args: Args) -> Result<()> {
    install_panic_hook();
    enable_raw_mode()?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run_inner(&mut terminal, args).await;

    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
    let _ = terminal.show_cursor();

    result
}

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(
            stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        original(info);
    }));
}

// ───────────────────────────────────────────────────────── state ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerMode {
    Prompt,
    Steer,
    FollowUp,
}

impl ComposerMode {
    fn cycle_stream(self) -> Self {
        match self {
            Self::Prompt | Self::FollowUp => Self::Steer,
            Self::Steer => Self::FollowUp,
        }
    }
}

#[derive(Debug, Default)]
struct SessionState {
    model_label: String,
    thinking: Option<ThinkingLevel>,
    steering_mode: Option<SteeringMode>,
    follow_up_mode: Option<FollowUpMode>,
    auto_compaction: Option<bool>,
    auto_retry: Option<bool>,
    session_name: Option<String>,
    queue_steering: Vec<String>,
    queue_follow_up: Vec<String>,
    available_models: Vec<Model>,
    commands: Vec<CommandInfo>,
    stats: Option<SessionStats>,
}

struct App {
    transcript: Transcript,
    input: String,
    is_streaming: bool,
    ticks: u64,
    quit: bool,
    scroll: Option<u16>,
    show_thinking: bool,
    spawn_error: Option<String>,
    composer_mode: ComposerMode,
    session: SessionState,
    modal: Option<Modal>,
    flash: Option<(String, u64)>, // transient status message with a decay tick
    ext_ui: ExtUiState,
    history: History,
    theme: Theme,
}

impl App {
    fn new(spawn_error: Option<String>) -> Self {
        Self {
            transcript: Transcript::default(),
            input: String::new(),
            is_streaming: false,
            ticks: 0,
            quit: false,
            scroll: None,
            show_thinking: false,
            spawn_error,
            composer_mode: ComposerMode::Prompt,
            session: SessionState {
                model_label: "unknown model".into(),
                ..Default::default()
            },
            modal: None,
            flash: None,
            ext_ui: ExtUiState::default(),
            history: History::load(),
            theme: *theme::default_theme(),
        }
    }

    fn cycle_theme(&mut self) {
        let all = theme::builtins();
        let i = theme::index_of(&self.theme);
        let next = (i + 1) % all.len();
        self.theme = all[next];
    }

    fn set_theme_by_name(&mut self, name: &str) -> bool {
        if let Some(t) = theme::find(name) {
            self.theme = *t;
            return true;
        }
        false
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), self.ticks));
    }

    fn apply_state(&mut self, s: &State) {
        if let Some(m) = &s.model {
            self.session.model_label = format!("{}/{}", m.provider, m.id);
        }
        self.session.thinking = Some(s.thinking_level);
        self.session.steering_mode = Some(s.steering_mode);
        self.session.follow_up_mode = Some(s.follow_up_mode);
        self.session.auto_compaction = Some(s.auto_compaction_enabled);
        self.session.session_name = s.session_name.clone();
    }

    fn on_event(&mut self, ev: Incoming) {
        match ev {
            Incoming::AgentStart => self.is_streaming = true,
            Incoming::AgentEnd { .. } => {
                self.is_streaming = false;
                // Composer returns to Prompt outside streaming.
                self.composer_mode = ComposerMode::Prompt;
            }
            Incoming::MessageUpdate {
                assistant_message_event,
                ..
            } => match assistant_message_event {
                AssistantEvent::TextDelta { delta, .. } => {
                    self.transcript.append_assistant(&delta);
                }
                AssistantEvent::ThinkingDelta { delta, .. } => {
                    self.transcript.append_thinking(&delta);
                }
                AssistantEvent::Error { reason, .. } => {
                    self.transcript
                        .push(Entry::Warn(format!("stream error: {reason:?}")));
                }
                _ => {}
            },
            Incoming::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => self.transcript.start_tool(tool_call_id, tool_name, args),
            Incoming::ToolExecutionUpdate {
                tool_call_id,
                partial_result,
                ..
            } => self
                .transcript
                .update_tool_output(&tool_call_id, &partial_result),
            Incoming::ToolExecutionEnd {
                tool_call_id,
                result,
                is_error,
                ..
            } => self
                .transcript
                .finish_tool(&tool_call_id, &result, is_error),
            Incoming::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => self.transcript.push_retry_waiting(
                attempt,
                max_attempts,
                delay_ms,
                error_message.unwrap_or_else(|| "transient error".into()),
            ),
            Incoming::AutoRetryEnd {
                success,
                attempt,
                final_error,
            } => {
                let state = if success {
                    RetryState::Succeeded
                } else {
                    RetryState::Exhausted(final_error.unwrap_or_else(|| "unknown".into()))
                };
                self.transcript.resolve_retry(attempt, state);
            }
            Incoming::CompactionStart { reason } => {
                self.transcript.push_compaction_start(format!("{reason:?}"));
            }
            Incoming::CompactionEnd {
                reason,
                result,
                aborted,
                error_message,
                ..
            } => {
                let reason = format!("{reason:?}");
                let state = if aborted {
                    CompactionState::Aborted
                } else if let Some(msg) = error_message {
                    CompactionState::Failed(msg)
                } else {
                    CompactionState::Done {
                        summary: result.map(|r| r.summary),
                    }
                };
                self.transcript.finish_compaction(reason, state);
            }
            Incoming::ExtensionError { error, .. } => {
                self.transcript.push(Entry::Error(format!(
                    "extension error: {}",
                    error.as_deref().unwrap_or("(no detail)")
                )));
            }
            Incoming::QueueUpdate {
                steering,
                follow_up,
            } => {
                self.session.queue_steering = steering;
                self.session.queue_follow_up = follow_up;
            }
            _ => {}
        }
    }
}

// ───────────────────────────────────────────────────────── bootstrap ──

fn import_messages(app: &mut App, messages: Vec<AgentMessage>) {
    for m in messages {
        match m {
            AgentMessage::User { content, .. } => {
                let text = user_content_text(&content);
                app.transcript.push(Entry::User(text));
            }
            AgentMessage::Assistant { content, .. } => {
                let mut thinking = String::new();
                let mut assistant_text = String::new();
                let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
                for block in content {
                    match block {
                        AssistantBlock::Thinking { thinking: t } => {
                            if !thinking.is_empty() {
                                thinking.push('\n');
                            }
                            thinking.push_str(&t);
                        }
                        AssistantBlock::Text { text } => {
                            if !assistant_text.is_empty() {
                                assistant_text.push('\n');
                            }
                            assistant_text.push_str(&text);
                        }
                        AssistantBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => tool_calls.push((id, name, arguments)),
                    }
                }
                if !thinking.is_empty() {
                    app.transcript.push(Entry::Thinking(thinking));
                }
                if !assistant_text.is_empty() {
                    app.transcript.push(Entry::Assistant(assistant_text));
                }
                for (id, name, args) in tool_calls {
                    app.transcript.start_tool(id, name, args);
                }
            }
            AgentMessage::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let payload = ToolResultPayload {
                    content,
                    details: serde_json::Value::Null,
                };
                app.transcript
                    .finish_tool(&tool_call_id, &payload, is_error);
            }
            AgentMessage::BashExecution {
                command,
                output,
                exit_code,
                cancelled,
                truncated,
                full_output_path,
                ..
            } => {
                app.transcript.push(Entry::BashExec(BashExec {
                    command,
                    output: crate::ui::ansi::strip(&output),
                    exit_code,
                    cancelled,
                    truncated,
                    full_output_path,
                }));
            }
        }
    }
}

fn user_content_text(c: &UserContent) -> String {
    match c {
        UserContent::Text(s) => s.clone(),
        UserContent::Blocks(bs) => bs
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => text.clone(),
                ContentBlock::Image { .. } => "[image]".into(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

// ───────────────────────────────────────────────────────── main loop ──

async fn run_inner(terminal: &mut Terminal<CrosstermBackend<Stdout>>, args: Args) -> Result<()> {
    let (client_and_io, spawn_error) =
        match client::spawn(&args.pi_bin, &args.pi_argv(), args.debug_rpc) {
            Ok(pair) => (Some(pair), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };

    let mut app = App::new(spawn_error);

    if let Some((client, mut io)) = client_and_io {
        bootstrap(&client, &mut app).await;
        app.transcript.push(Entry::Info(format!(
            "connected — theme: {} · F1 cmds · F5 model · F6 think · F7 stats · /theme · ? help",
            app.theme.name
        )));
        ui_loop(terminal, &mut app, Some(&client), &mut io.events).await?;
        if let Err(e) = client::shutdown(client, io).await {
            tracing::warn!(error = ?e, "shutdown error");
        }
    } else {
        ui_loop(terminal, &mut app, None, &mut dummy_events()).await?;
    }
    Ok(())
}

async fn bootstrap(client: &RpcClient, app: &mut App) {
    if let Ok(ok) = client.call(RpcCommand::GetState).await
        && let Some(v) = ok.data
        && let Ok(s) = serde_json::from_value::<State>(v)
    {
        app.apply_state(&s);
    }
    if let Ok(ok) = client.call(RpcCommand::GetMessages).await
        && let Some(v) = ok.data
        && let Some(arr) = v.get("messages").and_then(|x| x.as_array())
    {
        let mut messages: Vec<AgentMessage> = Vec::with_capacity(arr.len());
        for m in arr {
            if let Ok(msg) = serde_json::from_value::<AgentMessage>(m.clone()) {
                messages.push(msg);
            }
        }
        import_messages(app, messages);
    }
    if let Ok(ok) = client.call(RpcCommand::GetCommands).await
        && let Some(v) = ok.data
        && let Some(arr) = v.get("commands").and_then(|x| x.as_array())
    {
        app.session.commands = arr
            .iter()
            .filter_map(|m| serde_json::from_value::<CommandInfo>(m.clone()).ok())
            .collect();
    }
    if let Ok(ok) = client.call(RpcCommand::GetAvailableModels).await
        && let Some(v) = ok.data
        && let Some(arr) = v.get("models").and_then(|x| x.as_array())
    {
        app.session.available_models = arr
            .iter()
            .filter_map(|m| serde_json::from_value::<Model>(m.clone()).ok())
            .collect();
    }
    refresh_stats(client, app).await;
}

async fn refresh_stats(client: &RpcClient, app: &mut App) {
    if let Ok(ok) = client.call(RpcCommand::GetSessionStats).await
        && let Some(v) = ok.data
        && let Ok(s) = serde_json::from_value::<SessionStats>(v)
    {
        app.session.stats = Some(s);
    }
}

fn dummy_events() -> tokio::sync::mpsc::Receiver<Incoming> {
    let (_tx, rx) = tokio::sync::mpsc::channel::<Incoming>(1);
    rx
}

/// Route a single RPC event. Extension UI requests are handled here so we
/// have access to the client (to send responses for dialogs and to set the
/// terminal title via crossterm); everything else delegates to `App::on_event`.
async fn handle_incoming(msg: Incoming, app: &mut App, client: Option<&RpcClient>) {
    if let Incoming::ExtensionUiRequest { id, method, rest } = &msg {
        let req = parse_ext(method, id, rest);
        handle_ext_request(req, app, client).await;
        return;
    }
    app.on_event(msg);
}

async fn handle_ext_request(req: ExtReq, app: &mut App, client: Option<&RpcClient>) {
    match req {
        ExtReq::Select { id, title, options } => {
            app.modal = Some(Modal::ExtSelect {
                request_id: id,
                title,
                options,
                selected: 0,
            });
        }
        ExtReq::Confirm { id, title, message } => {
            app.modal = Some(Modal::ExtConfirm {
                request_id: id,
                title,
                message,
                selected: 0,
            });
        }
        ExtReq::Input {
            id,
            title,
            placeholder,
        } => {
            app.modal = Some(Modal::ExtInput {
                request_id: id,
                title,
                placeholder,
                value: String::new(),
            });
        }
        ExtReq::Editor { id, title, prefill } => {
            app.modal = Some(Modal::ExtEditor {
                request_id: id,
                title,
                value: prefill,
            });
        }
        ExtReq::Notify { message, kind } => {
            app.ext_ui.push_toast(message, kind, app.ticks);
        }
        ExtReq::SetStatus { key, text } => app.ext_ui.set_status(key, text),
        ExtReq::SetWidget { key, widget } => app.ext_ui.set_widget(key, widget),
        ExtReq::SetTitle { title } => {
            use crossterm::terminal::SetTitle;
            // Best-effort — if writing to the tty fails we just log.
            if let Err(e) = crossterm::execute!(stdout(), SetTitle(&title)) {
                tracing::warn!(error = ?e, "SetTitle failed");
            }
            app.ext_ui.terminal_title = Some(title);
        }
        ExtReq::SetEditorText { text } => {
            app.input = text;
        }
        ExtReq::Unknown(m) => {
            // We've lost the id by the time we get here (parse only keeps it
            // for recognized dialog methods). Log only — the dialog, if any,
            // will time out on pi's side per the spec.
            tracing::warn!(method = %m, "unknown extension_ui_request method");
            let _ = client; // suppress unused-variable warning when no dialog
        }
    }
}

async fn ui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    client: Option<&RpcClient>,
    events: &mut tokio::sync::mpsc::Receiver<Incoming>,
) -> Result<()> {
    let mut crossterm_events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.tick().await;
    let mut stats_tick = tokio::time::interval(Duration::from_secs(5));
    stats_tick.tick().await;

    loop {
        terminal.draw(|f| draw(f, app))?;
        if app.quit {
            break;
        }

        tokio::select! {
            Some(msg) = events.recv() => handle_incoming(msg, app, client).await,
            Some(Ok(ev)) = crossterm_events.next() => handle_crossterm(ev, app, client).await,
            _ = tick.tick() => {
                app.ticks = app.ticks.wrapping_add(1);
                // Expire flash after ~1.5s.
                if let Some((_, at)) = app.flash
                    && app.ticks.wrapping_sub(at) > 15 {
                    app.flash = None;
                }
                app.ext_ui.expire_toasts(app.ticks);
            }
            _ = stats_tick.tick() => {
                if let Some(c) = client { refresh_stats(c, app).await; }
            }
        }
    }
    Ok(())
}

// ───────────────────────────────────────────────────────── input ──

async fn handle_crossterm(ev: Event, app: &mut App, client: Option<&RpcClient>) {
    // If a modal is open, route input to it first.
    if app.modal.is_some()
        && let Event::Key(k) = ev
        && k.kind == KeyEventKind::Press
    {
        handle_modal_key(k.code, k.modifiers, app, client).await;
        return;
    }

    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Press => {
            handle_key(k.code, k.modifiers, app, client).await;
        }
        Event::Paste(text) => {
            for ch in text.chars() {
                if ch == '\n' || ch == '\r' {
                    app.input.push(' ');
                } else {
                    app.input.push(ch);
                }
            }
            app.history.reset_walk();
        }
        Event::Mouse(MouseEvent { kind, .. }) => match kind {
            MouseEventKind::ScrollUp => {
                let cur = app.scroll.unwrap_or(u16::MAX);
                app.scroll = Some(cur.saturating_sub(3));
            }
            MouseEventKind::ScrollDown => {
                let cur = app.scroll.unwrap_or(0);
                app.scroll = Some(cur.saturating_add(3));
            }
            _ => {}
        },
        _ => {}
    }
}

async fn handle_key(code: KeyCode, mods: KeyModifiers, app: &mut App, client: Option<&RpcClient>) {
    match (code, mods) {
        (KeyCode::Char('c') | KeyCode::Char('d'), KeyModifiers::CONTROL) => app.quit = true,
        // Ctrl+Shift+T cycles theme — check first since it also matches
        // KeyCode::Char('T'), KeyModifiers::CONTROL | SHIFT on many terminals.
        (KeyCode::Char('t') | KeyCode::Char('T'), m)
            if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
        {
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
        (KeyCode::Char(' '), KeyModifiers::CONTROL) => {
            if app.is_streaming {
                app.composer_mode = app.composer_mode.cycle_stream();
                app.flash(format!("composer: {:?}", app.composer_mode));
            }
        }
        (KeyCode::F(1), _) => {
            app.modal = Some(Modal::Commands(ListModal::new(
                "commands",
                "↑↓ pick · Enter insert · Esc close",
                app.session.commands.clone(),
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
                app.flash("no stats yet");
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
                Ok(path) => app.flash(format!("exported → {}", path.display())),
                Err(e) => app.flash(format!("export failed: {e}")),
            }
        }
        (KeyCode::Up, KeyModifiers::NONE) => {
            if let Some(t) = app.history.prev(&app.input) {
                app.input = t;
            }
        }
        (KeyCode::Down, KeyModifiers::NONE) => {
            if let Some(t) = app.history.next() {
                app.input = t;
            }
        }
        (KeyCode::Char('/'), KeyModifiers::NONE) if app.input.is_empty() => {
            // Open commands modal as a slash autocomplete source.
            app.modal = Some(Modal::Commands(ListModal::new(
                "/ commands",
                "type to filter · Enter insert · Esc close",
                app.session.commands.clone(),
            )));
        }
        (KeyCode::Esc, _) => {
            if app.is_streaming {
                if let Some(c) = client {
                    let _ = c.fire(RpcCommand::Abort).await;
                    app.transcript.push(Entry::Warn("aborted".into()));
                }
            } else if !app.input.is_empty() {
                app.input.clear();
            } else {
                app.quit = true;
            }
        }
        (KeyCode::Enter, _) => submit(app, client).await,
        (KeyCode::Backspace, _) => {
            app.input.pop();
        }
        (KeyCode::Char(ch), m)
            if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
        {
            app.input.push(ch);
            app.history.reset_walk();
        }
        (KeyCode::PageUp, _) => {
            let cur = app.scroll.unwrap_or(u16::MAX);
            app.scroll = Some(cur.saturating_sub(5));
        }
        (KeyCode::PageDown, _) => {
            let cur = app.scroll.unwrap_or(0);
            app.scroll = Some(cur.saturating_add(5));
        }
        (KeyCode::End, _) => app.scroll = None,
        _ => {}
    }
}

async fn handle_modal_key(
    code: KeyCode,
    _mods: KeyModifiers,
    app: &mut App,
    client: Option<&RpcClient>,
) {
    let Some(modal) = app.modal.as_mut() else {
        return;
    };
    match modal {
        Modal::Stats(_) | Modal::Help => match code {
            KeyCode::Esc | KeyCode::Enter => app.modal = None,
            _ => {}
        },
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
                    // "theme <name>" — apply inline instead of prefilling.
                    if let Some(theme_name) = cmd.name.strip_prefix("theme ") {
                        if app.set_theme_by_name(theme_name) {
                            app.flash(format!("theme → {}", app.theme.name));
                        }
                        app.modal = None;
                        return;
                    }
                    app.input.clear();
                    app.input.push('/');
                    app.input.push_str(&cmd.name);
                    app.input.push(' ');
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
                            Err(e) => app.flash(format!("set_model failed: {e}")),
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
                        app.input = entry.text;
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
                                app.flash(format!("forked at {}", truncate_preview(&f.text, 40)));
                                // Reload transcript at the fork point.
                                bootstrap(c, app).await;
                            }
                            Err(e) => app.flash(format!("fork failed: {e}")),
                        }
                    }
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
                        Err(e) => app.flash(format!("set_thinking_level failed: {e}")),
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
    }
}

/// Shared list key handler. Returns `true` if the key was consumed here.
fn handle_list_keys(
    query: &mut String,
    selected: &mut usize,
    code: KeyCode,
    visible_count: usize,
) -> bool {
    match code {
        KeyCode::Up => {
            if *selected > 0 {
                *selected -= 1;
            }
            true
        }
        KeyCode::Down => {
            if visible_count > 0 && *selected + 1 < visible_count {
                *selected += 1;
            }
            true
        }
        KeyCode::Char(ch) => {
            query.push(ch);
            *selected = 0;
            true
        }
        KeyCode::Backspace => {
            query.pop();
            *selected = 0;
            true
        }
        _ => false,
    }
}

fn filtered_commands<'a>(
    items: &'a [CommandInfo],
    q: &'a str,
) -> impl Iterator<Item = &'a CommandInfo> + 'a {
    items.iter().filter(move |c| matches_query(&c.name, q))
}

fn filtered_count_commands(items: &[CommandInfo], q: &str) -> usize {
    filtered_commands(items, q).count()
}

fn filtered_models<'a>(items: &'a [Model], q: &'a str) -> impl Iterator<Item = &'a Model> + 'a {
    items
        .iter()
        .filter(move |m| matches_query(&m.id, q) || matches_query(&m.provider, q))
}

fn filtered_count_models(items: &[Model], q: &str) -> usize {
    filtered_models(items, q).count()
}

fn filtered_history<'a>(
    items: &'a [HistoryEntry],
    q: &'a str,
) -> impl Iterator<Item = &'a HistoryEntry> + 'a {
    items
        .iter()
        .rev()
        .filter(move |e| matches_query(&e.text, q))
}

fn filtered_count_history(items: &[HistoryEntry], q: &str) -> usize {
    filtered_history(items, q).count()
}

fn filtered_forks<'a>(
    items: &'a [ForkMessage],
    q: &'a str,
) -> impl Iterator<Item = &'a ForkMessage> + 'a {
    items.iter().filter(move |f| matches_query(&f.text, q))
}

fn filtered_count_forks(items: &[ForkMessage], q: &str) -> usize {
    filtered_forks(items, q).count()
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn last_tool_id(transcript: &Transcript) -> Option<String> {
    transcript.entries().iter().rev().find_map(|e| match e {
        Entry::ToolCall(tc) => Some(tc.id.clone()),
        _ => None,
    })
}

async fn submit(app: &mut App, client: Option<&RpcClient>) {
    let text = app.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    let Some(client) = client else {
        return;
    };
    app.input.clear();

    app.history.record(&text);

    // Local slash commands — intercepted before pi sees them. Each branch is a
    // no-op if we return; otherwise fall through to send to pi (for extension /
    // prompt / skill commands that pi itself handles).
    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        match name {
            "help" => {
                app.modal = Some(Modal::Help);
                return;
            }
            "stats" => {
                if let Some(stats) = &app.session.stats {
                    app.modal = Some(Modal::Stats(Box::new(stats.clone())));
                }
                return;
            }
            "export" => {
                match crate::ui::export::export(&app.transcript) {
                    Ok(p) => app.flash(format!("exported → {}", p.display())),
                    Err(e) => app.flash(format!("export failed: {e}")),
                }
                return;
            }
            "rename" => {
                if arg.is_empty() {
                    app.flash("usage: /rename <name>");
                    return;
                }
                let name_str = arg.to_string();
                match client
                    .call(RpcCommand::SetSessionName {
                        name: name_str.clone(),
                    })
                    .await
                {
                    Ok(_) => {
                        app.session.session_name = Some(name_str.clone());
                        app.flash(format!("session renamed → {name_str}"));
                    }
                    Err(e) => app.flash(format!("rename failed: {e}")),
                }
                return;
            }
            "new" => {
                match client
                    .call(RpcCommand::NewSession {
                        parent_session: None,
                    })
                    .await
                {
                    Ok(_) => {
                        app.transcript = Transcript::default();
                        app.flash("new session started");
                    }
                    Err(e) => app.flash(format!("new session failed: {e}")),
                }
                return;
            }
            "export-html" => {
                match client
                    .call(RpcCommand::ExportHtml { output_path: None })
                    .await
                {
                    Ok(ok) => {
                        let path = ok
                            .data
                            .as_ref()
                            .and_then(|v| v.get("path"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("(no path)")
                            .to_string();
                        app.flash(format!("html → {path}"));
                    }
                    Err(e) => app.flash(format!("export-html failed: {e}")),
                }
                return;
            }
            "switch" => {
                if arg.is_empty() {
                    app.flash("usage: /switch <session-file>");
                    return;
                }
                match client
                    .call(RpcCommand::SwitchSession {
                        session_path: arg.to_string(),
                    })
                    .await
                {
                    Ok(_) => {
                        app.flash(format!("switched → {arg}"));
                        // Reload state + messages from the new session.
                        bootstrap(client, app).await;
                    }
                    Err(e) => app.flash(format!("switch failed: {e}")),
                }
                return;
            }
            "fork" => {
                match client.call(RpcCommand::GetForkMessages).await {
                    Ok(ok) => {
                        let items: Vec<ForkMessage> = ok
                            .data
                            .as_ref()
                            .and_then(|v| v.get("messages"))
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|m| {
                                        serde_json::from_value::<ForkMessage>(m.clone()).ok()
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if items.is_empty() {
                            app.flash("no fork candidates");
                            return;
                        }
                        app.modal = Some(Modal::Forks(ListModal::new(
                            "forks",
                            "type to filter · Enter fork · Esc close",
                            items,
                        )));
                    }
                    Err(e) => app.flash(format!("get_fork_messages failed: {e}")),
                }
                return;
            }
            "compact" => {
                let _ = client
                    .fire(RpcCommand::Compact {
                        custom_instructions: if arg.is_empty() {
                            None
                        } else {
                            Some(arg.to_string())
                        },
                    })
                    .await;
                app.flash("compacting…");
                return;
            }
            "theme" => {
                if arg.is_empty() {
                    app.cycle_theme();
                    app.flash(format!("theme → {}", app.theme.name));
                } else if app.set_theme_by_name(arg) {
                    app.flash(format!("theme → {}", app.theme.name));
                } else {
                    app.flash(format!(
                        "unknown theme: {arg} — try one of: {}",
                        theme::builtins()
                            .iter()
                            .map(|t| t.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                return;
            }
            "themes" => {
                // Build a picker list from built-ins (as CommandInfo for reuse).
                let items: Vec<CommandInfo> = theme::builtins()
                    .iter()
                    .map(|t| CommandInfo {
                        name: format!("theme {}", t.name),
                        description: Some(format!("switch to {}", t.name)),
                        source: crate::rpc::types::CommandSource::Extension,
                        location: None,
                        path: None,
                    })
                    .collect();
                app.modal = Some(Modal::Commands(ListModal::new(
                    "themes",
                    "type to filter · Enter apply · Esc close",
                    items,
                )));
                return;
            }
            _ => {
                // Unknown local slash — fall through to pi so extension /
                // prompt / skill commands still work.
            }
        }
    }

    if let Some(cmd) = text.strip_prefix('!') {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        let cmd = cmd.to_string();
        app.transcript.push(Entry::User(format!("!{cmd}")));
        match client
            .call(RpcCommand::Bash {
                command: cmd.clone(),
            })
            .await
        {
            Ok(ok) => {
                let value = ok.data.unwrap_or(serde_json::Value::Null);
                let exec = parse_bash_result(&cmd, &value);
                app.transcript.push(Entry::BashExec(exec));
            }
            Err(e) => {
                let msg = match e {
                    RpcError::Remote { message, .. } => message,
                    other => other.to_string(),
                };
                app.transcript
                    .push(Entry::Error(format!("bash failed: {msg}")));
            }
        }
        return;
    }

    app.transcript.push(Entry::User(text.clone()));
    let rpc = match (app.is_streaming, app.composer_mode) {
        (false, _) => RpcCommand::Prompt {
            message: text,
            images: vec![],
            streaming_behavior: None,
        },
        (true, ComposerMode::Steer | ComposerMode::Prompt) => RpcCommand::Steer {
            message: text,
            images: vec![],
        },
        (true, ComposerMode::FollowUp) => RpcCommand::FollowUp {
            message: text,
            images: vec![],
        },
    };
    if let Err(e) = client.fire(rpc).await {
        let msg = match e {
            RpcError::Remote { message, .. } => message,
            other => other.to_string(),
        };
        app.transcript
            .push(Entry::Error(format!("submit failed: {msg}")));
    }
}

fn parse_bash_result(command: &str, v: &serde_json::Value) -> BashExec {
    BashExec {
        command: command.to_string(),
        output: v
            .get("output")
            .and_then(|o| o.as_str())
            .map(crate::ui::ansi::strip)
            .unwrap_or_default(),
        exit_code: v.get("exitCode").and_then(|o| o.as_i64()).unwrap_or(-1) as i32,
        cancelled: v
            .get("cancelled")
            .and_then(|o| o.as_bool())
            .unwrap_or(false),
        truncated: v
            .get("truncated")
            .and_then(|o| o.as_bool())
            .unwrap_or(false),
        full_output_path: v
            .get("fullOutputPath")
            .and_then(|o| o.as_str())
            .map(String::from),
    }
}

// ───────────────────────────────────────────────────────── drawing ──

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn draw(f: &mut ratatui::Frame, app: &App) {
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

    let constraints: Vec<Constraint> = {
        let mut c = vec![
            Constraint::Length(1), // header
            Constraint::Min(3),    // body
        ];
        if above_h > 0 {
            c.push(Constraint::Length(above_h));
        }
        c.push(Constraint::Length(3)); // editor
        if below_h > 0 {
            c.push(Constraint::Length(below_h));
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
    let editor_area = rects[idx];
    idx += 1;
    let below_rect = if below_h > 0 {
        let r = rects[idx];
        idx += 1;
        Some(r)
    } else {
        None
    };
    let footer = rects[idx];

    draw_header(f, header, app);
    draw_body(f, body, app);
    if let Some(r) = above_rect {
        draw_widgets(f, r, &above_widgets);
    }
    draw_editor(f, editor_area, app);
    if let Some(r) = below_rect {
        draw_widgets(f, r, &below_widgets);
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

    let line = Line::from(vec![
        Span::styled(
            " rata-pi ",
            Style::default()
                .fg(t.text)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw("  "),
        spinner,
        Span::styled(&app.session.model_label, Style::default().fg(t.accent)),
        Span::styled(session_name, Style::default().fg(t.dim)),
        Span::raw("  ·  "),
        status,
        thinking_badge,
        Span::styled(queue, Style::default().fg(t.dim)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn entries_to_lines(entries: &[Entry], show_thinking: bool, t: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(entries.len() * 4);
    for e in entries {
        match e {
            Entry::User(s) => {
                let label = Span::styled(
                    "you › ",
                    Style::default()
                        .fg(t.role_user)
                        .add_modifier(Modifier::BOLD),
                );
                for (i, part) in s.split('\n').enumerate() {
                    let prefix = if i == 0 {
                        label.clone()
                    } else {
                        Span::raw("      ")
                    };
                    lines.push(Line::from(vec![prefix, Span::raw(part.to_string())]));
                }
                lines.push(Line::default());
            }
            Entry::Thinking(s) => {
                if show_thinking {
                    for (i, part) in s.split('\n').enumerate() {
                        let prefix = Span::styled(
                            if i == 0 { "think › " } else { "        " },
                            Style::default().fg(t.role_thinking),
                        );
                        lines.push(Line::from(vec![
                            prefix,
                            Span::styled(
                                part.to_string(),
                                Style::default().fg(t.muted).add_modifier(Modifier::ITALIC),
                            ),
                        ]));
                    }
                    lines.push(Line::default());
                } else {
                    let count = s.lines().count().max(1);
                    lines.push(Line::from(Span::styled(
                        format!("▸ thinking ({count} lines — Ctrl+T to reveal)"),
                        Style::default().fg(t.role_thinking),
                    )));
                }
            }
            Entry::Assistant(md) => {
                let label = Span::styled(
                    "pi  › ",
                    Style::default()
                        .fg(t.role_assistant)
                        .add_modifier(Modifier::BOLD),
                );
                let rendered = markdown::render(md);
                if rendered.is_empty() {
                    lines.push(Line::from(vec![label]));
                } else {
                    for (i, mut l) in rendered.into_iter().enumerate() {
                        let prefix = if i == 0 {
                            label.clone()
                        } else {
                            Span::raw("      ")
                        };
                        l.spans.insert(0, prefix);
                        lines.push(l);
                    }
                }
                lines.push(Line::default());
            }
            Entry::ToolCall(tc) => push_tool_lines(&mut lines, tc, t),
            Entry::BashExec(bx) => push_bash_lines(&mut lines, bx, t),
            Entry::Info(s) => lines.push(Line::from(Span::styled(
                format!("· {s}"),
                Style::default().fg(t.dim),
            ))),
            Entry::Warn(s) => lines.push(Line::from(Span::styled(
                format!("⚠ {s}"),
                Style::default().fg(t.warning),
            ))),
            Entry::Error(s) => lines.push(Line::from(Span::styled(
                format!("✗ {s}"),
                Style::default().fg(t.error),
            ))),
            Entry::Compaction(c) => push_compaction_line(&mut lines, c, t),
            Entry::Retry(r) => push_retry_line(&mut lines, r, t),
        }
    }
    lines
}

fn push_tool_lines(lines: &mut Vec<Line<'static>>, tc: &ToolCall, t: &Theme) {
    let (sym, color) = match tc.status {
        ToolStatus::Running => ("…", t.role_tool),
        ToolStatus::Ok => ("✓", t.success),
        ToolStatus::Err => ("✗", t.error),
    };
    let arg_preview = truncate_preview(&args_preview(&tc.args), 60);
    let header_style = if tc.status == ToolStatus::Err || tc.is_error {
        Style::default().fg(t.error).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    };
    let expand_hint = if tc.expanded { "▾" } else { "▸" };
    lines.push(Line::from(vec![
        Span::styled(format!("{expand_hint} {sym} {} ", tc.name), header_style),
        Span::styled(arg_preview, Style::default().fg(t.dim)),
    ]));
    if tc.expanded {
        let output = crate::ui::ansi::strip(&tc.output);
        if output.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                "    (no output yet)",
                Style::default().fg(t.dim),
            )));
        } else {
            let diff_like = looks_like_diff(&output);
            for part in output.split('\n').take(200) {
                let styled = if diff_like {
                    diff_style_for(part, t)
                } else {
                    Style::default().fg(t.muted)
                };
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(part.to_string(), styled),
                ]));
            }
        }
    } else if !tc.output.trim().is_empty() {
        let first = tc
            .output
            .lines()
            .next()
            .map(|s| truncate_preview(s, 80))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(first, Style::default().fg(t.dim)),
        ]));
    }
    lines.push(Line::default());
}

fn args_preview(args: &serde_json::Value) -> String {
    if let Some(obj) = args.as_object() {
        let mut parts: Vec<String> = obj
            .iter()
            .map(|(k, v)| match v {
                serde_json::Value::String(s) => format!("{k}={:?}", truncate_preview(s, 40)),
                _ => format!("{k}={v}"),
            })
            .collect();
        parts.truncate(4);
        parts.join("  ")
    } else if args.is_null() {
        String::new()
    } else {
        serde_json::to_string(args).unwrap_or_default()
    }
}

/// Heuristic: does this output look like a unified diff? Triggers if we see
/// `+++ ` or `--- ` or `@@ ` markers anywhere in the first ~20 lines.
fn looks_like_diff(s: &str) -> bool {
    for line in s.lines().take(20) {
        if line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("@@ ") {
            return true;
        }
    }
    false
}

fn diff_style_for(line: &str, t: &Theme) -> Style {
    if line.starts_with("+++ ") || line.starts_with("--- ") {
        Style::default()
            .fg(t.diff_file)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default().fg(t.diff_hunk)
    } else if line.starts_with('+') {
        Style::default().fg(t.diff_add)
    } else if line.starts_with('-') {
        Style::default().fg(t.diff_remove)
    } else {
        Style::default().fg(t.muted)
    }
}

fn truncate_preview(s: &str, max: usize) -> String {
    let s: String = s.chars().take(max + 1).collect();
    if s.chars().count() > max {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    } else {
        s
    }
}

fn push_bash_lines(lines: &mut Vec<Line<'static>>, bx: &BashExec, t: &Theme) {
    let status_color = if bx.cancelled {
        t.warning
    } else if bx.exit_code == 0 {
        t.success
    } else {
        t.error
    };
    let status_txt = if bx.cancelled {
        " cancelled ".to_string()
    } else {
        format!(" exit {} ", bx.exit_code)
    };
    lines.push(Line::from(vec![
        Span::styled(
            "$ ",
            Style::default()
                .fg(t.role_bash)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            bx.command.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            status_txt,
            Style::default()
                .bg(status_color)
                .fg(Color::Rgb(0, 0, 0))
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let body = crate::ui::ansi::strip(&bx.output);
    for part in body.split('\n').take(200) {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(part.to_string(), Style::default().fg(t.muted)),
        ]));
    }
    if bx.truncated {
        let path = bx
            .full_output_path
            .as_deref()
            .unwrap_or("(path not provided)");
        lines.push(Line::from(Span::styled(
            format!("  … output truncated — full log: {path}"),
            Style::default().fg(t.warning),
        )));
    }
    lines.push(Line::default());
}

fn push_compaction_line(lines: &mut Vec<Line<'static>>, c: &Compaction, t: &Theme) {
    let (sym, color, label) = match &c.state {
        CompactionState::Running => ("⟲", t.accent_strong, "compacting".to_string()),
        CompactionState::Done { summary } => {
            let s = summary
                .as_deref()
                .map(|s| truncate_preview(s, 100))
                .unwrap_or_default();
            (
                "⟲",
                t.success,
                if s.is_empty() {
                    "compaction complete".to_string()
                } else {
                    format!("compaction: {s}")
                },
            )
        }
        CompactionState::Aborted => ("⟲", t.warning, "compaction aborted".into()),
        CompactionState::Failed(msg) => ("⟲", t.error, format!("compaction failed: {msg}")),
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("{sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
        Span::raw(" "),
        Span::styled(format!("({})", c.reason), Style::default().fg(t.dim)),
    ]));
}

fn push_retry_line(lines: &mut Vec<Line<'static>>, r: &Retry, t: &Theme) {
    let (sym, color, label) = match &r.state {
        RetryState::Waiting { delay_ms, error } => (
            "↻",
            t.warning,
            format!(
                "retry {}/{} in {}ms — {}",
                r.attempt,
                r.max_attempts,
                delay_ms,
                truncate_preview(error, 80),
            ),
        ),
        RetryState::Succeeded => ("↻", t.success, format!("retry {} succeeded", r.attempt)),
        RetryState::Exhausted(msg) => (
            "↻",
            t.error,
            format!(
                "retry exhausted at {}: {}",
                r.attempt,
                truncate_preview(msg, 80)
            ),
        ),
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("{sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
    ]));
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" transcript ")
        .border_style(Style::default().add_modifier(Modifier::DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(err) = &app.spawn_error {
        let msg = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                "⚠ pi is not available",
                Style::default()
                    .fg(app.theme.error)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(err.clone()),
            Line::default(),
            Line::from(Span::styled(
                "press q or Ctrl+C to quit",
                Style::default().fg(app.theme.dim),
            )),
        ]))
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        return;
    }

    let lines = entries_to_lines(app.transcript.entries(), app.show_thinking, &app.theme);
    let text = Text::from(lines);
    let line_count = text.lines.len() as u16;
    let viewport = inner.height;
    let max_offset = line_count.saturating_sub(viewport);
    let offset = match app.scroll {
        None => max_offset,
        Some(v) => v.min(max_offset),
    };
    let para = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((offset, 0));
    f.render_widget(para, inner);
}

fn draw_editor(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let is_bash = app.input.trim_start().starts_with('!');
    let (color, title) = if is_bash {
        (t.role_bash, " bash (! prefix · Enter run) ".to_string())
    } else if app.is_streaming {
        let label = match app.composer_mode {
            ComposerMode::Steer | ComposerMode::Prompt => "steer",
            ComposerMode::FollowUp => "follow-up",
        };
        (
            t.border_active,
            format!(" {label} (Ctrl+Space cycle · Esc abort) "),
        )
    } else {
        (
            t.border_idle,
            " prompt (Enter submit · / commands · Esc clear) ".to_string(),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let spans = vec![
        Span::styled(app.input.as_str(), Style::default().fg(t.text)),
        Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)),
    ];
    f.render_widget(
        Paragraph::new(Line::from(spans)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
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
            kb("Esc"),
            Span::raw(" abort · "),
            kb("Ctrl+Space"),
            Span::raw(" cycle · "),
            kb("F7"),
            Span::raw(" stats · "),
            kb("Ctrl+T"),
            Span::raw(" thinking · "),
            kb("PgUp/PgDn"),
            Span::raw(" scroll · "),
            kb("Ctrl+C"),
            Span::raw(" quit"),
        ]
    } else {
        vec![
            kb("Enter"),
            Span::raw(" send · "),
            kb("/"),
            Span::raw(" cmds · "),
            kb("F5"),
            Span::raw(" model · "),
            kb("F6"),
            Span::raw(" think · "),
            kb("F7"),
            Span::raw(" stats · "),
            kb("F8"),
            Span::raw(" compact · "),
            kb("?"),
            Span::raw(" help · "),
            kb("Ctrl+C"),
            Span::raw(" quit"),
        ]
    };
    if let Some((msg, _)) = &app.flash {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            format!("• {msg}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    // Extension statuses (keyed by extension) right after hints.
    for (k, v) in &app.ext_ui.statuses {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            format!("[{k}] "),
            Style::default().fg(Color::Magenta),
        ));
        spans.push(Span::raw(v.clone()));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().add_modifier(Modifier::DIM)),
        hints,
    );
}

fn kb(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::Cyan))
}

// ───────────────────────────────────────────────────────── modals ──

fn draw_modal(f: &mut ratatui::Frame, area: Rect, modal: &Modal, app: &App) {
    let theme = &app.theme;
    let (title, body, hint, max_w, max_h) = match modal {
        Modal::Help => (
            " help ".to_string(),
            help_text(theme),
            "Esc close".to_string(),
            70,
            22,
        ),
        Modal::Stats(s) => (
            " stats ".to_string(),
            stats_text(s, theme),
            "Esc close".to_string(),
            70,
            18,
        ),
        Modal::Commands(list) => (
            format!(" {} ", list.title),
            commands_text(list, theme),
            list.hint.clone(),
            80,
            22,
        ),
        Modal::Models(list) => (
            format!(" {} ", list.title),
            models_text(list, theme),
            list.hint.clone(),
            80,
            22,
        ),
        Modal::Thinking(radio) => (
            format!(" {} ", radio.title),
            thinking_text(radio, theme),
            "↑↓ · Enter set · Esc close".to_string(),
            50,
            12,
        ),
        Modal::History(list) => (
            format!(" {} ", list.title),
            history_text(list, theme),
            list.hint.clone(),
            90,
            24,
        ),
        Modal::Forks(list) => (
            format!(" {} ", list.title),
            forks_text(list, theme),
            list.hint.clone(),
            90,
            24,
        ),
        Modal::ExtSelect {
            title,
            options,
            selected,
            ..
        } => (
            format!(" ext: {title} "),
            ext_select_text(options, *selected, theme),
            "↑↓ · Enter pick · Esc cancel".to_string(),
            70,
            20,
        ),
        Modal::ExtConfirm {
            title,
            message,
            selected,
            ..
        } => (
            format!(" ext: {title} "),
            ext_confirm_text(message.as_deref(), *selected, theme),
            "Y/N · ←→ · Enter · Esc".to_string(),
            60,
            10,
        ),
        Modal::ExtInput {
            title,
            placeholder,
            value,
            ..
        } => (
            format!(" ext: {title} "),
            ext_input_text(placeholder.as_deref(), value, theme),
            "Enter submit · Esc cancel".to_string(),
            70,
            8,
        ),
        Modal::ExtEditor { title, value, .. } => (
            format!(" ext: {title} "),
            ext_input_text(None, value, theme),
            "Enter submit · Esc cancel".to_string(),
            80,
            14,
        ),
    };

    let t = &app.theme;
    let rect = centered(area, max_w, max_h);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_bottom(Line::from(Span::styled(
            format!(" {hint} "),
            Style::default().fg(t.dim),
        )))
        .border_style(Style::default().fg(t.border_modal));

    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
}

fn help_text(t: &Theme) -> Text<'static> {
    Text::from(vec![
        Line::from(vec![
            kb("Enter"),
            Span::raw(" submit   "),
            kb("Shift+Enter"),
            Span::raw(" newline (deferred)"),
        ]),
        Line::from(vec![
            kb("!cmd"),
            Span::raw(" bash RPC · "),
            kb("/"),
            Span::raw(" commands"),
        ]),
        Line::from(vec![
            Span::styled("  slash: ", Style::default().fg(t.dim)),
            Span::styled(
                "/help /stats /export /export-html /rename <n> /new /switch <p> /fork /compact /theme",
                Style::default().fg(t.warning),
            ),
        ]),
        Line::from(vec![
            kb("Esc"),
            Span::raw(" abort/clear/quit · "),
            kb("Ctrl+C"),
            Span::raw(" quit"),
        ]),
        Line::from(vec![
            kb("Ctrl+R"),
            Span::raw(" history search · "),
            kb("Ctrl+S"),
            Span::raw(" export markdown · "),
            kb("↑/↓"),
            Span::raw(" history"),
        ]),
        Line::default(),
        Line::from(vec![
            kb("Ctrl+T"),
            Span::raw(" thinking · "),
            kb("Ctrl+Shift+T"),
            Span::raw(" cycle theme · "),
            kb("/theme <name>"),
            Span::raw(" pick"),
        ]),
        Line::from(vec![
            kb("Ctrl+E"),
            Span::raw(" expand/collapse last tool card"),
        ]),
        Line::from(vec![
            kb("Ctrl+Space"),
            Span::raw(" cycle composer mode (steer / follow-up)"),
        ]),
        Line::default(),
        Line::from(vec![
            kb("F1"),
            Span::raw(" commands   "),
            kb("F5"),
            Span::raw(" model   "),
            kb("F6"),
            Span::raw(" thinking"),
        ]),
        Line::from(vec![
            kb("F7"),
            Span::raw(" stats   "),
            kb("F8"),
            Span::raw(" compact now   "),
            kb("F9"),
            Span::raw(" auto-compact"),
        ]),
        Line::from(vec![
            kb("F10"),
            Span::raw(" auto-retry   "),
            kb("?"),
            Span::raw(" this help"),
        ]),
        Line::default(),
        Line::from(vec![
            kb("PgUp/PgDn"),
            Span::raw(" scroll · "),
            kb("End"),
            Span::raw(" auto-follow"),
        ]),
    ])
}

fn stats_text(s: &SessionStats, t: &Theme) -> Text<'static> {
    let mut lines = vec![
        label_value("session", s.session_name_opt(), t),
        label_value(
            "messages",
            format!(
                "{} user · {} assistant · {} tools",
                s.user_messages, s.assistant_messages, s.tool_calls
            ),
            t,
        ),
        label_value(
            "tokens",
            format!(
                "in {} · out {} · cache R {} · cache W {} · total {}",
                s.tokens.input,
                s.tokens.output,
                s.tokens.cache_read,
                s.tokens.cache_write,
                s.tokens.total
            ),
            t,
        ),
        label_value("cost", format!("${:.4}", s.cost), t),
    ];
    if let Some(ctx) = &s.context_usage {
        lines.push(label_value(
            "context",
            format!(
                "{} / {} tokens ({}%)",
                ctx.tokens.unwrap_or(0),
                ctx.context_window,
                ctx.percent.map(|p| format!("{p:.0}")).unwrap_or_default()
            ),
            t,
        ));
    }
    if let Some(file) = &s.session_file {
        lines.push(label_value("file", file.clone(), t));
    }
    if let Some(id) = &s.session_id {
        lines.push(label_value("id", id.clone(), t));
    }
    Text::from(lines)
}

trait SessionStatsExt {
    fn session_name_opt(&self) -> String;
}
impl SessionStatsExt for SessionStats {
    fn session_name_opt(&self) -> String {
        self.session_id.clone().unwrap_or_else(|| "—".into())
    }
}

fn label_value(k: &str, v: impl Into<String>, t: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:>12}  ", k), Style::default().fg(t.accent)),
        Span::styled(v.into(), Style::default().fg(t.text)),
    ])
}

fn commands_text(list: &ListModal<CommandInfo>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&CommandInfo> = filtered_commands(&list.items, &list.query).collect();
    for (i, c) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let badge = match c.source {
            crate::rpc::types::CommandSource::Extension => "ext",
            crate::rpc::types::CommandSource::Prompt => "prompt",
            crate::rpc::types::CommandSource::Skill => "skill",
        };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let desc = c.description.as_deref().unwrap_or("");
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} /{} ", c.name), style),
            Span::styled(format!("[{badge}] "), Style::default().fg(t.warning)),
            Span::styled(desc.to_string(), Style::default().fg(t.dim)),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

fn models_text(list: &ListModal<Model>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&Model> = filtered_models(&list.items, &list.query).collect();
    for (i, m) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let cw = m
            .context_window
            .map(|c| format!(" · {}k ctx", c / 1000))
            .unwrap_or_default();
        let reasoning = if m.reasoning { " · reasoning" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(format!("{}/{}", m.provider, m.id), style),
            Span::styled(format!("{cw}{reasoning}"), Style::default().fg(t.dim)),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

fn thinking_text(radio: &RadioModal<ThinkingLevel>, t: &Theme) -> Text<'static> {
    let lines: Vec<Line<'static>> = radio
        .options
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let (marker, style) = if i == radio.selected {
                (
                    "◉",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                ("○", Style::default().fg(t.dim))
            };
            Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled((*label).to_string(), style),
            ])
        })
        .collect();
    Text::from(lines)
}

fn forks_text(list: &ListModal<ForkMessage>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&ForkMessage> = filtered_forks(&list.items, &list.query).collect();
    for (i, f) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(
                truncate_preview(&f.entry_id, 10),
                Style::default().fg(t.warning),
            ),
            Span::raw("  "),
            Span::styled(truncate_preview(&f.text.replace('\n', " ⏎ "), 200), style),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no fork candidates)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

fn history_text(list: &ListModal<HistoryEntry>, t: &Theme) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    let filtered: Vec<&HistoryEntry> = filtered_history(&list.items, &list.query).collect();
    for (i, e) in filtered.iter().enumerate() {
        let marker = if i == list.selected { "▸" } else { " " };
        let style = if i == list.selected {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let preview = truncate_preview(&e.text.replace('\n', " ⏎ "), 200);
        lines.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(preview, style),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
    }
    Text::from(lines)
}

fn ext_select_text(options: &[String], selected: usize, t: &Theme) -> Text<'static> {
    let lines: Vec<Line<'static>> = options
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let marker = if i == selected { "▸" } else { " " };
            let style = if i == selected {
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.text)
            };
            Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(o.clone(), style),
            ])
        })
        .collect();
    Text::from(lines)
}

fn ext_confirm_text(message: Option<&str>, selected: usize, t: &Theme) -> Text<'static> {
    let mut lines = Vec::new();
    if let Some(m) = message {
        lines.push(Line::from(Span::styled(
            m.to_string(),
            Style::default().fg(t.text),
        )));
        lines.push(Line::default());
    }
    let sel_yes = selected == 1;
    let yes_style = if sel_yes {
        Style::default()
            .bg(t.success)
            .fg(Color::Rgb(0, 0, 0))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.success).add_modifier(Modifier::DIM)
    };
    let no_style = if !sel_yes {
        Style::default()
            .bg(t.error)
            .fg(Color::Rgb(0, 0, 0))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.error).add_modifier(Modifier::DIM)
    };
    lines.push(Line::from(vec![
        Span::styled("  [ No ]  ", no_style),
        Span::raw("     "),
        Span::styled("  [ Yes ]  ", yes_style),
    ]));
    Text::from(lines)
}

fn ext_input_text(placeholder: Option<&str>, value: &str, t: &Theme) -> Text<'static> {
    let mut lines = Vec::new();
    if let Some(p) = placeholder {
        lines.push(Line::from(Span::styled(
            format!("({p})"),
            Style::default().fg(t.dim),
        )));
        lines.push(Line::default());
    }
    let display = if value.is_empty() {
        Line::from(vec![Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        )])
    } else {
        Line::from(vec![
            Span::styled(value.to_string(), Style::default().fg(t.text)),
            Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)),
        ])
    };
    lines.push(display);
    Text::from(lines)
}
