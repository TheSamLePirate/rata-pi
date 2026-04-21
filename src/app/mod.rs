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
//!
//! Submodules:
//! - `visuals` · per-entry rendering cache (Visual, VisualsCache, fingerprint_entry)
//! - `draw`    · terminal chrome drawing (header, body, editor, footer, status)

mod draw;
mod visuals;

// `Visual`, `VisualsCache`, `update_visuals_cache` are used by prepare_frame_caches
// and build_one_visual.
use visuals::{Visual, VisualsCache, update_visuals_cache};
// `fingerprint_entry` is referenced only from the cache tests below.
#[cfg(test)]
use visuals::fingerprint_entry;

// The draw entry point is called from the ui_loop. `kb` and
// `draw_scrollbar` are also pulled up because the modal-body builders
// (help_text, commands_text, etc.) still live in this module.
use draw::{draw, draw_scrollbar, fmt_elapsed, kb};

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
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::cli::Args;
use crate::history::{History, HistoryEntry};
use crate::rpc::client::{self, RpcClient, RpcError};
use crate::rpc::commands::{ExtensionUiResponse, RpcCommand};
use crate::rpc::events::{AssistantEvent, Incoming};
use crate::rpc::types::{
    AgentMessage, AssistantBlock, CommandInfo, ContentBlock, FollowUpMode, ForkMessage, Model,
    SessionStats, State, SteeringMode, StopReason, ThinkingLevel, ToolResultPayload, UserContent,
};
use crate::theme::{self, Theme};
use crate::ui::cards::{Card, InlineRow};
use crate::ui::ext_ui::{ExtReq, ExtUiState, WidgetPlacement, parse as parse_ext};
use crate::ui::markdown;
use crate::ui::modal::{ListModal, Modal, RadioModal, centered, matches_query};
use crate::ui::transcript::{
    BashExec, Compaction, CompactionState, Entry, Retry, RetryState, ToolCall, ToolStatus,
    Transcript,
};

pub async fn run(args: Args) -> Result<()> {
    let caps = crate::term_caps::detect();
    install_panic_hook(caps.kitty_keyboard);
    enable_raw_mode()?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    if caps.kitty_keyboard {
        // Ask the terminal for disambiguated modifier + key-release events.
        // Lets Ctrl+Shift+T (and similar) actually arrive with both flags.
        use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        );
    }
    tracing::info!(
        kind = ?caps.kind,
        kitty_keyboard = caps.kitty_keyboard,
        graphics = caps.graphics,
        "terminal caps"
    );
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run_inner(&mut terminal, args).await;

    if caps.kitty_keyboard {
        use crossterm::event::PopKeyboardEnhancementFlags;
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
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

fn install_panic_hook(kitty: bool) {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if kitty {
            use crossterm::event::PopKeyboardEnhancementFlags;
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
        // V2.11 · persist the panic for post-mortem inspection.
        let crash_path = write_crash_dump(info);
        if let Some(path) = crash_path.as_deref() {
            eprintln!("rata-pi: crash dump written to {path}");
        }
        original(info);
    }));
}

/// Write a timestamped crash dump to the platform's state dir:
/// `~/.local/state/rata-pi/` on Linux (XDG), `~/Library/Application
/// Support/rata-pi/` on macOS, `%LOCALAPPDATA%\rata-pi\` on Windows.
/// Best-effort: returns the path on success, None on any failure.
fn write_crash_dump(info: &panic::PanicHookInfo<'_>) -> Option<String> {
    use std::io::Write as _;

    let dirs = directories::BaseDirs::new()?;
    // XDG state_dir is only populated on Linux; everywhere else fall back
    // to data_local_dir which is the conventional state location.
    let state = dirs
        .state_dir()
        .unwrap_or_else(|| dirs.data_local_dir())
        .join("rata-pi");
    std::fs::create_dir_all(&state).ok()?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = state.join(format!("crash-{ts}.log"));

    let mut f = std::fs::File::create(&path).ok()?;
    let _ = writeln!(f, "rata-pi crash · ts={ts}");
    let _ = writeln!(
        f,
        "version={} os={} arch={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let loc = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<no location>".into());
    let _ = writeln!(f, "location={loc}");
    let payload = info
        .payload()
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>");
    let _ = writeln!(f, "payload={payload}");
    let _ = writeln!(f);
    let bt = std::backtrace::Backtrace::force_capture();
    let _ = writeln!(f, "backtrace:\n{bt}");
    Some(path.display().to_string())
}

// ───────────────────────────────────────────────────────── state ──

/// Hit-test map populated by `draw_body`. Mouse handlers consult it to map
/// screen coordinates to transcript entries.
#[derive(Debug, Default, Clone)]
struct MouseMap {
    body_rect: Rect,
    /// `(y_start, y_end_exclusive, entry_idx)` for each visible card/row.
    visible: Vec<(u16, u16, usize)>,
    /// Rect of the "⬇ live tail" chip when visible.
    live_tail_chip: Option<Rect>,
}

impl MouseMap {
    fn clear(&mut self) {
        self.visible.clear();
        self.live_tail_chip = None;
    }

    fn entry_at(&self, x: u16, y: u16) -> Option<usize> {
        if !rect_contains(self.body_rect, x, y) {
            return None;
        }
        for &(y0, y1, idx) in &self.visible {
            if y >= y0 && y < y1 {
                return Some(idx);
            }
        }
        None
    }
}

fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

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
    composer: crate::composer::Composer,
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

    // ── V2.4: mouse hit-test map, refreshed on every draw ────────────────
    //
    // V2.11.3 · plain field (was RefCell<MouseMap>). The map is a
    // *product* of rendering: `draw` takes `&mut MouseMap` and populates
    // it; after the frame we stash it into this field so mouse handlers
    // have last-frame coordinates. No interior mutability needed.
    mouse_map: MouseMap,

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

    // ── V2.7: git status (refreshed on stats_tick) ───────────────────────
    git_status: Option<crate::git::GitStatus>,
    /// V2.11.1 · a `git status` child is already in flight; skip spawning
    /// another one until it resolves. Prevents stacking git processes on
    /// slow disks.
    git_refresh_inflight: bool,

    /// V2.11.1 · sender half of the file-walk channel. Populated once in
    /// `ui_loop`; `spawn_file_walk` clones it to deliver the completed
    /// `FileList`.
    file_walk_tx: Option<tokio::sync::mpsc::Sender<crate::files::FileList>>,

    /// V2.11.2 · per-entry rendered-visual + height cache. Keeps
    /// markdown/syntect/line_count work out of the draw hot path by only
    /// rebuilding slots whose fingerprint changed (or whose cached width
    /// no longer matches).
    visuals_cache: VisualsCache,

    // ── V2.10: vim mode toggle ──────────────────────────────────────────
    vim_enabled: bool,

    // ── V2.9: plan mode ─────────────────────────────────────────────────
    plan: crate::plan::Plan,
    /// Continue-prompt staged for automatic follow-up dispatch after
    /// `agent_end` fires during plan-mode auto-run.
    pending_auto_prompt: Option<String>,

    // ── V2.11: notifications ───────────────────────────────────────────
    /// Tick count when the current turn started (on `agent_start`). Used
    /// to compute duration for the agent-end desktop notification.
    agent_start_tick: Option<u64>,
    /// Tool calls observed during the current turn. Summarised in the
    /// agent-end notification body.
    tool_calls_this_turn: u32,
    /// Whether the user enabled desktop notifications. Default on; `/notify`
    /// toggles. Notifications still go through OSC 777 only unless the
    /// `notify` feature is compiled in.
    notify_enabled: bool,
}

impl App {
    fn new(spawn_error: Option<String>) -> Self {
        Self {
            transcript: Transcript::default(),
            composer: crate::composer::Composer::default(),
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
            mouse_map: MouseMap::default(),
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
            git_status: None,
            git_refresh_inflight: false,
            file_walk_tx: None,
            vim_enabled: false,
            plan: crate::plan::Plan::default(),
            pending_auto_prompt: None,
            agent_start_tick: None,
            tool_calls_this_turn: 0,
            notify_enabled: true,
            visuals_cache: VisualsCache::default(),
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

    /// Scan the most-recent assistant message for plan markers and apply
    /// them. Stages an automatic continue prompt into `pending_auto_prompt`
    /// when the plan is still running — the main loop dispatches it as a
    /// follow-up RPC right after handling the event.
    fn apply_plan_markers_on_agent_end(&mut self) {
        let Some(text) = self
            .transcript
            .entries()
            .iter()
            .rev()
            .find_map(|e| match e {
                Entry::Assistant(s) => Some(s.clone()),
                _ => None,
            })
        else {
            return;
        };

        // V2.12.g · detect an interview (marker → fenced block → bare
        // JSON, in that order) and open the modal. Strip the detected
        // range from the visible assistant card so the user doesn't see
        // the raw JSON, and push an Info row for a clear audit trail.
        // Skipped when we're already in a modal (pre-existing interview
        // or other blocker).
        if self.modal.is_none()
            && let Some((iv, ranges)) = crate::interview::detect_interview(&text)
        {
            let stripped = crate::interview::strip_ranges(&text, ranges);
            self.transcript.rewrite_last_assistant(stripped);
            let title = iv.title.clone();
            let n_fields = iv.fields.iter().filter(|f| f.is_interactive()).count();
            let plur = if n_fields == 1 { "" } else { "s" };
            let state = crate::interview::InterviewState::from_interview(iv);
            self.modal = Some(Modal::Interview(Box::new(state)));
            self.transcript.push(Entry::Info(format!(
                "✍ agent opened an interview: \"{title}\" ({n_fields} question{plur}) — answer and Ctrl+Enter to submit (Esc cancels)"
            )));
            self.flash(format!("interview · {title}"));
        }

        let mut advanced = false;
        let mut created_plan = false;
        for m in crate::plan::parse_markers(&text) {
            match m {
                crate::plan::Marker::PlanSet(items) => {
                    self.plan.set_all(items);
                    created_plan = true;
                    self.transcript.push(Entry::Info(format!(
                        "plan set by agent: {} steps",
                        self.plan.total()
                    )));
                }
                crate::plan::Marker::PlanAdd(text) => {
                    self.plan.add(text.clone());
                    self.transcript.push(Entry::Info(format!("plan + {text}")));
                }
                crate::plan::Marker::Done => {
                    if let Some(done_text) = self.plan.mark_done() {
                        self.transcript
                            .push(Entry::Info(format!("✓ step done: {done_text}")));
                        advanced = true;
                    }
                }
                crate::plan::Marker::Failed(reason) => {
                    if let Some(text) = self.plan.mark_fail(reason.clone()) {
                        self.transcript
                            .push(Entry::Warn(format!("✗ step failed: {text} — {reason}")));
                    }
                }
            }
        }

        // Queue a continue follow-up if the plan is still running.
        if self.plan.is_active() && self.plan.auto_run {
            if let Some(cur) = self.plan.current() {
                let attempts = cur.attempts;
                let step_text = cur.text.clone();
                let n = self.plan.current_idx().unwrap_or(0) + 1;
                if attempts >= crate::plan::MAX_ATTEMPTS {
                    self.plan.auto_run = false;
                    self.transcript.push(Entry::Warn(format!(
                        "step {n} stuck after {attempts} attempts — halting auto-run"
                    )));
                } else if advanced || created_plan || attempts == 0 {
                    // Advance OR first attempt of a fresh step — kick pi off.
                    self.pending_auto_prompt = Some(format!("Proceed with step {n}: {step_text}"));
                    self.plan.bump_attempt();
                } else {
                    // Agent didn't mark the step done. Nudge to continue.
                    self.pending_auto_prompt = Some(format!(
                        "Continue step {n} ({step_text}). When complete, end with [[STEP_DONE]]."
                    ));
                    self.plan.bump_attempt();
                }
            }
        } else if self.plan.all_done() {
            self.transcript
                .push(Entry::Info("plan complete ✓".to_string()));
        }
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
                self.agent_start_tick = Some(self.ticks);
                self.tool_calls_this_turn = 0;
            }
            Incoming::AgentEnd { messages } => {
                self.is_streaming = false;
                self.composer_mode = ComposerMode::Prompt;
                self.tool_running = 0;
                // V2.12.f · for non-retryable API failures (e.g. "Your
                // credit balance is too low"), pi sets stop_reason=Error
                // + error_message on the assistant message and closes
                // the turn with agent_end. No stream-error event fires
                // before this, so we MUST scan agent_end.messages for a
                // failed assistant entry and surface it. Otherwise the
                // user sees nothing at all.
                let mut had_error = false;
                for m in &messages {
                    if let AgentMessage::Assistant {
                        stop_reason: Some(StopReason::Error),
                        error_message,
                        ..
                    } = m
                    {
                        had_error = true;
                        let msg = error_message
                            .clone()
                            .unwrap_or_else(|| "agent ended with error".to_string());
                        self.transcript.push(Entry::Error(format!("pi: {msg}")));
                        if self.notify_enabled {
                            let body = truncate_preview(&msg, 100);
                            let _ = crate::notify::notify("pi · agent error", &body);
                        }
                        break;
                    }
                }
                if had_error {
                    self.set_live(LiveState::Error);
                } else {
                    self.set_live(LiveState::Idle);
                }
                // Plan-mode marker handling: scan the last assistant text
                // for [[STEP_DONE]] / [[STEP_FAILED]] / [[PLAN_SET]] /
                // [[PLAN_ADD]] and apply.
                self.apply_plan_markers_on_agent_end();
                if self.notify_enabled && !had_error {
                    let dur = self
                        .agent_start_tick
                        .map(|t0| self.ticks.saturating_sub(t0))
                        .unwrap_or(0);
                    // Notify only on turns that took ≥ 10 s (100 ticks)
                    // so quick round-trips don't spam the desktop.
                    if let Some((title, body)) =
                        crate::notify::agent_end_notice(dur, 100, self.tool_calls_this_turn)
                    {
                        let _ = crate::notify::notify(&title, &body);
                    }
                }
                self.agent_start_tick = None;
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
                AssistantEvent::Error { reason, error } => {
                    self.set_live(LiveState::Error);
                    // Pi sends the final failed AssistantMessage in the
                    // `error` field (was mis-named `partial` in older
                    // rata-pi — silent default-to-null was masking every
                    // API failure). Probe it for a human-readable message.
                    let detail = extract_error_detail(&error);
                    let msg = match detail {
                        Some(d) => format!("stream error ({reason:?}): {d}"),
                        None => format!("stream error: {reason:?}"),
                    };
                    self.transcript.push(Entry::Error(msg.clone()));
                    if self.notify_enabled {
                        let body = truncate_preview(&msg, 100);
                        let _ = crate::notify::notify("pi · stream error", &body);
                    }
                }
                _ => {}
            },
            Incoming::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => {
                self.tool_running = self.tool_running.saturating_add(1);
                self.tool_calls_this_turn = self.tool_calls_this_turn.saturating_add(1);
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
                if is_error && self.notify_enabled {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .next()
                        .unwrap_or("");
                    let first = text.lines().next().unwrap_or("").trim();
                    let body = if first.is_empty() {
                        "tool call failed".to_string()
                    } else {
                        truncate_preview(first, 80)
                    };
                    let _ = crate::notify::notify("pi · tool error", &body);
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
                    RetryState::Exhausted(final_error.clone().unwrap_or_else(|| "unknown".into()))
                };
                if success && self.is_streaming {
                    self.set_live(LiveState::Llm);
                } else {
                    self.set_live(LiveState::Idle);
                }
                if !success && self.notify_enabled {
                    let body = final_error
                        .as_deref()
                        .map(|s| truncate_preview(s, 80))
                        .unwrap_or_else(|| "retries exhausted".to_string());
                    let _ = crate::notify::notify("pi · retries exhausted", &body);
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
            // V2.12.f · pi rejected a fire-and-forget command (usually the
            // `prompt` RPC — "insufficient credits", "rate limit",
            // "context too large"). Surface it in the transcript instead
            // of silently dropping.
            Incoming::CommandError { command, message } => {
                self.is_streaming = false;
                self.composer_mode = ComposerMode::Prompt;
                self.set_live(LiveState::Error);
                self.tool_running = 0;
                self.transcript
                    .push(Entry::Error(format!("{command}: {message}")));
                if self.notify_enabled {
                    let body = truncate_preview(&message, 100);
                    let _ = crate::notify::notify(&format!("pi · {command} failed"), &body);
                }
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

/// If a plan is active, prepend its full context to the user prompt. If
/// not, append a short capability hint so pi knows about the markers.
fn wrap_with_plan(plan: &crate::plan::Plan, user_text: &str) -> String {
    if plan.is_active() {
        let mut out = plan.as_context();
        out.push_str("User request:\n");
        out.push_str(user_text);
        out
    } else {
        let mut out = String::with_capacity(user_text.len() + 400);
        out.push_str(user_text);
        out.push_str(crate::plan::capability_hint());
        // V2.12.g · let the agent know about the Interview marker too.
        out.push_str(crate::interview::capability_hint());
        out
    }
}

/// Dig a human-readable error message out of the `partial` payload that
/// pi carries alongside stream errors. Providers disagree on the field
/// name (`error`, `message`, nested `error.message` for Anthropic /
/// OpenAI schemas), so we probe a few likely paths and fall back to the
/// whole object stringified.
fn extract_error_detail(v: &serde_json::Value) -> Option<String> {
    if v.is_null() {
        return None;
    }
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    // Top-level string fields. `errorMessage` is the canonical field on
    // pi's AssistantMessage; the others catch provider-direct payloads.
    for key in [
        "errorMessage",
        "error_message",
        "error",
        "message",
        "detail",
        "reason",
    ] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    // Nested `error.message` / `error.type` (Anthropic, OpenAI).
    if let Some(err) = v.get("error") {
        if let Some(s) = err.as_str() {
            return Some(s.to_string());
        }
        if let Some(s) = err.get("message").and_then(|x| x.as_str()) {
            let ty = err.get("type").and_then(|x| x.as_str()).unwrap_or("");
            return Some(if ty.is_empty() {
                s.to_string()
            } else {
                format!("{ty}: {s}")
            });
        }
    }
    None
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
        ui_loop(terminal, &mut app, None, &mut offline_events()).await?;
    }
    Ok(())
}

async fn bootstrap(client: &RpcClient, app: &mut App) {
    // V2.11.2 · fire all bootstrap RPCs concurrently. They have no
    // dependencies on each other and each is a separate round-trip to pi,
    // so serial-await wasted ~N × RTT at startup. Stats refresh now joins
    // the party too.
    // V3.a · each sub-call is bounded at 3 s. A degraded pi (accepts input,
    // never responds) can no longer hang startup indefinitely — a missing
    // piece just degrades to the offline-default value for that slot.
    const BOOT_TIMEOUT: Duration = Duration::from_secs(3);
    let (state, messages, commands, models, stats) = tokio::join!(
        client.call_timeout(RpcCommand::GetState, BOOT_TIMEOUT),
        client.call_timeout(RpcCommand::GetMessages, BOOT_TIMEOUT),
        client.call_timeout(RpcCommand::GetCommands, BOOT_TIMEOUT),
        client.call_timeout(RpcCommand::GetAvailableModels, BOOT_TIMEOUT),
        client.call_timeout(RpcCommand::GetSessionStats, BOOT_TIMEOUT),
    );

    if let Ok(ok) = state
        && let Some(v) = ok.data
        && let Ok(s) = serde_json::from_value::<State>(v)
    {
        app.apply_state(&s);
    }
    if let Ok(ok) = messages
        && let Some(v) = ok.data
        && let Some(arr) = v.get("messages").and_then(|x| x.as_array())
    {
        let mut msgs: Vec<AgentMessage> = Vec::with_capacity(arr.len());
        for m in arr {
            if let Ok(msg) = serde_json::from_value::<AgentMessage>(m.clone()) {
                msgs.push(msg);
            }
        }
        import_messages(app, msgs);
    }
    if let Ok(ok) = commands
        && let Some(v) = ok.data
        && let Some(arr) = v.get("commands").and_then(|x| x.as_array())
    {
        app.session.commands = arr
            .iter()
            .filter_map(|m| serde_json::from_value::<CommandInfo>(m.clone()).ok())
            .collect();
    }
    if let Ok(ok) = models
        && let Some(v) = ok.data
        && let Some(arr) = v.get("models").and_then(|x| x.as_array())
    {
        app.session.available_models = arr
            .iter()
            .filter_map(|m| serde_json::from_value::<Model>(m.clone()).ok())
            .collect();
    }
    if let Ok(ok) = stats
        && let Some(v) = ok.data
        && let Ok(s) = serde_json::from_value::<SessionStats>(v)
    {
        app.session.stats = Some(s);
    }
    // Initial git status is fetched via the background task in `ui_loop`
    // (spawn_git_refresh is fired once immediately after bootstrap).
}

/// Viewport cap for the file-finder fuzzy-filter list. Keeping this
/// modest is a perf knob: even at 20k files, only the top 500 are scored
/// AND materialised into a Vec — the rest are ignored by `filter`.
const FILES_CAP: usize = 500;

/// Populate modal + transcript caches that the draw closure relies on.
/// Runs once per frame from the UI loop. The operations are idempotent —
/// they short-circuit when the inputs haven't changed.
///
/// `terminal_size` is the full terminal size, from which we derive the
/// transcript body's content-width (terminal-width minus 2 borders and
/// 1 scrollbar column). The body always spans full terminal width so
/// this math is exact.
fn prepare_frame_caches(app: &mut App, terminal_size: ratatui::layout::Size) {
    if let Some(Modal::Files(ff)) = app.modal.as_mut() {
        ff.refresh_filter(FILES_CAP);
        ensure_file_preview(ff);
    }
    let content_w = terminal_size.width.saturating_sub(3);
    update_visuals_cache(app, content_w);
}

/// Build the right-pane preview cache for the currently selected file,
/// IF it's missing or stale. Runs disk read + syntect ONCE per selection
/// change — not once per frame.
fn ensure_file_preview(ff: &mut crate::ui::modal::FileFinder) {
    let Some(path) = ff.current_path().map(str::to_string) else {
        ff.preview_cache = None;
        return;
    };
    if ff.preview_cache.as_ref().is_some_and(|c| c.path == path) {
        return;
    }
    let root = ff.files.root.clone();
    let lines = build_preview_lines(&root, &path);
    ff.preview_cache = Some(crate::ui::modal::PreviewCache { path, lines });
}

/// Pure function: read + highlight + assemble the preview rows. Produces
/// plain `Line` data with no theme dependency for the syntect portion; the
/// surrounding header/footer chrome is themed in `file_preview_lines` from
/// the live app theme.
fn build_preview_lines(root: &std::path::Path, path: &str) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    match crate::files::preview(root, path) {
        Some((text, lang)) => {
            let highlighted = crate::ui::syntax::highlight(&text, &lang);
            if highlighted.is_empty() {
                out.push(Line::from(Span::raw("(empty file)")));
            } else {
                out.extend(highlighted);
            }
        }
        None => {
            out.push(Line::from(Span::raw(
                "(preview unavailable — binary or too large)",
            )));
        }
    }
    out
}

/// Fire `git status` in the background; the result arrives via the
/// `git_rx` channel in the main loop. Fire-and-forget so the stats ticker
/// never blocks event polling.
fn spawn_git_refresh(app: &mut App, tx: &tokio::sync::mpsc::Sender<Option<crate::git::GitStatus>>) {
    if app.git_refresh_inflight {
        return;
    }
    app.git_refresh_inflight = true;
    let tx = tx.clone();
    tokio::spawn(async move {
        let st = crate::git::status().await;
        let result = if st.is_repo { Some(st) } else { None };
        let _ = tx.send(result).await;
    });
}

async fn refresh_stats(client: &RpcClient, app: &mut App) {
    // V3.a · 1 s bound. This runs every 5 s from the stats tick; if pi is
    // degraded we'd rather drop a refresh (and flash once) than stall the
    // whole event loop. Late responses are discarded by the reader because
    // `call_timeout` evicts the pending waiter on timeout.
    match client
        .call_timeout(RpcCommand::GetSessionStats, Duration::from_secs(1))
        .await
    {
        Ok(ok) => {
            if let Some(v) = ok.data
                && let Ok(s) = serde_json::from_value::<SessionStats>(v)
            {
                app.session.stats = Some(s);
            }
        }
        Err(RpcError::Timeout(_)) => {
            app.flash("pi didn't answer stats in 1s");
        }
        Err(_) => { /* Closed / Remote: other paths surface it. */ }
    }
}

/// Offline-mode event receiver: a dead channel. We drop the sender
/// immediately so the receiver resolves to `None` on first poll, but
/// crucially the branch `Some(msg) = events.recv()` never fires — the
/// offline UI loop is driven purely by ticks + crossterm input. A
/// stale TODO about replacing this with an `Option<Receiver>` would
/// save two lines but add plumbing; the one-cell buffer here is
/// effectively free.
fn offline_events() -> tokio::sync::mpsc::Receiver<Incoming> {
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
            app.composer.set_text(&text);
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

    // V2.11.1 · git status runs in a background task and delivers results
    // via this channel so the stats_tick branch never blocks. Buffer 4 is
    // plenty — the latest result wins and older ones can be coalesced.
    let (git_tx, mut git_rx) = tokio::sync::mpsc::channel::<Option<crate::git::GitStatus>>(4);
    // Fire once on boot so the header chip lights up without waiting for
    // the first 5-second tick.
    spawn_git_refresh(app, &git_tx);

    // V2.11.1 · file-walk channel: `spawn_file_walk` fires `walk_cwd()` on
    // a blocking thread and delivers the `FileList` here. The select
    // branch then pokes the active Files modal with `set_files`.
    let (file_walk_tx, mut file_walk_rx) = tokio::sync::mpsc::channel::<crate::files::FileList>(1);
    app.file_walk_tx = Some(file_walk_tx);

    // 30 fps soft cap on redraws. During heavy pi streaming we coalesce
    // bursts of events under a single render so the markdown / syntect /
    // virtualization work doesn't swamp the runtime.
    const MIN_FRAME: Duration = Duration::from_millis(33);
    let mut last_draw = tokio::time::Instant::now()
        .checked_sub(MIN_FRAME * 2)
        .unwrap_or_else(tokio::time::Instant::now);

    loop {
        if tokio::time::Instant::now().duration_since(last_draw) >= MIN_FRAME {
            // V2.11.1 · Build modal caches once per frame so draw(&App)
            // stays pure and the fuzzy matcher / file reader / syntect
            // never fire inside the render closure.
            // V2.11.2 · also refreshes the per-entry visuals + heights
            // cache so markdown/syntect/line_count stays out of draw.
            let size = terminal.size()?;
            prepare_frame_caches(app, size);
            // V2.11.3 · MouseMap is a frame-scoped output: draw populates
            // it, then we stash the completed map on App for the next
            // mouse-click handler. No more RefCell<MouseMap>.
            let mut mm = MouseMap::default();
            terminal.draw(|f| draw(f, app, &mut mm))?;
            app.mouse_map = mm;
            last_draw = tokio::time::Instant::now();
        }
        if app.quit {
            break;
        }

        tokio::select! {
            Some(msg) = events.recv() => {
                handle_incoming(msg, app, client).await;
                // Drain any additional buffered events so one redraw covers
                // a whole text_delta burst instead of re-rendering per token.
                for _ in 0..64 {
                    match events.try_recv() {
                        Ok(msg) => handle_incoming(msg, app, client).await,
                        Err(_) => break,
                    }
                }
                // Plan mode: if the last agent_end scheduled a continue
                // follow-up, fire it now via the current client.
                if let (Some(text), Some(c)) = (app.pending_auto_prompt.take(), client) {
                    let rpc = RpcCommand::Prompt {
                        message: wrap_with_plan(&app.plan, &text),
                        images: vec![],
                        streaming_behavior: None,
                    };
                    if let Err(e) = c.fire(rpc).await {
                        app.transcript.push(Entry::Error(format!(
                            "plan auto-run send failed: {e}"
                        )));
                    }
                }
            }
            maybe_ev = crossterm_events.next() => match maybe_ev {
                Some(Ok(ev)) => handle_crossterm(ev, app, client).await,
                Some(Err(e)) => {
                    // Terminal read error — log once and exit the loop. A
                    // TTY that died (SIGHUP, container detach) would
                    // otherwise keep returning Err and we'd spin on ticks.
                    tracing::error!(error = %e, "crossterm read failed — quitting");
                    app.quit = true;
                }
                None => {
                    // Stream ended (EOF on stdin). Exit gracefully.
                    tracing::info!("crossterm event stream ended — quitting");
                    app.quit = true;
                }
            },
            _ = tick.tick() => {
                app.ticks = app.ticks.wrapping_add(1);
                app.tick_status();
                app.clamp_focus();
                if let Some((_, at)) = app.flash
                    && app.ticks.wrapping_sub(at) > 15 {
                    app.flash = None;
                }
                app.ext_ui.expire_toasts(app.ticks);
            }
            _ = stats_tick.tick() => {
                if let Some(c) = client { refresh_stats(c, app).await; }
                spawn_git_refresh(app, &git_tx);
            }
            Some(result) = git_rx.recv() => {
                app.git_status = result;
                app.git_refresh_inflight = false;
            }
            Some(list) = file_walk_rx.recv() => {
                if let Some(Modal::Files(ff)) = app.modal.as_mut() {
                    ff.set_files(list);
                }
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
                    app.composer.insert_char(' ');
                } else {
                    app.composer.insert_char(ch);
                }
            }
            app.history.reset_walk();
        }
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: _,
        }) => match kind {
            MouseEventKind::ScrollUp => {
                let cur = app.scroll.unwrap_or(u16::MAX);
                app.scroll = Some(cur.saturating_sub(4));
            }
            MouseEventKind::ScrollDown => {
                let cur = app.scroll.unwrap_or(0);
                app.scroll = Some(cur.saturating_add(4));
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                on_mouse_click(column, row, app);
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
fn on_mouse_click(x: u16, y: u16, app: &mut App) {
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
                app.flash("nothing to copy yet");
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

async fn handle_modal_key(
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
            KeyCode::Esc | KeyCode::Enter => app.modal = None,
            _ => {}
        },
        Modal::Diff(d) => match code {
            KeyCode::Esc => app.modal = None,
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
                            Ok(_) => app.flash(format!("switched to {name}")),
                            Err(e) => app.flash(format!("switch failed: {e}")),
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
                                app.flash(format!("forked at {}", truncate_preview(&f.text, 40)));
                                bootstrap(c, app).await;
                            }
                            Err(e) => app.flash(format!("fork failed: {e}")),
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
    items: &'a [crate::ui::commands::MenuItem],
    q: &'a str,
) -> impl Iterator<Item = &'a crate::ui::commands::MenuItem> + 'a {
    items
        .iter()
        .filter(move |c| crate::ui::commands::matches(c, q))
}

fn filtered_count_commands(items: &[crate::ui::commands::MenuItem], q: &str) -> usize {
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

/// Insert a picked file reference into the composer.
///
/// `Insert` mode: append `@path` (with a leading space if the composer isn't
/// empty). `AtToken` mode: replace the current `@...` token — scan back from
/// the end of `app.input` to the last `@` and swap from there.
fn insert_file_ref(app: &mut App, path: &str, mode: crate::ui::modal::FilePickMode) {
    use crate::ui::modal::FilePickMode;
    let token = format!("@{path}");
    match mode {
        FilePickMode::Insert => {
            if app.composer.is_empty() {
                app.composer.set_text(&token);
            } else {
                if !app.composer.text().ends_with(' ') {
                    app.composer.insert_char(' ');
                }
                app.composer.insert_str(&token);
            }
        }
        FilePickMode::AtToken => {
            let full = app.composer.text();
            if let Some(pos) = full.rfind('@') {
                let mut new_text = full[..pos].to_string();
                new_text.push_str(&token);
                app.composer.set_text(&new_text);
            } else {
                app.composer.insert_str(&token);
            }
        }
    }
    app.history.reset_walk();
}

/// Vim normal-mode key dispatcher. Deliberately small subset — we rely
/// on the user falling back to insert mode for anything not covered.
fn handle_vim_normal(ch: char, app: &mut App) {
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

// ───────────────────────────────────────────────── interview mode ──

/// Process one keypress against the live interview modal state.
/// Returns `true` when the user pressed Ctrl+Enter on a valid form —
/// the caller then dispatches the response.
fn interview_key(
    state: &mut crate::interview::InterviewState,
    code: KeyCode,
    mods: KeyModifiers,
) -> bool {
    use crate::interview::FieldValue;

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
        (KeyCode::Right, m) if !m.contains(KeyModifiers::ALT) => {
            if *cursor < value.len() {
                *cursor = next_char_boundary(value, *cursor);
            }
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

fn try_submit_interview(state: &mut crate::interview::InterviewState) -> bool {
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
async fn dispatch_interview_response(
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

fn handle_plan_slash(app: &mut App, arg: &str) {
    let mut parts = arg.splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    match sub {
        "" | "show" => {
            app.modal = Some(Modal::PlanView);
        }
        "set" => {
            let items = crate::plan::Plan::parse_list(rest);
            if items.is_empty() {
                app.flash("usage: /plan set step1 | step2 | step3");
            } else {
                app.plan.set_all(items);
                app.flash(format!(
                    "plan set · {} steps · auto-run on",
                    app.plan.total()
                ));
            }
        }
        "add" => {
            if rest.is_empty() {
                app.flash("usage: /plan add <text>");
            } else {
                app.plan.add(rest.into());
                app.flash(format!("plan + \"{}\"", truncate_preview(rest, 40)));
            }
        }
        "done" => match app.plan.mark_done() {
            Some(t) => app.flash(format!("✓ step: {}", truncate_preview(&t, 40))),
            None => app.flash("no active step"),
        },
        "fail" => {
            let reason = if rest.is_empty() {
                "manual".to_string()
            } else {
                rest.to_string()
            };
            match app.plan.mark_fail(reason.clone()) {
                Some(t) => app.flash(format!("✗ step: {} — {reason}", truncate_preview(&t, 40))),
                None => app.flash("no active step"),
            }
        }
        "next" | "skip" => match app.plan.mark_done() {
            Some(t) => app.flash(format!("→ skipped: {}", truncate_preview(&t, 40))),
            None => app.flash("no active step"),
        },
        "clear" => {
            app.plan.clear();
            app.flash("plan cleared");
        }
        "auto" => match rest.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" | "yes" => {
                app.plan.auto_run = true;
                app.flash("plan auto-run ON");
            }
            "off" | "false" | "0" | "no" => {
                app.plan.auto_run = false;
                app.flash("plan auto-run OFF");
            }
            _ => app.flash("usage: /plan auto on | off"),
        },
        other => {
            app.flash(format!(
                "unknown /plan {other} — try set/add/done/fail/next/clear/auto/show"
            ));
        }
    }
}

fn open_file_finder(app: &mut App, query: String, mode: crate::ui::modal::FilePickMode) {
    // V2.11.1 · opening the modal is instant. The walk runs on a blocking
    // thread (see `spawn_file_walk`) and fills `files` asynchronously so
    // large repos no longer freeze the UI for the 200-500 ms walk.
    let ff = crate::ui::modal::FileFinder {
        title: "files".into(),
        hint: "type to filter · Enter inserts @path · Esc cancels".into(),
        files: crate::files::FileList::empty(),
        query,
        selected: 0,
        mode,
        loading: true,
        filtered: Vec::new(),
        filter_query: None,
        preview_cache: None,
    };
    app.modal = Some(Modal::Files(ff));
    spawn_file_walk(app);
}

/// Spawn the filesystem walk on a blocking thread and deliver the result
/// via `app.file_walk_rx`. Caller is expected to have already set a
/// `FileFinder` modal in `loading: true` state.
fn spawn_file_walk(app: &mut App) {
    let Some(tx) = app.file_walk_tx.clone() else {
        return;
    };
    tokio::task::spawn_blocking(move || {
        let list = crate::files::walk_cwd();
        let _ = tx.blocking_send(list);
    });
}

/// Copy `text` to the clipboard, reporting outcome via the transient flash.
fn do_copy(app: &mut App, text: &str) {
    match crate::clipboard::copy(text) {
        Ok(ok) => {
            let tag = match ok.backend {
                crate::clipboard::Backend::Arboard => "",
                crate::clipboard::Backend::Osc52 => " (osc52)",
            };
            app.flash(format!("✓ copied {} chars{tag}", ok.bytes));
        }
        Err(e) => app.flash(format!("copy failed: {e}")),
    }
}

/// Plain-text rendering of a transcript entry for clipboard copy.
fn entry_as_plain_text(e: &Entry) -> String {
    match e {
        Entry::User(s) => s.clone(),
        Entry::Thinking(s) => s.clone(),
        Entry::Assistant(s) => s.clone(),
        Entry::ToolCall(tc) => {
            let mut out = format!("# tool: {}\n", tc.name);
            if !tc.args.is_null() {
                out.push_str(&format!(
                    "args: {}\n",
                    serde_json::to_string_pretty(&tc.args).unwrap_or_else(|_| tc.args.to_string())
                ));
            }
            if !tc.output.is_empty() {
                out.push_str("---\n");
                out.push_str(&crate::ui::ansi::strip(&tc.output));
            }
            out
        }
        Entry::BashExec(bx) => {
            let mut out = format!("$ {}\n", bx.command);
            out.push_str(&crate::ui::ansi::strip(&bx.output));
            if bx.exit_code != 0 {
                out.push_str(&format!("\n[exit {}]", bx.exit_code));
            }
            out
        }
        Entry::Info(s) | Entry::Warn(s) | Entry::Error(s) => s.clone(),
        Entry::Compaction(c) => format!("compaction: {:?}", c.state),
        Entry::Retry(r) => format!("retry attempt {}", r.attempt),
        Entry::TurnMarker { number } => format!("--- turn {number} ---"),
    }
}

/// Built-ins first (so they're easy to discover), then pi's own commands.
fn merged_commands(pi_commands: &[CommandInfo]) -> Vec<crate::ui::commands::MenuItem> {
    crate::ui::commands::merged_menu(pi_commands)
}

/// Handle slash commands that do NOT need pi. Returns true if consumed.
async fn try_local_slash(app: &mut App, name: &str, arg: &str) -> bool {
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
            let names: Vec<&'static str> = theme::builtins().iter().map(|t| t.name).collect();
            app.modal = Some(Modal::Commands(ListModal::new(
                "themes",
                "type to filter · Enter apply · Esc close",
                crate::ui::commands::theme_items(names),
            )));
            true
        }
        "version" => {
            app.flash(format!("rata-pi v{}", env!("CARGO_PKG_VERSION")));
            true
        }
        "log" => {
            let path = crate::history::History::load()
                .path()
                .cloned()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(no log path)".into());
            app.flash(format!("log dir → see tracing file near: {path}"));
            true
        }
        "env" => {
            let term = std::env::var("TERM").unwrap_or_default();
            let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
            let caps = crate::term_caps::detect();
            app.flash(format!(
                "TERM={term} TERM_PROGRAM={term_program} kind={:?} kb={} gfx={}",
                caps.kind, caps.kitty_keyboard, caps.graphics
            ));
            true
        }
        "find" | "files" => {
            open_file_finder(app, arg.to_string(), crate::ui::modal::FilePickMode::Insert);
            true
        }
        "plan" => {
            handle_plan_slash(app, arg);
            true
        }
        "vim" => {
            app.vim_enabled = true;
            app.composer.mode = crate::composer::Mode::Insert;
            app.flash("vim mode on — Esc → NORMAL · i/a/o insert · hjkl/wb move");
            true
        }
        "default" | "emacs" => {
            app.vim_enabled = false;
            app.composer.mode = crate::composer::Mode::Insert;
            app.flash("default editor mode · Esc clears composer");
            true
        }
        // ── git (all local: we shell out to `git`) ───────────────────
        "status" => {
            let st = crate::git::status().await;
            app.modal = Some(Modal::GitStatus(Box::new(st)));
            true
        }
        "diff" => {
            let staged = arg.contains("--staged") || arg.contains("--cached");
            match crate::git::diff(staged).await {
                Ok(d) => {
                    app.modal = Some(Modal::Diff(crate::ui::modal::DiffView {
                        title: if staged {
                            "staged changes".into()
                        } else {
                            "working-tree changes".into()
                        },
                        staged,
                        diff: d,
                        scroll: 0,
                    }));
                }
                Err(e) => app.flash(format!("git diff failed: {e}")),
            }
            true
        }
        "git-log" => {
            let n: u32 = if arg.is_empty() {
                30
            } else {
                arg.parse().unwrap_or(30)
            };
            match crate::git::log(n).await {
                Ok(commits) => {
                    app.modal = Some(Modal::GitLog(crate::ui::modal::GitLogState {
                        commits,
                        selected: 0,
                    }));
                }
                Err(e) => app.flash(format!("git log failed: {e}")),
            }
            true
        }
        "branch" => {
            match crate::git::branches().await {
                Ok(bs) => {
                    app.modal = Some(Modal::GitBranch(crate::ui::modal::GitBranchState {
                        branches: bs,
                        query: String::new(),
                        selected: 0,
                    }));
                }
                Err(e) => app.flash(format!("git branch failed: {e}")),
            }
            true
        }
        "switch-branch" => {
            if arg.is_empty() {
                app.flash("usage: /switch-branch <name>");
            } else {
                match crate::git::switch(arg).await {
                    Ok(_) => app.flash(format!("switched to {arg}")),
                    Err(e) => app.flash(format!("switch failed: {e}")),
                }
            }
            true
        }
        "commit" => {
            if arg.is_empty() {
                app.flash("usage: /commit <message>");
            } else {
                match crate::git::commit_all(arg).await {
                    Ok(o) => {
                        let head = o.lines().next().unwrap_or("committed");
                        app.flash(format!("commit: {head}"));
                    }
                    Err(e) => app.flash(format!("commit failed: {e}")),
                }
            }
            true
        }
        "stash" => {
            match crate::git::stash().await {
                Ok(o) => app.flash(o.lines().next().unwrap_or("stashed").to_string()),
                Err(e) => app.flash(format!("stash failed: {e}")),
            }
            true
        }
        "doctor" => {
            app.modal = Some(Modal::Doctor(doctor_checks(app)));
            true
        }
        "mcp" => {
            app.modal = Some(Modal::Mcp(mcp_rows(app)));
            true
        }
        "shortcuts" | "keys" | "hotkeys" => {
            app.modal = Some(Modal::Shortcuts { scroll: 0 });
            true
        }
        "settings" | "prefs" | "preferences" => {
            app.modal = Some(Modal::Settings(crate::ui::modal::SettingsState::default()));
            true
        }
        "notify" => {
            app.notify_enabled = !app.notify_enabled;
            let state = if app.notify_enabled { "on" } else { "off" };
            let b = crate::notify::notify("pi · notifications", &format!("notifications {state}"));
            app.flash(format!("notifications {state} · backends: {}", b.label()));
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
                        do_copy(app, text);
                    }
                }
                Err(e) => app.flash(format!("copy failed: {e}")),
            }
            true
        }
        "retry" => {
            // Replay the most recent user prompt.
            let last_user = app.transcript.entries().iter().rev().find_map(|e| match e {
                Entry::User(s) => Some(s.clone()),
                _ => None,
            });
            if let Some(text) = last_user {
                let rpc = if app.is_streaming {
                    RpcCommand::Steer {
                        message: text,
                        images: vec![],
                    }
                } else {
                    RpcCommand::Prompt {
                        message: text,
                        images: vec![],
                        streaming_behavior: None,
                    }
                };
                if let Err(e) = client.fire(rpc).await {
                    app.flash(format!("retry failed: {e}"));
                } else {
                    app.flash("retried last prompt");
                }
            } else {
                app.flash("no previous user prompt");
            }
            true
        }
        "abort" => {
            let _ = client.fire(RpcCommand::Abort).await;
            app.flash("aborted");
            true
        }
        "abort-bash" => {
            let _ = client.fire(RpcCommand::AbortBash).await;
            app.flash("bash aborted");
            true
        }
        "abort-retry" => {
            let _ = client.fire(RpcCommand::AbortRetry).await;
            app.flash("retry aborted");
            true
        }
        "steer-mode" => {
            let mode = match arg {
                "all" => crate::rpc::types::SteeringMode::All,
                "one-at-a-time" => crate::rpc::types::SteeringMode::OneAtATime,
                _ => {
                    app.flash("usage: /steer-mode <all | one-at-a-time>");
                    return true;
                }
            };
            if let Err(e) = client.fire(RpcCommand::SetSteeringMode { mode }).await {
                app.flash(format!("steer-mode failed: {e}"));
            } else {
                app.session.steering_mode = Some(mode);
                app.flash(format!("steer-mode → {arg}"));
            }
            true
        }
        "follow-up-mode" => {
            let mode = match arg {
                "all" => crate::rpc::types::FollowUpMode::All,
                "one-at-a-time" => crate::rpc::types::FollowUpMode::OneAtATime,
                _ => {
                    app.flash("usage: /follow-up-mode <all | one-at-a-time>");
                    return true;
                }
            };
            if let Err(e) = client.fire(RpcCommand::SetFollowUpMode { mode }).await {
                app.flash(format!("follow-up-mode failed: {e}"));
            } else {
                app.session.follow_up_mode = Some(mode);
                app.flash(format!("follow-up-mode → {arg}"));
            }
            true
        }
        _ => false,
    }
}

async fn submit(app: &mut App, client: Option<&RpcClient>) {
    let text = app.composer.text().trim().to_string();
    if text.is_empty() {
        return;
    }
    app.composer.clear();
    app.history.record(&text);

    // 1) Local slash commands — these work even without pi connected.
    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        if try_local_slash(app, name, arg).await {
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
    // Wrap outgoing message with plan context / capability hint so pi's
    // LLM sees the plan (or knows the markers it can emit to create one).
    let wrapped = wrap_with_plan(&app.plan, &text);
    let rpc = match (app.is_streaming, app.composer_mode) {
        (false, _) => RpcCommand::Prompt {
            message: wrapped,
            images: vec![],
            streaming_behavior: None,
        },
        (true, ComposerMode::Steer | ComposerMode::Prompt) => RpcCommand::Steer {
            message: wrapped,
            images: vec![],
        },
        (true, ComposerMode::FollowUp) => RpcCommand::FollowUp {
            message: wrapped,
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

// ───────────────────────────────────────── transcript visuals (V2.2) ──
//
// Each `Entry` in the transcript turns into exactly one `Visual`: either a
// bordered `Card` (user / thinking / assistant / bash / tool) or an
// `InlineRow` (info / warn / error / compaction / retry). The rendering
// primitives (`Visual`, `Card`-clipping, `fingerprint_entry`, the
// `VisualsCache`) live in the `visuals` submodule; `build_one_visual`
// still lives in this module because the per-entry renderer depends on
// many local helper fns (tool_card, markdown::render, syntect) that
// aren't worth moving just yet.

/// V2.11.2 · render one Entry to its `Visual`. Factored out of the old
/// `build_visuals` so the visuals cache can rebuild individual slots
/// without re-rendering untouched neighbours.
///
/// `is_live_tail` is true for the single entry that is currently being
/// streamed — in that case we append the blinking block cursor and swap
/// the title chip to "pi · streaming". Non-tail assistant entries get a
/// stable "pi" title and no cursor, so their cached body never changes.
fn build_one_visual(entry: &Entry, app: &App, is_live_tail: bool) -> Visual {
    let t = &app.theme;
    match entry {
        Entry::User(s) => Visual::Card(Card {
            icon: "❯",
            title: "you".into(),
            right_title: None,
            body: plain_paragraph(s, t.text),
            border_color: t.role_user,
            icon_color: t.role_user,
            title_color: t.role_user,
        }),
        Entry::Thinking(s) => {
            if app.show_thinking {
                let body = thinking_body(s, t);
                let tokens = approx_tokens(s);
                Visual::Card(Card {
                    icon: "✦",
                    title: "thinking".into(),
                    right_title: Some(format!("{tokens} tok")),
                    body,
                    border_color: t.role_thinking,
                    icon_color: t.role_thinking,
                    title_color: t.role_thinking,
                })
            } else {
                let count = s.lines().count().max(1);
                Visual::Inline(InlineRow {
                    lines: vec![Line::from(Span::styled(
                        format!("  ▸ thinking ({count} lines — Ctrl+T to reveal)"),
                        Style::default().fg(t.role_thinking),
                    ))],
                })
            }
        }
        Entry::Assistant(md) => {
            let mut body = markdown::render(md);
            if is_live_tail {
                let cursor_on = (app.ticks / 5).is_multiple_of(2);
                let cursor_span = Span::styled(
                    if cursor_on { "▌" } else { " " },
                    Style::default()
                        .fg(t.role_assistant)
                        .add_modifier(Modifier::BOLD),
                );
                if let Some(last) = body.last_mut() {
                    last.spans.push(cursor_span);
                } else {
                    body.push(Line::from(cursor_span));
                }
            }
            Visual::Card(Card {
                icon: "✦",
                title: if is_live_tail {
                    "pi · streaming".into()
                } else {
                    "pi".into()
                },
                right_title: Some(app.session.model_label.clone()),
                body: if body.is_empty() {
                    vec![Line::from(Span::styled("…", Style::default().fg(t.dim)))]
                } else {
                    body
                },
                border_color: t.role_assistant,
                icon_color: t.role_assistant,
                title_color: t.role_assistant,
            })
        }
        Entry::ToolCall(tc) => Visual::Card(tool_card(tc, t)),
        Entry::BashExec(bx) => Visual::Card(bash_card(bx, t)),
        Entry::Info(s) => Visual::Inline(InlineRow {
            lines: vec![Line::from(Span::styled(
                format!("  · {s}"),
                Style::default().fg(t.dim),
            ))],
        }),
        Entry::Warn(s) => Visual::Inline(InlineRow {
            lines: vec![Line::from(Span::styled(
                format!("  ⚠ {s}"),
                Style::default().fg(t.warning),
            ))],
        }),
        Entry::Error(s) => Visual::Inline(InlineRow {
            lines: vec![Line::from(Span::styled(
                format!("  ✗ {s}"),
                Style::default().fg(t.error),
            ))],
        }),
        Entry::Compaction(c) => Visual::Inline(InlineRow {
            lines: compaction_lines(c, t),
        }),
        Entry::Retry(r) => Visual::Inline(InlineRow {
            lines: retry_lines(r, t),
        }),
        Entry::TurnMarker { number } => Visual::Inline(InlineRow {
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
        }),
    }
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
            // The actual list-width-aware body is rebuilt below once we
            // know the split rects. Use a placeholder here; the real text
            // is assigned after `list_area` is computed.
            Text::default(),
            list.hint.clone(),
            120,
            26,
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
        Modal::Files(ff) => (
            format!(" {} ", ff.title),
            Text::default(),
            ff.hint.clone(),
            120,
            26,
        ),
        Modal::Diff(d) => (
            format!(" {} ", if d.staged { "diff --staged" } else { "diff" }),
            Text::from(diff_body_lines(d, theme)),
            "PgUp/PgDn scroll · Esc close".to_string(),
            140,
            40,
        ),
        Modal::GitStatus(s) => (
            " git status ".to_string(),
            Text::from(git_status_body(s, theme)),
            "Esc close".to_string(),
            70,
            18,
        ),
        Modal::GitLog(state) => (
            " git log ".to_string(),
            Text::from(git_log_body(state, theme)),
            "↑↓ scroll · Esc close".to_string(),
            110,
            30,
        ),
        Modal::GitBranch(state) => (
            " branches ".to_string(),
            Text::from(git_branch_body(state, theme)),
            "type filter · Enter switch · Esc close".to_string(),
            70,
            24,
        ),
        Modal::PlanView => (
            " plan ".to_string(),
            Text::from(plan_full_lines(&app.plan, theme)),
            "/plan set | add | done | fail | clear · Esc close".to_string(),
            90,
            26,
        ),
        Modal::Doctor(checks) => (
            " doctor ".to_string(),
            Text::from(doctor_body(checks, theme)),
            "Esc close".to_string(),
            80,
            22,
        ),
        Modal::Mcp(rows) => (
            " mcp servers ".to_string(),
            Text::from(mcp_body(rows, theme)),
            "Esc close".to_string(),
            80,
            18,
        ),
        Modal::Interview(state) => (
            format!(" ✍ interview · {} ", state.title),
            Text::from(interview_body(state, theme)),
            "Tab/↓ next · Shift+Tab/↑ prev · PgUp/PgDn scroll · Space toggle · Enter send (on Submit) · Ctrl+S submit · Esc cancel".to_string(),
            110,
            32,
        ),
        Modal::Shortcuts { .. } => (
            " ⌨  shortcuts ".to_string(),
            Text::from(shortcuts_body(theme)),
            "↑↓/j/k scroll · PgUp/PgDn ±10 · g/G top/bot · Esc close".to_string(),
            100,
            32,
        ),
        Modal::Settings(state) => (
            " ⚙  settings ".to_string(),
            Text::from(settings_body(app, state, theme)),
            "↑↓ nav · Enter/Space toggle · ←/→ cycle · PgUp/PgDn ±5 · Esc close"
                .to_string(),
            110,
            32,
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

    // Two-pane layout when this is a Commands modal AND the terminal is
    // wide enough: left ~60% = list, right ~40% = detail pane.
    let (list_area, detail_area) = match modal {
        Modal::Commands(_) | Modal::Files(_) if inner.width >= 80 => {
            let left_w = (inner.width * 6) / 10;
            let right_w = inner.width - left_w - 1;
            let list_rect = Rect::new(inner.x, inner.y, left_w, inner.height);
            let rule_rect = Rect::new(inner.x + left_w, inner.y, 1, inner.height);
            let detail_rect = Rect::new(inner.x + left_w + 1, inner.y, right_w, inner.height);
            for dy in 0..inner.height {
                let r = Rect::new(rule_rect.x, rule_rect.y + dy, 1, 1);
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled("│", Style::default().fg(t.dim)))),
                    r,
                );
            }
            (list_rect, Some(detail_rect))
        }
        _ => (inner, None),
    };

    // Reserve the scrollbar column up front so we can build the body text
    // at the correct width (which lets `commands_text` truncate instead
    // of wrap). This keeps rendered-rows == source-rows so the scroll
    // cap lands precisely on the last item.
    let list_width_full = list_area.width;
    let text_width = list_width_full.saturating_sub(1).max(1);

    // Rebuild the Commands body now that we know the list width.
    let body_owned = match modal {
        Modal::Commands(l) => commands_text(l, t, text_width),
        Modal::Files(ff) => file_finder_text(ff, t, text_width),
        _ => body,
    };

    // For list-style modals, center the selected row.
    let selected_line = match modal {
        Modal::Commands(l) => Some(commands_selected_line(l)),
        Modal::Models(l) => Some(2 + l.selected as u16),
        Modal::History(l) => Some(2 + l.selected as u16),
        Modal::Forks(l) => Some(2 + l.selected as u16),
        Modal::Files(ff) => Some(2 + ff.selected as u16),
        Modal::GitLog(s) => Some(2 + s.selected as u16),
        Modal::ExtSelect { selected, .. } => Some(*selected as u16),
        // Interview: focus-follow auto-scroll. When the user manually
        // scrolled (PgUp/PgDn), their offset wins — we'll read it below
        // via the `state.user_scrolled` gate.
        Modal::Interview(state) if !state.user_scrolled => {
            let (_, focus_rows) = interview_body_and_focus_rows(state, theme);
            focus_rows.get(state.focus).copied()
        }
        // Settings: same focus-follow-on-select as list modals. When the
        // user PgUp/PgDn'd manually, their offset wins.
        Modal::Settings(state) if !state.user_scrolled => {
            let rows = build_settings_rows(app);
            settings_row_source_line(&rows, state.selected)
        }
        // Diff / Shortcuts modals use raw line-scroll (no selection).
        _ => None,
    };

    // Authoritative rendered-row count = ratatui's own wrap math.
    let source_lines = body_owned.lines.len() as u16;
    let rendered = Paragraph::new(body_owned.clone())
        .wrap(Wrap { trim: false })
        .line_count(text_width) as u16;
    let total_lines = rendered.max(source_lines);
    let viewport = inner.height;
    let max_scroll = total_lines.saturating_sub(viewport);
    let scroll_y = match modal {
        // Diff: free-form scroll, no selection. Clamp to end-of-content.
        Modal::Diff(d) => d.scroll.min(max_scroll),
        // Shortcuts: free-form scroll, no selection.
        Modal::Shortcuts { scroll } => (*scroll).min(max_scroll),
        // Interview: honor the user's explicit scroll when they moved
        // the viewport manually; otherwise auto-track focus.
        Modal::Interview(state) if state.user_scrolled => state.scroll.min(max_scroll),
        // Settings: same pattern as Interview.
        Modal::Settings(state) if state.user_scrolled => state.scroll.min(max_scroll),
        _ => match selected_line {
            Some(line) if total_lines > viewport => {
                let half = viewport / 2;
                line.saturating_sub(half).min(max_scroll)
            }
            _ => 0,
        },
    };

    // Render the scrollbar when needed and compute the main text rect.
    let main_area = if total_lines > viewport && list_area.width > 2 {
        let sbar = Rect::new(list_area.x + text_width, list_area.y, 1, list_area.height);
        draw_scrollbar(f, sbar, scroll_y, viewport, total_lines, t);
        Rect::new(list_area.x, list_area.y, text_width, list_area.height)
    } else {
        list_area
    };

    f.render_widget(
        Paragraph::new(body_owned)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0)),
        main_area,
    );

    // Detail pane (Commands modal only).
    if let (Some(da), Modal::Commands(list)) = (detail_area, modal) {
        let detail = command_detail_lines(list, t);
        f.render_widget(
            Paragraph::new(Text::from(detail)).wrap(Wrap { trim: false }),
            da,
        );
    }
    if let (Some(da), Modal::Files(ff)) = (detail_area, modal) {
        let detail = file_preview_lines(ff, t);
        f.render_widget(
            Paragraph::new(Text::from(detail)).wrap(Wrap { trim: false }),
            da,
        );
    }
}

/// Compute the terminal-row index of the selected item in `commands_text`.
/// Mirrors the category-grouping + description-line layout so the scroll
/// computation can keep the selected item centered.
/// Left-pane body for the FileFinder modal. Width-aware truncation so the
/// scroll math remains one-row-per-item.
fn file_finder_text(
    ff: &crate::ui::modal::FileFinder,
    t: &Theme,
    list_width: u16,
) -> Text<'static> {
    // Reads directly from `ff.filtered`, which `prepare_frame_caches`
    // keeps in sync with the query. No matcher runs inside this function.
    let mut lines: Vec<Line<'static>> = Vec::new();
    let scored = &ff.filtered;

    let hint_bits = if ff.loading {
        "   (indexing files…)".to_string()
    } else if ff.files.truncated {
        format!(
            "   ({} / {}+ files truncated)",
            scored.len(),
            crate::files::MAX_FILES
        )
    } else {
        format!("   ({} items)", scored.len())
    };

    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            ff.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(hint_bits, Style::default().fg(t.dim)),
    ]));
    lines.push(Line::default());

    if ff.loading && scored.is_empty() {
        lines.push(Line::from(Span::styled(
            "walking repo (respects .gitignore)…",
            Style::default().fg(t.dim),
        )));
        return Text::from(lines);
    }

    if scored.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
        return Text::from(lines);
    }

    let max = list_width as usize;
    for (i, (path, _score)) in scored.iter().enumerate() {
        let is_sel = i == ff.selected;
        let marker = if is_sel { "▸" } else { " " };
        let style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let display = if path.chars().count() + 4 > max {
            // Prefer showing the filename over the path prefix.
            let name = path.rsplit('/').next().unwrap_or(path);
            let prefix_len = max.saturating_sub(name.chars().count() + 5);
            if prefix_len >= 3 {
                let prefix: String = path.chars().take(prefix_len.saturating_sub(2)).collect();
                format!("{prefix}…/{name}")
            } else {
                truncate_preview(name, max.saturating_sub(2))
            }
        } else {
            path.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {marker} "), style),
            Span::styled(display, style),
        ]));
    }
    Text::from(lines)
}

/// Right-pane preview of the selected file: first ~40 lines, syntect-
/// highlighted by extension. Reads purely from `ff.preview_cache`, which
/// `prepare_frame_caches` populates on selection changes. The file system
/// is NOT touched inside this function.
fn file_preview_lines(ff: &crate::ui::modal::FileFinder, t: &Theme) -> Vec<Line<'static>> {
    let Some(cache) = ff.preview_cache.as_ref() else {
        let msg = if ff.loading {
            "(waiting for index…)"
        } else {
            "(no selection)"
        };
        return vec![Line::from(Span::styled(msg, Style::default().fg(t.dim)))];
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  ▤ ", Style::default().fg(t.accent)),
        Span::styled(
            cache.path.clone(),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::default());
    lines.extend(cache.lines.iter().cloned());
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "  Enter inserts @path · Esc closes",
        Style::default().fg(t.dim),
    )));
    lines
}

// ─────────────────────────────────────────────────────────── git bodies ──

fn diff_body_lines(d: &crate::ui::modal::DiffView, t: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    out.push(Line::from(vec![Span::styled(
        format!("  {}", d.title),
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )]));
    out.push(Line::default());
    if d.diff.trim().is_empty() {
        out.push(Line::from(Span::styled(
            "  (no changes)",
            Style::default().fg(t.dim),
        )));
        return out;
    }
    out.extend(crate::ui::diff::render(&d.diff, t));
    out
}

fn git_status_body(s: &crate::git::GitStatus, t: &Theme) -> Vec<Line<'static>> {
    if !s.is_repo {
        return vec![Line::from(Span::styled(
            "  (not a git repository)",
            Style::default().fg(t.dim),
        ))];
    }
    let mut out = Vec::new();
    let dirty_dot = if s.dirty() {
        Span::styled(
            " ●",
            Style::default().fg(t.warning).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" ○", Style::default().fg(t.success))
    };
    out.push(Line::from(vec![
        Span::styled(
            "  branch  ",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            s.branch.clone().unwrap_or_else(|| "(detached)".into()),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        dirty_dot,
    ]));
    out.push(Line::from(vec![Span::styled(
        format!("          ahead {} · behind {}", s.ahead, s.behind,),
        Style::default().fg(t.dim),
    )]));
    out.push(Line::default());
    out.push(status_row("staged", s.staged, t.diff_add, t));
    out.push(status_row("unstaged", s.unstaged, t.diff_remove, t));
    out.push(status_row("untracked", s.untracked, t.warning, t));
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  hint: /diff · /diff --staged · /log · /branch · /commit <msg> · /stash",
        Style::default().fg(t.dim),
    )));
    out
}

/// Build the readiness-check rows for the /doctor modal.
fn doctor_checks(app: &App) -> Vec<crate::ui::modal::DoctorCheck> {
    use crate::ui::modal::{DoctorCheck, DoctorStatus};
    let caps = crate::term_caps::detect();
    let mut rows = Vec::new();

    // pi on PATH
    let pi_path = which_pi();
    rows.push(DoctorCheck {
        label: "pi binary",
        status: if pi_path.is_some() {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Fail
        },
        detail: pi_path.unwrap_or_else(|| "not found on PATH".into()),
    });

    // pi connection live
    rows.push(DoctorCheck {
        label: "pi connection",
        status: if app.spawn_error.is_some() {
            DoctorStatus::Fail
        } else {
            DoctorStatus::Pass
        },
        detail: app
            .spawn_error
            .clone()
            .unwrap_or_else(|| "connected".into()),
    });

    // terminal
    rows.push(DoctorCheck {
        label: "terminal",
        status: DoctorStatus::Info,
        detail: format!("{:?}", caps.kind),
    });
    rows.push(DoctorCheck {
        label: "kitty keyboard",
        status: if caps.kitty_keyboard {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Warn
        },
        detail: if caps.kitty_keyboard {
            "enabled (Ctrl+Shift+T disambiguated)".into()
        } else {
            "not advertised (Alt+T / F12 fallbacks)".into()
        },
    });
    rows.push(DoctorCheck {
        label: "graphics",
        status: if caps.graphics {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Info
        },
        detail: if caps.graphics {
            "supported".into()
        } else {
            "no image protocol detected".into()
        },
    });

    // clipboard
    let cb_ok = arboard::Clipboard::new().is_ok();
    rows.push(DoctorCheck {
        label: "clipboard",
        status: if cb_ok {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Warn
        },
        detail: if cb_ok {
            "arboard (native)".into()
        } else {
            "OSC 52 fallback".into()
        },
    });

    // git
    let in_repo = app.git_status.as_ref().map(|s| s.is_repo).unwrap_or(false);
    rows.push(DoctorCheck {
        label: "git",
        status: if in_repo {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Info
        },
        detail: if in_repo {
            app.git_status
                .as_ref()
                .and_then(|s| s.branch.clone())
                .unwrap_or_else(|| "(detached)".into())
        } else {
            "not a git repository".into()
        },
    });

    // theme
    rows.push(DoctorCheck {
        label: "theme",
        status: DoctorStatus::Info,
        detail: app.theme.name.to_string(),
    });

    // notifications
    let notify_feat = cfg!(feature = "notify");
    rows.push(DoctorCheck {
        label: "notifications",
        status: if app.notify_enabled {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Info
        },
        detail: format!(
            "{} · osc777 always · native {}",
            if app.notify_enabled { "on" } else { "off" },
            if notify_feat {
                "feature enabled"
            } else {
                "feature disabled"
            }
        ),
    });

    rows
}

/// Check whether `pi` is resolvable on PATH; return the first match path.
fn which_pi() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("pi");
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Render doctor rows as styled lines.
fn doctor_body(checks: &[crate::ui::modal::DoctorCheck], t: &Theme) -> Vec<Line<'static>> {
    use crate::ui::modal::DoctorStatus;
    let mut out = Vec::with_capacity(checks.len() + 2);
    out.push(Line::default());
    for c in checks {
        let (glyph, color) = match c.status {
            DoctorStatus::Pass => ("✓", t.success),
            DoctorStatus::Warn => ("▲", t.warning),
            DoctorStatus::Fail => ("✗", t.error),
            DoctorStatus::Info => ("·", t.dim),
        };
        out.push(Line::from(vec![
            Span::styled(
                format!("  {glyph}  "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<16}", c.label),
                Style::default().fg(t.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(c.detail.clone(), Style::default().fg(t.muted)),
        ]));
    }
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  hint: /notify toggles · /mcp shows MCP servers · Esc closes",
        Style::default().fg(t.dim),
    )));
    out
}

/// Build MCP rows. pi 0.x does not currently expose MCP servers via the
/// JSONL RPC, so we ship a single informational row. Future-proof: when pi
/// adds `get_mcp_servers`, populate this list from its response.
fn mcp_rows(_app: &App) -> Vec<crate::ui::modal::McpRow> {
    use crate::ui::modal::{McpRow, McpStatus};
    vec![McpRow {
        name: "mcp info".into(),
        status: McpStatus::Unknown,
        detail: "pi does not expose MCP server state over RPC yet".into(),
    }]
}

fn mcp_body(rows: &[crate::ui::modal::McpRow], t: &Theme) -> Vec<Line<'static>> {
    use crate::ui::modal::McpStatus;
    let mut out = Vec::with_capacity(rows.len() + 2);
    out.push(Line::default());
    for r in rows {
        let (glyph, color) = match r.status {
            McpStatus::Connected => ("●", t.success),
            McpStatus::Disconnected => ("○", t.error),
            McpStatus::Unknown => ("·", t.dim),
        };
        out.push(Line::from(vec![
            Span::styled(
                format!("  {glyph}  "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<18}", r.name),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(r.detail.clone(), Style::default().fg(t.muted)),
        ]));
    }
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  hint: MCP servers are configured in pi's settings, not rata-pi",
        Style::default().fg(t.dim),
    )));
    out
}

/// Render the Interview modal body: header with title / description,
/// then one block per field. Focused interactive field gets a `▶`
/// marker + accent color; required-but-empty fields get a red chip.
/// V2.13.a · read-only keybinding reference. One entry per key; grouped
/// into sections that match the app's input surfaces.
fn shortcuts_body(t: &Theme) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();

    let section = |out: &mut Vec<Line<'static>>, title: &str| {
        out.push(Line::default());
        out.push(Line::from(vec![
            Span::styled(
                format!("  {title}  "),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(60), Style::default().fg(t.dim)),
        ]));
    };
    let row = |out: &mut Vec<Line<'static>>, keys: &str, action: &str| {
        out.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}  ", keys),
                Style::default()
                    .fg(t.accent_strong)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(action.to_string(), Style::default().fg(t.text)),
        ]));
    };

    out.push(Line::from(Span::styled(
        "  Every keyboard action rata-pi responds to.",
        Style::default().fg(t.muted),
    )));

    section(&mut out, "Global");
    row(&mut out, "Ctrl+C", "quit");
    row(&mut out, "Ctrl+D", "quit");

    section(&mut out, "Editor (idle — no modal)");
    row(&mut out, "Enter", "submit prompt");
    row(&mut out, "Shift+Enter / Ctrl+J", "insert newline");
    row(&mut out, "Esc (streaming)", "abort current run");
    row(&mut out, "Esc (empty composer)", "quit");
    row(&mut out, "Esc (composer has text)", "clear composer");
    row(&mut out, "Ctrl+F", "enter focus mode");
    row(&mut out, "Ctrl+T", "toggle thinking visibility");
    row(&mut out, "Alt+T / Ctrl+Shift+T / F12", "cycle theme");
    row(&mut out, "Ctrl+E", "toggle expand on last tool card");
    row(&mut out, "Ctrl+P", "fuzzy file finder");
    row(&mut out, "Ctrl+Y", "copy last assistant message");
    row(&mut out, "Ctrl+R", "prompt-history picker");
    row(&mut out, "Ctrl+S", "export transcript to markdown");
    row(&mut out, "Ctrl+Space (streaming)", "cycle composer intent");
    row(&mut out, "F1 / /", "commands modal");
    row(&mut out, "F5", "model picker");
    row(&mut out, "F6", "thinking-level picker");
    row(&mut out, "F7", "stats modal");
    row(&mut out, "F8", "compact context now");
    row(&mut out, "F9", "toggle auto-compaction");
    row(&mut out, "F10", "toggle auto-retry");
    row(&mut out, "?", "help modal");
    row(&mut out, "↑ / ↓", "prompt history prev / next");
    row(&mut out, "PgUp / PgDn", "scroll transcript ±5");
    row(&mut out, "End (empty composer)", "re-pin live tail");

    section(&mut out, "Composer editing");
    row(&mut out, "← / →", "cursor left / right");
    row(&mut out, "Alt+← / Alt+→", "word left / word right");
    row(&mut out, "Home / Ctrl+A", "start of line");
    row(&mut out, "End", "end of line (composer non-empty)");
    row(&mut out, "Backspace", "delete char before cursor");
    row(&mut out, "Delete", "delete char under cursor");
    row(&mut out, "Ctrl+U", "kill to start of line");
    row(&mut out, "Ctrl+K", "kill to end of line");
    row(&mut out, "Ctrl+W", "kill word back");

    section(&mut out, "Focus mode (Ctrl+F)");
    row(&mut out, "j / ↓", "next card");
    row(&mut out, "k / ↑", "previous card");
    row(&mut out, "g / Home", "first card");
    row(&mut out, "G / End", "last card");
    row(&mut out, "PgUp / PgDn", "±5 cards");
    row(&mut out, "Enter / Space", "toggle expand on tool card");
    row(&mut out, "y / c / Ctrl+Y", "copy focused card to clipboard");
    row(&mut out, "Esc / q", "exit focus mode");

    section(&mut out, "Modal — any");
    row(&mut out, "↑ / ↓", "move selection");
    row(&mut out, "PgUp / PgDn", "page ±5");
    row(&mut out, "Home / End", "first / last");
    row(&mut out, "Enter", "apply selection");
    row(&mut out, "Esc", "close modal");
    row(&mut out, "(printable key)", "append to filter query");
    row(&mut out, "Backspace", "delete from filter query");

    section(&mut out, "Vim mode (opt-in via /vim)");
    row(&mut out, "Esc", "Normal mode");
    row(
        &mut out,
        "i / a / I / A",
        "Insert at cursor / after / start / end",
    );
    row(&mut out, "o / O", "new line below / above + Insert");
    row(&mut out, "h j k l", "left / down / up / right");
    row(&mut out, "w / b", "word right / left");
    row(&mut out, "0 / $", "start / end of line");
    row(&mut out, "x", "delete char under cursor");
    row(&mut out, "dd", "delete line");
    row(&mut out, "gg / G", "top / bottom");

    section(&mut out, "Interview modal");
    row(&mut out, "Tab / ↓", "next field (incl. Submit button)");
    row(&mut out, "Shift+Tab / ↑", "previous field");
    row(&mut out, "← / →", "cycle select / move multiselect cursor");
    row(&mut out, "Alt+← / Alt+→", "word motion in text / number");
    row(&mut out, "Space", "toggle boolean / multiselect option");
    row(&mut out, "1..9", "on select, pick Nth option");
    row(&mut out, "Shift+Enter", "newline (multiline text only)");
    row(&mut out, "Enter (text / select)", "advance focus");
    row(&mut out, "Enter (Submit button)", "submit the form");
    row(&mut out, "Ctrl+S / Ctrl+Enter", "submit from anywhere");
    row(&mut out, "PgUp / PgDn", "scroll viewport ±10");
    row(&mut out, "Ctrl+Home / Ctrl+End", "scroll to top / bottom");
    row(&mut out, "Esc", "cancel interview");

    section(&mut out, "Mouse");
    row(&mut out, "wheel up / down", "scroll transcript");
    row(&mut out, "left click on card", "focus card");
    row(&mut out, "double-click tool card", "toggle expand");
    row(&mut out, "click ⬇ live-tail chip", "re-pin live tail");

    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  See /settings for runtime flags and state. See /help for a quick summary.",
        Style::default().fg(t.dim),
    )));

    out
}

// ───────────────────────────────────────── settings modal (V2.13.b) ──

/// One key applied to the Settings modal. Returns `(Some(action), _)`
/// when the user pressed Enter/Space/←/→ on an interactive row — the
/// caller dispatches it. `(_, true)` means close the modal (Esc).
fn settings_modal_key(
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
        KeyCode::Char('j') | KeyCode::Down => step_by(state, 1),
        KeyCode::Char('k') | KeyCode::Up => step_by(state, -1),
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
enum CycleDir {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy)]
enum SettingsAction {
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
async fn dispatch_settings_action(
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
                app.flash(format!("auto-compact {}", on_off(next)));
            } else {
                // V3.a · local flag flipped but there's no pi to persist it.
                // Warn the user so they don't think the setting stuck.
                app.flash(format!(
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
                app.flash(format!("auto-retry {}", on_off(next)));
            } else {
                app.flash(format!(
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
enum SettingsRow {
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
enum ToggleAction {
    ShowThinking,
    Notify,
    Vim,
    AutoCompact,
    AutoRetry,
    PlanAutoRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CycleAction {
    Theme,
    ThinkingLevel,
    SteeringMode,
    FollowUpMode,
    Model,
}

fn build_settings_rows(app: &App) -> Vec<SettingsRow> {
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
        value: which_pi().unwrap_or_else(|| "not on PATH".into()),
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
    let caps = crate::term_caps::detect();
    rows.push(R::Info {
        label: "terminal".into(),
        value: format!("{:?}", caps.kind),
    });
    rows.push(R::Info {
        label: "kitty keyboard".into(),
        value: if caps.kitty_keyboard {
            "advertised".into()
        } else {
            "not advertised (Alt+T / F12 fallback)".into()
        },
    });
    rows.push(R::Info {
        label: "graphics".into(),
        value: if caps.graphics {
            "supported".into()
        } else {
            "no image protocol detected".into()
        },
    });
    rows.push(R::Info {
        label: "clipboard".into(),
        value: if arboard::Clipboard::new().is_ok() {
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
    let hist = crate::history::History::load();
    rows.push(R::Info {
        label: "history file".into(),
        value: hist
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no history path)".into()),
    });
    let state_dir = directories::BaseDirs::new()
        .map(|d| {
            d.state_dir()
                .unwrap_or_else(|| d.data_local_dir())
                .join("rata-pi")
                .display()
                .to_string()
        })
        .unwrap_or_else(|| "(unknown)".into());
    rows.push(R::Info {
        label: "crash dumps".into(),
        value: state_dir,
    });

    rows.push(R::Header("Build"));
    rows.push(R::Info {
        label: "rata-pi".into(),
        value: env!("CARGO_PKG_VERSION").into(),
    });
    rows.push(R::Info {
        label: "platform".into(),
        value: format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH),
    });

    rows
}

fn mode_display(m: Option<String>) -> String {
    m.unwrap_or_else(|| "default".into())
        .to_ascii_lowercase()
        .replace('_', "-")
}

fn settings_row_source_line(rows: &[SettingsRow], selected: usize) -> Option<u16> {
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

fn settings_body(
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
            SettingsRow::Cycle { label, display, .. } => {
                let arrow_color = if focused { t.accent_strong } else { t.dim };
                let label_style = if focused {
                    Style::default().fg(t.text).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.muted)
                };
                out.push(Line::from(vec![
                    Span::styled(format!("  {marker} "), marker_style),
                    Span::styled(format!("{:<22}", label), label_style),
                    Span::styled(
                        "◂ ",
                        Style::default()
                            .fg(arrow_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        display.clone(),
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " ▸",
                        Style::default()
                            .fg(arrow_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
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

fn interview_body(state: &crate::interview::InterviewState, t: &Theme) -> Vec<Line<'static>> {
    interview_body_and_focus_rows(state, t).0
}

/// Build the modal body AND a parallel vector mapping each focus slot
/// (0..=fields.len()) to its starting source-line index. The draw path
/// uses that mapping to compute scroll offsets that keep the focused
/// field visible. Position `fields.len()` is the submit button.
fn interview_body_and_focus_rows(
    state: &crate::interview::InterviewState,
    t: &Theme,
) -> (Vec<Line<'static>>, Vec<u16>) {
    use crate::interview::FieldValue;

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

fn status_row(label: &str, count: u32, color: Color, t: &Theme) -> Line<'static> {
    let dot = if count > 0 { "●" } else { "○" };
    Line::from(vec![
        Span::styled(
            format!("  {:>10}  ", label),
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{dot} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{count}"), Style::default().fg(t.text)),
    ])
}

fn git_log_body(state: &crate::ui::modal::GitLogState, t: &Theme) -> Vec<Line<'static>> {
    if state.commits.is_empty() {
        return vec![Line::from(Span::styled(
            "  (no commits)",
            Style::default().fg(t.dim),
        ))];
    }
    let mut out: Vec<Line<'static>> = Vec::with_capacity(state.commits.len() + 2);
    out.push(Line::from(vec![Span::styled(
        format!("  {} commits", state.commits.len()),
        Style::default().fg(t.dim),
    )]));
    out.push(Line::default());
    for (i, c) in state.commits.iter().enumerate() {
        let is_sel = i == state.selected;
        let marker = if is_sel { "▸" } else { " " };
        let subject_style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        out.push(Line::from(vec![
            Span::styled(
                format!(" {marker} "),
                Style::default()
                    .fg(if is_sel { t.accent } else { t.dim })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<8}  ", c.hash),
                Style::default().fg(t.warning).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<20}  ", truncate_preview(&c.author, 20)),
                Style::default().fg(t.accent_strong),
            ),
            Span::styled(
                format!("{:<14}  ", truncate_preview(&c.date, 14)),
                Style::default().fg(t.dim),
            ),
            Span::styled(c.subject.clone(), subject_style),
        ]));
    }
    out
}

fn plan_full_lines(plan: &crate::plan::Plan, t: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if !plan.is_active() && !plan.all_done() && plan.total() == 0 {
        out.push(Line::from(Span::styled(
            "  (no plan)",
            Style::default().fg(t.dim),
        )));
        out.push(Line::default());
        out.push(Line::from(Span::styled(
            "  Tell the agent what to do and let it propose a plan, or run:",
            Style::default().fg(t.dim),
        )));
        out.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                "/plan set step 1 | step 2 | step 3",
                Style::default().fg(t.accent_strong),
            ),
        ]));
        return out;
    }

    out.push(Line::from(vec![
        Span::styled(
            "  ▸ progress  ",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} / {}", plan.count_done(), plan.total()),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if plan.auto_run {
                "   auto-run ●"
            } else {
                "   auto-run ○"
            },
            Style::default().fg(if plan.auto_run { t.success } else { t.dim }),
        ),
    ]));
    out.push(Line::default());
    for (i, it) in plan.items.iter().enumerate() {
        let (color, bold) = match it.status {
            crate::plan::Status::Done => (t.success, false),
            crate::plan::Status::Active => (t.accent, true),
            crate::plan::Status::Pending => (t.dim, false),
            crate::plan::Status::Failed => (t.error, true),
        };
        let mut style = Style::default().fg(color);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        out.push(Line::from(vec![
            Span::styled(format!("  {} {:>2}. ", it.status.marker(), i + 1), style),
            Span::styled(it.text.clone(), style),
        ]));
        if it.attempts > 0 && it.status == crate::plan::Status::Active {
            out.push(Line::from(Span::styled(
                format!(
                    "       attempts: {}/{}",
                    it.attempts,
                    crate::plan::MAX_ATTEMPTS
                ),
                Style::default().fg(t.dim),
            )));
        }
    }
    if let Some(r) = plan.fail_reason.as_deref() {
        out.push(Line::default());
        out.push(Line::from(vec![
            Span::styled(
                "  failure: ",
                Style::default().fg(t.error).add_modifier(Modifier::BOLD),
            ),
            Span::styled(r.to_string(), Style::default().fg(t.error)),
        ]));
    }
    out
}

/// Compact plan card shown above the editor while a plan is active.
fn plan_card(plan: &crate::plan::Plan, t: &Theme) -> crate::ui::cards::Card {
    use crate::ui::cards::Card;
    let mut body = Vec::new();
    // Show a focused window around the active step for the compact card.
    let active = plan.current_idx();
    for (i, it) in plan.items.iter().enumerate() {
        let (color, bold) = match it.status {
            crate::plan::Status::Done => (t.success, false),
            crate::plan::Status::Active => (t.accent, true),
            crate::plan::Status::Pending => (t.dim, false),
            crate::plan::Status::Failed => (t.error, true),
        };
        let mut style = Style::default().fg(color);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        let marker = it.status.marker();
        body.push(Line::from(vec![
            Span::styled(format!("{marker} "), style),
            Span::styled(it.text.clone(), style),
        ]));
        // Cap compact card height — if plan is long, show only 6 items
        // centered around active.
        if plan.items.len() > 6
            && let Some(a) = active
            && (i + 3 < a || i > a + 3)
        {
            body.pop();
        }
    }
    let right = format!(
        "{}/{} {}",
        plan.count_done(),
        plan.total(),
        if plan.auto_run { "·auto" } else { "" }
    );
    Card {
        icon: "◆",
        title: "plan".into(),
        right_title: Some(right),
        body,
        border_color: if plan.fail_reason.is_some() {
            t.error
        } else {
            t.accent
        },
        icon_color: t.accent,
        title_color: t.accent,
    }
}

fn git_branch_body(state: &crate::ui::modal::GitBranchState, t: &Theme) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            state.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
    ]));
    out.push(Line::default());
    let q = state.query.to_ascii_lowercase();
    let filtered: Vec<&crate::git::Branch> = state
        .branches
        .iter()
        .filter(|b| q.is_empty() || b.name.to_ascii_lowercase().contains(&q))
        .collect();
    if filtered.is_empty() {
        out.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
        return out;
    }
    for (i, b) in filtered.iter().enumerate() {
        let is_sel = i == state.selected;
        let marker = if is_sel { "▸" } else { " " };
        let chip = if b.current {
            Span::styled(
                " (current) ",
                Style::default().fg(t.success).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        };
        let style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        out.push(Line::from(vec![
            Span::styled(format!("  {marker} "), style),
            Span::styled(b.name.clone(), style),
            chip,
        ]));
    }
    out
}

fn commands_selected_line(list: &ListModal<crate::ui::commands::MenuItem>) -> u16 {
    let filtered: Vec<&crate::ui::commands::MenuItem> =
        filtered_commands(&list.items, &list.query).collect();
    // filter + blank line above the list.
    let mut line: u16 = 2;
    let mut last_cat: Option<crate::ui::commands::Category> = None;
    for (i, it) in filtered.iter().enumerate() {
        if last_cat != Some(it.category) {
            if last_cat.is_some() {
                line = line.saturating_add(1); // inter-group blank
            }
            line = line.saturating_add(1); // header row
            last_cat = Some(it.category);
        }
        if i == list.selected {
            return line;
        }
        line = line.saturating_add(1); // item row
        if !it.description.is_empty() {
            line = line.saturating_add(1); // description row
        }
    }
    line
}

fn help_text(t: &Theme) -> Text<'static> {
    Text::from(vec![
        Line::from(vec![
            kb("Enter", t),
            Span::raw(" submit   "),
            kb("Shift+Enter", t),
            Span::raw(" newline (deferred)"),
        ]),
        Line::from(vec![
            kb("!cmd", t),
            Span::raw(" bash RPC · "),
            kb("/", t),
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
            kb("Esc", t),
            Span::raw(" abort/clear/quit · "),
            kb("Ctrl+C", t),
            Span::raw(" quit"),
        ]),
        Line::from(vec![
            kb("Ctrl+R", t),
            Span::raw(" history search · "),
            kb("Ctrl+S", t),
            Span::raw(" export markdown · "),
            kb("↑/↓", t),
            Span::raw(" history"),
        ]),
        Line::default(),
        Line::from(vec![
            kb("Ctrl+T", t),
            Span::raw(" thinking · "),
            kb("Ctrl+Shift+T", t),
            Span::raw(" cycle theme · "),
            kb("/theme <name>", t),
            Span::raw(" pick"),
        ]),
        Line::from(vec![
            kb("Ctrl+E", t),
            Span::raw(" expand/collapse last tool card"),
        ]),
        Line::from(vec![
            kb("Ctrl+Space", t),
            Span::raw(" cycle composer mode (steer / follow-up)"),
        ]),
        Line::default(),
        Line::from(vec![
            kb("F1", t),
            Span::raw(" commands   "),
            kb("F5", t),
            Span::raw(" model   "),
            kb("F6", t),
            Span::raw(" thinking"),
        ]),
        Line::from(vec![
            kb("F7", t),
            Span::raw(" stats   "),
            kb("F8", t),
            Span::raw(" compact now   "),
            kb("F9", t),
            Span::raw(" auto-compact"),
        ]),
        Line::from(vec![
            kb("F10", t),
            Span::raw(" auto-retry   "),
            kb("?", t),
            Span::raw(" this help"),
        ]),
        Line::default(),
        Line::from(vec![
            kb("PgUp/PgDn", t),
            Span::raw(" scroll · "),
            kb("End", t),
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

/// Categorized, two-pane body text for the Commands modal.
///
/// Line layout:
///   line 0:  "filter: <query>    (N items)"
///   line 1:  blank
///   line 2+: list — category headers (not selectable; dim) and items.
///            Item lines include source badge + icon + name + args + description.
///
/// The caller (draw_modal) renders this `Text` in the LEFT pane and uses
/// `command_detail_lines` to populate the RIGHT pane with the focused
/// item's description + argument + example.
fn commands_text(
    list: &ListModal<crate::ui::commands::MenuItem>,
    t: &Theme,
    list_width: u16,
) -> Text<'static> {
    use crate::ui::commands::Category;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let filtered: Vec<&crate::ui::commands::MenuItem> =
        filtered_commands(&list.items, &list.query).collect();

    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(t.dim)),
        Span::styled(
            list.query.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   ({} items)", filtered.len()),
            Style::default().fg(t.dim),
        ),
    ]));
    lines.push(Line::default());

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(t.dim),
        )));
        return Text::from(lines);
    }

    let mut last_cat: Option<Category> = None;
    for (i, it) in filtered.iter().enumerate() {
        // Category divider on boundary.
        if last_cat != Some(it.category) {
            if last_cat.is_some() {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", it.category.icon()),
                    Style::default().fg(t.accent),
                ),
                Span::styled(
                    it.category.label().to_string(),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
            ]));
            last_cat = Some(it.category);
        }

        let is_sel = i == list.selected;
        let marker = if is_sel { "▸" } else { " " };
        let name_style = if is_sel {
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.text)
        };
        let badge = match it.source {
            crate::rpc::types::CommandSource::Extension => "ext",
            crate::rpc::types::CommandSource::Prompt => "prompt",
            crate::rpc::types::CommandSource::Skill => "skill",
            crate::rpc::types::CommandSource::Builtin => "builtin",
        };
        // Build the primary item row, truncated to the list pane width so
        // it never wraps — the scroll math needs rendered rows == source
        // rows for the scroll cap to land on the last item.
        let raw_main = format!("  {marker} /{}", it.name);
        let args_piece = if it.args.is_empty() {
            String::new()
        } else {
            format!(" {}", it.args)
        };
        let badge_piece = format!("   [{badge}]");
        // Budget: list_width cells total. The badge sits at the end; the
        // name + args occupy the rest. Truncate the name if necessary.
        let available = list_width as usize;
        let badge_len = badge_piece.chars().count();
        let args_len = args_piece.chars().count();
        let name_budget = available.saturating_sub(badge_len + args_len);
        let name_trunc = if raw_main.chars().count() > name_budget {
            truncate_preview(&raw_main, name_budget)
        } else {
            raw_main
        };
        let mut spans = vec![Span::styled(name_trunc, name_style)];
        if !args_piece.is_empty() {
            spans.push(Span::styled(args_piece, Style::default().fg(t.warning)));
        }
        spans.push(Span::styled(badge_piece, Style::default().fg(t.dim)));
        lines.push(Line::from(spans));
        // Description: indent 6, truncate to fit so it never wraps.
        if !it.description.is_empty() {
            let desc_w = (list_width as usize).saturating_sub(6);
            let desc_trunc = truncate_preview(&it.description, desc_w);
            lines.push(Line::from(Span::styled(
                format!("      {desc_trunc}"),
                Style::default().fg(t.dim),
            )));
        }
    }
    Text::from(lines)
}

/// Right-pane detail lines for the currently-selected item.
fn command_detail_lines(
    list: &ListModal<crate::ui::commands::MenuItem>,
    t: &Theme,
) -> Vec<Line<'static>> {
    let filtered: Vec<&crate::ui::commands::MenuItem> =
        filtered_commands(&list.items, &list.query).collect();
    let Some(it) = filtered.get(list.selected) else {
        return vec![Line::from(Span::styled(
            "(no selection)",
            Style::default().fg(t.dim),
        ))];
    };

    let mut lines = Vec::new();
    // Title row.
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", it.category.icon()),
            Style::default().fg(t.accent),
        ),
        Span::styled(
            format!("/{}", it.name),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if it.args.is_empty() {
                String::new()
            } else {
                format!(" {}", it.args)
            },
            Style::default().fg(t.warning),
        ),
    ]));
    // Category + source chip.
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {}", it.category.label()),
            Style::default().fg(t.dim),
        ),
        Span::styled(
            format!(
                " · {}",
                match it.source {
                    crate::rpc::types::CommandSource::Extension => "extension",
                    crate::rpc::types::CommandSource::Prompt => "prompt template",
                    crate::rpc::types::CommandSource::Skill => "skill",
                    crate::rpc::types::CommandSource::Builtin => "built-in",
                }
            ),
            Style::default().fg(t.muted),
        ),
    ]));
    lines.push(Line::default());

    // Description (wrapped).
    if !it.description.is_empty() {
        lines.push(Line::from(Span::styled(
            "description",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(it.description.clone(), Style::default().fg(t.text)),
        ]));
        lines.push(Line::default());
    }

    if !it.args.is_empty() {
        lines.push(Line::from(Span::styled(
            "arguments",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(it.args.to_string(), Style::default().fg(t.warning)),
        ]));
        lines.push(Line::default());
    }

    if !it.example.is_empty() {
        lines.push(Line::from(Span::styled(
            "example",
            Style::default().fg(t.dim).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(it.example.to_string(), Style::default().fg(t.accent_strong)),
        ]));
        lines.push(Line::default());
    }

    let action_hint = if it.is_theme() {
        "Enter applies the theme · Esc closes"
    } else if it.is_builtin() && it.args.is_empty() {
        "Enter runs it · Esc closes"
    } else if it.is_builtin() {
        "Enter prefills the composer · Esc closes"
    } else {
        "Enter prefills /name · Esc closes"
    };
    lines.push(Line::from(Span::styled(
        format!("  {action_hint}"),
        Style::default().fg(t.dim),
    )));

    lines
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

// ─────────────────────────────────────────────── visuals cache tests ──

#[cfg(test)]
mod visuals_cache_tests {
    use super::*;

    fn user(s: &str) -> Entry {
        Entry::User(s.into())
    }

    fn assistant(s: &str) -> Entry {
        Entry::Assistant(s.into())
    }

    #[test]
    fn fingerprint_equal_for_equal_content() {
        let a = fingerprint_entry(&user("hello"), false);
        let b = fingerprint_entry(&user("hello"), false);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_differs_on_appended_text() {
        // Assistant text grows; the fingerprint must change so the cache
        // rebuilds. We key on len(), so strictly monotonic growth works.
        let a = fingerprint_entry(&assistant("hi"), false);
        let b = fingerprint_entry(&assistant("hi there"), false);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_differs_across_variants() {
        // Two entries with equal body-len but different variants must not
        // share a fingerprint — we hash std::mem::discriminant first.
        let a = fingerprint_entry(&user("abc"), false);
        let b = fingerprint_entry(&assistant("abc"), false);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_respects_show_thinking() {
        // Thinking is rendered differently when show_thinking toggles,
        // so the key must include the flag.
        let e = Entry::Thinking("abc".into());
        let a = fingerprint_entry(&e, false);
        let b = fingerprint_entry(&e, true);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_covers_tool_status_transitions() {
        let mut tc = crate::ui::transcript::ToolCall {
            id: "x".into(),
            name: "bash".into(),
            args: serde_json::Value::Null,
            output: String::new(),
            status: crate::ui::transcript::ToolStatus::Running,
            is_error: false,
            expanded: false,
        };
        let running = fingerprint_entry(&Entry::ToolCall(tc.clone()), false);
        tc.status = crate::ui::transcript::ToolStatus::Ok;
        let ok = fingerprint_entry(&Entry::ToolCall(tc.clone()), false);
        tc.expanded = true;
        let expanded = fingerprint_entry(&Entry::ToolCall(tc), false);
        assert_ne!(running, ok);
        assert_ne!(ok, expanded);
    }
}

// ────────────────────────────────────── App::on_event reducer tests ──

#[cfg(test)]
mod reducer_tests {
    //! Drive the full state machine through scripted `Incoming` events.
    //!
    //! `App::on_event` is a pure `(State, Event) -> State` function — it
    //! doesn't touch the RPC client, the filesystem, or the terminal. So we
    //! construct a fresh `App`, feed it events, and assert against the
    //! resulting public state.
    //!
    //! These tests pin down the behaviors the UI depends on. Any regression
    //! in the live-status machine, the transcript stream-assembly, or the
    //! turn bookkeeping will show up here.
    use super::*;
    use crate::rpc::types::{
        AgentMessage, AssistantBlock, ContentBlock, Cost, ToolResultPayload, Usage,
    };

    fn app() -> App {
        let mut a = App::new(None);
        // Notifications emit OSC 777 to stdout; disable so test runs
        // don't dump escape sequences into the test harness output.
        a.notify_enabled = false;
        a
    }

    fn text_delta(s: &str) -> Incoming {
        Incoming::MessageUpdate {
            message: serde_json::Value::Null,
            assistant_message_event: AssistantEvent::TextDelta {
                content_index: 0,
                delta: s.into(),
                partial: serde_json::Value::Null,
            },
        }
    }

    fn thinking_delta(s: &str) -> Incoming {
        Incoming::MessageUpdate {
            message: serde_json::Value::Null,
            assistant_message_event: AssistantEvent::ThinkingDelta {
                content_index: 0,
                delta: s.into(),
                partial: serde_json::Value::Null,
            },
        }
    }

    fn tool_result_text(s: &str) -> ToolResultPayload {
        ToolResultPayload {
            content: vec![ContentBlock::Text { text: s.into() }],
            details: serde_json::Value::Null,
        }
    }

    // ── Agent lifecycle ──────────────────────────────────────────────────

    #[test]
    fn agent_start_sets_streaming_and_llm_state() {
        let mut a = app();
        assert!(!a.is_streaming);
        assert!(matches!(a.live, LiveState::Idle));
        a.on_event(Incoming::AgentStart);
        assert!(a.is_streaming);
        assert!(matches!(a.live, LiveState::Llm));
        assert_eq!(a.tool_calls_this_turn, 0);
        assert!(a.agent_start_tick.is_some());
    }

    #[test]
    fn agent_end_returns_to_idle_and_clears_tool_running() {
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(Incoming::ToolExecutionStart {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            args: serde_json::Value::Null,
        });
        assert_eq!(a.tool_running, 1);
        a.on_event(Incoming::AgentEnd { messages: vec![] });
        assert!(!a.is_streaming);
        assert_eq!(a.tool_running, 0);
        assert!(matches!(a.live, LiveState::Idle));
        assert_eq!(a.agent_start_tick, None);
    }

    // ── Turn bookkeeping ─────────────────────────────────────────────────

    #[test]
    fn turn_start_bumps_counter_without_first_divider() {
        let mut a = app();
        a.on_event(Incoming::TurnStart);
        assert_eq!(a.turn_count, 1);
        // No divider before turn 1.
        let has_marker = a
            .transcript
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::TurnMarker { .. }));
        assert!(!has_marker);
    }

    #[test]
    fn second_turn_start_pushes_divider() {
        let mut a = app();
        a.on_event(Incoming::TurnStart);
        a.on_event(Incoming::TurnStart);
        assert_eq!(a.turn_count, 2);
        let markers: Vec<u32> = a
            .transcript
            .entries()
            .iter()
            .filter_map(|e| match e {
                Entry::TurnMarker { number } => Some(*number),
                _ => None,
            })
            .collect();
        assert_eq!(markers, vec![2]);
    }

    #[test]
    fn turn_end_with_cost_accrues_session_total() {
        let mut a = app();
        let ev = Incoming::TurnEnd {
            message: Some(AgentMessage::Assistant {
                content: vec![AssistantBlock::Text { text: "hi".into() }],
                api: None,
                provider: None,
                model: None,
                usage: Some(Usage {
                    input: 100,
                    output: 10,
                    cache_read: 0,
                    cache_write: 0,
                    cost: Some(Cost {
                        total: 0.012,
                        ..Default::default()
                    }),
                }),
                stop_reason: None,
                error_message: None,
                timestamp: 0,
                entry_id: None,
            }),
            tool_results: vec![],
        };
        a.on_event(ev);
        assert!((a.cost_session - 0.012).abs() < 1e-9);
        assert_eq!(a.cost_series.back().copied(), Some(0.012));
    }

    // ── Message deltas → transcript ──────────────────────────────────────

    #[test]
    fn text_delta_accumulates_into_single_assistant_entry() {
        let mut a = app();
        a.on_event(text_delta("Hel"));
        a.on_event(text_delta("lo "));
        a.on_event(text_delta("world"));
        let last = a.transcript.entries().last().unwrap();
        assert!(matches!(last, Entry::Assistant(s) if s == "Hello world"));
        assert!(matches!(a.live, LiveState::Streaming));
    }

    #[test]
    fn thinking_delta_is_separate_entry_from_assistant_text() {
        let mut a = app();
        a.on_event(thinking_delta("hmm"));
        a.on_event(text_delta("ok"));
        let n = a.transcript.entries().len();
        assert_eq!(n, 2);
        assert!(matches!(
            a.transcript.entries()[0],
            Entry::Thinking(ref s) if s == "hmm"
        ));
        assert!(matches!(
            a.transcript.entries()[1],
            Entry::Assistant(ref s) if s == "ok"
        ));
        assert!(matches!(a.live, LiveState::Streaming));
    }

    #[test]
    fn stream_error_pushes_error_entry_and_sets_error_state() {
        // V2.12.f · this used to push Entry::Warn; now it's Entry::Error so
        // API failures land in the visible error channel.
        let mut a = app();
        a.on_event(Incoming::MessageUpdate {
            message: serde_json::Value::Null,
            assistant_message_event: AssistantEvent::Error {
                reason: crate::rpc::types::ErrorReason::Error,
                error: serde_json::Value::Null,
            },
        });
        assert!(matches!(a.live, LiveState::Error));
        assert!(matches!(
            a.transcript.entries().last(),
            Some(Entry::Error(_))
        ));
    }

    // ── Tool execution lifecycle ─────────────────────────────────────────

    #[test]
    fn tool_start_then_end_updates_counters_and_transcript() {
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(Incoming::ToolExecutionStart {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            args: serde_json::json!({"command": "ls"}),
        });
        assert_eq!(a.tool_running, 1);
        assert_eq!(a.tool_done, 0);
        assert_eq!(a.tool_calls_this_turn, 1);
        assert!(matches!(a.live, LiveState::Tool));
        a.on_event(Incoming::ToolExecutionEnd {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            result: tool_result_text("done"),
            is_error: false,
        });
        assert_eq!(a.tool_running, 0);
        assert_eq!(a.tool_done, 1);
        // Back to LLM since we were streaming.
        assert!(matches!(a.live, LiveState::Llm));
    }

    #[test]
    fn tool_end_when_not_streaming_stays_tool_not_llm() {
        let mut a = app();
        // No AgentStart — simulating the tool_result stage after agent_end.
        a.on_event(Incoming::ToolExecutionStart {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            args: serde_json::Value::Null,
        });
        a.on_event(Incoming::ToolExecutionEnd {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            result: tool_result_text("ok"),
            is_error: false,
        });
        // Live state isn't flipped back to LLM (we weren't streaming).
        assert!(!matches!(a.live, LiveState::Llm));
    }

    // ── Auto-retry ───────────────────────────────────────────────────────

    #[test]
    fn auto_retry_start_enters_retrying_and_records_entry() {
        let mut a = app();
        a.on_event(Incoming::AutoRetryStart {
            attempt: 1,
            max_attempts: 3,
            delay_ms: 500,
            error_message: Some("429 Too Many Requests".into()),
        });
        match a.live {
            LiveState::Retrying {
                attempt,
                max_attempts,
                delay_ms,
            } => {
                assert_eq!(attempt, 1);
                assert_eq!(max_attempts, 3);
                assert_eq!(delay_ms, 500);
            }
            _ => panic!("expected Retrying state"),
        }
        assert!(matches!(
            a.transcript.entries().last(),
            Some(Entry::Retry(_))
        ));
    }

    #[test]
    fn auto_retry_end_succeeded_while_streaming_goes_llm() {
        let mut a = app();
        a.is_streaming = true;
        a.on_event(Incoming::AutoRetryStart {
            attempt: 1,
            max_attempts: 3,
            delay_ms: 100,
            error_message: None,
        });
        a.on_event(Incoming::AutoRetryEnd {
            success: true,
            attempt: 1,
            final_error: None,
        });
        assert!(matches!(a.live, LiveState::Llm));
    }

    #[test]
    fn auto_retry_end_exhausted_goes_idle() {
        let mut a = app();
        a.on_event(Incoming::AutoRetryStart {
            attempt: 3,
            max_attempts: 3,
            delay_ms: 100,
            error_message: None,
        });
        a.on_event(Incoming::AutoRetryEnd {
            success: false,
            attempt: 3,
            final_error: Some("rate limited".into()),
        });
        assert!(matches!(a.live, LiveState::Idle));
    }

    // ── Compaction ───────────────────────────────────────────────────────

    #[test]
    fn compaction_start_and_end_flow() {
        let mut a = app();
        a.on_event(Incoming::CompactionStart {
            reason: crate::rpc::types::CompactionReason::Threshold,
        });
        assert!(matches!(a.live, LiveState::Compacting));
        assert!(matches!(
            a.transcript.entries().last(),
            Some(Entry::Compaction(_))
        ));
        a.on_event(Incoming::CompactionEnd {
            reason: crate::rpc::types::CompactionReason::Threshold,
            result: Some(crate::rpc::types::CompactionResult {
                summary: "summarised".into(),
                first_kept_entry_id: None,
                tokens_before: 10_000,
                details: serde_json::Value::Null,
            }),
            aborted: false,
            will_retry: false,
            error_message: None,
        });
        assert!(matches!(a.live, LiveState::Idle));
    }

    // ── Queue update ─────────────────────────────────────────────────────

    #[test]
    fn queue_update_replaces_session_queues() {
        let mut a = app();
        a.on_event(Incoming::QueueUpdate {
            steering: vec!["steer A".into(), "steer B".into()],
            follow_up: vec!["follow-up X".into()],
        });
        assert_eq!(a.session.queue_steering, vec!["steer A", "steer B"]);
        assert_eq!(a.session.queue_follow_up, vec!["follow-up X"]);
    }

    // ── Extension errors ─────────────────────────────────────────────────

    #[test]
    fn extension_error_pushes_error_entry() {
        let mut a = app();
        a.on_event(Incoming::ExtensionError {
            extension_path: Some("/ext.js".into()),
            event: Some("init".into()),
            error: Some("boom".into()),
        });
        let last = a.transcript.entries().last().unwrap();
        assert!(matches!(last, Entry::Error(s) if s.contains("boom")));
    }

    // ── Command errors (V2.12.f) — API failures, rate limits, etc. ────────

    #[test]
    fn command_error_pushes_error_and_clears_streaming() {
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        assert!(a.is_streaming);
        a.on_event(Incoming::CommandError {
            command: "prompt".into(),
            message: "insufficient credits".into(),
        });
        assert!(!a.is_streaming);
        assert!(matches!(a.live, LiveState::Error));
        assert_eq!(a.tool_running, 0);
        let last = a.transcript.entries().last().unwrap();
        match last {
            Entry::Error(s) => {
                assert!(s.contains("prompt"), "got {s}");
                assert!(s.contains("insufficient credits"), "got {s}");
            }
            other => panic!("expected Entry::Error, got {other:?}"),
        }
    }

    #[test]
    fn stream_error_extracts_anthropic_nested_message() {
        let mut a = app();
        a.on_event(Incoming::MessageUpdate {
            message: serde_json::Value::Null,
            assistant_message_event: AssistantEvent::Error {
                reason: crate::rpc::types::ErrorReason::Error,
                error: serde_json::json!({
                    "error": {
                        "type": "invalid_request_error",
                        "message": "Your credit balance is too low to access the Claude API."
                    }
                }),
            },
        });
        assert!(matches!(a.live, LiveState::Error));
        let last = a.transcript.entries().last().unwrap();
        match last {
            Entry::Error(s) => {
                assert!(s.contains("credit balance is too low"), "got {s}");
                assert!(s.contains("invalid_request_error"), "got {s}");
            }
            other => panic!("expected Entry::Error, got {other:?}"),
        }
    }

    #[test]
    fn stream_error_falls_back_on_unknown_partial_shape() {
        let mut a = app();
        a.on_event(Incoming::MessageUpdate {
            message: serde_json::Value::Null,
            assistant_message_event: AssistantEvent::Error {
                reason: crate::rpc::types::ErrorReason::Error,
                error: serde_json::Value::Null,
            },
        });
        let last = a.transcript.entries().last().unwrap();
        assert!(matches!(last, Entry::Error(s) if s.contains("stream error")));
    }

    #[test]
    fn extract_error_detail_handles_top_level_message() {
        let v = serde_json::json!({"message": "rate limited"});
        assert_eq!(extract_error_detail(&v), Some("rate limited".into()));
    }

    #[test]
    fn extract_error_detail_returns_none_for_null() {
        assert!(extract_error_detail(&serde_json::Value::Null).is_none());
    }

    #[test]
    fn extract_error_detail_handles_assistant_message_error_message_field() {
        // Pi's AssistantMessage carries the error in the `errorMessage`
        // (camelCase) field. Make sure we dig it out.
        let v = serde_json::json!({
            "role": "assistant",
            "content": [],
            "stopReason": "error",
            "errorMessage": "Your credit balance is too low to access the Claude API."
        });
        assert_eq!(
            extract_error_detail(&v).as_deref(),
            Some("Your credit balance is too low to access the Claude API.")
        );
    }

    // ── V2.12.f regression: "nothing shown" when API returns credit error

    #[test]
    fn agent_end_with_error_assistant_message_pushes_error_entry() {
        // This is the exact shape pi sends for a non-retryable provider
        // error (insufficient credits, model not found, etc.): the
        // assistant message carries stopReason=error + errorMessage, and
        // `agent_end.messages` includes it. No stream error event fires.
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(Incoming::AgentEnd {
            messages: vec![AgentMessage::Assistant {
                content: vec![],
                api: None,
                provider: Some("anthropic".into()),
                model: Some("claude-haiku-4-5".into()),
                usage: None,
                stop_reason: Some(StopReason::Error),
                error_message: Some(
                    "Your credit balance is too low to access the Claude API.".into(),
                ),
                timestamp: 0,
                entry_id: None,
            }],
        });
        assert!(!a.is_streaming);
        assert!(matches!(a.live, LiveState::Error));
        let last = a.transcript.entries().last().unwrap();
        match last {
            Entry::Error(s) => {
                assert!(s.contains("pi:"), "got {s}");
                assert!(s.contains("credit balance is too low"), "got {s}");
            }
            other => panic!("expected Entry::Error, got {other:?}"),
        }
    }

    #[test]
    fn agent_end_parses_real_wire_json_with_error() {
        // Belt and braces: deserialize the real JSON pi puts on the wire
        // for a credit-error agent_end. If this parses without losing
        // the errorMessage, we're good.
        let wire = serde_json::json!({
            "type": "agent_end",
            "messages": [{
                "role": "assistant",
                "content": [],
                "api": "anthropic-messages",
                "provider": "anthropic",
                "model": "claude-haiku-4-5",
                "usage": {
                    "input": 0,
                    "output": 0,
                    "cacheRead": 0,
                    "cacheWrite": 0
                },
                "stopReason": "error",
                "errorMessage": "402 {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Your credit balance is too low to access the Claude API. Please go to Plans & Billing to upgrade or purchase credits.\"}}",
                "timestamp": 1_713_628_800_000_i64
            }]
        });
        let ev: Incoming = serde_json::from_value(wire).expect("parse agent_end");
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(ev);
        let last = a.transcript.entries().last().unwrap();
        match last {
            Entry::Error(s) => {
                assert!(s.contains("credit balance is too low"), "got {s}");
            }
            other => panic!("expected Entry::Error, got {other:?}"),
        }
    }

    // ── V2.12.g · interview mode ──────────────────────────────────────

    #[test]
    fn agent_end_with_interview_marker_opens_modal() {
        // When the agent's assistant text contains [[INTERVIEW: ...]],
        // agent_end should open the Interview modal with the defaults
        // hydrated.
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(text_delta(
            r#"Let's get started.
[[INTERVIEW: {
    "title": "Project setup",
    "fields": [
        { "type": "text", "id": "name", "label": "Project name", "default": "my-app" },
        { "type": "toggle", "id": "typescript", "label": "Use TypeScript?", "default": true }
    ]
}]]
"#,
        ));
        a.on_event(Incoming::AgentEnd { messages: vec![] });

        match &a.modal {
            Some(Modal::Interview(state)) => {
                assert_eq!(state.title, "Project setup");
                assert_eq!(state.fields.len(), 2);
                // Focus starts on the first interactive field (Text).
                assert!(matches!(
                    state.fields[state.focus],
                    crate::interview::FieldValue::Text { .. }
                ));
            }
            other => panic!("expected Modal::Interview, got {other:?}"),
        }
    }

    #[test]
    fn agent_end_opens_modal_from_ask_markers() {
        // Primary path: flat [[ASK_*]] markers (plan-mode style). Verify
        // the modal opens, the markers are stripped from the visible
        // assistant card, and the info row is pushed.
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(text_delta(
            "\
Let me set up your project.

[[ASK_TITLE: Project setup]]
[[ASK_SECTION: Basics]]
[[ASK_TEXT!: name | Project name | my-app]]
[[ASK_PICK: framework | Framework | React | Vue* | Svelte]]
[[ASK_YESNO: typescript | Use TypeScript? | yes]]
[[ASK_SUBMIT: Create]]

I'll take it from here.",
        ));
        a.on_event(Incoming::AgentEnd { messages: vec![] });

        match &a.modal {
            Some(Modal::Interview(state)) => {
                assert_eq!(state.title, "Project setup");
                assert_eq!(state.submit_label, "Create");
                // 1 section + 3 interactive = 4 fields total.
                assert_eq!(state.fields.len(), 4);
            }
            other => panic!("expected Modal::Interview, got {other:?}"),
        }

        // Card no longer contains any [[ASK_ markers.
        let text = a
            .transcript
            .entries()
            .iter()
            .rev()
            .find_map(|e| match e {
                Entry::Assistant(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        assert!(!text.contains("[[ASK_"), "got {text:?}");
        assert!(text.contains("Let me set up your project"));
        assert!(text.contains("I'll take it from here"));

        // Info row lands with the count.
        let has_info = a.transcript.entries().iter().any(|e| {
            matches!(e, Entry::Info(s)
                if s.contains("Project setup") && s.contains("3 question"))
        });
        assert!(has_info, "expected info row about the interview");
    }

    #[test]
    fn agent_end_detects_interview_in_fenced_code_block() {
        // Agents often drop the [[INTERVIEW:]] wrapper and put the JSON
        // in a fenced code block instead. Verify the lenient detector
        // opens the modal AND strips the JSON from the assistant card.
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(text_delta(
            "Here's a quick form:\n\n```json\n{\n  \"title\": \"Settings\",\n  \"fields\": [\n    { \"type\": \"text\", \"id\": \"name\", \"label\": \"Name\" }\n  ]\n}\n```\n\nPlease fill it out.",
        ));
        a.on_event(Incoming::AgentEnd { messages: vec![] });

        // Modal is open.
        match &a.modal {
            Some(Modal::Interview(state)) => {
                assert_eq!(state.title, "Settings");
            }
            other => panic!("expected Modal::Interview, got {other:?}"),
        }

        // Assistant card no longer contains the raw JSON.
        let assistant_text = a
            .transcript
            .entries()
            .iter()
            .rev()
            .find_map(|e| match e {
                Entry::Assistant(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        assert!(
            !assistant_text.contains("\"fields\""),
            "got {assistant_text:?}"
        );
        assert!(assistant_text.contains("Here's a quick form"));
        assert!(assistant_text.contains("Please fill it out"));

        // Info row records the event.
        let has_info = a.transcript.entries().iter().any(
            |e| matches!(e, Entry::Info(s) if s.contains("interview") && s.contains("Settings")),
        );
        assert!(has_info, "expected info row about the interview");
    }

    #[test]
    fn interview_submit_button_via_tab_then_enter() {
        // Build a minimal form, navigate focus to the submit slot via
        // focus_next(), then drive Enter through interview_key and
        // verify it reports "submit".
        let src = r#"[[ASK_TEXT: n | Name | x]]"#;
        let (iv, _) = crate::interview::parse_ask_markers(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        assert!(!state.focus_on_submit());
        // Tab: Name → Submit slot.
        let _ = interview_key(&mut state, KeyCode::Tab, KeyModifiers::NONE);
        assert!(state.focus_on_submit());
        // Enter on the submit slot → submit.
        let submit = interview_key(&mut state, KeyCode::Enter, KeyModifiers::NONE);
        assert!(submit, "Enter on the Submit slot should submit");
    }

    #[test]
    fn interview_submit_ctrl_s_anywhere() {
        let src = r#"[[ASK_TEXT: n | Name | x]]"#;
        let (iv, _) = crate::interview::parse_ask_markers(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        // Focus on the Name field (not submit slot).
        assert!(!state.focus_on_submit());
        let submit = interview_key(&mut state, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(submit, "Ctrl+S should submit from any focus");
    }

    // ── V2.13.b · /settings modal ──────────────────────────────────────

    #[test]
    fn settings_rows_cover_every_section() {
        let a = app();
        let rows = build_settings_rows(&a);
        let titles: Vec<&'static str> = rows
            .iter()
            .filter_map(|r| match r {
                SettingsRow::Header(t) => Some(*t),
                _ => None,
            })
            .collect();
        assert_eq!(
            titles,
            vec![
                "Session",
                "Model",
                "Behavior",
                "Appearance",
                "Live state",
                "Capabilities",
                "Paths",
                "Build",
            ]
        );
    }

    #[test]
    fn settings_rows_expose_every_toggle_action() {
        let a = app();
        let rows = build_settings_rows(&a);
        let mut seen: std::collections::HashSet<ToggleAction> = std::collections::HashSet::new();
        for r in &rows {
            if let SettingsRow::Toggle { action, .. } = r {
                seen.insert(*action);
            }
        }
        for expected in [
            ToggleAction::ShowThinking,
            ToggleAction::Notify,
            ToggleAction::Vim,
            ToggleAction::AutoCompact,
            ToggleAction::AutoRetry,
            ToggleAction::PlanAutoRun,
        ] {
            assert!(seen.contains(&expected), "missing {expected:?}");
        }
    }

    #[test]
    fn settings_rows_expose_every_cycle_action() {
        let a = app();
        let rows = build_settings_rows(&a);
        let mut seen: std::collections::HashSet<CycleAction> = std::collections::HashSet::new();
        for r in &rows {
            if let SettingsRow::Cycle { action, .. } = r {
                seen.insert(*action);
            }
        }
        for expected in [
            CycleAction::Theme,
            CycleAction::ThinkingLevel,
            CycleAction::SteeringMode,
            CycleAction::FollowUpMode,
            CycleAction::Model,
        ] {
            assert!(seen.contains(&expected), "missing {expected:?}");
        }
    }

    #[test]
    fn settings_enter_on_toggle_dispatches_action() {
        let mut a = app();
        a.modal = Some(Modal::Settings(crate::ui::modal::SettingsState::default()));
        // Find the ShowThinking toggle row and park the selection on it.
        let rows = build_settings_rows(&a);
        let target = rows
            .iter()
            .position(|r| {
                matches!(
                    r,
                    SettingsRow::Toggle { action, .. }
                        if *action == ToggleAction::ShowThinking
                )
            })
            .unwrap();
        if let Some(Modal::Settings(s)) = a.modal.as_mut() {
            s.selected = target;
        }
        let (action, close) = settings_modal_key(KeyCode::Enter, KeyModifiers::NONE, &mut a);
        assert!(!close);
        assert!(matches!(
            action,
            Some(SettingsAction::Toggle(ToggleAction::ShowThinking))
        ));
    }

    #[test]
    fn settings_esc_closes() {
        let mut a = app();
        a.modal = Some(Modal::Settings(crate::ui::modal::SettingsState::default()));
        let (_, close) = settings_modal_key(KeyCode::Esc, KeyModifiers::NONE, &mut a);
        assert!(close);
    }

    /// V3.a · AutoCompact / AutoRetry toggles flip the local state even when
    /// pi is offline; the user sees a flash explaining the change won't
    /// persist until next session.
    #[tokio::test]
    async fn settings_toggle_offline_flashes_for_rpc_backed_flags() {
        for (action, expected_label) in [
            (ToggleAction::AutoCompact, "auto-compact"),
            (ToggleAction::AutoRetry, "auto-retry"),
        ] {
            let mut a = app();
            a.flash = None;
            let before_compact = a.session.auto_compaction;
            let before_retry = a.session.auto_retry;
            dispatch_settings_action(&mut a, None, SettingsAction::Toggle(action)).await;
            // Local flag flipped.
            match action {
                ToggleAction::AutoCompact => assert_ne!(
                    a.session.auto_compaction, before_compact,
                    "auto_compaction should toggle locally even when offline"
                ),
                ToggleAction::AutoRetry => assert_ne!(
                    a.session.auto_retry, before_retry,
                    "auto_retry should toggle locally even when offline"
                ),
                _ => unreachable!(),
            }
            // Flash explains the offline caveat.
            let flash_text = a.flash.as_ref().map(|(m, _)| m.as_str()).unwrap_or("");
            assert!(
                flash_text.contains(expected_label) && flash_text.contains("offline"),
                "expected offline flash for {action:?}, got {flash_text:?}"
            );
        }
    }

    // ── V2.13.a · /shortcuts modal ──────────────────────────────────────

    #[test]
    fn shortcuts_body_has_every_section() {
        let t = crate::theme::default_theme();
        let lines = shortcuts_body(t);
        let text = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<String>();
        // Every section header must be present so the user doesn't hunt.
        for header in [
            "Global",
            "Editor (idle — no modal)",
            "Composer editing",
            "Focus mode (Ctrl+F)",
            "Modal — any",
            "Vim mode (opt-in via /vim)",
            "Interview modal",
            "Mouse",
        ] {
            assert!(text.contains(header), "missing section: {header}");
        }
        // Sanity: a few representative keybindings too.
        for key in ["Ctrl+C", "Ctrl+F", "F7", "PgUp / PgDn", "Esc", "Tab"] {
            assert!(text.contains(key), "missing key: {key}");
        }
    }

    #[test]
    fn interview_pgdown_scrolls_and_marks_user_scroll() {
        let src = r#"[[ASK_TEXT: a | A]]"#;
        let (iv, _) = crate::interview::parse_ask_markers(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        assert_eq!(state.scroll, 0);
        assert!(!state.user_scrolled);
        let _ = interview_key(&mut state, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(state.scroll, 10);
        assert!(state.user_scrolled);
    }

    #[test]
    fn interview_tab_resets_user_scrolled_for_autofocus() {
        let src = r#"[[ASK_TEXT: a | A]] [[ASK_TEXT: b | B]]"#;
        let (iv, _) = crate::interview::parse_ask_markers(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        // Simulate the user scrolling manually.
        state.scroll = 20;
        state.user_scrolled = true;
        let _ = interview_key(&mut state, KeyCode::Tab, KeyModifiers::NONE);
        // Tab moves focus; user_scrolled clears so auto-follow kicks in.
        assert!(!state.user_scrolled);
    }

    #[test]
    fn interview_ctrl_home_and_end_scroll_to_bounds() {
        let src = r#"[[ASK_TEXT: a | A]]"#;
        let (iv, _) = crate::interview::parse_ask_markers(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        let _ = interview_key(&mut state, KeyCode::End, KeyModifiers::CONTROL);
        assert_eq!(state.scroll, u16::MAX);
        assert!(state.user_scrolled);
        let _ = interview_key(&mut state, KeyCode::Home, KeyModifiers::CONTROL);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn interview_submit_blocked_when_required_empty() {
        let src = r#"[[ASK_TEXT!: n | Name]]"#; // required, no default
        let (iv, _) = crate::interview::parse_ask_markers(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        state.focus = state.submit_slot();
        let submit = interview_key(&mut state, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!submit, "required-but-empty should refuse submit");
        assert!(state.validation_error.is_some());
        // Focus should have jumped to the offending field.
        assert!(!state.focus_on_submit());
    }

    #[test]
    fn agent_end_detects_interview_in_bare_json() {
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(text_delta(
            "{\"title\":\"Raw\",\"fields\":[{\"type\":\"toggle\",\"id\":\"ok\",\"label\":\"OK\"}]}",
        ));
        a.on_event(Incoming::AgentEnd { messages: vec![] });
        match &a.modal {
            Some(Modal::Interview(state)) => assert_eq!(state.title, "Raw"),
            other => panic!("expected Modal::Interview, got {other:?}"),
        }
    }

    #[test]
    fn interview_response_includes_answers_as_json() {
        // Hydrate a state, fill the text field, serialise, assert shape.
        let src = r#"[[INTERVIEW: {
            "title": "Quick",
            "fields": [
                { "type": "text", "id": "note", "label": "Note" },
                { "type": "toggle", "id": "ok", "label": "OK", "default": true }
            ]
        }]]"#;
        let (iv, _) = crate::interview::parse_marker(src).unwrap();
        let mut state = crate::interview::InterviewState::from_interview(iv);
        if let crate::interview::FieldValue::Text { value, cursor, .. } = &mut state.fields[0] {
            *value = "hello".into();
            *cursor = 5;
        }
        let resp = state.as_response();
        assert!(resp.contains("<interview-response>"));
        assert!(resp.contains("\"note\": \"hello\""));
        assert!(resp.contains("\"ok\": true"));
    }

    #[test]
    fn message_update_error_uses_correct_field_name() {
        // Regression: AssistantEvent::Error used to declare `partial`
        // but pi sends `error`. With #[serde(default)] the old code
        // silently accepted the message with an empty payload and
        // rendered "stream error: Error" with no detail. Verify we
        // now parse the `error` field and extract errorMessage.
        let wire = serde_json::json!({
            "type": "message_update",
            "message": null,
            "assistantMessageEvent": {
                "type": "error",
                "reason": "error",
                "error": {
                    "role": "assistant",
                    "content": [],
                    "stopReason": "error",
                    "errorMessage": "429 rate_limit: Too many requests"
                }
            }
        });
        let ev: Incoming = serde_json::from_value(wire).expect("parse message_update");
        let mut a = app();
        a.on_event(ev);
        assert!(matches!(a.live, LiveState::Error));
        let last = a.transcript.entries().last().unwrap();
        match last {
            Entry::Error(s) => {
                assert!(s.contains("rate_limit"), "got {s}");
            }
            other => panic!("expected Entry::Error, got {other:?}"),
        }
    }

    // ── Liveness probe ───────────────────────────────────────────────────

    #[test]
    fn any_event_bumps_last_event_tick() {
        let mut a = app();
        a.ticks = 100;
        a.last_event_tick = 0;
        a.on_event(Incoming::AgentStart);
        assert_eq!(a.last_event_tick, 100);
    }

    // ── Full turn transcript shape ───────────────────────────────────────

    #[test]
    fn full_turn_produces_expected_entries() {
        let mut a = app();
        a.on_event(Incoming::AgentStart);
        a.on_event(Incoming::TurnStart);
        a.on_event(thinking_delta("let me think…"));
        a.on_event(text_delta("Hello "));
        a.on_event(text_delta("world"));
        a.on_event(Incoming::ToolExecutionStart {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            args: serde_json::json!({"command": "ls"}),
        });
        a.on_event(Incoming::ToolExecutionEnd {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            result: tool_result_text("file1\nfile2"),
            is_error: false,
        });
        a.on_event(Incoming::AgentEnd { messages: vec![] });

        // Thinking, Assistant, ToolCall (no TurnMarker because it's turn 1)
        let kinds: Vec<&str> = a
            .transcript
            .entries()
            .iter()
            .map(|e| match e {
                Entry::Thinking(_) => "thinking",
                Entry::Assistant(_) => "assistant",
                Entry::ToolCall(_) => "tool",
                Entry::TurnMarker { .. } => "turn",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["thinking", "assistant", "tool"]);
        assert!(!a.is_streaming);
        assert!(matches!(a.live, LiveState::Idle));
    }
}
