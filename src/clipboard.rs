//! Clipboard abstraction.
//!
//! Writes go through `arboard` first (native macOS / Windows / X11 / Wayland
//! support). If that fails (headless Linux, SSH session, Docker without a
//! DISPLAY) we fall back to OSC 52 — an escape sequence that asks the
//! terminal emulator itself to put the text on the system clipboard. Most
//! modern terminals honour it (iTerm2, WezTerm, Kitty, Alacritty, Ghostty,
//! Tmux with `set -g set-clipboard on`).

use std::io::{self, Write};

use base64::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Arboard,
    Osc52,
}

#[derive(Debug, Clone)]
pub struct CopyOutcome {
    pub backend: Backend,
    pub bytes: usize,
}

/// Copy `text` to the system clipboard. Tries arboard; on any error falls
/// back to OSC 52 written to stdout.
pub fn copy(text: &str) -> io::Result<CopyOutcome> {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())) {
        Ok(()) => Ok(CopyOutcome {
            backend: Backend::Arboard,
            bytes: text.len(),
        }),
        Err(_) => {
            osc52_copy(text)?;
            Ok(CopyOutcome {
                backend: Backend::Osc52,
                bytes: text.len(),
            })
        }
    }
}

fn osc52_copy(text: &str) -> io::Result<()> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut out = io::stdout().lock();
    out.write_all(b"\x1b]52;c;")?;
    out.write_all(b64.as_bytes())?;
    out.write_all(b"\x07")?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encoding_is_standard() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"hello");
        assert_eq!(encoded, "aGVsbG8=");
    }
}
