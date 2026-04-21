//! Domain types mirroring pi's JSON shapes.
//!
//! Source of truth for the wire format is the pi docs at `docs/rpc.md`.
//! We use serde attributes to match pi's mixed conventions:
//! - event/response `type` tags are `snake_case`
//! - most object fields are `camelCase`
//! - some enum values use `kebab-case` (`one-at-a-time`) or `lowercase` (`xhigh`)
//!
//! Unknown fields are tolerated via `#[serde(default)]` and `Option<…>` so we don't
//! crash when pi evolves. Fields we don't yet render stay as `serde_json::Value` so
//! downstream milestones can widen coverage without churning existing code.

#![allow(dead_code)] // Many types are scaffolding for M2+ but are written once here.

use serde::{Deserialize, Serialize};

// ───────────────────────────────────────────────────────────── models / thinking ──

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub cost: Option<ModelCost>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SteeringMode {
    All,
    OneAtATime,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FollowUpMode {
    All,
    OneAtATime,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamingBehavior {
    Steer,
    FollowUp,
}

// ─────────────────────────────────────────────────────────────────── content ──

/// Text or image block used inside message content and tool results.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

/// Assistant message content block: text, thinking, or tool call.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolCall {
        id: String,
        name: String,
        #[serde(default)]
        arguments: serde_json::Value,
    },
}

/// User content can be a plain string OR an array of blocks (with images).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub extracted_text: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
}

// ─────────────────────────────────────────────────────────────────── usage ──

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub cost: Option<Cost>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
    #[serde(default)]
    pub total: f64,
}

// ─────────────────────────────────────────────────────────────────── messages ──
//
// Wire shape: a flat object with `role` as the discriminator, e.g.
//   { "role": "user", "content": "hi", "timestamp": 1234, "attachments": [] }
// We use an internally-tagged enum keyed on `role`.

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(
    tag = "role",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentMessage {
    User {
        content: UserContent,
        #[serde(default)]
        timestamp: i64,
        #[serde(default)]
        attachments: Vec<Attachment>,
        #[serde(default)]
        entry_id: Option<String>,
    },
    Assistant {
        content: Vec<AssistantBlock>,
        #[serde(default)]
        api: Option<String>,
        #[serde(default)]
        provider: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        usage: Option<Usage>,
        #[serde(default)]
        stop_reason: Option<StopReason>,
        /// Present when `stop_reason == Error` (or rarely `Aborted`) —
        /// carries the provider's error message, e.g. "Your credit
        /// balance is too low to access the Claude API."
        #[serde(default)]
        error_message: Option<String>,
        #[serde(default)]
        timestamp: i64,
        #[serde(default)]
        entry_id: Option<String>,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: Vec<ContentBlock>,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        timestamp: i64,
        #[serde(default)]
        entry_id: Option<String>,
    },
    BashExecution {
        command: String,
        output: String,
        #[serde(default)]
        exit_code: i32,
        #[serde(default)]
        cancelled: bool,
        #[serde(default)]
        truncated: bool,
        #[serde(default)]
        full_output_path: Option<String>,
        #[serde(default)]
        timestamp: i64,
        #[serde(default)]
        entry_id: Option<String>,
    },
}

/// Result payload shared by `tool_execution_update.partialResult` and
/// `tool_execution_end.result`.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolResultPayload {
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub details: serde_json::Value,
}

// ───────────────────────────────────────────────────────────── session state ──

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct State {
    #[serde(default)]
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub steering_mode: SteeringMode,
    pub follow_up_mode: FollowUpMode,
    #[serde(default)]
    pub session_file: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub session_name: Option<String>,
    pub auto_compaction_enabled: bool,
    #[serde(default)]
    pub message_count: u64,
    #[serde(default)]
    pub pending_message_count: u64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    #[serde(default)]
    pub session_file: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub user_messages: u64,
    #[serde(default)]
    pub assistant_messages: u64,
    #[serde(default)]
    pub tool_calls: u64,
    #[serde(default)]
    pub tool_results: u64,
    #[serde(default)]
    pub total_messages: u64,
    #[serde(default)]
    pub tokens: StatsTokens,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub context_usage: Option<ContextUsage>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StatsTokens {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub total: u64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    #[serde(default)]
    pub tokens: Option<u64>,
    pub context_window: u64,
    #[serde(default)]
    pub percent: Option<f64>,
}

// ─────────────────────────────────────────────────────────── compaction ──

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompactionResult {
    pub summary: String,
    #[serde(default)]
    pub first_kept_entry_id: Option<String>,
    #[serde(default)]
    pub tokens_before: u64,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CompactionReason {
    Manual,
    Threshold,
    Overflow,
}

// ─────────────────────────────────────────────────────────── commands catalog ──

#[derive(Deserialize, Debug, Clone)]
pub struct CommandInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub source: CommandSource,
    #[serde(default)]
    pub location: Option<CommandLocation>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommandSource {
    Extension,
    Prompt,
    Skill,
    /// Local built-in of rata-pi (never emitted by pi; present so we can
    /// mix them into the Commands picker alongside extension/prompt/skill).
    #[serde(skip_deserializing)]
    Builtin,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommandLocation {
    User,
    Project,
    Path,
}

// ────────────────────────────────────────────────────── fork / assistant events ──

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForkMessage {
    pub entry_id: String,
    pub text: String,
}

/// Done reason emitted inside `message_update.assistantMessageEvent.done`.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DoneReason {
    Stop,
    Length,
    ToolUse,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorReason {
    Aborted,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_user_message_with_string_content() {
        let json = r#"{"role":"user","content":"hi","timestamp":1,"attachments":[]}"#;
        let m: AgentMessage = serde_json::from_str(json).unwrap();
        match m {
            AgentMessage::User {
                content: UserContent::Text(t),
                ..
            } => assert_eq!(t, "hi"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserializes_assistant_with_tool_call() {
        let json = r#"{
          "role":"assistant",
          "content":[
            {"type":"text","text":"hi"},
            {"type":"toolCall","id":"c1","name":"bash","arguments":{"command":"ls"}}
          ],
          "timestamp":1
        }"#;
        let m: AgentMessage = serde_json::from_str(json).unwrap();
        let AgentMessage::Assistant { content, .. } = m else {
            panic!("wrong variant")
        };
        assert_eq!(content.len(), 2);
        matches!(&content[1], AssistantBlock::ToolCall { name, .. } if name == "bash");
    }

    #[test]
    fn steering_mode_is_kebab_case() {
        assert_eq!(
            serde_json::to_string(&SteeringMode::OneAtATime).unwrap(),
            "\"one-at-a-time\""
        );
    }

    #[test]
    fn thinking_level_xhigh_is_lowercase() {
        assert_eq!(
            serde_json::to_string(&ThinkingLevel::Xhigh).unwrap(),
            "\"xhigh\""
        );
    }

    #[test]
    fn state_deserialization_accepts_minimal_payload() {
        let json = r#"{
          "thinkingLevel":"medium",
          "isStreaming":false,
          "isCompacting":false,
          "steeringMode":"all",
          "followUpMode":"one-at-a-time",
          "autoCompactionEnabled":true
        }"#;
        let s: State = serde_json::from_str(json).unwrap();
        assert_eq!(s.thinking_level, ThinkingLevel::Medium);
        assert_eq!(s.steering_mode, SteeringMode::All);
        assert_eq!(s.follow_up_mode, FollowUpMode::OneAtATime);
    }
}
