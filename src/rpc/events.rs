//! Inbound RPC messages from pi's stdout.
//!
//! Each JSONL line is either a `response` (correlated to a command we sent) or an
//! `event` (unsolicited lifecycle / message / tool / extension-UI update).
//!
//! The `Incoming` enum covers both. We dispatch on the `type` tag and route
//! responses to the correlation map while events flow to the UI's mpsc.
//!
//! Fields we don't actively consume are kept as `serde_json::Value` so we
//! tolerate spec churn and can widen coverage in later milestones without
//! breaking the wire format.

#![allow(dead_code)] // Several variants are rendered only in later milestones.

use serde::Deserialize;

use super::types::{
    AgentMessage, CompactionReason, CompactionResult, DoneReason, ErrorReason, ToolResultPayload,
};

/// Top-level inbound message.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Incoming {
    // ── responses (correlate to outgoing command `id`) ────────────────────
    Response {
        #[serde(default)]
        id: Option<String>,
        command: String,
        success: bool,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        data: Option<serde_json::Value>,
    },

    // ── agent lifecycle ───────────────────────────────────────────────────
    AgentStart,
    AgentEnd {
        #[serde(default)]
        messages: Vec<AgentMessage>,
    },
    TurnStart,
    TurnEnd {
        #[serde(default)]
        message: Option<AgentMessage>,
        #[serde(default)]
        tool_results: Vec<AgentMessage>,
    },

    // ── messages ──────────────────────────────────────────────────────────
    MessageStart {
        message: AgentMessage,
    },
    MessageUpdate {
        /// The full (still-partial) assistant message. We keep it as Value
        /// because pi emits this MANY times per turn; deep-deserializing on
        /// every delta is wasteful and the UI reconstructs state from deltas.
        #[serde(default)]
        message: serde_json::Value,
        assistant_message_event: AssistantEvent,
    },
    MessageEnd {
        message: AgentMessage,
    },

    // ── tool execution ────────────────────────────────────────────────────
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        #[serde(default)]
        args: serde_json::Value,
        partial_result: ToolResultPayload,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: ToolResultPayload,
        #[serde(default)]
        is_error: bool,
    },

    // ── queue ─────────────────────────────────────────────────────────────
    QueueUpdate {
        #[serde(default)]
        steering: Vec<String>,
        #[serde(default)]
        follow_up: Vec<String>,
    },

    // ── compaction ────────────────────────────────────────────────────────
    CompactionStart {
        reason: CompactionReason,
    },
    CompactionEnd {
        reason: CompactionReason,
        #[serde(default)]
        result: Option<CompactionResult>,
        #[serde(default)]
        aborted: bool,
        #[serde(default)]
        will_retry: bool,
        #[serde(default)]
        error_message: Option<String>,
    },

    // ── auto-retry ────────────────────────────────────────────────────────
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        #[serde(default)]
        delay_ms: u64,
        #[serde(default)]
        error_message: Option<String>,
    },
    AutoRetryEnd {
        success: bool,
        #[serde(default)]
        attempt: u32,
        #[serde(default)]
        final_error: Option<String>,
    },

    // ── extension ─────────────────────────────────────────────────────────
    ExtensionError {
        #[serde(default)]
        extension_path: Option<String>,
        #[serde(default)]
        event: Option<String>,
        #[serde(default)]
        error: Option<String>,
    },
    ExtensionUiRequest {
        id: String,
        method: String,
        #[serde(flatten)]
        rest: serde_json::Value,
    },

    // ── synthetic (not from pi) ───────────────────────────────────────────
    //
    // V2.12.f · when pi replies to a fire-and-forget command (typically
    // `prompt` / `steer` / `follow_up`) with `{success: false}`, the
    // reader task surfaces it here so the UI can push an Entry::Error
    // instead of silently dropping the error at the RPC layer. Never
    // deserialized from the wire — only constructed internally.
    #[serde(skip)]
    CommandError {
        command: String,
        message: String,
    },
}

/// The `assistantMessageEvent` discriminator inside `message_update`.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AssistantEvent {
    Start {
        #[serde(default)]
        partial: serde_json::Value,
    },
    TextStart {
        #[serde(default)]
        content_index: usize,
        #[serde(default)]
        partial: serde_json::Value,
    },
    TextDelta {
        #[serde(default)]
        content_index: usize,
        delta: String,
        #[serde(default)]
        partial: serde_json::Value,
    },
    TextEnd {
        #[serde(default)]
        content_index: usize,
        #[serde(default)]
        content: String,
        #[serde(default)]
        partial: serde_json::Value,
    },
    ThinkingStart {
        #[serde(default)]
        content_index: usize,
        #[serde(default)]
        partial: serde_json::Value,
    },
    ThinkingDelta {
        #[serde(default)]
        content_index: usize,
        delta: String,
        #[serde(default)]
        partial: serde_json::Value,
    },
    ThinkingEnd {
        #[serde(default)]
        content_index: usize,
        #[serde(default)]
        content: String,
        #[serde(default)]
        partial: serde_json::Value,
    },
    ToolcallStart {
        #[serde(default)]
        content_index: usize,
        #[serde(default)]
        partial: serde_json::Value,
    },
    ToolcallDelta {
        #[serde(default)]
        content_index: usize,
        delta: String,
        #[serde(default)]
        partial: serde_json::Value,
    },
    ToolcallEnd {
        #[serde(default)]
        content_index: usize,
        tool_call: serde_json::Value,
        #[serde(default)]
        partial: serde_json::Value,
    },
    Done {
        reason: DoneReason,
        #[serde(default)]
        partial: serde_json::Value,
    },
    Error {
        reason: ErrorReason,
        #[serde(default)]
        partial: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Incoming {
        serde_json::from_str(s).unwrap_or_else(|e| panic!("parse failed on {s}: {e}"))
    }

    #[test]
    fn parses_response() {
        let j = r#"{"type":"response","id":"req-1","command":"prompt","success":true}"#;
        match parse(j) {
            Incoming::Response {
                id,
                command,
                success,
                ..
            } => {
                assert_eq!(id.as_deref(), Some("req-1"));
                assert_eq!(command, "prompt");
                assert!(success);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parses_agent_start_agent_end() {
        matches!(parse(r#"{"type":"agent_start"}"#), Incoming::AgentStart);
        let e = parse(r#"{"type":"agent_end","messages":[]}"#);
        matches!(e, Incoming::AgentEnd { .. });
    }

    #[test]
    fn parses_text_delta_message_update() {
        let j = r#"{
          "type":"message_update",
          "message":{},
          "assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"hi","partial":{}}
        }"#;
        let Incoming::MessageUpdate {
            assistant_message_event,
            ..
        } = parse(j)
        else {
            panic!("wrong variant")
        };
        let AssistantEvent::TextDelta { delta, .. } = assistant_message_event else {
            panic!("wrong sub-variant")
        };
        assert_eq!(delta, "hi");
    }

    #[test]
    fn parses_tool_execution_end() {
        let j = r#"{
          "type":"tool_execution_end",
          "toolCallId":"c1",
          "toolName":"bash",
          "result":{"content":[{"type":"text","text":"ok"}],"details":{}},
          "isError":false
        }"#;
        let Incoming::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            is_error,
            ..
        } = parse(j)
        else {
            panic!("wrong variant")
        };
        assert_eq!(tool_call_id, "c1");
        assert_eq!(tool_name, "bash");
        assert!(!is_error);
    }

    #[test]
    fn parses_compaction_end_with_reason_and_result() {
        let j = r#"{
          "type":"compaction_end",
          "reason":"threshold",
          "result":{"summary":"s","firstKeptEntryId":"e","tokensBefore":1234,"details":{}},
          "aborted":false,
          "willRetry":true
        }"#;
        let Incoming::CompactionEnd {
            reason, will_retry, ..
        } = parse(j)
        else {
            panic!("wrong variant")
        };
        assert_eq!(reason, CompactionReason::Threshold);
        assert!(will_retry);
    }

    #[test]
    fn parses_extension_ui_request_flattens_method_args() {
        let j = r#"{
          "type":"extension_ui_request",
          "id":"uuid-1",
          "method":"select",
          "title":"Pick",
          "options":["A","B"]
        }"#;
        let Incoming::ExtensionUiRequest { id, method, rest } = parse(j) else {
            panic!("wrong variant")
        };
        assert_eq!(id, "uuid-1");
        assert_eq!(method, "select");
        assert_eq!(rest["title"], "Pick");
        assert_eq!(rest["options"][0], "A");
    }
}
