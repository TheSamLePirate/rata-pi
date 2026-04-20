//! Semantic theme palette.
//!
//! Every color used in the UI goes through a `Theme` so one switch re-skins
//! the whole app. Drawing code reads `theme.accent`, never `Color::Cyan`.
//!
//! Built-ins live in `builtin`; user themes (TOML) are deferred to V2.0.1.

pub mod builtin;

use ratatui::style::Color;

/// Full semantic palette. Every draw site reads through these names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,

    // general
    pub accent: Color,
    pub accent_strong: Color,
    pub muted: Color,
    pub dim: Color,
    pub text: Color,

    // state
    pub success: Color,
    pub warning: Color,
    pub error: Color,

    // roles
    pub role_user: Color,
    pub role_assistant: Color,
    pub role_thinking: Color,
    pub role_tool: Color,
    pub role_bash: Color,

    // borders
    pub border_idle: Color,
    pub border_active: Color,
    pub border_modal: Color,

    // diffs
    pub diff_add: Color,
    pub diff_remove: Color,
    pub diff_hunk: Color,
    pub diff_file: Color,

    // gauge gradient (0% → 100%)
    pub gauge_low: Color,
    pub gauge_mid: Color,
    pub gauge_high: Color,
}

/// The list of built-in themes in cycle order.
pub fn builtins() -> &'static [Theme] {
    builtin::ALL
}

/// Find a built-in by name; case-insensitive.
pub fn find(name: &str) -> Option<&'static Theme> {
    let wanted = name.to_ascii_lowercase();
    builtins()
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(&wanted))
}

/// Index of a theme within `builtins()`, or 0.
pub fn index_of(t: &Theme) -> usize {
    builtins()
        .iter()
        .position(|b| b.name == t.name)
        .unwrap_or(0)
}

/// The default theme on first launch.
pub fn default_theme() -> &'static Theme {
    &builtin::TOKYO_NIGHT
}

/// Gauge color for `pct ∈ [0,1]`.
pub fn gauge_color(theme: &Theme, pct: f64) -> Color {
    match pct {
        p if p >= 0.85 => theme.gauge_high,
        p if p >= 0.65 => theme.gauge_mid,
        _ => theme.gauge_low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_findable_by_name() {
        for t in builtins() {
            let found = find(t.name).expect("find by name");
            assert_eq!(found.name, t.name);
        }
    }

    #[test]
    fn find_is_case_insensitive() {
        assert!(find("TOKYO-NIGHT").is_some());
        assert!(find("Tokyo-Night").is_some());
    }

    #[test]
    fn find_returns_none_for_unknown() {
        assert!(find("gibberish").is_none());
    }

    #[test]
    fn gauge_color_gradient() {
        let t = &builtin::TOKYO_NIGHT;
        assert_eq!(gauge_color(t, 0.1), t.gauge_low);
        assert_eq!(gauge_color(t, 0.7), t.gauge_mid);
        assert_eq!(gauge_color(t, 0.9), t.gauge_high);
    }
}
