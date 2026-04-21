//! Cross-cutting helpers shared by `mod.rs`, `events.rs`, `input.rs`,
//! `modals/*.rs`, and `draw.rs`. Extracted in V3.d.5 — no behavior change,
//! all functions keep `pub(super)` visibility so their siblings inside the
//! `app` module continue to see them unchanged.

/// Very rough char→token approximation — ~4 chars/token for English. Only
/// used for the live throughput sparkline, not anything billed.
pub(super) fn approx_tokens(s: &str) -> u32 {
    (s.chars().count() as u32).div_ceil(4)
}

/// Dig a human-readable error message out of the `partial` payload that
/// pi carries alongside stream errors. Providers disagree on the field
/// name (`error`, `message`, nested `error.message` for Anthropic /
/// OpenAI schemas), so we probe a few likely paths and fall back to the
/// whole object stringified.
pub(super) fn extract_error_detail(v: &serde_json::Value) -> Option<String> {
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
pub(super) fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    } else {
        s.to_string()
    }
}

/// Compact single-line preview of a tool's arguments.
pub(super) fn args_preview(args: &serde_json::Value) -> String {
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

/// Stringify a boolean as `"on"` / `"off"` for flash messages.
pub(super) fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// V3.i.1 · named replacement for `let _ = fallible_call.await`. Drops
/// the value, but when the caller passes a `reason` we log it at
/// `tracing::debug!` so an intentional-discard is auditable from a
/// trace without peppering the call sites with explicit `match`
/// blocks. Use when a fire-and-forget RPC's failure doesn't need to
/// reach the user (pi surfaces command errors via a distinct event).
pub(super) fn ignore<T, E: std::fmt::Debug>(r: Result<T, E>, reason: &'static str) {
    if let Err(e) = r {
        tracing::debug!(error = ?e, reason, "ignored");
    }
}
