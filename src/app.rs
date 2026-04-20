//! M1 app: streaming chat over RPC.
//!
//! Spawns pi, fires `get_state` to learn the current model, then runs a tokio
//! `select!` over:
//! - `Incoming` events from the RPC reader
//! - `crossterm` key/paste/resize events
//! - a 100 ms ticker for spinner animation
//!
//! User input is a single-line composer (multi-line lands in M5). Submitting
//! fires `prompt`; `Esc` during streaming fires `abort`; `Ctrl+C`/`Ctrl+D`
//! quits. Streaming assistant text appends to the last transcript entry via
//! `Transcript::append_assistant`.

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
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::cli::Args;
use crate::rpc::client::{self, RpcClient, RpcError};
use crate::rpc::commands::RpcCommand;
use crate::rpc::events::{AssistantEvent, Incoming};
use crate::rpc::types::{State, StreamingBehavior};
use crate::ui::transcript::{Entry, ToolStatus, Transcript};

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
    streaming_failed: Option<String>,
    model_label: String,
    ticks: u64,
    quit: bool,
    /// User-controlled scroll offset from the top of the transcript in source
    /// lines. `None` means auto-follow the bottom.
    scroll: Option<u16>,
    /// Populated when pi couldn't be spawned — the whole UI becomes read-only
    /// with a banner.
    spawn_error: Option<String>,
}

impl App {
    fn new(model_label: String, spawn_error: Option<String>) -> Self {
        Self {
            transcript: Transcript::default(),
            input: String::new(),
            is_streaming: false,
            streaming_failed: None,
            model_label,
            ticks: 0,
            quit: false,
            scroll: None,
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
            Incoming::AgentStart => {
                self.is_streaming = true;
                self.streaming_failed = None;
            }
            Incoming::AgentEnd { .. } => {
                self.is_streaming = false;
            }
            Incoming::MessageUpdate {
                assistant_message_event,
                ..
            } => match assistant_message_event {
                AssistantEvent::TextDelta { delta, .. } => {
                    self.transcript.append_assistant(&delta);
                }
                AssistantEvent::Error { reason, .. } => {
                    self.streaming_failed = Some(format!("{reason:?}"));
                }
                _ => {}
            },
            Incoming::ToolExecutionStart { tool_name, .. } => {
                self.transcript.push(Entry::Tool {
                    name: tool_name,
                    status: ToolStatus::Running,
                });
            }
            Incoming::ToolExecutionEnd {
                tool_name,
                is_error,
                ..
            } => {
                self.transcript.finish_tool(&tool_name, !is_error);
            }
            Incoming::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => {
                self.transcript.push(Entry::Warn(format!(
                    "retry {attempt}/{max_attempts} in {delay_ms}ms: {}",
                    error_message.as_deref().unwrap_or("transient error")
                )));
            }
            Incoming::AutoRetryEnd {
                success,
                final_error,
                ..
            } => {
                if success {
                    self.transcript.push(Entry::Info("retry succeeded".into()));
                } else {
                    self.transcript.push(Entry::Error(format!(
                        "retry exhausted: {}",
                        final_error.as_deref().unwrap_or("unknown")
                    )));
                }
            }
            Incoming::CompactionStart { reason } => {
                self.transcript
                    .push(Entry::Info(format!("compaction started ({reason:?})")));
            }
            Incoming::CompactionEnd {
                reason,
                aborted,
                error_message,
                ..
            } => {
                if aborted {
                    self.transcript
                        .push(Entry::Warn(format!("compaction aborted ({reason:?})")));
                } else if let Some(msg) = error_message {
                    self.transcript
                        .push(Entry::Error(format!("compaction failed: {msg}")));
                } else {
                    self.transcript
                        .push(Entry::Info("compaction complete".into()));
                }
            }
            Incoming::ExtensionError { error, .. } => {
                self.transcript.push(Entry::Error(format!(
                    "extension error: {}",
                    error.as_deref().unwrap_or("(no detail)")
                )));
            }
            // Everything else (turn_*, message_start/end, tool_execution_update,
            // queue_update, extension_ui_request, response) is rendered in
            // M2/M3/M4; ignoring them here doesn't affect correctness.
            _ => {}
        }
    }
}

// ───────────────────────────────────────────────────────── main loop ──

async fn run_inner(terminal: &mut Terminal<CrosstermBackend<Stdout>>, args: Args) -> Result<()> {
    // Try to spawn pi. If it fails we still present the UI with a banner so
    // the user can see what's wrong and quit cleanly.
    let (client_and_io, spawn_error) =
        match client::spawn(&args.pi_bin, &args.pi_argv(), args.debug_rpc) {
            Ok(pair) => (Some(pair), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };

    let mut app = App::new("unknown model".into(), spawn_error.clone());

    if let Some((client, mut io)) = client_and_io {
        // Best-effort fetch of current state for the header.
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
            "connected to pi — type a message and press Enter. Ctrl+C to quit.".into(),
        ));

        ui_loop(terminal, &mut app, Some(&client), &mut io.events).await?;

        if let Err(e) = client::shutdown(client, io).await {
            tracing::warn!(error = ?e, "shutdown error");
        }
    } else {
        // No client; just run the UI until the user quits.
        ui_loop(terminal, &mut app, None, &mut dummy_events()).await?;
    }

    Ok(())
}

/// Returns a receiver that is immediately closed — lets us share the same
/// `ui_loop` code when pi failed to spawn.
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
    // First tick is immediate; discard so the spinner matches wall-clock.
    ticker.tick().await;

    loop {
        terminal.draw(|f| draw(f, app))?;
        if app.quit {
            break;
        }

        tokio::select! {
            Some(msg) = events.recv() => {
                app.on_event(msg);
            }
            Some(Ok(ev)) = crossterm_events.next() => {
                handle_crossterm(ev, app, client).await;
            }
            _ = ticker.tick() => {
                app.ticks = app.ticks.wrapping_add(1);
            }
        }
    }

    Ok(())
}

async fn handle_crossterm(ev: Event, app: &mut App, client: Option<&RpcClient>) {
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Press => match (k.code, k.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL)
            | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                app.quit = true;
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
            (KeyCode::Enter, _) => {
                submit(app, client).await;
            }
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
            (KeyCode::End, _) => {
                app.scroll = None;
            }
            _ => {}
        },
        Event::Paste(text) => {
            // Bracketed paste: insert literally (collapsed newlines to spaces
            // since the M1 composer is single-line). Multi-line editor in M5.
            for ch in text.chars() {
                if ch == '\n' || ch == '\r' {
                    app.input.push(' ');
                } else {
                    app.input.push(ch);
                }
            }
        }
        Event::Resize(_, _) => { /* next draw picks up the new size */ }
        _ => {}
    }
}

async fn submit(app: &mut App, client: Option<&RpcClient>) {
    let text = app.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    let Some(client) = client else {
        return;
    };

    app.transcript.push(Entry::User(text.clone()));
    app.input.clear();

    let cmd = if app.is_streaming {
        // During streaming we must specify streamingBehavior. Default to
        // "steer" — deliver after the current turn's tool calls finish.
        RpcCommand::Prompt {
            message: text,
            images: vec![],
            streaming_behavior: Some(StreamingBehavior::Steer),
        }
    } else {
        RpcCommand::Prompt {
            message: text,
            images: vec![],
            streaming_behavior: None,
        }
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

fn draw_header(f: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
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
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn entries_to_text(entries: &[Entry]) -> Text<'_> {
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(entries.len() * 2);
    for e in entries {
        match e {
            Entry::User(s) => {
                for (i, part) in s.split('\n').enumerate() {
                    let prefix = if i == 0 {
                        Span::styled(
                            "you › ",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("      ")
                    };
                    lines.push(Line::from(vec![prefix, Span::raw(part.to_string())]));
                }
                lines.push(Line::default());
            }
            Entry::Assistant(s) => {
                for (i, part) in s.split('\n').enumerate() {
                    let prefix = if i == 0 {
                        Span::styled(
                            "pi  › ",
                            Style::default()
                                .fg(Color::Blue)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("      ")
                    };
                    lines.push(Line::from(vec![prefix, Span::raw(part.to_string())]));
                }
                lines.push(Line::default());
            }
            Entry::Tool { name, status } => {
                let (sym, color) = match status {
                    ToolStatus::Running => ("…", Color::Yellow),
                    ToolStatus::Ok => ("✓", Color::Green),
                    ToolStatus::Err => ("✗", Color::Red),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {sym} "), Style::default().fg(color)),
                    Span::styled(
                        format!("tool: {name}"),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                ]));
            }
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
        }
    }
    Text::from(lines)
}

fn draw_body(f: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
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

    let text = entries_to_text(app.transcript.entries());
    let line_count = text.lines.len() as u16;
    let viewport = inner.height;
    let max_offset = line_count.saturating_sub(viewport);
    let offset = match app.scroll {
        None => max_offset, // auto-follow bottom
        Some(v) => v.min(max_offset),
    };
    let para = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((offset, 0));
    f.render_widget(para, inner);
}

fn draw_editor(f: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    let border_color = if app.is_streaming {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = if app.is_streaming {
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

    // Simple single-line rendering with a block cursor at end-of-input.
    let mut spans = vec![Span::raw(app.input.as_str())];
    spans.push(Span::styled(
        " ",
        Style::default().add_modifier(Modifier::REVERSED),
    ));
    let p = Paragraph::new(Line::from(spans)).wrap(Wrap { trim: false });
    f.render_widget(p, inner);
}

fn draw_footer(f: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    let hints = if app.is_streaming {
        vec![
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" abort  "),
            Span::styled("Ctrl+C", Style::default().fg(Color::Cyan)),
            Span::raw(" quit  "),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Cyan)),
            Span::raw(" scroll  "),
            Span::styled("End", Style::default().fg(Color::Cyan)),
            Span::raw(" follow"),
        ]
    } else {
        vec![
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" send  "),
            Span::styled("Ctrl+C", Style::default().fg(Color::Cyan)),
            Span::raw(" quit  "),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Cyan)),
            Span::raw(" scroll  "),
            Span::styled("End", Style::default().fg(Color::Cyan)),
            Span::raw(" follow"),
        ]
    };
    f.render_widget(
        Paragraph::new(Line::from(hints)).style(Style::default().add_modifier(Modifier::DIM)),
        area,
    );
}
