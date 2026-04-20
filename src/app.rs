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

use std::collections::VecDeque;
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
use ratatui::widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph, Sparkline, Wrap};

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
use crate::ui::cards::{Card, InlineRow};
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

/// High-level "what's pi doing right now" state for the StatusWidget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveState {
    Idle,
    Sending,
    Llm,
    Thinking,
    Tool,
    Streaming,
    Compacting,
    Retrying {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
    },
    Error,
}

impl LiveState {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Sending => "sending",
            Self::Llm => "llm",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Streaming => "streaming",
            Self::Compacting => "compacting",
            Self::Retrying { .. } => "retrying",
            Self::Error => "error",
        }
    }

    fn wants_spinner(self) -> bool {
        !matches!(self, Self::Idle | Self::Error)
    }
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

    // ── V2.2: focus mode (Ctrl+F toggles; j/k navigate cards) ────────────
    focus_idx: Option<usize>,

    // ── V2.1: live status signals ────────────────────────────────────────
    live: LiveState,
    live_since_tick: u64,
    tool_running: u32,
    tool_done: u32,
    tokens_this_sec: u32,
    throughput: VecDeque<u32>, // last 60 secs, one bucket per second
    cost_session: f64,
    cost_series: VecDeque<f64>, // last 30 turns
    last_event_tick: u64,
    turn_count: u32,
    last_sec_tick: u64,
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
            focus_idx: None,
            live: LiveState::Idle,
            live_since_tick: 0,
            tool_running: 0,
            tool_done: 0,
            tokens_this_sec: 0,
            throughput: VecDeque::with_capacity(60),
            cost_session: 0.0,
            cost_series: VecDeque::with_capacity(30),
            last_event_tick: 0,
            turn_count: 0,
            last_sec_tick: 0,
        }
    }

    fn set_live(&mut self, s: LiveState) {
        if self.live != s {
            self.live = s;
            self.live_since_tick = self.ticks;
        }
    }

    fn push_tokens(&mut self, n: u32) {
        self.tokens_this_sec = self.tokens_this_sec.saturating_add(n);
    }

    /// Called on every tick; rolls the per-second throughput bucket forward.
    fn tick_status(&mut self) {
        let sec = self.ticks / 10;
        if sec > self.last_sec_tick {
            for _ in self.last_sec_tick..sec {
                self.throughput
                    .push_back(std::mem::take(&mut self.tokens_this_sec));
                if self.throughput.len() > 60 {
                    self.throughput.pop_front();
                }
            }
            self.last_sec_tick = sec;
        }
    }

    fn event_received(&mut self) {
        self.last_event_tick = self.ticks;
    }

    fn elapsed_since_live(&self) -> Duration {
        let dt = self.ticks.saturating_sub(self.live_since_tick);
        Duration::from_millis(dt * 100)
    }

    fn recent_avg_throughput(&self) -> u32 {
        if self.throughput.is_empty() {
            return 0;
        }
        let sum: u32 = self.throughput.iter().sum();
        sum / self.throughput.len() as u32
    }

    fn heartbeat_color(&self) -> Color {
        let delta = self.ticks.saturating_sub(self.last_event_tick);
        if self.live == LiveState::Idle {
            self.theme.dim
        } else if delta < 3 {
            self.theme.success
        } else if delta < 100 {
            // fade toward warning after 10s of silence while streaming
            self.theme.warning
        } else {
            self.theme.error
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

    /// Ensure `focus_idx` stays within bounds as the transcript grows or is
    /// reset. Call after any transcript mutation.
    fn clamp_focus(&mut self) {
        if let Some(i) = self.focus_idx {
            let n = self.transcript.entries().len();
            if n == 0 {
                self.focus_idx = None;
            } else if i >= n {
                self.focus_idx = Some(n - 1);
            }
        }
    }

    fn on_event(&mut self, ev: Incoming) {
        self.event_received();
        match ev {
            Incoming::AgentStart => {
                self.is_streaming = true;
                self.set_live(LiveState::Llm);
            }
            Incoming::AgentEnd { .. } => {
                self.is_streaming = false;
                self.composer_mode = ComposerMode::Prompt;
                self.set_live(LiveState::Idle);
                self.tool_running = 0;
            }
            Incoming::TurnStart => {
                self.turn_count = self.turn_count.saturating_add(1);
                // Only emit a visible separator between turns (not before the
                // first). The marker reads as a graphical "turn N" divider
                // in the transcript.
                if self.turn_count > 1 {
                    self.transcript.push(Entry::TurnMarker {
                        number: self.turn_count,
                    });
                }
            }
            Incoming::TurnEnd {
                message: Some(AgentMessage::Assistant { usage: Some(u), .. }),
                ..
            } => {
                if let Some(c) = u.cost {
                    self.cost_session += c.total;
                    self.cost_series.push_back(c.total);
                    if self.cost_series.len() > 30 {
                        self.cost_series.pop_front();
                    }
                }
            }
            Incoming::MessageUpdate {
                assistant_message_event,
                ..
            } => match assistant_message_event {
                AssistantEvent::TextDelta { delta, .. } => {
                    self.set_live(LiveState::Streaming);
                    self.push_tokens(approx_tokens(&delta));
                    self.transcript.append_assistant(&delta);
                }
                AssistantEvent::ThinkingDelta { delta, .. } => {
                    self.set_live(LiveState::Thinking);
                    self.push_tokens(approx_tokens(&delta));
                    self.transcript.append_thinking(&delta);
                }
                AssistantEvent::Error { reason, .. } => {
                    self.set_live(LiveState::Error);
                    self.transcript
                        .push(Entry::Warn(format!("stream error: {reason:?}")));
                }
                _ => {}
            },
            Incoming::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => {
                self.tool_running = self.tool_running.saturating_add(1);
                self.set_live(LiveState::Tool);
                self.transcript.start_tool(tool_call_id, tool_name, args);
            }
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
            } => {
                self.tool_running = self.tool_running.saturating_sub(1);
                self.tool_done = self.tool_done.saturating_add(1);
                if self.tool_running == 0 && self.is_streaming {
                    self.set_live(LiveState::Llm);
                }
                self.transcript
                    .finish_tool(&tool_call_id, &result, is_error);
            }
            Incoming::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => {
                self.set_live(LiveState::Retrying {
                    attempt,
                    max_attempts,
                    delay_ms,
                });
                self.transcript.push_retry_waiting(
                    attempt,
                    max_attempts,
                    delay_ms,
                    error_message.unwrap_or_else(|| "transient error".into()),
                );
            }
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
                if success && self.is_streaming {
                    self.set_live(LiveState::Llm);
                } else {
                    self.set_live(LiveState::Idle);
                }
                self.transcript.resolve_retry(attempt, state);
            }
            Incoming::CompactionStart { reason } => {
                self.set_live(LiveState::Compacting);
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
                if self.is_streaming {
                    self.set_live(LiveState::Llm);
                } else {
                    self.set_live(LiveState::Idle);
                }
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

/// Very rough char→token approximation — ~4 chars/token for English. Only
/// used for the live throughput sparkline, not anything billed.
fn approx_tokens(s: &str) -> u32 {
    (s.chars().count() as u32).div_ceil(4)
}

/// Truncate a string to `max` characters, appending an ellipsis if cut.
fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    } else {
        s.to_string()
    }
}

/// Compact single-line preview of a tool's arguments.
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
                app.tick_status();
                app.clamp_focus();
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
                app.scroll = Some(cur.saturating_sub(4));
            }
            MouseEventKind::ScrollDown => {
                let cur = app.scroll.unwrap_or(0);
                app.scroll = Some(cur.saturating_add(4));
            }
            _ => {}
        },
        _ => {}
    }
}

/// Key handler active while focus mode is on. Navigate cards, Enter expands
/// tool cards, Esc exits.
fn handle_focus_key(code: KeyCode, _mods: KeyModifiers, app: &mut App) {
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
        KeyCode::Char('q') => {
            app.focus_idx = None;
        }
        _ => {}
    }
}

async fn handle_key(code: KeyCode, mods: KeyModifiers, app: &mut App, client: Option<&RpcClient>) {
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
            app.modal = Some(Modal::Commands(ListModal::new(
                "/ commands",
                "type to filter · Enter apply/insert · Esc close",
                merged_commands(&app.session.commands),
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
            app.scroll = Some(cur.saturating_sub(10));
        }
        (KeyCode::PageDown, _) => {
            let cur = app.scroll.unwrap_or(0);
            app.scroll = Some(cur.saturating_add(10));
        }
        (KeyCode::Home, _) => {
            app.scroll = Some(0);
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
                    // Built-in slash commands execute inline — no-argument
                    // commands (like /help, /stats, /export, /theme cycle,
                    // /new, /compact, /model, /think, /cycle-*, /copy,
                    // /auto-*) fire immediately; commands that typically need
                    // an argument still prefill so the user can type it.
                    if crate::ui::commands::is_builtin(&cmd) {
                        let needs_arg = matches!(cmd.name.as_str(), "rename" | "switch");
                        if needs_arg {
                            app.input.clear();
                            app.input.push('/');
                            app.input.push_str(&cmd.name);
                            app.input.push(' ');
                        } else {
                            // Close modal first, then dispatch locally.
                            app.modal = None;
                            let name = cmd.name.clone();
                            if !try_local_slash(app, &name, "") {
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

/// Built-ins first (so they're easy to discover), then pi's own commands.
fn merged_commands(pi_commands: &[CommandInfo]) -> Vec<CommandInfo> {
    let mut v = crate::ui::commands::builtins();
    v.extend_from_slice(pi_commands);
    v
}

/// Handle slash commands that do NOT need pi. Returns true if consumed.
fn try_local_slash(app: &mut App, name: &str, arg: &str) -> bool {
    match name {
        "help" => {
            app.modal = Some(Modal::Help);
            true
        }
        "stats" => {
            if let Some(stats) = &app.session.stats {
                app.modal = Some(Modal::Stats(Box::new(stats.clone())));
            } else {
                app.flash("no stats yet — try again once pi has responded");
            }
            true
        }
        "export" => {
            match crate::ui::export::export(&app.transcript) {
                Ok(p) => app.flash(format!("exported → {}", p.display())),
                Err(e) => app.flash(format!("export failed: {e}")),
            }
            true
        }
        "clear" => {
            app.transcript = Transcript::default();
            app.flash("transcript view cleared (pi session intact)");
            true
        }
        "theme" => {
            if arg.is_empty() {
                app.cycle_theme();
                app.flash(format!("theme → {}", app.theme.name));
            } else if app.set_theme_by_name(arg) {
                app.flash(format!("theme → {}", app.theme.name));
            } else {
                app.flash(format!(
                    "unknown theme: {arg} — try: {}",
                    theme::builtins()
                        .iter()
                        .map(|t| t.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            true
        }
        "themes" => {
            let items: Vec<CommandInfo> = theme::builtins()
                .iter()
                .map(|t| CommandInfo {
                    name: format!("theme {}", t.name),
                    description: Some(format!("switch to {}", t.name)),
                    source: crate::rpc::types::CommandSource::Builtin,
                    location: None,
                    path: None,
                })
                .collect();
            app.modal = Some(Modal::Commands(ListModal::new(
                "themes",
                "type to filter · Enter apply · Esc close",
                items,
            )));
            true
        }
        _ => false,
    }
}

/// Handle slash commands that DO need pi. Returns true if consumed.
async fn try_pi_slash(app: &mut App, client: &RpcClient, name: &str, arg: &str) -> bool {
    match name {
        "rename" => {
            if arg.is_empty() {
                app.flash("usage: /rename <name>");
                return true;
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
            true
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
            true
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
            true
        }
        "switch" => {
            if arg.is_empty() {
                app.flash("usage: /switch <session-file>");
                return true;
            }
            match client
                .call(RpcCommand::SwitchSession {
                    session_path: arg.to_string(),
                })
                .await
            {
                Ok(_) => {
                    app.flash(format!("switched → {arg}"));
                    bootstrap(client, app).await;
                }
                Err(e) => app.flash(format!("switch failed: {e}")),
            }
            true
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
                        return true;
                    }
                    app.modal = Some(Modal::Forks(ListModal::new(
                        "forks",
                        "type to filter · Enter fork · Esc close",
                        items,
                    )));
                }
                Err(e) => app.flash(format!("get_fork_messages failed: {e}")),
            }
            true
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
            true
        }
        "model" => {
            app.modal = Some(Modal::Models(ListModal::new(
                "model",
                "↑↓ pick · Enter set · Esc close",
                app.session.available_models.clone(),
            )));
            true
        }
        "think" => {
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
            true
        }
        "cycle-model" => {
            match client.call(RpcCommand::CycleModel).await {
                Ok(ok) => {
                    if let Some(v) = ok.data
                        && let Some(m) = v.get("model")
                        && let Some(prov) = m.get("provider").and_then(|x| x.as_str())
                        && let Some(id) = m.get("id").and_then(|x| x.as_str())
                    {
                        app.session.model_label = format!("{prov}/{id}");
                        app.flash(format!("model → {prov}/{id}"));
                    } else {
                        app.flash("cycled model");
                    }
                }
                Err(e) => app.flash(format!("cycle_model failed: {e}")),
            }
            true
        }
        "cycle-think" => {
            match client.call(RpcCommand::CycleThinkingLevel).await {
                Ok(_) => app.flash("cycled thinking level"),
                Err(e) => app.flash(format!("cycle_thinking_level failed: {e}")),
            }
            true
        }
        "auto-compact" => {
            let next = !app.session.auto_compaction.unwrap_or(true);
            app.session.auto_compaction = Some(next);
            let _ = client
                .fire(RpcCommand::SetAutoCompaction { enabled: next })
                .await;
            app.flash(format!("auto-compact {}", on_off(next)));
            true
        }
        "auto-retry" => {
            let next = !app.session.auto_retry.unwrap_or(true);
            app.session.auto_retry = Some(next);
            let _ = client
                .fire(RpcCommand::SetAutoRetry { enabled: next })
                .await;
            app.flash(format!("auto-retry {}", on_off(next)));
            true
        }
        "copy" => {
            match client.call(RpcCommand::GetLastAssistantText).await {
                Ok(ok) => {
                    let text = ok
                        .data
                        .as_ref()
                        .and_then(|v| v.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if text.is_empty() {
                        app.flash("no assistant message yet");
                    } else {
                        app.flash(format!(
                            "copied {} chars (clipboard wiring in V2.4)",
                            text.len()
                        ));
                    }
                }
                Err(e) => app.flash(format!("copy failed: {e}")),
            }
            true
        }
        _ => false,
    }
}

async fn submit(app: &mut App, client: Option<&RpcClient>) {
    let text = app.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    app.input.clear();
    app.history.record(&text);

    // 1) Local slash commands — these work even without pi connected.
    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        if try_local_slash(app, name, arg) {
            return;
        }
        // Local didn't handle it — if pi is here, try pi-requiring ones.
        if let Some(c) = client {
            if try_pi_slash(app, c, name, arg).await {
                return;
            }
        } else {
            app.flash(format!("unknown /{name} (pi is offline)"));
            return;
        }
    }

    // 2) Anything from here on needs pi.
    let Some(client) = client else {
        app.flash("pi is offline");
        return;
    };

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
    if !app.is_streaming {
        app.set_live(LiveState::Sending);
    }
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

    // StatusWidget takes a 4-row strip between the editor and footer, unless
    // the terminal is too short.
    let status_h: u16 = if area.height >= 20 { 4 } else { 0 };

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
    draw_body(f, body, app);
    if let Some(r) = above_rect {
        draw_widgets(f, r, &above_widgets);
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

    let line = Line::from(vec![
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
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ───────────────────────────────────────── transcript visuals (V2.2) ──
//
// Each `Entry` in the transcript turns into exactly one `Visual`: either a
// bordered `Card` (user / thinking / assistant / bash / tool) or an `InlineRow`
// (info / warn / error / compaction / retry). The `Visual` knows how to
// compute its own height at a given width and how to render itself into a
// `Rect`, which lets `draw_body` virtualize scroll without a scene graph.

enum Visual {
    Card(Card),
    Inline(InlineRow),
}

impl Visual {
    fn height(&self, outer_w: u16) -> u16 {
        match self {
            Self::Card(c) => c.height(outer_w),
            Self::Inline(r) => r.height(outer_w),
        }
    }

    fn render(&self, f: &mut ratatui::Frame, area: Rect, idx: usize, app: &App) {
        match self {
            Self::Card(c) => {
                let mut c = c.clone();
                c.focused = app.focus_idx == Some(idx);
                // Keep role color on the border; the Card renderer swaps to
                // BorderType::Double + prepends a ▶ marker when focused.
                c.render(f, area, &app.theme);
            }
            Self::Inline(r) => r.render(f, area),
        }
    }

    /// Render a visual into `target` when part of it is scrolled off the
    /// top (`skip > 0`) and/or the bottom extends below the viewport.
    ///
    /// Strategy: allocate a scratch `Buffer` sized to the visual's *full*
    /// height, render into it, then copy the visible rows into the frame.
    /// This gives correct output for both partial-top and partial-bottom
    /// cases — including the auto-follow "show the tail of a long card"
    /// case where the top of the card is off-screen but the last N rows
    /// (including the bottom border) must be visible.
    fn render_clipped(
        &self,
        f: &mut ratatui::Frame,
        target: Rect,
        idx: usize,
        app: &App,
        skip: u16,
        full_h: u16,
    ) {
        use ratatui::buffer::Buffer;
        let full = Rect::new(0, 0, target.width.max(1), full_h.max(1));
        let mut scratch = Buffer::empty(full);
        match self {
            Self::Card(c) => {
                let mut c = c.clone();
                c.focused = app.focus_idx == Some(idx);
                c.render_to_buffer(full, &mut scratch, &app.theme);
            }
            Self::Inline(r) => {
                use ratatui::widgets::{Paragraph, Widget, Wrap};
                Paragraph::new(r.lines.clone())
                    .wrap(Wrap { trim: false })
                    .render(full, &mut scratch);
            }
        }
        // Blit [skip .. skip + target.height] from scratch into the frame.
        let frame_buf = f.buffer_mut();
        for dy in 0..target.height {
            let src_y = skip.saturating_add(dy);
            if src_y >= full.height {
                break;
            }
            for dx in 0..target.width {
                let src_pos = (dx, src_y);
                let dst_pos = (target.x + dx, target.y + dy);
                let cell = scratch[src_pos].clone();
                frame_buf[dst_pos] = cell;
            }
        }
    }
}

fn build_visuals(app: &App) -> Vec<Visual> {
    let t = &app.theme;
    let mut out = Vec::with_capacity(app.transcript.entries().len());
    for e in app.transcript.entries() {
        match e {
            Entry::User(s) => out.push(Visual::Card(Card {
                icon: "❯",
                title: "you".into(),
                right_title: None,
                body: plain_paragraph(s, t.text),
                border_color: t.role_user,
                icon_color: t.role_user,
                title_color: t.role_user,
                focused: false,
            })),
            Entry::Thinking(s) => {
                if app.show_thinking {
                    let body = thinking_body(s, t);
                    let tokens = approx_tokens(s);
                    out.push(Visual::Card(Card {
                        icon: "✦",
                        title: "thinking".into(),
                        right_title: Some(format!("{tokens} tok")),
                        body,
                        border_color: t.role_thinking,
                        icon_color: t.role_thinking,
                        title_color: t.role_thinking,
                        focused: false,
                    }));
                } else {
                    let count = s.lines().count().max(1);
                    out.push(Visual::Inline(InlineRow {
                        lines: vec![Line::from(Span::styled(
                            format!("  ▸ thinking ({count} lines — Ctrl+T to reveal)"),
                            Style::default().fg(t.role_thinking),
                        ))],
                    }));
                }
            }
            Entry::Assistant(md) => {
                let body = markdown::render(md);
                out.push(Visual::Card(Card {
                    icon: "✦",
                    title: "pi".into(),
                    right_title: Some(app.session.model_label.clone()),
                    body: if body.is_empty() {
                        vec![Line::from(Span::styled("…", Style::default().fg(t.dim)))]
                    } else {
                        body
                    },
                    border_color: t.role_assistant,
                    icon_color: t.role_assistant,
                    title_color: t.role_assistant,
                    focused: false,
                }));
            }
            Entry::ToolCall(tc) => out.push(Visual::Card(tool_card(tc, t))),
            Entry::BashExec(bx) => out.push(Visual::Card(bash_card(bx, t))),
            Entry::Info(s) => out.push(Visual::Inline(InlineRow {
                lines: vec![Line::from(Span::styled(
                    format!("  · {s}"),
                    Style::default().fg(t.dim),
                ))],
            })),
            Entry::Warn(s) => out.push(Visual::Inline(InlineRow {
                lines: vec![Line::from(Span::styled(
                    format!("  ⚠ {s}"),
                    Style::default().fg(t.warning),
                ))],
            })),
            Entry::Error(s) => out.push(Visual::Inline(InlineRow {
                lines: vec![Line::from(Span::styled(
                    format!("  ✗ {s}"),
                    Style::default().fg(t.error),
                ))],
            })),
            Entry::Compaction(c) => out.push(Visual::Inline(InlineRow {
                lines: compaction_lines(c, t),
            })),
            Entry::Retry(r) => out.push(Visual::Inline(InlineRow {
                lines: retry_lines(r, t),
            })),
            Entry::TurnMarker { number } => {
                out.push(Visual::Inline(InlineRow {
                    lines: vec![
                        Line::default(),
                        Line::from(vec![
                            Span::styled("  ──────  ", Style::default().fg(t.dim)),
                            Span::styled(
                                format!("turn {number}"),
                                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                "  ──────────────────────────────────",
                                Style::default().fg(t.dim),
                            ),
                        ]),
                    ],
                }));
            }
        }
    }
    out
}

fn plain_paragraph(s: &str, color: Color) -> Vec<Line<'static>> {
    s.split('\n')
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(color))))
        .collect()
}

fn thinking_body(s: &str, t: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for l in s.split('\n') {
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(t.role_thinking)),
            Span::styled(
                l.to_string(),
                Style::default().fg(t.muted).add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
    lines
}

fn tool_card(tc: &ToolCall, t: &Theme) -> Card {
    let (status_icon, color, status_label) = match tc.status {
        ToolStatus::Running => ("⚙", t.role_tool, "running"),
        ToolStatus::Ok => ("✓", t.success, "ok"),
        ToolStatus::Err => ("✗", t.error, "error"),
    };
    let color = if tc.is_error { t.error } else { color };

    let body = build_tool_body(tc, t);

    let expand = if tc.expanded { "▾" } else { "▸" };
    // Right-title shows the primary arg summary when we can find one,
    // falling back to the status label.
    let right = primary_arg_chip(tc)
        .map(|s| truncate_preview(&s, 60))
        .unwrap_or_else(|| format!("{status_icon} {status_label}"));

    Card {
        icon: tool_family_icon(&tc.name),
        title: format!("{expand} {}", tc.name),
        right_title: Some(right),
        body,
        border_color: color,
        icon_color: color,
        title_color: color,
        focused: false,
    }
}

/// Dispatch tool body rendering by tool-family. Unknown tools fall back to
/// the generic args+out layout.
fn build_tool_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    match tool_family(&tc.name) {
        ToolFamily::Edit => build_edit_body(tc, t),
        ToolFamily::ReadFile => build_read_body(tc, t),
        ToolFamily::Grep => build_grep_body(tc, t),
        ToolFamily::Write => build_write_body(tc, t),
        ToolFamily::Bash => build_generic_body(tc, t),
        ToolFamily::Todo => build_todo_body(tc, t),
        ToolFamily::Generic => build_generic_body(tc, t),
    }
}

#[derive(Debug, Clone, Copy)]
enum ToolFamily {
    Bash,
    Edit,
    ReadFile,
    Grep,
    Write,
    Todo,
    Generic,
}

fn tool_family(name: &str) -> ToolFamily {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        "bash" | "run" | "shell" | "exec" | "command" => ToolFamily::Bash,
        "edit" | "apply_patch" | "str_replace" | "str_replace_editor" | "multi_edit" | "patch" => {
            ToolFamily::Edit
        }
        "read" | "read_file" | "readfile" | "view" | "cat" => ToolFamily::ReadFile,
        "grep" | "search" | "rg" | "ripgrep" => ToolFamily::Grep,
        "write" | "write_file" | "create" | "create_file" => ToolFamily::Write,
        "todo" | "todowrite" | "tasks" => ToolFamily::Todo,
        _ => ToolFamily::Generic,
    }
}

fn tool_family_icon(name: &str) -> &'static str {
    match tool_family(name) {
        ToolFamily::Bash => "$",
        ToolFamily::Edit => "±",
        ToolFamily::ReadFile => "▤",
        ToolFamily::Grep => "⌕",
        ToolFamily::Write => "✎",
        ToolFamily::Todo => "☐",
        _ => "⚙",
    }
}

/// Extract a human-readable chip for the card right-title from common args.
fn primary_arg_chip(tc: &ToolCall) -> Option<String> {
    let obj = tc.args.as_object()?;
    for k in ["file_path", "path", "filename", "file", "target"] {
        if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    for k in ["pattern", "query", "q"] {
        if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(format!("\"{}\"", truncate_preview(v, 40)));
        }
    }
    for k in ["command", "cmd"] {
        if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(truncate_preview(v, 40));
        }
    }
    None
}

fn build_generic_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let mut body: Vec<Line<'static>> = Vec::new();
    let arg_preview = args_preview(&tc.args);
    if !arg_preview.is_empty() {
        body.push(Line::from(vec![
            Span::styled("args  ", Style::default().fg(t.dim)),
            Span::styled(
                truncate_preview(&arg_preview, 300),
                Style::default().fg(t.muted),
            ),
        ]));
    }
    add_output_body(&mut body, tc, t);
    body_or_ellipsis(body, t)
}

fn build_edit_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let file_path = obj
        .and_then(|o| o.get("file_path").or_else(|| o.get("path")))
        .and_then(|v| v.as_str());
    let old_s = obj
        .and_then(|o| o.get("old_string").or_else(|| o.get("old")))
        .and_then(|v| v.as_str());
    let new_s = obj
        .and_then(|o| o.get("new_string").or_else(|| o.get("new")))
        .and_then(|v| v.as_str());

    let mut body: Vec<Line<'static>> = Vec::new();

    // Show a synthetic diff when we have old+new (common for str_replace).
    if let (Some(old_s), Some(new_s)) = (old_s, new_s) {
        let lang = file_path
            .and_then(|p| p.rsplit('.').next())
            .unwrap_or("")
            .to_string();
        body.push(Line::from(vec![
            Span::styled("file  ", Style::default().fg(t.dim)),
            Span::styled(
                file_path.unwrap_or("(inline)").to_string(),
                Style::default().fg(t.muted),
            ),
        ]));
        body.push(Line::default());
        for l in old_s.lines() {
            body.push(diff_body_line("-", l, &lang, t.diff_remove, t));
        }
        for l in new_s.lines() {
            body.push(diff_body_line("+", l, &lang, t.diff_add, t));
        }
        body.push(Line::default());
    } else if let Some(p) = file_path {
        body.push(Line::from(vec![
            Span::styled("file  ", Style::default().fg(t.dim)),
            Span::styled(p.to_string(), Style::default().fg(t.muted)),
        ]));
    }

    // Output: if it's a proper unified diff, use the diff widget. Else raw.
    let output = crate::ui::ansi::strip(&tc.output);
    if !output.trim().is_empty() {
        let show = tc.expanded || body.len() < 6;
        if show {
            body.push(Line::default());
            if crate::ui::diff::is_unified_diff(&output) {
                body.extend(crate::ui::diff::render(&output, t));
            } else {
                for part in output.split('\n').take(if tc.expanded { 400 } else { 8 }) {
                    body.push(Line::from(Span::styled(
                        part.to_string(),
                        Style::default().fg(t.muted),
                    )));
                }
            }
        } else {
            body.push(Line::from(Span::styled(
                "(Enter / Ctrl+E to see result)",
                Style::default().fg(t.dim),
            )));
        }
    }

    body_or_ellipsis(body, t)
}

fn build_read_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let file_path = obj
        .and_then(|o| o.get("file_path").or_else(|| o.get("path")))
        .and_then(|v| v.as_str());
    let mut body: Vec<Line<'static>> = Vec::new();
    if let Some(p) = file_path {
        body.push(Line::from(vec![
            Span::styled("path  ", Style::default().fg(t.dim)),
            Span::styled(p.to_string(), Style::default().fg(t.muted)),
        ]));
    }
    let output = crate::ui::ansi::strip(&tc.output);
    if tc.expanded && !output.trim().is_empty() {
        let lang = file_path
            .and_then(|p| p.rsplit('.').next())
            .unwrap_or("")
            .to_string();
        body.push(Line::default());
        let content = strip_line_numbers(&output);
        for l in crate::ui::syntax::highlight(&content, &lang)
            .into_iter()
            .take(400)
        {
            body.push(l);
        }
    } else if !output.is_empty() {
        let lines_total = output.lines().count();
        body.push(Line::from(Span::styled(
            format!("{lines_total} lines — Enter / Ctrl+E to view"),
            Style::default().fg(t.dim),
        )));
    }
    body_or_ellipsis(body, t)
}

fn build_grep_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let pattern = obj
        .and_then(|o| o.get("pattern").or_else(|| o.get("query")))
        .and_then(|v| v.as_str());
    let mut body: Vec<Line<'static>> = Vec::new();
    if let Some(p) = pattern {
        body.push(Line::from(vec![
            Span::styled("query  ", Style::default().fg(t.dim)),
            Span::styled(
                format!("\"{p}\""),
                Style::default()
                    .fg(t.role_tool)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let output = crate::ui::ansi::strip(&tc.output);
    if !output.trim().is_empty() {
        body.push(Line::default());
        let max = if tc.expanded { 400 } else { 8 };
        let mut last_file = String::new();
        for part in output.lines().take(max) {
            if let Some((file, rest)) = part.split_once(':') {
                if file != last_file {
                    body.push(Line::from(Span::styled(
                        file.to_string(),
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    )));
                    last_file = file.to_string();
                }
                body.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(rest.to_string(), Style::default().fg(t.muted)),
                ]));
            } else {
                body.push(Line::from(Span::styled(
                    part.to_string(),
                    Style::default().fg(t.muted),
                )));
            }
        }
    }
    body_or_ellipsis(body, t)
}

fn build_write_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let obj = tc.args.as_object();
    let file_path = obj
        .and_then(|o| o.get("file_path").or_else(|| o.get("path")))
        .and_then(|v| v.as_str());
    let content = obj
        .and_then(|o| o.get("content").or_else(|| o.get("file_text")))
        .and_then(|v| v.as_str());

    let mut body: Vec<Line<'static>> = Vec::new();
    if let Some(p) = file_path {
        body.push(Line::from(vec![
            Span::styled("path  ", Style::default().fg(t.dim)),
            Span::styled(p.to_string(), Style::default().fg(t.muted)),
        ]));
    }
    if let Some(c) = content {
        let lang = file_path
            .and_then(|p| p.rsplit('.').next())
            .unwrap_or("")
            .to_string();
        body.push(Line::default());
        let max = if tc.expanded { 400 } else { 6 };
        for l in crate::ui::syntax::highlight(c, &lang).into_iter().take(max) {
            body.push(l);
        }
        if !tc.expanded && c.lines().count() > 6 {
            body.push(Line::from(Span::styled(
                format!("(+{} more lines — Enter to expand)", c.lines().count() - 6),
                Style::default().fg(t.dim),
            )));
        }
    }
    body_or_ellipsis(body, t)
}

fn build_todo_body(tc: &ToolCall, t: &Theme) -> Vec<Line<'static>> {
    let mut body: Vec<Line<'static>> = Vec::new();
    let items = tc
        .args
        .as_object()
        .and_then(|o| o.get("todos").or_else(|| o.get("items")))
        .and_then(|v| v.as_array());
    if let Some(items) = items {
        for item in items.iter().take(20) {
            let text = item
                .get("content")
                .or_else(|| item.get("text"))
                .or_else(|| item.get("task"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no text)");
            let status = item
                .get("status")
                .or_else(|| item.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let (mark, color) = match status {
                "completed" | "done" => ("☑", t.success),
                "in_progress" | "active" => ("◐", t.warning),
                _ => ("☐", t.dim),
            };
            body.push(Line::from(vec![
                Span::styled(
                    format!("{mark} "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(text.to_string(), Style::default().fg(t.text)),
            ]));
        }
    } else {
        body.push(Line::from(Span::styled(
            "(no todo items)",
            Style::default().fg(t.dim),
        )));
    }
    body_or_ellipsis(body, t)
}

fn add_output_body(body: &mut Vec<Line<'static>>, tc: &ToolCall, t: &Theme) {
    if tc.expanded {
        let output = crate::ui::ansi::strip(&tc.output);
        if output.trim().is_empty() {
            body.push(Line::from(Span::styled(
                "(no output yet)",
                Style::default().fg(t.dim),
            )));
        } else if crate::ui::diff::is_unified_diff(&output) {
            body.push(Line::default());
            body.extend(crate::ui::diff::render(&output, t));
        } else {
            for part in output.split('\n').take(400) {
                body.push(Line::from(Span::styled(
                    part.to_string(),
                    Style::default().fg(t.muted),
                )));
            }
        }
    } else if !tc.output.trim().is_empty() {
        let preview = tc
            .output
            .lines()
            .next()
            .map(|s| truncate_preview(s, 200))
            .unwrap_or_default();
        body.push(Line::from(vec![
            Span::styled("out  ", Style::default().fg(t.dim)),
            Span::styled(preview, Style::default().fg(t.muted)),
        ]));
        if tc.output.lines().count() > 1 {
            body.push(Line::from(Span::styled(
                format!(
                    "(+{} more — Enter / Ctrl+E to expand)",
                    tc.output.lines().count() - 1
                ),
                Style::default().fg(t.dim),
            )));
        }
    }
}

fn body_or_ellipsis(body: Vec<Line<'static>>, t: &Theme) -> Vec<Line<'static>> {
    if body.is_empty() {
        vec![Line::from(Span::styled("…", Style::default().fg(t.dim)))]
    } else {
        body
    }
}

fn diff_body_line(prefix: &str, text: &str, lang: &str, color: Color, t: &Theme) -> Line<'static> {
    let spans = if lang.is_empty() {
        vec![Span::styled(text.to_string(), Style::default().fg(color))]
    } else {
        crate::ui::syntax::highlight(text, lang)
            .into_iter()
            .next()
            .map(|l| l.spans)
            .unwrap_or_else(|| vec![Span::styled(text.to_string(), Style::default().fg(color))])
    };
    let mut line_spans = vec![Span::styled(
        format!(" {prefix} "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];
    line_spans.extend(spans);
    let _ = t;
    Line::from(line_spans)
}

/// Drop common "  N→content" or "  N\tcontent" line-number prefixes that many
/// read_file tools emit, so syntect can tokenize clean source.
fn strip_line_numbers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let stripped = line
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ')
            .trim_start_matches(['\t', '→', '|', ':']);
        // If that ate everything, keep original.
        let keep = if stripped.is_empty() { line } else { stripped };
        out.push_str(keep);
        out.push('\n');
    }
    out
}

fn bash_card(bx: &BashExec, t: &Theme) -> Card {
    let (status_color, status_txt) = if bx.cancelled {
        (t.warning, "cancelled".to_string())
    } else if bx.exit_code == 0 {
        (t.success, format!("exit {}", bx.exit_code))
    } else {
        (t.error, format!("exit {}", bx.exit_code))
    };
    let body_text = crate::ui::ansi::strip(&bx.output);
    let mut body: Vec<Line<'static>> = Vec::new();
    for part in body_text.split('\n').take(300) {
        body.push(Line::from(Span::styled(
            part.to_string(),
            Style::default().fg(t.muted),
        )));
    }
    if body.is_empty() {
        body.push(Line::from(Span::styled(
            "(no output)",
            Style::default().fg(t.dim),
        )));
    }
    if bx.truncated {
        let path = bx
            .full_output_path
            .as_deref()
            .unwrap_or("(path not provided)");
        body.push(Line::from(Span::styled(
            format!("… truncated — full log: {path}"),
            Style::default().fg(t.warning),
        )));
    }

    Card {
        icon: "$",
        title: format!("$ {}", bx.command),
        right_title: Some(status_txt),
        body,
        border_color: status_color,
        icon_color: t.role_bash,
        title_color: t.role_bash,
        focused: false,
    }
}

fn compaction_lines(c: &Compaction, t: &Theme) -> Vec<Line<'static>> {
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
    vec![Line::from(vec![
        Span::styled(
            format!("  {sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
        Span::raw(" "),
        Span::styled(format!("({})", c.reason), Style::default().fg(t.dim)),
    ])]
}

fn retry_lines(r: &Retry, t: &Theme) -> Vec<Line<'static>> {
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
    vec![Line::from(vec![
        Span::styled(
            format!("  {sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
    ])]
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &App) {
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

    // Build one Visual per entry, skipping Thinking when hidden.
    let visuals = build_visuals(app);
    if visuals.is_empty() {
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

    // Heights at the content width (leaves room for the scrollbar column).
    let heights: Vec<u16> = visuals.iter().map(|v| v.height(content_w)).collect();
    let total_h: u16 = heights.iter().fold(0u16, |a, b| a.saturating_add(*b));
    let viewport = content.height;
    let max_offset = total_h.saturating_sub(viewport);

    // Resolve the scroll offset: auto-follow bottom OR follow focus OR user.
    let offset: u16 = if let Some(idx) = app.focus_idx {
        // Center the focused card in the viewport.
        let mut prefix: u16 = 0;
        for h in &heights[..idx.min(heights.len())] {
            prefix = prefix.saturating_add(*h);
        }
        let focused_h = heights.get(idx).copied().unwrap_or(0);
        let mid = prefix.saturating_add(focused_h / 2);
        mid.saturating_sub(viewport / 2).min(max_offset)
    } else {
        match app.scroll {
            None => max_offset,
            Some(v) => v.min(max_offset),
        }
    };

    // Render visuals that intersect [offset .. offset+viewport).
    let mut y_cursor: u16 = 0;
    let mut draw_y: u16 = content.y;
    let end_y = content.y + viewport;
    for (i, (v, h)) in visuals.iter().zip(heights.iter()).enumerate() {
        let top = y_cursor;
        let bottom = y_cursor.saturating_add(*h);
        y_cursor = bottom;
        if bottom <= offset {
            continue;
        }
        if top >= offset + viewport {
            break;
        }
        // How much of this visual's top is already scrolled off?
        let skip = offset.saturating_sub(top);
        let remaining_vertical = end_y.saturating_sub(draw_y);
        let draw_h = h.saturating_sub(skip).min(remaining_vertical);
        if draw_h == 0 {
            break;
        }
        let target = Rect::new(content.x, draw_y, content.width, draw_h);
        if skip == 0 && draw_h >= *h {
            // Card fully fits: draw normally.
            v.render(f, target, i, app);
        } else {
            // Either the top is scrolled off (skip > 0) or the card is
            // taller than the remaining viewport (bottom is cut). Render
            // the full card into a scratch buffer, then blit the visible
            // slice into the frame's buffer. Guarantees the tail of a
            // long streaming assistant card is always in view in auto-
            // follow mode.
            v.render_clipped(f, target, i, app, skip, *h);
            if skip > 0 {
                // Paint a subtle "continues above" hint on the first visible
                // row so the user knows there's earlier content.
                render_cutoff_hint(f, Rect::new(target.x, target.y, target.width, 1), t);
            }
        }
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

fn draw_scrollbar(
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
    } else if app.focus_idx.is_some() {
        vec![
            kb("j/k"),
            Span::raw(" nav · "),
            kb("Enter"),
            Span::raw(" expand · "),
            kb("g/G"),
            Span::raw(" top/bot · "),
            kb("Esc"),
            Span::raw(" exit · "),
            kb("Ctrl+C"),
            Span::raw(" quit"),
        ]
    } else {
        vec![
            kb("Enter"),
            Span::raw(" send · "),
            kb("/"),
            Span::raw(" cmds · "),
            kb("Ctrl+F"),
            Span::raw(" focus · "),
            kb("F5"),
            Span::raw(" model · "),
            kb("F6"),
            Span::raw(" think · "),
            kb("F7"),
            Span::raw(" stats · "),
            kb("Alt+T"),
            Span::raw(" theme · "),
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

    // For list-style modals, scroll the body so the selected row stays in
    // the viewport (centered where possible). The body layout puts the
    // filter on line 0, a blank on line 1, and items starting at line 2.
    let selected_line = match modal {
        Modal::Commands(l) => Some(2 + l.selected as u16),
        Modal::Models(l) => Some(2 + l.selected as u16),
        Modal::History(l) => Some(2 + l.selected as u16),
        Modal::Forks(l) => Some(2 + l.selected as u16),
        Modal::ExtSelect { selected, .. } => Some(*selected as u16),
        _ => None,
    };

    let body_owned = body;
    let total_lines = body_owned.lines.len() as u16;
    let viewport = inner.height;
    let scroll_y = if let Some(line) = selected_line {
        if total_lines > viewport {
            let half = viewport / 2;
            line.saturating_sub(half)
                .min(total_lines.saturating_sub(viewport))
        } else {
            0
        }
    } else {
        0
    };

    let main_area = if total_lines > viewport && inner.width > 2 {
        // Reserve a 1-column scrollbar track on the right.
        let w = inner.width.saturating_sub(1);
        let sbar = Rect::new(inner.x + w, inner.y, 1, inner.height);
        draw_scrollbar(f, sbar, scroll_y, viewport, total_lines, t);
        Rect::new(inner.x, inner.y, w, inner.height)
    } else {
        inner
    };

    f.render_widget(
        Paragraph::new(body_owned)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0)),
        main_area,
    );
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
            crate::rpc::types::CommandSource::Builtin => "builtin",
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
