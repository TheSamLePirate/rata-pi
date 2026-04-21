//! Desktop-notification surface.
//!
//! Two layers:
//!
//! * **OSC 777** — terminal-emitted notification (iTerm2, WezTerm, kitty,
//!   Ghostty, gnome-terminal, konsole). Always compiled; costs nothing when
//!   the terminal ignores it.
//! * **`notify-rust`** — native desktop notification via DBus / NSUserNotification /
//!   WinToast. Feature-gated under `notify` so the default build stays slim.
//!
//! The public entry point is [`notify`] — it always tries OSC 777 and, when
//! the `notify` feature is on, also fires a native notification. Returns the
//! backends that fired so the caller can surface a toast like "notified
//! (osc777 · native)".
//!
//! Avoid spamming: the caller should rate-limit. We don't try to deduplicate
//! here; the transcript event loop is closer to the signal.

use std::fmt::Write as _;
use std::io::Write;

/// Which backends fired for a single [`notify`] call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Backends {
    pub osc777: bool,
    pub native: bool,
}

impl Backends {
    #[allow(dead_code)]
    pub fn any(self) -> bool {
        self.osc777 || self.native
    }

    pub fn label(self) -> String {
        match (self.osc777, self.native) {
            (true, true) => "osc777 · native".into(),
            (true, false) => "osc777".into(),
            (false, true) => "native".into(),
            (false, false) => "none".into(),
        }
    }
}

/// Fire a desktop notification. Title + body are sanitised (BEL / ESC / CR /
/// LF removed) to keep OSC 777 framing safe on weird terminals.
///
/// Never returns an error — best-effort. A terminal that ignores OSC 777 is
/// indistinguishable from one that acts on it, from our side.
pub fn notify(title: &str, body: &str) -> Backends {
    let title = sanitize(title);
    let body = sanitize(body);

    let mut out = Backends::default();
    if emit_osc777(&title, &body).is_ok() {
        out.osc777 = true;
    }
    #[cfg(feature = "notify")]
    {
        if send_native(&title, &body).is_ok() {
            out.native = true;
        }
    }
    out
}

/// Strip BEL (`\x07`), ESC (`\x1b`), CR, LF. OSC 777 frames end with BEL or
/// ST, so a stray BEL in user text closes the sequence early.
fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\x07' | '\x1b' | '\r' | '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// `ESC ] 777 ; notify ; <title> ; <body> BEL`. Widely recognised.
fn emit_osc777(title: &str, body: &str) -> std::io::Result<()> {
    let mut payload = String::with_capacity(title.len() + body.len() + 32);
    payload.push_str("\x1b]777;notify;");
    let _ = write!(&mut payload, "{};{}", title, body);
    payload.push('\x07');
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(payload.as_bytes())?;
    stdout.flush()
}

#[cfg(feature = "notify")]
fn send_native(title: &str, body: &str) -> Result<(), notify_rust::error::Error> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .appname("rata-pi")
        .show()
        .map(|_| ())
}

// ────────────────────────────────────────────────────── event classification

/// Decide the one-line title/body for a "turn completed" notification.
///
/// Only notifies for long-running turns (≥ `long_turn_threshold_ticks` at
/// 100 ms per tick), on the theory that sub-3s turns don't deserve a ping.
pub fn agent_end_notice(
    duration_ticks: u64,
    long_turn_threshold_ticks: u64,
    tool_count: u32,
) -> Option<(String, String)> {
    if duration_ticks < long_turn_threshold_ticks {
        return None;
    }
    let secs = duration_ticks / 10;
    let title = "pi · response ready".to_string();
    let body = if tool_count > 0 {
        format!(
            "{secs}s · {tool_count} tool call{s}",
            s = if tool_count == 1 { "" } else { "s" }
        )
    } else {
        format!("{secs}s")
    };
    Some((title, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_control_chars() {
        assert_eq!(sanitize("a\x07b\x1bc\rd\ne"), "a b c d e");
    }

    #[test]
    fn backends_label() {
        assert_eq!(
            Backends {
                osc777: true,
                native: false
            }
            .label(),
            "osc777"
        );
        assert_eq!(
            Backends {
                osc777: true,
                native: true
            }
            .label(),
            "osc777 · native"
        );
        assert!(!Backends::default().any());
    }

    #[test]
    fn agent_end_notice_below_threshold() {
        assert!(agent_end_notice(20, 100, 0).is_none());
    }

    #[test]
    fn agent_end_notice_formats_body() {
        let (title, body) = agent_end_notice(150, 100, 2).unwrap();
        assert!(title.starts_with("pi"));
        assert!(body.contains("15s"));
        assert!(body.contains("2 tool calls"));
    }

    #[test]
    fn agent_end_notice_no_tools() {
        let (_, body) = agent_end_notice(200, 100, 0).unwrap();
        assert_eq!(body, "20s");
    }
}
