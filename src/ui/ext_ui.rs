//! Extension UI protocol — typed parsing of `extension_ui_request` payloads
//! and state types (toasts, statuses, widgets) that the app renders.

use std::collections::{HashMap, VecDeque};

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    Info,
    Warning,
    Error,
}

impl NotifyKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "warning" => Self::Warning,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetPlacement {
    AboveEditor,
    BelowEditor,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: NotifyKind,
    pub born_tick: u64,
    pub ttl_ticks: u64,
}

#[derive(Debug, Clone)]
pub struct Widget {
    pub lines: Vec<String>,
    pub placement: WidgetPlacement,
}

/// Parsed extension UI request. Unknown / unsupported methods fall through to
/// `Unknown` so the router can log gracefully.
#[derive(Debug, Clone)]
pub enum ExtReq {
    Select {
        id: String,
        title: String,
        options: Vec<String>,
    },
    Confirm {
        id: String,
        title: String,
        message: Option<String>,
    },
    Input {
        id: String,
        title: String,
        placeholder: Option<String>,
    },
    Editor {
        id: String,
        title: String,
        prefill: String,
    },
    Notify {
        message: String,
        kind: NotifyKind,
    },
    SetStatus {
        key: String,
        text: Option<String>,
    },
    SetWidget {
        key: String,
        widget: Option<Widget>,
    },
    SetTitle {
        title: String,
    },
    SetEditorText {
        text: String,
    },
    Unknown(String),
}

pub fn parse(method: &str, id: &str, rest: &Value) -> ExtReq {
    let s = |k: &str| rest.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let strings = |k: &str| -> Vec<String> {
        rest.get(k)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let widget_placement = || match rest.get("widgetPlacement").and_then(|v| v.as_str()) {
        Some("belowEditor") => WidgetPlacement::BelowEditor,
        _ => WidgetPlacement::AboveEditor,
    };

    match method {
        "select" => ExtReq::Select {
            id: id.into(),
            title: s("title").unwrap_or_else(|| "Select".into()),
            options: strings("options"),
        },
        "confirm" => ExtReq::Confirm {
            id: id.into(),
            title: s("title").unwrap_or_else(|| "Confirm".into()),
            message: s("message"),
        },
        "input" => ExtReq::Input {
            id: id.into(),
            title: s("title").unwrap_or_else(|| "Input".into()),
            placeholder: s("placeholder"),
        },
        "editor" => ExtReq::Editor {
            id: id.into(),
            title: s("title").unwrap_or_else(|| "Editor".into()),
            prefill: s("prefill").unwrap_or_default(),
        },
        "notify" => ExtReq::Notify {
            message: s("message").unwrap_or_default(),
            kind: rest
                .get("notifyType")
                .and_then(|v| v.as_str())
                .map(NotifyKind::from_str)
                .unwrap_or(NotifyKind::Info),
        },
        "setStatus" => ExtReq::SetStatus {
            key: s("statusKey").unwrap_or_default(),
            text: s("statusText"),
        },
        "setWidget" => {
            let lines = strings("widgetLines");
            let widget = if lines.is_empty() {
                None
            } else {
                Some(Widget {
                    lines,
                    placement: widget_placement(),
                })
            };
            ExtReq::SetWidget {
                key: s("widgetKey").unwrap_or_default(),
                widget,
            }
        }
        "setTitle" => ExtReq::SetTitle {
            title: s("title").unwrap_or_default(),
        },
        "set_editor_text" => ExtReq::SetEditorText {
            text: s("text").unwrap_or_default(),
        },
        other => ExtReq::Unknown(other.into()),
    }
}

/// Keyed state that extensions can set / clear.
#[derive(Debug, Default)]
pub struct ExtUiState {
    pub statuses: HashMap<String, String>,
    pub widgets: HashMap<String, Widget>,
    pub toasts: VecDeque<Toast>,
    pub terminal_title: Option<String>,
}

impl ExtUiState {
    pub fn push_toast(&mut self, text: String, kind: NotifyKind, now: u64) {
        let ttl = match kind {
            NotifyKind::Info => 30,    // 3 s at 100 ms tick
            NotifyKind::Warning => 60, // 6 s
            NotifyKind::Error => 120,  // 12 s (user can Esc)
        };
        self.toasts.push_back(Toast {
            text,
            kind,
            born_tick: now,
            ttl_ticks: ttl,
        });
        // Cap stack length.
        while self.toasts.len() > 6 {
            self.toasts.pop_front();
        }
    }

    /// Drop expired toasts based on current tick.
    pub fn expire_toasts(&mut self, now: u64) {
        self.toasts
            .retain(|t| now.wrapping_sub(t.born_tick) < t.ttl_ticks);
    }

    pub fn set_status(&mut self, key: String, text: Option<String>) {
        match text {
            Some(t) if !t.is_empty() => {
                self.statuses.insert(key, t);
            }
            _ => {
                self.statuses.remove(&key);
            }
        }
    }

    pub fn set_widget(&mut self, key: String, widget: Option<Widget>) {
        match widget {
            Some(w) => {
                self.widgets.insert(key, w);
            }
            None => {
                self.widgets.remove(&key);
            }
        }
    }

    pub fn widgets_at(&self, placement: WidgetPlacement) -> Vec<&Widget> {
        self.widgets
            .values()
            .filter(|w| w.placement == placement)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_select() {
        let rest = json!({"title":"Pick","options":["A","B"]});
        let r = parse("select", "u1", &rest);
        let ExtReq::Select { id, title, options } = r else {
            panic!()
        };
        assert_eq!(id, "u1");
        assert_eq!(title, "Pick");
        assert_eq!(options, vec!["A", "B"]);
    }

    #[test]
    fn parses_confirm_with_message() {
        let rest = json!({"title":"Clear?","message":"All lost"});
        let r = parse("confirm", "u2", &rest);
        let ExtReq::Confirm { message, .. } = r else {
            panic!()
        };
        assert_eq!(message.as_deref(), Some("All lost"));
    }

    #[test]
    fn parses_notify_warning() {
        let rest = json!({"message":"m","notifyType":"warning"});
        let r = parse("notify", "", &rest);
        let ExtReq::Notify { kind, .. } = r else {
            panic!()
        };
        assert_eq!(kind, NotifyKind::Warning);
    }

    #[test]
    fn parses_set_widget_placement() {
        let rest = json!({"widgetKey":"k","widgetLines":["a","b"],"widgetPlacement":"belowEditor"});
        let r = parse("setWidget", "", &rest);
        let ExtReq::SetWidget {
            widget: Some(w), ..
        } = r
        else {
            panic!()
        };
        assert_eq!(w.placement, WidgetPlacement::BelowEditor);
        assert_eq!(w.lines, vec!["a", "b"]);
    }

    #[test]
    fn parses_set_widget_clear_when_lines_empty() {
        let rest = json!({"widgetKey":"k"});
        let r = parse("setWidget", "", &rest);
        let ExtReq::SetWidget { widget, .. } = r else {
            panic!()
        };
        assert!(widget.is_none());
    }

    #[test]
    fn parses_unknown_method() {
        let r = parse("mystery", "", &json!({}));
        assert!(matches!(r, ExtReq::Unknown(_)));
    }

    #[test]
    fn toasts_expire() {
        let mut s = ExtUiState::default();
        s.push_toast("hi".into(), NotifyKind::Info, 0);
        assert_eq!(s.toasts.len(), 1);
        s.expire_toasts(31);
        assert_eq!(s.toasts.len(), 0);
    }

    #[test]
    fn set_status_clears_on_none() {
        let mut s = ExtUiState::default();
        s.set_status("k".into(), Some("x".into()));
        assert_eq!(s.statuses.get("k").map(String::as_str), Some("x"));
        s.set_status("k".into(), None);
        assert!(!s.statuses.contains_key("k"));
    }
}
