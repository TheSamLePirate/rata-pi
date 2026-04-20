//! Outbound RPC commands.
//!
//! Every command in the pi spec is modeled. Serialization strategy:
//! - enum tag: `type` (snake_case)
//! - struct variant fields: `camelCase` (matches the wire)
//! - `id` is attached at send time via `Envelope` so every call gets a fresh id,
//!   keeping the command enum value itself reusable / cloneable.

#![allow(dead_code)] // Many commands are wired to UI only in later milestones.

use serde::Serialize;

use super::types::{FollowUpMode, SteeringMode, StreamingBehavior, ThinkingLevel};

/// Tagged envelope written to pi's stdin. `id` is optional per spec.
#[derive(Serialize, Debug)]
pub struct Envelope<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub command: &'a RpcCommand,
}

impl Envelope<'_> {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RpcCommand {
    // ── prompting ─────────────────────────────────────────────────────────
    Prompt {
        message: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImagePayload>,
        #[serde(skip_serializing_if = "Option::is_none")]
        streaming_behavior: Option<StreamingBehavior>,
    },
    Steer {
        message: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImagePayload>,
    },
    FollowUp {
        message: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImagePayload>,
    },
    Abort,
    NewSession {
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_session: Option<String>,
    },

    // ── state ─────────────────────────────────────────────────────────────
    GetState,
    GetMessages,

    // ── model ─────────────────────────────────────────────────────────────
    SetModel {
        provider: String,
        model_id: String,
    },
    CycleModel,
    GetAvailableModels,

    // ── thinking ──────────────────────────────────────────────────────────
    SetThinkingLevel {
        level: ThinkingLevel,
    },
    CycleThinkingLevel,

    // ── queue modes ───────────────────────────────────────────────────────
    SetSteeringMode {
        mode: SteeringMode,
    },
    SetFollowUpMode {
        mode: FollowUpMode,
    },

    // ── compaction ────────────────────────────────────────────────────────
    Compact {
        #[serde(skip_serializing_if = "Option::is_none")]
        custom_instructions: Option<String>,
    },
    SetAutoCompaction {
        enabled: bool,
    },

    // ── retry ─────────────────────────────────────────────────────────────
    SetAutoRetry {
        enabled: bool,
    },
    AbortRetry,

    // ── bash ──────────────────────────────────────────────────────────────
    Bash {
        command: String,
    },
    AbortBash,

    // ── session ───────────────────────────────────────────────────────────
    GetSessionStats,
    ExportHtml {
        #[serde(skip_serializing_if = "Option::is_none")]
        output_path: Option<String>,
    },
    SwitchSession {
        session_path: String,
    },
    Fork {
        entry_id: String,
    },
    GetForkMessages,
    GetLastAssistantText,
    SetSessionName {
        name: String,
    },

    // ── commands catalog ──────────────────────────────────────────────────
    GetCommands,
}

/// Image payload for `prompt`, `steer`, `follow_up`. Base64 data + mime type.
#[derive(Serialize, Debug, Clone)]
pub struct ImagePayload {
    #[serde(rename = "type")]
    pub kind: ImageKind,
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

impl ImagePayload {
    pub fn new(data: String, mime_type: String) -> Self {
        Self {
            kind: ImageKind::Image,
            data,
            mime_type,
        }
    }
}

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Image,
}

/// Typed reply to an `extension_ui_request`. This is sent as its own top-level
/// JSONL record (NOT wrapped in `Envelope`) — the `id` field here is the matching
/// `extension_ui_request.id`, not a command-correlation id.
#[derive(Serialize, Debug, Clone)]
pub struct ExtensionUiResponse {
    #[serde(rename = "type")]
    pub kind: ExtensionUiResponseTag,
    pub id: String,
    #[serde(flatten)]
    pub body: ExtensionUiResponseBody,
}

impl ExtensionUiResponse {
    pub fn value(request_id: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            kind: ExtensionUiResponseTag::ExtensionUiResponse,
            id: request_id.into(),
            body: ExtensionUiResponseBody::Value {
                value: value.into(),
            },
        }
    }

    pub fn confirmed(request_id: impl Into<String>, confirmed: bool) -> Self {
        Self {
            kind: ExtensionUiResponseTag::ExtensionUiResponse,
            id: request_id.into(),
            body: ExtensionUiResponseBody::Confirmed { confirmed },
        }
    }

    pub fn cancelled(request_id: impl Into<String>) -> Self {
        Self {
            kind: ExtensionUiResponseTag::ExtensionUiResponse,
            id: request_id.into(),
            body: ExtensionUiResponseBody::Cancelled { cancelled: true },
        }
    }
}

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionUiResponseTag {
    ExtensionUiResponse,
}

#[derive(Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ExtensionUiResponseBody {
    Value { value: String },
    Confirmed { confirmed: bool },
    Cancelled { cancelled: bool },
}

/// Associated command name, used to correlate a response with its request even
/// when `id` isn't supplied (we always supply one; the spec allows omitting).
impl RpcCommand {
    pub fn command_tag(&self) -> &'static str {
        match self {
            Self::Prompt { .. } => "prompt",
            Self::Steer { .. } => "steer",
            Self::FollowUp { .. } => "follow_up",
            Self::Abort => "abort",
            Self::NewSession { .. } => "new_session",
            Self::GetState => "get_state",
            Self::GetMessages => "get_messages",
            Self::SetModel { .. } => "set_model",
            Self::CycleModel => "cycle_model",
            Self::GetAvailableModels => "get_available_models",
            Self::SetThinkingLevel { .. } => "set_thinking_level",
            Self::CycleThinkingLevel => "cycle_thinking_level",
            Self::SetSteeringMode { .. } => "set_steering_mode",
            Self::SetFollowUpMode { .. } => "set_follow_up_mode",
            Self::Compact { .. } => "compact",
            Self::SetAutoCompaction { .. } => "set_auto_compaction",
            Self::SetAutoRetry { .. } => "set_auto_retry",
            Self::AbortRetry => "abort_retry",
            Self::Bash { .. } => "bash",
            Self::AbortBash => "abort_bash",
            Self::GetSessionStats => "get_session_stats",
            Self::ExportHtml { .. } => "export_html",
            Self::SwitchSession { .. } => "switch_session",
            Self::Fork { .. } => "fork",
            Self::GetForkMessages => "get_fork_messages",
            Self::GetLastAssistantText => "get_last_assistant_text",
            Self::SetSessionName { .. } => "set_session_name",
            Self::GetCommands => "get_commands",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn envelope_json(id: &str, cmd: RpcCommand) -> Value {
        let e = Envelope {
            id: Some(id.into()),
            command: &cmd,
        };
        serde_json::from_str(&e.to_json().unwrap()).unwrap()
    }

    #[test]
    fn prompt_round_trip_shape() {
        let j = envelope_json(
            "req-1",
            RpcCommand::Prompt {
                message: "hi".into(),
                images: vec![],
                streaming_behavior: None,
            },
        );
        assert_eq!(j, json!({"id":"req-1","type":"prompt","message":"hi"}));
    }

    #[test]
    fn prompt_with_streaming_behavior_and_image() {
        let j = envelope_json(
            "req-2",
            RpcCommand::Prompt {
                message: "look".into(),
                images: vec![ImagePayload::new("BASE64".into(), "image/png".into())],
                streaming_behavior: Some(StreamingBehavior::Steer),
            },
        );
        assert_eq!(
            j,
            json!({
                "id":"req-2",
                "type":"prompt",
                "message":"look",
                "images":[{"type":"image","data":"BASE64","mimeType":"image/png"}],
                "streamingBehavior":"steer"
            })
        );
    }

    #[test]
    fn set_model_uses_camel_case_field() {
        let j = envelope_json(
            "x",
            RpcCommand::SetModel {
                provider: "anthropic".into(),
                model_id: "claude-sonnet-4".into(),
            },
        );
        assert_eq!(
            j,
            json!({"id":"x","type":"set_model","provider":"anthropic","modelId":"claude-sonnet-4"})
        );
    }

    #[test]
    fn extension_ui_response_cancelled_shape() {
        let r = ExtensionUiResponse::cancelled("uuid-1");
        let j: Value = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(
            j,
            json!({"type":"extension_ui_response","id":"uuid-1","cancelled":true})
        );
    }

    #[test]
    fn extension_ui_response_value_shape() {
        let r = ExtensionUiResponse::value("uuid-2", "Allow");
        let j: Value = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(
            j,
            json!({"type":"extension_ui_response","id":"uuid-2","value":"Allow"})
        );
    }

    #[test]
    fn extension_ui_response_confirmed_shape() {
        let r = ExtensionUiResponse::confirmed("uuid-3", true);
        let j: Value = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(
            j,
            json!({"type":"extension_ui_response","id":"uuid-3","confirmed":true})
        );
    }

    #[test]
    fn abort_serializes_without_fields() {
        let j = envelope_json("a", RpcCommand::Abort);
        assert_eq!(j, json!({"id":"a","type":"abort"}));
    }

    #[test]
    fn compact_omits_optional_instructions() {
        let j = envelope_json(
            "c",
            RpcCommand::Compact {
                custom_instructions: None,
            },
        );
        assert_eq!(j, json!({"id":"c","type":"compact"}));
    }
}
