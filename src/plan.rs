//! Plan mode — a user-authored multi-step plan that the agent must follow.
//!
//! **Enforcement strategy**
//!
//! 1. When a plan is active, every outgoing user prompt is wrapped with the
//!    current plan state. Pi's LLM sees the full plan + a spotlight on the
//!    *current* step and two marker contracts it must emit when finishing:
//!      - `[[STEP_DONE]]`              → step complete, advance
//!      - `[[STEP_FAILED: <reason>]]` → step failed, halt auto-run
//!
//! 2. On `agent_end`, we scan the last assistant message for those markers
//!    and advance (or halt) accordingly. If the marker is absent AND the
//!    step is still active, the app can optionally send a *continue*
//!    follow-up RPC automatically — capped at `MAX_ATTEMPTS` per step so
//!    stuck plans don't loop forever.
//!
//! 3. The user can always override: `/plan done`, `/plan fail`,
//!    `/plan next`, `/plan clear`.

use std::fmt;

/// Cap on auto-continue follow-ups per step before we halt and wait for the
/// user to intervene.
pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Active,
    Done,
    Failed,
}

impl Status {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::Active => "[→]",
            Self::Done => "[x]",
            Self::Failed => "[✗]",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Done => "done",
            Self::Failed => "failed",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub text: String,
    pub status: Status,
    /// How many auto-continue follow-ups we've sent for this step.
    pub attempts: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub items: Vec<Item>,
    /// When true, on `agent_end` we automatically send a follow-up to keep
    /// the agent progressing. Switched off when a step fails or attempts
    /// hit the cap.
    pub auto_run: bool,
    pub fail_reason: Option<String>,
}

impl Plan {
    pub fn is_active(&self) -> bool {
        !self.items.is_empty() && !self.all_done()
    }

    pub fn all_done(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.status == Status::Done)
    }

    pub fn current_idx(&self) -> Option<usize> {
        self.items
            .iter()
            .position(|i| i.status == Status::Active)
            .or_else(|| self.items.iter().position(|i| i.status == Status::Pending))
    }

    pub fn current(&self) -> Option<&Item> {
        self.current_idx().and_then(|i| self.items.get(i))
    }

    pub fn count_done(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == Status::Done)
            .count()
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    pub fn set_all(&mut self, texts: Vec<String>) {
        self.items = texts
            .into_iter()
            .enumerate()
            .map(|(i, t)| Item {
                text: t,
                status: if i == 0 {
                    Status::Active
                } else {
                    Status::Pending
                },
                attempts: 0,
            })
            .collect();
        self.auto_run = true;
        self.fail_reason = None;
    }

    pub fn add(&mut self, text: String) {
        let has_active = self.items.iter().any(|i| i.status == Status::Active);
        self.items.push(Item {
            text,
            status: if has_active {
                Status::Pending
            } else {
                Status::Active
            },
            attempts: 0,
        });
    }

    /// Mark the current step done and promote the next pending to active.
    pub fn mark_done(&mut self) -> Option<String> {
        let i = self.current_idx()?;
        if self.items[i].status == Status::Active || self.items[i].status == Status::Pending {
            self.items[i].status = Status::Done;
        }
        let text = self.items[i].text.clone();
        if let Some(next) = self
            .items
            .iter()
            .position(|it| it.status == Status::Pending)
        {
            self.items[next].status = Status::Active;
            self.items[next].attempts = 0;
        }
        Some(text)
    }

    pub fn mark_fail(&mut self, reason: String) -> Option<String> {
        let i = self.current_idx()?;
        self.items[i].status = Status::Failed;
        let text = self.items[i].text.clone();
        self.fail_reason = Some(reason);
        self.auto_run = false;
        Some(text)
    }

    pub fn bump_attempt(&mut self) -> u32 {
        let Some(i) = self.current_idx() else {
            return 0;
        };
        self.items[i].attempts = self.items[i].attempts.saturating_add(1);
        self.items[i].attempts
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.auto_run = true;
        self.fail_reason = None;
    }

    /// V3.f.3 · merge an amendment into the active plan. Items whose
    /// text matches an existing step keep their status + attempts;
    /// new texts are appended as Pending. If nothing is Active after
    /// the merge, the first Pending is promoted so the plan doesn't
    /// silently stall.
    pub fn merge_amendment(&mut self, new_items: Vec<String>) {
        let mut out = Vec::with_capacity(new_items.len());
        for text in new_items {
            let keep = self
                .items
                .iter()
                .position(|it| it.text == text)
                .map(|i| self.items.remove(i));
            out.push(keep.unwrap_or(Item {
                text,
                status: Status::Pending,
                attempts: 0,
            }));
        }
        if !out.iter().any(|it| it.status == Status::Active)
            && let Some(next) = out.iter_mut().find(|it| it.status == Status::Pending)
        {
            next.status = Status::Active;
            next.attempts = 0;
        }
        self.items = out;
        self.fail_reason = None;
    }

    /// Parse `"item a | item b | item c"` into a `Vec<String>`.
    pub fn parse_list(s: &str) -> Vec<String> {
        s.split('|')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect()
    }

    /// Render the plan as a context block to prepend to user prompts.
    pub fn as_context(&self) -> String {
        let mut out = String::new();
        out.push_str("You are executing a multi-step plan. Follow it.\n\n");
        out.push_str("Plan:\n");
        for (i, it) in self.items.iter().enumerate() {
            out.push_str(&format!(
                "  {} {}. {}\n",
                it.status.marker(),
                i + 1,
                it.text
            ));
        }
        if let Some(cur) = self.current() {
            let n = self.current_idx().map(|i| i + 1).unwrap_or(0);
            out.push_str(&format!("\nFocus on step {n}: {}\n\n", cur.text));
            out.push_str("Contract:\n");
            out.push_str("- When this step is complete, end your reply with exactly:\n");
            out.push_str("    [[STEP_DONE]]\n");
            out.push_str("- If you cannot complete it, end with:\n");
            out.push_str("    [[STEP_FAILED: <short reason>]]\n");
            out.push_str("- Only emit those markers when the step is actually complete.\n\n");
        }
        out
    }
}

/// V3.f · who authored a plan or amendment proposal. Drives the UX:
/// Agent proposals go through the Plan Review modal (accept/deny/edit);
/// User-authored `/plan set …` activates immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOrigin {
    Agent,
    #[allow(dead_code)] // surfaced in V3.f.x if we ever reuse the review flow for user proposals
    User,
}

/// V3.f · a plan proposed by the agent but not yet accepted by the user.
/// Lives alongside the real `Plan` so `wrap_with_plan()` can ignore it —
/// only accepted plans participate in prompt wrapping or auto-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalKind {
    /// Brand-new plan from `[[PLAN_SET: …]]` — acceptance replaces any
    /// active plan wholesale.
    NewPlan,
    /// `[[PLAN_ADD: …]]` against an existing active plan — acceptance
    /// merges the amended list via `Plan::merge_amendment`, preserving
    /// status on matching steps.
    Amendment,
}

#[derive(Debug, Clone)]
pub struct ProposedPlan {
    pub items: Vec<String>,
    /// V3.f.2 reads this to branch on the Edit-mode semantics.
    #[allow(dead_code)]
    pub origin: PlanOrigin,
    pub kind: ProposalKind,
    /// Agent-suggested auto-run hint. The user may override in the review
    /// modal before accepting. Per V3 answer #3 we keep the answer YOLO:
    /// acceptance defaults to ON unless the user flips it.
    pub suggested_auto_run: bool,
    /// Tick at which the proposal arrived. Useful for staleness / debug.
    #[allow(dead_code)]
    pub created_at_tick: u64,
}

/// Result of scanning assistant text for the plan markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    Done,
    Failed(String),
    /// `[[PLAN_SET: a | b | c]]` — the agent proposed / replaced a plan.
    PlanSet(Vec<String>),
    /// `[[PLAN_ADD: new step]]` — the agent discovered a new step.
    PlanAdd(String),
}

/// Find every marker in assistant text, in document order. An assistant
/// reply may legitimately contain several (e.g. `[[PLAN_ADD: x]]` then
/// `[[STEP_DONE]]`), so we return them all rather than just the first.
pub fn parse_markers(text: &str) -> Vec<Marker> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let body = &after[..close];
        // Advance BEFORE dispatch so `rest` always moves forward.
        rest = &after[close + 2..];

        if body == "STEP_DONE" {
            out.push(Marker::Done);
            continue;
        }
        if let Some(r) = body.strip_prefix("STEP_FAILED") {
            let reason = r.trim_start_matches(':').trim();
            let reason = if reason.is_empty() {
                "no reason given".to_string()
            } else {
                reason.to_string()
            };
            out.push(Marker::Failed(reason));
            continue;
        }
        if let Some(r) = body.strip_prefix("PLAN_SET") {
            let payload = r.trim_start_matches(':').trim();
            if !payload.is_empty() {
                let items = Plan::parse_list(payload);
                if !items.is_empty() {
                    out.push(Marker::PlanSet(items));
                }
            }
            continue;
        }
        if let Some(r) = body.strip_prefix("PLAN_ADD") {
            let payload = r.trim_start_matches(':').trim();
            if !payload.is_empty() {
                out.push(Marker::PlanAdd(payload.to_string()));
            }
            continue;
        }
    }
    out
}

/// V3.f.3 · strip every `[[PLAN_SET: …]]`, `[[PLAN_ADD: …]]`,
/// `[[STEP_DONE]]`, and `[[STEP_FAILED: …]]` marker from `text`. Used
/// to hide protocol bracket syntax from the transcript once its
/// semantic effect has been applied — mirror of interview's strip.
/// When `show_raw_markers` is on in /settings, `apply_plan_markers_on_agent_end`
/// skips this pass so the raw bracket text shows through for debugging.
pub fn strip_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        // Push text before the "[[".
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else {
            // Unterminated "[[" — push it as-is and stop.
            out.push_str(&rest[open..]);
            return out;
        };
        let body = &after[..close];
        let body_upper = body.split_once(':').map(|(h, _)| h).unwrap_or(body);
        let is_plan_marker = matches!(
            body_upper.trim(),
            "STEP_DONE" | "STEP_FAILED" | "PLAN_SET" | "PLAN_ADD"
        );
        if is_plan_marker {
            // Drop the whole marker AND absorb a trailing newline if
            // any so we don't leave visual gaps in the assistant text.
            let consumed = after.as_ptr() as usize - rest.as_ptr() as usize + close + 2;
            rest = &rest[consumed..];
            if let Some(stripped_nl) = rest.strip_prefix('\n') {
                rest = stripped_nl;
            }
        } else {
            // Foreign "[[…]]" — keep it intact.
            out.push_str("[[");
            out.push_str(body);
            out.push_str("]]");
            rest = &after[close + 2..];
        }
    }
    out.push_str(rest);
    out.trim_end().to_string()
}

/// Short capability hint appended to user prompts when no plan is active.
/// Tells pi which markers it can emit to create / extend / advance a plan.
pub fn capability_hint() -> &'static str {
    "\n\n(rata-pi plan protocol — use these markers when useful:\n\
     [[PLAN_SET: step 1 | step 2 | step 3]]   propose or replace a plan\n\
     [[PLAN_ADD: new step]]                   append to the current plan\n\
     [[STEP_DONE]]                            end the current step\n\
     [[STEP_FAILED: reason]]                  abandon the current step)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_all_makes_first_active() {
        let mut p = Plan::default();
        p.set_all(vec!["a".into(), "b".into()]);
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].status, Status::Active);
        assert_eq!(p.items[1].status, Status::Pending);
        assert!(p.is_active());
    }

    #[test]
    fn add_when_empty_becomes_active() {
        let mut p = Plan::default();
        p.add("first".into());
        assert_eq!(p.items[0].status, Status::Active);
        p.add("next".into());
        assert_eq!(p.items[1].status, Status::Pending);
    }

    #[test]
    fn mark_done_advances() {
        let mut p = Plan::default();
        p.set_all(vec!["a".into(), "b".into()]);
        assert_eq!(p.mark_done().as_deref(), Some("a"));
        assert_eq!(p.items[0].status, Status::Done);
        assert_eq!(p.items[1].status, Status::Active);
        assert_eq!(p.mark_done().as_deref(), Some("b"));
        assert_eq!(p.items[1].status, Status::Done);
        assert!(p.all_done());
        assert!(!p.is_active());
    }

    #[test]
    fn mark_fail_halts_auto_run() {
        let mut p = Plan::default();
        p.set_all(vec!["a".into()]);
        p.mark_fail("stuck".into());
        assert_eq!(p.items[0].status, Status::Failed);
        assert_eq!(p.fail_reason.as_deref(), Some("stuck"));
        assert!(!p.auto_run);
    }

    #[test]
    fn parse_markers_done_and_failed() {
        assert_eq!(
            parse_markers("great work [[STEP_DONE]]"),
            vec![Marker::Done]
        );
        assert_eq!(
            parse_markers("oops [[STEP_FAILED: connection dropped]] retry?"),
            vec![Marker::Failed("connection dropped".into())],
        );
        assert!(parse_markers("neither").is_empty());
    }

    #[test]
    fn parse_markers_plan_set_and_add() {
        let m = parse_markers("sure: [[PLAN_SET: step a | step b | step c]]");
        assert_eq!(
            m,
            vec![Marker::PlanSet(vec![
                "step a".into(),
                "step b".into(),
                "step c".into(),
            ])]
        );
        assert_eq!(
            parse_markers("hmm [[PLAN_ADD: new discovered step]]"),
            vec![Marker::PlanAdd("new discovered step".into())],
        );
    }

    #[test]
    fn parse_markers_mixed_in_order() {
        let t = "I'll break this down. [[PLAN_SET: a | b]]\n\
                 Now doing a. [[PLAN_ADD: c]]\n\
                 Done. [[STEP_DONE]]";
        let m = parse_markers(t);
        assert_eq!(m.len(), 3);
        matches!(m[0], Marker::PlanSet(_));
        matches!(m[1], Marker::PlanAdd(_));
        assert_eq!(m[2], Marker::Done);
    }

    #[test]
    fn parse_list_respects_pipe_separator() {
        let v = Plan::parse_list("one | two with space | three");
        assert_eq!(v, vec!["one", "two with space", "three"]);
    }

    #[test]
    fn count_done_and_total() {
        let mut p = Plan::default();
        p.set_all(vec!["a".into(), "b".into(), "c".into()]);
        p.mark_done();
        assert_eq!(p.count_done(), 1);
        assert_eq!(p.total(), 3);
    }

    #[test]
    fn attempts_bump_on_step() {
        let mut p = Plan::default();
        p.set_all(vec!["a".into()]);
        assert_eq!(p.bump_attempt(), 1);
        assert_eq!(p.bump_attempt(), 2);
        assert_eq!(p.items[0].attempts, 2);
    }

    /// V3.f.3 · strip every plan marker from assistant text, leaving the
    /// prose intact.
    #[test]
    fn strip_markers_removes_all_four_kinds() {
        let src = "Sure, here's the plan: [[PLAN_SET: a | b | c]]\n\
                   Working on a. [[PLAN_ADD: d]]\n\
                   [[STEP_DONE]]\n\
                   But I had to give up. [[STEP_FAILED: tool missing]]";
        let out = strip_markers(src);
        assert!(!out.contains("[[PLAN_"), "markers leaked: {out}");
        assert!(!out.contains("[[STEP_"), "markers leaked: {out}");
        assert!(out.contains("Sure, here's the plan:"));
        assert!(out.contains("Working on a."));
        assert!(out.contains("But I had to give up."));
    }

    /// V3.f.3 · non-plan `[[foo]]` passages are untouched.
    #[test]
    fn strip_markers_keeps_foreign_brackets() {
        let src = "See [[Reference]] and [[OTHER: payload]] and [[STEP_DONE]]";
        let out = strip_markers(src);
        assert!(out.contains("[[Reference]]"));
        assert!(out.contains("[[OTHER: payload]]"));
        assert!(!out.contains("[[STEP_DONE]]"));
    }

    #[test]
    fn strip_markers_handles_unterminated() {
        // An unterminated "[[" must not panic or drop the tail.
        let src = "broken [[PLAN_SET: a";
        let out = strip_markers(src);
        assert!(out.contains("broken"));
        assert!(out.contains("[[PLAN_SET: a"));
    }
}
