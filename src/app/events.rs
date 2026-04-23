//! Reducer — applies a decoded `Incoming` event to `App` state.
//!
//! Extracted from `app/mod.rs` in V3.d.3. No behavior change. `App::on_event`
//! and `App::apply_state` remain as thin method wrappers that delegate into
//! the free functions here, which keeps existing callers (including the
//! ~24 reducer tests) compiling without changes.
//!
//! `import_messages` / `user_content_text` also live here — they're the
//! bootstrap-time equivalent of the streaming reducer.

use crate::rpc::events::{AssistantEvent, Incoming};
use crate::rpc::types::{
    AgentMessage, AssistantBlock, ContentBlock, State, StopReason, ToolResultPayload, UserContent,
};
use crate::ui::transcript::{BashExec, CompactionState, Entry, RetryState};

use super::{App, ComposerMode, LiveState, approx_tokens, extract_error_detail, truncate_preview};

pub(super) fn apply_state(app: &mut App, s: &State) {
    if let Some(m) = &s.model {
        app.session.model_label = format!("{}/{}", m.provider, m.id);
    }
    app.session.thinking = Some(s.thinking_level);
    app.session.steering_mode = Some(s.steering_mode);
    app.session.follow_up_mode = Some(s.follow_up_mode);
    app.session.auto_compaction = Some(s.auto_compaction_enabled);
    app.session.session_name = s.session_name.clone();
}

pub(super) fn on_event(app: &mut App, ev: Incoming) {
    app.event_received();
    match ev {
        Incoming::AgentStart => {
            app.is_streaming = true;
            app.set_live(LiveState::Llm);
            app.agent_start_tick = Some(app.ticks);
            app.tool_calls_this_turn = 0;
        }
        Incoming::AgentEnd { messages } => {
            app.is_streaming = false;
            app.composer_mode = ComposerMode::Prompt;
            app.tool_running = 0;
            // V2.12.f · for non-retryable API failures (e.g. "Your
            // credit balance is too low"), pi sets stop_reason=Error
            // + error_message on the assistant message and closes
            // the turn with agent_end. No stream-error event fires
            // before this, so we MUST scan agent_end.messages for a
            // failed assistant entry and surface it. Otherwise the
            // user sees nothing at all.
            let mut had_error = false;
            for m in &messages {
                if let AgentMessage::Assistant {
                    stop_reason: Some(StopReason::Error),
                    error_message,
                    ..
                } = m
                {
                    had_error = true;
                    let msg = error_message
                        .clone()
                        .unwrap_or_else(|| "agent ended with error".to_string());
                    app.transcript.push(Entry::Error(format!("pi: {msg}")));
                    if app.notify_enabled {
                        let body = truncate_preview(&msg, 100);
                        let _ = crate::notify::notify("pi · agent error", &body);
                    }
                    break;
                }
            }
            if had_error {
                app.set_live(LiveState::Error);
            } else {
                app.set_live(LiveState::Idle);
            }
            // Plan-mode marker handling: V3.f · parse from the
            // authoritative agent_end.messages payload first
            // (transcript tail is a fallback for bootstrap-time or
            // shape-drift cases).
            app.apply_plan_markers_on_agent_end(&messages);
            if app.notify_enabled && !had_error {
                let dur = app
                    .agent_start_tick
                    .map(|t0| app.ticks.saturating_sub(t0))
                    .unwrap_or(0);
                // Notify only on turns that took ≥ 10 s (100 ticks)
                // so quick round-trips don't spam the desktop.
                if let Some((title, body)) =
                    crate::notify::agent_end_notice(dur, 100, app.tool_calls_this_turn)
                {
                    let _ = crate::notify::notify(&title, &body);
                }
            }
            app.agent_start_tick = None;
        }
        Incoming::TurnStart => {
            app.turn_count = app.turn_count.saturating_add(1);
            // Only emit a visible separator between turns (not before the
            // first). The marker reads as a graphical "turn N" divider
            // in the transcript.
            if app.turn_count > 1 {
                app.transcript.push(Entry::TurnMarker {
                    number: app.turn_count,
                });
            }
        }
        Incoming::TurnEnd {
            message: Some(AgentMessage::Assistant { usage: Some(u), .. }),
            ..
        } => {
            if let Some(c) = u.cost {
                app.cost_session += c.total;
                app.cost_series.push_back(c.total);
                if app.cost_series.len() > 30 {
                    app.cost_series.pop_front();
                }
            }
        }
        Incoming::MessageUpdate {
            assistant_message_event,
            ..
        } => match assistant_message_event {
            AssistantEvent::TextDelta { delta, .. } => {
                app.set_live(LiveState::Streaming);
                app.push_tokens(approx_tokens(&delta));
                app.transcript.append_assistant(&delta);
            }
            AssistantEvent::ThinkingDelta { delta, .. } => {
                app.set_live(LiveState::Thinking);
                app.push_tokens(approx_tokens(&delta));
                app.transcript.append_thinking(&delta);
            }
            AssistantEvent::Error { reason, error } => {
                app.set_live(LiveState::Error);
                // Pi sends the final failed AssistantMessage in the
                // `error` field (was mis-named `partial` in older
                // Tau — silent default-to-null was masking every
                // API failure). Probe it for a human-readable message.
                let detail = extract_error_detail(&error);
                let msg = match detail {
                    Some(d) => format!("stream error ({reason:?}): {d}"),
                    None => format!("stream error: {reason:?}"),
                };
                app.transcript.push(Entry::Error(msg.clone()));
                if app.notify_enabled {
                    let body = truncate_preview(&msg, 100);
                    let _ = crate::notify::notify("pi · stream error", &body);
                }
            }
            _ => {}
        },
        Incoming::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => {
            app.tool_running = app.tool_running.saturating_add(1);
            app.tool_calls_this_turn = app.tool_calls_this_turn.saturating_add(1);
            app.set_live(LiveState::Tool);
            app.transcript.start_tool(tool_call_id, tool_name, args);
        }
        Incoming::ToolExecutionUpdate {
            tool_call_id,
            partial_result,
            ..
        } => app
            .transcript
            .update_tool_output(&tool_call_id, &partial_result),
        Incoming::ToolExecutionEnd {
            tool_call_id,
            result,
            is_error,
            ..
        } => {
            app.tool_running = app.tool_running.saturating_sub(1);
            app.tool_done = app.tool_done.saturating_add(1);
            if app.tool_running == 0 && app.is_streaming {
                app.set_live(LiveState::Llm);
            }
            if is_error && app.notify_enabled {
                let text = result
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .next()
                    .unwrap_or("");
                let first = text.lines().next().unwrap_or("").trim();
                let body = if first.is_empty() {
                    "tool call failed".to_string()
                } else {
                    truncate_preview(first, 80)
                };
                let _ = crate::notify::notify("pi · tool error", &body);
            }
            app.transcript.finish_tool(&tool_call_id, &result, is_error);
        }
        Incoming::AutoRetryStart {
            attempt,
            max_attempts,
            delay_ms,
            error_message,
        } => {
            app.set_live(LiveState::Retrying {
                attempt,
                max_attempts,
                delay_ms,
            });
            app.transcript.push_retry_waiting(
                attempt,
                max_attempts,
                delay_ms,
                error_message.unwrap_or_else(|| "transient error".into()),
            );
        }
        Incoming::AutoRetryEnd {
            success,
            attempt,
            final_error,
        } => {
            let state = if success {
                RetryState::Succeeded
            } else {
                RetryState::Exhausted(final_error.clone().unwrap_or_else(|| "unknown".into()))
            };
            if success && app.is_streaming {
                app.set_live(LiveState::Llm);
            } else {
                app.set_live(LiveState::Idle);
            }
            if !success && app.notify_enabled {
                let body = final_error
                    .as_deref()
                    .map(|s| truncate_preview(s, 80))
                    .unwrap_or_else(|| "retries exhausted".to_string());
                let _ = crate::notify::notify("pi · retries exhausted", &body);
            }
            app.transcript.resolve_retry(attempt, state);
        }
        Incoming::CompactionStart { reason } => {
            app.set_live(LiveState::Compacting);
            app.transcript.push_compaction_start(format!("{reason:?}"));
        }
        Incoming::CompactionEnd {
            reason,
            result,
            aborted,
            error_message,
            ..
        } => {
            let reason = format!("{reason:?}");
            let state = if aborted {
                CompactionState::Aborted
            } else if let Some(msg) = error_message {
                CompactionState::Failed(msg)
            } else {
                CompactionState::Done {
                    summary: result.map(|r| r.summary),
                }
            };
            if app.is_streaming {
                app.set_live(LiveState::Llm);
            } else {
                app.set_live(LiveState::Idle);
            }
            app.transcript.finish_compaction(reason, state);
        }
        Incoming::ExtensionError { error, .. } => {
            app.transcript.push(Entry::Error(format!(
                "extension error: {}",
                error.as_deref().unwrap_or("(no detail)")
            )));
        }
        // V2.12.f · pi rejected a fire-and-forget command (usually the
        // `prompt` RPC — "insufficient credits", "rate limit",
        // "context too large"). Surface it in the transcript instead
        // of silently dropping.
        Incoming::CommandError { command, message } => {
            app.is_streaming = false;
            app.composer_mode = ComposerMode::Prompt;
            app.set_live(LiveState::Error);
            app.tool_running = 0;
            app.transcript
                .push(Entry::Error(format!("{command}: {message}")));
            if app.notify_enabled {
                let body = truncate_preview(&message, 100);
                let _ = crate::notify::notify(&format!("pi · {command} failed"), &body);
            }
        }
        Incoming::QueueUpdate {
            steering,
            follow_up,
        } => {
            app.session.queue_steering = steering;
            app.session.queue_follow_up = follow_up;
        }
        _ => {}
    }
}

pub(super) fn import_messages(app: &mut App, messages: Vec<AgentMessage>) {
    for m in messages {
        match m {
            AgentMessage::User { content, .. } => {
                let text = user_content_text(&content);
                app.transcript.push(Entry::User(text));
            }
            AgentMessage::Assistant { content, .. } => {
                let mut thinking = String::new();
                let mut assistant_text = String::new();
                let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
                for block in content {
                    match block {
                        AssistantBlock::Thinking { thinking: t } => {
                            if !thinking.is_empty() {
                                thinking.push('\n');
                            }
                            thinking.push_str(&t);
                        }
                        AssistantBlock::Text { text } => {
                            if !assistant_text.is_empty() {
                                assistant_text.push('\n');
                            }
                            assistant_text.push_str(&text);
                        }
                        AssistantBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => tool_calls.push((id, name, arguments)),
                    }
                }
                if !thinking.is_empty() {
                    app.transcript.push(Entry::Thinking(thinking));
                }
                if !assistant_text.is_empty() {
                    app.transcript.push(Entry::Assistant(assistant_text));
                }
                for (id, name, args) in tool_calls {
                    app.transcript.start_tool(id, name, args);
                }
            }
            AgentMessage::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let payload = ToolResultPayload {
                    content,
                    details: serde_json::Value::Null,
                };
                app.transcript
                    .finish_tool(&tool_call_id, &payload, is_error);
            }
            AgentMessage::BashExecution {
                command,
                output,
                exit_code,
                cancelled,
                truncated,
                full_output_path,
                ..
            } => {
                app.transcript.push(Entry::BashExec(BashExec {
                    command,
                    output: crate::ui::ansi::strip(&output),
                    exit_code,
                    cancelled,
                    truncated,
                    full_output_path,
                }));
            }
        }
    }
}

fn user_content_text(c: &UserContent) -> String {
    match c {
        UserContent::Text(s) => s.clone(),
        UserContent::Blocks(bs) => bs
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => text.clone(),
                ContentBlock::Image { .. } => "[image]".into(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}
