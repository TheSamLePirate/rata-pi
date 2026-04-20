//! Minimal M0 app shell.
//!
//! Responsibilities at this milestone:
//! - Enter the alt-screen with mouse capture, guaranteed to restore on panic or error.
//! - Spawn pi (or surface a friendly error), but do not wire the RPC loop yet — that's M1.
//! - Run a tokio `select!` over crossterm events and a 10 Hz ticker.
//! - Draw a header / body / footer shell with an animated spinner.

use std::io::{Stdout, stdout};
use std::panic;
use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::io::AsyncWriteExt;

use crate::cli::Args;
use crate::rpc;

pub async fn run(args: Args) -> Result<()> {
    install_panic_hook();
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run_inner(&mut terminal, args).await;

    // Restore regardless of result.
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
    let _ = terminal.show_cursor();

    result
}

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        original(info);
    }));
}

enum PiStatus {
    Running { pid: Option<u32> },
    Failed(String),
}

struct UiState {
    pi_status: PiStatus,
    ticks: u64,
}

enum Action {
    Quit,
}

async fn run_inner(terminal: &mut Terminal<CrosstermBackend<Stdout>>, args: Args) -> Result<()> {
    let mut pi_proc = match rpc::process::spawn(&args.pi_bin, &args.pi_argv()) {
        Ok(p) => {
            tracing::info!(pid = ?p.child.id(), pi_bin = %args.pi_bin, "spawned pi");
            Some(p)
        }
        Err(e) => {
            tracing::error!(error = %format!("{e:#}"), "pi spawn failed");
            None
        }
    };

    let mut state = UiState {
        pi_status: match &pi_proc {
            Some(p) => PiStatus::Running { pid: p.child.id() },
            None => PiStatus::Failed(
                "pi could not be spawned — see rata-pi log file for details".into(),
            ),
        },
        ticks: 0,
    };

    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(100));

    loop {
        terminal.draw(|f| draw(f, &state))?;

        tokio::select! {
            _ = ticker.tick() => { state.ticks = state.ticks.wrapping_add(1); }
            Some(Ok(ev)) = events.next() => {
                if let Some(Action::Quit) = handle_event(ev) {
                    break;
                }
            }
            else => break,
        }
    }

    if let Some(pi) = pi_proc.as_mut() {
        let _ = pi.stdin.shutdown().await;
        let _ = pi.child.kill().await;
        let _ = pi.child.wait().await;
    }

    Ok(())
}

fn handle_event(ev: Event) -> Option<Action> {
    let Event::Key(k) = ev else { return None };
    if k.kind != KeyEventKind::Press {
        return None;
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Action::Quit),
        (KeyCode::Char('c') | KeyCode::Char('d'), KeyModifiers::CONTROL) => Some(Action::Quit),
        _ => None,
    }
}

fn draw(f: &mut ratatui::Frame, state: &UiState) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" rata-pi ", Style::new().bold().reversed()),
            Span::raw("  "),
            Span::styled("M0 foundations", Style::new().dim()),
        ])),
        header,
    );

    const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let sp = SPINNER[(state.ticks as usize) % SPINNER.len()];

    let body_text = match &state.pi_status {
        PiStatus::Running { pid } => Text::from(vec![
            Line::from(vec![
                Span::styled(sp.to_string(), Style::new().cyan()),
                Span::raw(" pi is running "),
                Span::styled(
                    pid.map(|p| format!("(pid {p})")).unwrap_or_default(),
                    Style::new().dim(),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "RPC loop not wired yet — coming in M1.",
                Style::new().dim().italic(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "press q or Ctrl+C to quit",
                Style::new().dim(),
            )),
        ]),
        PiStatus::Failed(msg) => Text::from(vec![
            Line::from(Span::styled(
                "⚠ failed to spawn pi",
                Style::new().red().bold(),
            )),
            Line::from(""),
            Line::from(msg.clone()),
            Line::from(""),
            Line::from(Span::styled(
                "press q or Ctrl+C to quit",
                Style::new().dim(),
            )),
        ]),
    };
    f.render_widget(
        Paragraph::new(body_text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" session ")),
        body,
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" q", Style::new().cyan()),
            Span::raw(" quit  "),
            Span::styled("Ctrl+C", Style::new().cyan()),
            Span::raw(" quit"),
        ]))
        .style(Style::new().dim()),
        footer,
    );
}
