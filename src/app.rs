//! M2 app: streaming chat with thinking, tool cards, markdown, bash RPC.
//!
//! Events we consume:
//! - agent_start / agent_end — streaming state + spinner
//! - message_update{text_delta,thinking_delta,thinking_end,error}
//! - tool_execution_start/update/end — live tool card with streaming output
//! - auto_retry_start/end — typed retry row that updates in place
//! - compaction_start/end — typed compaction row that updates in place
//! - extension_error — error row
//!
//! User input:
//! - `Enter` submits a prompt (or steer during streaming)
//! - Prefix `!cmd` and Enter runs pi's `bash` RPC and inserts a BashExec row
//! - `Ctrl+T` toggles thinking visibility
//! - `Ctrl+E` expands/collapses the last tool call
//! - `Esc` aborts during streaming (or clears, or quits)
//! - `PgUp/PgDn` scrolls; `End` re-follows tail

use std::io::{Stdout, stdout};
use std::panic;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyCode, KeyEventKind, KeyModifiers,
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
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::cli::Args;
use crate::rpc::client::{self, RpcClient, RpcError};
use crate::rpc::commands::RpcCommand;
use crate::rpc::events::{AssistantEvent, Incoming};
use crate::rpc::types::{State, StreamingBehavior};
use crate::ui::markdown;
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

struct App {
    transcript: Transcript,
    input: String,
    is_streaming: bool,
    model_label: String,
    ticks: u64,
    quit: bool,
    /// Viewport scroll offset; `None` means auto-follow bottom.
    scroll: Option<u16>,
    show_thinking: bool,
    spawn_error: Option<String>,
}

impl App {
    fn new(model_label: String, spawn_error: Option<String>) -> Self {
        Self {
            transcript: Transcript::default(),
            input: String::new(),
            is_streaming: false,
            model_label,
            ticks: 0,
            quit: false,
            scroll: None,
            show_thinking: false,
            spawn_error,
        }
    }

    fn apply_state(&mut self, s: &State) {
        if let Some(m) = &s.model {
            self.model_label = format!("{}/{}", m.provider, m.id);
        }
    }

    fn on_event(&mut self, ev: Incoming) {
        match ev {
            Incoming::AgentStart => self.is_streaming = true,
            Incoming::AgentEnd { .. } => self.is_streaming = false,
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
            } => {
                self.transcript.start_tool(tool_call_id, tool_name, args);
            }
            Incoming::ToolExecutionUpdate {
                tool_call_id,
                partial_result,
                ..
            } => {
                self.transcript
                    .update_tool_output(&tool_call_id, &partial_result);
            }
            Incoming::ToolExecutionEnd {
                tool_call_id,
                result,
                is_error,
                ..
            } => {
                self.transcript
                    .finish_tool(&tool_call_id, &result, is_error);
            }
            Incoming::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => {
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
            _ => {}
        }
    }
}

// ───────────────────────────────────────────────────────── main loop ──

async fn run_inner(terminal: &mut Terminal<CrosstermBackend<Stdout>>, args: Args) -> Result<()> {
    let (client_and_io, spawn_error) =
        match client::spawn(&args.pi_bin, &args.pi_argv(), args.debug_rpc) {
            Ok(pair) => (Some(pair), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };

    let mut app = App::new("unknown model".into(), spawn_error);

    if let Some((client, mut io)) = client_and_io {
        match client.call(RpcCommand::GetState).await {
            Ok(ok) => {
                if let Some(v) = ok.data
                    && let Ok(state) = serde_json::from_value::<State>(v)
                {
                    app.apply_state(&state);
                }
            }
            Err(e) => tracing::warn!(error = ?e, "initial get_state failed"),
        }
        app.transcript.push(Entry::Info(
            "connected to pi — type a message and press Enter · `!cmd` runs bash · Ctrl+T thinking · Ctrl+C quit"
                .into(),
        ));

        ui_loop(terminal, &mut app, Some(&client), &mut io.events).await?;

        if let Err(e) = client::shutdown(client, io).await {
            tracing::warn!(error = ?e, "shutdown error");
        }
    } else {
        ui_loop(terminal, &mut app, None, &mut dummy_events()).await?;
    }

    Ok(())
}

fn dummy_events() -> tokio::sync::mpsc::Receiver<Incoming> {
    let (_tx, rx) = tokio::sync::mpsc::channel::<Incoming>(1);
    rx
}

async fn ui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    client: Option<&RpcClient>,
    events: &mut tokio::sync::mpsc::Receiver<Incoming>,
) -> Result<()> {
    let mut crossterm_events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.tick().await;

    loop {
        terminal.draw(|f| draw(f, app))?;
        if app.quit {
            break;
        }

        tokio::select! {
            Some(msg) = events.recv() => app.on_event(msg),
            Some(Ok(ev)) = crossterm_events.next() => handle_crossterm(ev, app, client).await,
            _ = ticker.tick() => app.ticks = app.ticks.wrapping_add(1),
        }
    }
    Ok(())
}

async fn handle_crossterm(ev: Event, app: &mut App, client: Option<&RpcClient>) {
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Press => match (k.code, k.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
            | (KeyCode::Char('d'), KeyModifiers::CONTROL) => app.quit = true,
            (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
                app.show_thinking = !app.show_thinking;
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                if let Some(id) = last_tool_id(&app.transcript) {
                    app.transcript.toggle_tool_expanded(&id);
                }
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
        },
        Event::Paste(text) => {
            for ch in text.chars() {
                if ch == '\n' || ch == '\r' {
                    app.input.push(' ');
                } else {
                    app.input.push(ch);
                }
            }
        }
        _ => {}
    }
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

    // Bash prefix `!cmd` — invoke pi's bash RPC directly.
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
    let cmd = RpcCommand::Prompt {
        message: text,
        images: vec![],
        streaming_behavior: if app.is_streaming {
            Some(StreamingBehavior::Steer)
        } else {
            None
        },
    };
    if let Err(e) = client.fire(cmd).await {
        let msg = match e {
            RpcError::Remote { message, .. } => message,
            other => other.to_string(),
        };
        app.transcript
            .push(Entry::Error(format!("prompt failed: {msg}")));
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
    let [header, body, editor_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(f, header, app);
    draw_body(f, body, app);
    draw_editor(f, editor_area, app);
    draw_footer(f, footer, app);
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let spinner = if app.is_streaming {
        Span::styled(
            format!("{} ", SPINNER[(app.ticks as usize) % SPINNER.len()]),
            Style::default().fg(Color::Cyan),
        )
    } else {
        Span::raw("  ")
    };
    let status = if app.is_streaming {
        Span::styled("streaming", Style::default().fg(Color::Cyan))
    } else if app.spawn_error.is_some() {
        Span::styled("pi offline", Style::default().fg(Color::Red))
    } else {
        Span::styled("idle", Style::default().add_modifier(Modifier::DIM))
    };
    let thinking_badge = if app.show_thinking {
        Span::styled("  think ●", Style::default().fg(Color::Magenta))
    } else {
        Span::styled("  think ○", Style::default().add_modifier(Modifier::DIM))
    };
    let line = Line::from(vec![
        Span::styled(
            " rata-pi ",
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw("  "),
        spinner,
        Span::styled(&app.model_label, Style::default().fg(Color::Magenta)),
        Span::raw("  ·  "),
        status,
        thinking_badge,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn entries_to_lines(entries: &[Entry], show_thinking: bool) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(entries.len() * 4);
    for e in entries {
        match e {
            Entry::User(s) => {
                let label = Span::styled(
                    "you › ",
                    Style::default()
                        .fg(Color::Green)
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
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::DIM),
                        );
                        lines.push(Line::from(vec![
                            prefix,
                            Span::styled(
                                part.to_string(),
                                Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
                            ),
                        ]));
                    }
                    lines.push(Line::default());
                } else {
                    let count = s.lines().count().max(1);
                    lines.push(Line::from(Span::styled(
                        format!("▸ thinking ({count} lines — Ctrl+T to reveal)"),
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::DIM),
                    )));
                }
            }
            Entry::Assistant(md) => {
                let label = Span::styled(
                    "pi  › ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                );
                let rendered = markdown::render(md);
                if rendered.is_empty() {
                    // still streaming: just prefix
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
            Entry::ToolCall(tc) => push_tool_lines(&mut lines, tc),
            Entry::BashExec(bx) => push_bash_lines(&mut lines, bx),
            Entry::Info(s) => lines.push(Line::from(Span::styled(
                format!("· {s}"),
                Style::default().add_modifier(Modifier::DIM),
            ))),
            Entry::Warn(s) => lines.push(Line::from(Span::styled(
                format!("⚠ {s}"),
                Style::default().fg(Color::Yellow),
            ))),
            Entry::Error(s) => lines.push(Line::from(Span::styled(
                format!("✗ {s}"),
                Style::default().fg(Color::Red),
            ))),
            Entry::Compaction(c) => push_compaction_line(&mut lines, c),
            Entry::Retry(r) => push_retry_line(&mut lines, r),
        }
    }
    lines
}

fn push_tool_lines(lines: &mut Vec<Line<'static>>, tc: &ToolCall) {
    let (sym, color) = match tc.status {
        ToolStatus::Running => ("…", Color::Yellow),
        ToolStatus::Ok => ("✓", Color::Green),
        ToolStatus::Err => ("✗", Color::Red),
    };
    let arg_preview = truncate_preview(&args_preview(&tc.args), 60);
    let header_style = if tc.status == ToolStatus::Err || tc.is_error {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    };
    let expand_hint = if tc.expanded { "▾" } else { "▸" };
    lines.push(Line::from(vec![
        Span::styled(format!("{expand_hint} {sym} {} ", tc.name), header_style),
        Span::styled(arg_preview, Style::default().add_modifier(Modifier::DIM)),
    ]));
    if tc.expanded {
        let output = crate::ui::ansi::strip(&tc.output);
        if output.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                "    (no output yet)",
                Style::default().add_modifier(Modifier::DIM),
            )));
        } else {
            for part in output.split('\n').take(200) {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(part.to_string(), Style::default().fg(Color::Gray)),
                ]));
            }
        }
    } else if !tc.output.trim().is_empty() {
        // One-line preview.
        let first = tc
            .output
            .lines()
            .next()
            .map(|s| truncate_preview(s, 80))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                first,
                Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            ),
        ]));
    }
    lines.push(Line::default());
}

fn args_preview(args: &serde_json::Value) -> String {
    // Prefer a single-line JSON preview of top-level keys.
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

fn truncate_preview(s: &str, max: usize) -> String {
    let s: String = s.chars().take(max + 1).collect();
    if s.chars().count() > max {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    } else {
        s
    }
}

fn push_bash_lines(lines: &mut Vec<Line<'static>>, bx: &BashExec) {
    let status_color = if bx.cancelled {
        Color::Yellow
    } else if bx.exit_code == 0 {
        Color::Green
    } else {
        Color::Red
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
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            bx.command.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            status_txt,
            Style::default()
                .bg(status_color)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let body = crate::ui::ansi::strip(&bx.output);
    for part in body.split('\n').take(200) {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(part.to_string(), Style::default().fg(Color::Gray)),
        ]));
    }
    if bx.truncated {
        let path = bx
            .full_output_path
            .as_deref()
            .unwrap_or("(path not provided)");
        lines.push(Line::from(Span::styled(
            format!("  … output truncated — full log: {path}"),
            Style::default().fg(Color::Yellow),
        )));
    }
    lines.push(Line::default());
}

fn push_compaction_line(lines: &mut Vec<Line<'static>>, c: &Compaction) {
    let (sym, color, label) = match &c.state {
        CompactionState::Running => ("⟲", Color::Cyan, "compacting".to_string()),
        CompactionState::Done { summary } => {
            let s = summary
                .as_deref()
                .map(|s| truncate_preview(s, 100))
                .unwrap_or_default();
            (
                "⟲",
                Color::Green,
                if s.is_empty() {
                    "compaction complete".to_string()
                } else {
                    format!("compaction: {s}")
                },
            )
        }
        CompactionState::Aborted => ("⟲", Color::Yellow, "compaction aborted".into()),
        CompactionState::Failed(msg) => ("⟲", Color::Red, format!("compaction failed: {msg}")),
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("{sym} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label, Style::default().fg(color)),
        Span::raw(" "),
        Span::styled(
            format!("({})", c.reason),
            Style::default().add_modifier(Modifier::DIM),
        ),
    ]));
}

fn push_retry_line(lines: &mut Vec<Line<'static>>, r: &Retry) {
    let (sym, color, label) = match &r.state {
        RetryState::Waiting { delay_ms, error } => (
            "↻",
            Color::Yellow,
            format!(
                "retry {}/{} in {}ms — {}",
                r.attempt,
                r.max_attempts,
                delay_ms,
                truncate_preview(error, 80),
            ),
        ),
        RetryState::Succeeded => ("↻", Color::Green, format!("retry {} succeeded", r.attempt)),
        RetryState::Exhausted(msg) => (
            "↻",
            Color::Red,
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
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(err.clone()),
            Line::default(),
            Line::from(Span::styled(
                "press q or Ctrl+C to quit",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ]))
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        return;
    }

    let lines = entries_to_lines(app.transcript.entries(), app.show_thinking);
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
    let is_bash = app.input.trim_start().starts_with('!');
    let border_color = if is_bash {
        Color::Yellow
    } else if app.is_streaming {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = if is_bash {
        " bash (! prefix · Enter run) "
    } else if app.is_streaming {
        " steer (Esc abort) "
    } else {
        " prompt (Enter submit · Esc clear) "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut spans = vec![Span::raw(app.input.as_str())];
    spans.push(Span::styled(
        " ",
        Style::default().add_modifier(Modifier::REVERSED),
    ));
    f.render_widget(
        Paragraph::new(Line::from(spans)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let hints = if app.is_streaming {
        vec![
            kb("Esc"),
            Span::raw(" abort  "),
            kb("Ctrl+T"),
            Span::raw(" thinking  "),
            kb("Ctrl+E"),
            Span::raw(" expand tool  "),
            kb("PgUp/PgDn"),
            Span::raw(" scroll  "),
            kb("End"),
            Span::raw(" follow  "),
            kb("Ctrl+C"),
            Span::raw(" quit"),
        ]
    } else {
        vec![
            kb("Enter"),
            Span::raw(" send  "),
            kb("!cmd"),
            Span::raw(" bash  "),
            kb("Ctrl+T"),
            Span::raw(" thinking  "),
            kb("Ctrl+E"),
            Span::raw(" expand tool  "),
            kb("PgUp/PgDn"),
            Span::raw(" scroll  "),
            kb("Ctrl+C"),
            Span::raw(" quit"),
        ]
    };
    f.render_widget(
        Paragraph::new(Line::from(hints)).style(Style::default().add_modifier(Modifier::DIM)),
        area,
    );
}

fn kb(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::Cyan))
}
