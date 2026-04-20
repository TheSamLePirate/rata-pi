//! Best-effort detection of terminal emulator capabilities.
//!
//! Pure env-var sniff; no escape-sequence probe. Used to decide whether to
//! enable Kitty keyboard enhancement and to pick an image-rendering path.

use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKind {
    Kitty,
    Iterm2,
    WezTerm,
    Ghostty,
    Alacritty,
    VSCode,
    Apple, // macOS Terminal.app
    Tmux,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct Caps {
    pub kind: TerminalKind,
    /// True when the terminal is known to implement the Kitty keyboard
    /// protocol (precise modifier reporting, key-release, etc.).
    pub kitty_keyboard: bool,
    /// True when the terminal can render graphics (Kitty / iTerm / WezTerm
    /// / Ghostty / Sixel). Used later by the `images` feature.
    pub graphics: bool,
}

pub fn detect() -> Caps {
    let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
    let term = env::var("TERM").unwrap_or_default();
    let kitty_window_id = env::var("KITTY_WINDOW_ID").is_ok();
    let ghostty = env::var("GHOSTTY_RESOURCES_DIR").is_ok();
    let tmux = term.starts_with("tmux") || env::var("TMUX").is_ok();

    let kind = if kitty_window_id || term.contains("kitty") {
        TerminalKind::Kitty
    } else if ghostty || term_program == "ghostty" {
        TerminalKind::Ghostty
    } else if term_program == "WezTerm" {
        TerminalKind::WezTerm
    } else if term_program == "iTerm.app" {
        TerminalKind::Iterm2
    } else if term_program == "Apple_Terminal" {
        TerminalKind::Apple
    } else if term_program == "vscode"
        || term.contains("xterm-256color") && env::var("VSCODE_INJECTION").is_ok()
    {
        TerminalKind::VSCode
    } else if term.contains("alacritty") {
        TerminalKind::Alacritty
    } else if tmux {
        TerminalKind::Tmux
    } else {
        TerminalKind::Unknown
    };

    let kitty_keyboard = matches!(
        kind,
        TerminalKind::Kitty | TerminalKind::Ghostty | TerminalKind::WezTerm
    );
    let graphics = matches!(
        kind,
        TerminalKind::Kitty | TerminalKind::Ghostty | TerminalKind::WezTerm | TerminalKind::Iterm2
    );

    Caps {
        kind,
        kitty_keyboard,
        graphics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_something_always() {
        let c = detect();
        // No assertions on a specific variant; we just want no panic in CI.
        let _ = format!("{c:?}");
    }
}
