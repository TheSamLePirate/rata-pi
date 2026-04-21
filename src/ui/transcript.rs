//! Transcript model: ordered entries spanning user prompts, assistant text,
//! thinking blocks, tool calls with streaming output, bash executions, and
//! meta events (info / warn / error / compaction / retry).
//!
//! Streaming mutators (`append_assistant`, `append_thinking`, `update_tool_output`)
//! mutate the tail of the list in place — preserves transcript order while
//! letting deltas arrive out-of-message-boundary.

use std::fmt;

use crate::rpc::types::ToolResultPayload;

#[derive(Debug, Clone)]
pub enum Entry {
    User(String),
    Thinking(String),
    Assistant(String),
    ToolCall(ToolCall),
    BashExec(BashExec),
    Info(String),
    Warn(String),
    Error(String),
    Compaction(Compaction),
    Retry(Retry),
    /// Horizontal turn divider (emitted on `turn_start`).
    TurnMarker {
        number: u32,
    },
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub output: String,
    pub status: ToolStatus,
    pub is_error: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct BashExec {
    pub command: String,
    pub output: String,
    pub exit_code: i32,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Compaction {
    pub reason: String,
    pub state: CompactionState,
}

#[derive(Debug, Clone)]
pub enum CompactionState {
    Running,
    Done { summary: Option<String> },
    Aborted,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Retry {
    pub attempt: u32,
    pub max_attempts: u32,
    pub state: RetryState,
}

#[derive(Debug, Clone)]
pub enum RetryState {
    Waiting { delay_ms: u64, error: String },
    Succeeded,
    Exhausted(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Ok,
    Err,
}

impl fmt::Display for ToolStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Running => "…",
            Self::Ok => "✓",
            Self::Err => "✗",
        })
    }
}

#[derive(Debug, Default)]
pub struct Transcript {
    entries: Vec<Entry>,
    /// V3.b · monotonic counter bumped by every mutator. The visuals cache
    /// reads this per frame; when it's unchanged (and neither theme nor
    /// width changed, and no live streaming tail is present) the whole
    /// fingerprint walk can be skipped.
    mutation_epoch: u64,
}

impl Transcript {
    fn bump(&mut self) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
    }

    /// V3.b · read by the visuals cache to decide whether anything needs
    /// re-fingerprinting this frame.
    pub fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    pub fn push(&mut self, e: Entry) {
        self.bump();
        self.entries.push(e);
    }

    /// Append a text delta to the last assistant entry, starting one if the
    /// tail isn't already an Assistant.
    pub fn append_assistant(&mut self, delta: &str) {
        self.bump();
        match self.entries.last_mut() {
            Some(Entry::Assistant(s)) => s.push_str(delta),
            _ => self.entries.push(Entry::Assistant(delta.to_string())),
        }
    }

    /// Rewrite the text of the most recent Assistant entry. If the
    /// rewrite would leave it empty, the entry is removed. Used by the
    /// interview detector to strip the raw form JSON from the visible
    /// transcript after opening the modal.
    pub fn rewrite_last_assistant(&mut self, new_text: String) {
        if let Some(i) = self
            .entries
            .iter()
            .rposition(|e| matches!(e, Entry::Assistant(_)))
        {
            self.bump();
            if new_text.trim().is_empty() {
                self.entries.remove(i);
            } else if let Entry::Assistant(s) = &mut self.entries[i] {
                *s = new_text;
            }
        }
    }

    /// Append a thinking delta. Thinking arrives before assistant text in a
    /// turn; if the tail is already a Thinking entry we extend it.
    pub fn append_thinking(&mut self, delta: &str) {
        self.bump();
        match self.entries.last_mut() {
            Some(Entry::Thinking(s)) => s.push_str(delta),
            _ => self.entries.push(Entry::Thinking(delta.to_string())),
        }
    }

    pub fn start_tool(&mut self, id: String, name: String, args: serde_json::Value) {
        self.bump();
        self.entries.push(Entry::ToolCall(ToolCall {
            id,
            name,
            args,
            output: String::new(),
            status: ToolStatus::Running,
            is_error: false,
            expanded: false,
        }));
    }

    pub fn update_tool_output(&mut self, id: &str, payload: &ToolResultPayload) {
        if let Some(tc) = self.find_tool_mut(id) {
            tc.output = text_of_payload(payload);
            self.bump();
        }
    }

    pub fn finish_tool(&mut self, id: &str, payload: &ToolResultPayload, is_error: bool) {
        if let Some(tc) = self.find_tool_mut(id) {
            tc.output = text_of_payload(payload);
            tc.is_error = is_error;
            tc.status = if is_error {
                ToolStatus::Err
            } else {
                ToolStatus::Ok
            };
            self.bump();
        }
    }

    pub fn toggle_tool_expanded(&mut self, id: &str) {
        if let Some(tc) = self.find_tool_mut(id) {
            tc.expanded = !tc.expanded;
            self.bump();
        }
    }

    fn find_tool_mut(&mut self, id: &str) -> Option<&mut ToolCall> {
        self.entries.iter_mut().rev().find_map(|e| match e {
            Entry::ToolCall(tc) if tc.id == id => Some(tc),
            _ => None,
        })
    }

    pub fn push_compaction_start(&mut self, reason: String) {
        self.bump();
        self.entries.push(Entry::Compaction(Compaction {
            reason,
            state: CompactionState::Running,
        }));
    }

    pub fn finish_compaction(&mut self, reason: String, state: CompactionState) {
        self.bump();
        // Update the most recent matching Running entry; otherwise push new.
        for e in self.entries.iter_mut().rev() {
            if let Entry::Compaction(c) = e
                && matches!(c.state, CompactionState::Running)
                && c.reason == reason
            {
                c.state = state;
                return;
            }
        }
        self.entries
            .push(Entry::Compaction(Compaction { reason, state }));
    }

    pub fn push_retry_waiting(
        &mut self,
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error: String,
    ) {
        self.bump();
        self.entries.push(Entry::Retry(Retry {
            attempt,
            max_attempts,
            state: RetryState::Waiting { delay_ms, error },
        }));
    }

    pub fn resolve_retry(&mut self, attempt: u32, state: RetryState) {
        for e in self.entries.iter_mut().rev() {
            if let Entry::Retry(r) = e
                && r.attempt == attempt
                && matches!(r.state, RetryState::Waiting { .. })
            {
                r.state = state;
                self.bump();
                return;
            }
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

fn text_of_payload(p: &ToolResultPayload) -> String {
    let mut out = String::new();
    for block in &p.content {
        if let crate::rpc::types::ContentBlock::Text { text } = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::types::ContentBlock;

    #[test]
    fn assistant_streaming_appends_to_latest_entry() {
        let mut t = Transcript::default();
        t.append_assistant("Hel");
        t.append_assistant("lo");
        assert!(matches!(t.entries().last(), Some(Entry::Assistant(s)) if s == "Hello"));
    }

    #[test]
    fn thinking_streaming_is_separate_from_assistant() {
        let mut t = Transcript::default();
        t.append_thinking("hmm");
        t.append_assistant("out");
        assert_eq!(t.entries().len(), 2);
    }

    #[test]
    fn tool_update_then_finish_ok() {
        let mut t = Transcript::default();
        t.start_tool("c1".into(), "bash".into(), serde_json::Value::Null);
        let payload = ToolResultPayload {
            content: vec![ContentBlock::Text {
                text: "partial".into(),
            }],
            details: serde_json::Value::Null,
        };
        t.update_tool_output("c1", &payload);
        let final_payload = ToolResultPayload {
            content: vec![ContentBlock::Text {
                text: "done".into(),
            }],
            details: serde_json::Value::Null,
        };
        t.finish_tool("c1", &final_payload, false);
        let Entry::ToolCall(tc) = t.entries().last().unwrap() else {
            panic!()
        };
        assert_eq!(tc.output, "done");
        assert_eq!(tc.status, ToolStatus::Ok);
    }
}
