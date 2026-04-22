# Plan Approval Flow Spec — `rata-pi`

Date: 2026-04-21  
Status: proposed design  
Scope: UX + state-model spec only, no code changes

## Goal

When the agent proposes a plan with `[[PLAN_SET: ...]]`, the user must be able to:

- **Accept** the plan
- **Deny** the plan
- **Edit** the plan

before the plan becomes active and before any auto-run execution starts.

---

## Problem this solves

Current behavior auto-accepts an agent-created plan and may immediately auto-run it.

That causes three UX problems:

1. the user may not notice a plan was created
2. the user does not control whether the plan should be adopted
3. execution can begin before the user has reviewed the steps

This spec changes plan creation from:

- **agent proposes → app accepts automatically**

to:

- **agent proposes → user reviews → user decides**

---

## Target behavior

## Trigger

An assistant turn ends and contains:

- `[[PLAN_SET: step 1 | step 2 | step 3]]`

## New flow

1. rata-pi parses the proposed plan.
2. rata-pi stores it as a **proposed** plan, not active.
3. rata-pi opens a **Plan Review** modal.
4. User chooses one of:
   - **Accept**
   - **Edit**
   - **Deny**
5. Only after **Accept**:
   - the plan becomes active
   - the compact plan card appears
   - future prompts are wrapped with plan context
   - auto-run may start if enabled

---

## State model

## Current state

Today there is effectively only one plan state:

- active `Plan`

## Proposed state

Introduce two layers:

### 1. Proposed plan
A temporary review object for plans suggested by the agent.

Suggested shape:

```rust
struct ProposedPlan {
    items: Vec<PlanDraftItem>,
    source: ProposedPlanSource, // Agent or User if ever reused
    raw_text: Option<String>,
    created_at_tick: u64,
    suggested_auto_run: bool,
}

struct PlanDraftItem {
    text: String,
}
```

### 2. Active plan
The existing executable `Plan` model remains the canonical running plan.

Rules:

- `proposed_plan` does **not** affect prompt wrapping
- `proposed_plan` does **not** trigger auto-run
- only `active_plan` participates in `wrap_with_plan()`

---

## Modal: Plan Review

## Purpose

A blocking modal that appears automatically when the agent proposes a new plan.

## Title

- ` proposed plan `
- or ` agent proposed a plan `

## Body

Show:

- short explanation
- list of steps
- auto-run default state
- actions

Example:

```text
The agent proposed a 4-step plan.
Review it before execution.

[ ] 1. inspect repo structure
[ ] 2. trace plan-mode state flow
[ ] 3. identify UX failures
[ ] 4. write review doc

auto-run after accept: OFF

Actions:
  Accept   Edit   Deny
```

## Required actions

### Accept
- converts proposed plan into active `Plan`
- first step becomes active
- auto-run stays OFF by default unless user explicitly enables it
- closes modal
- pushes transcript info row like:
  - `plan accepted: 4 steps`

### Edit
- enters editable plan-review state
- user can modify step text
- user can add/remove steps
- user can toggle auto-run before accept

### Deny
- discards proposed plan
- closes modal
- pushes transcript info row like:
  - `plan proposal rejected`

---

## Editing requirements

Minimum acceptable edit support:

1. move selection across steps
2. edit current step text
3. add a step below
4. delete current step
5. toggle auto-run on/off
6. accept edited plan
7. deny/cancel edited plan

Nice-to-have later:

- reorder steps
- duplicate step
- split/merge steps

## Minimal keyboard suggestion

### Review mode
- `↑↓` / `j k` → move focus between actions/steps
- `Enter` → activate focused action
- `a` → accept
- `e` → edit
- `d` → deny
- `Esc` → deny or cancel review

### Edit mode
- `↑↓` / `j k` → move between steps
- `Enter` / `i` → edit current step
- `a` → add step below
- `x` / `Del` → delete step
- `t` → toggle auto-run
- `Ctrl+S` → accept edited plan
- `Esc` → back to review or deny draft

---

## Auto-run policy

## New rule

**Fresh agent-proposed plans must never auto-run before user acceptance.**

## Recommended default after acceptance

Safer default:

- **auto-run OFF** after user accepts

Reason:

- acceptance means “yes, this plan is reasonable”
- it does not necessarily mean “start executing immediately without another prompt”

Alternative acceptable behavior:

- preserve proposed `auto_run = false`
- let user explicitly enable auto-run in review/edit modal

## Explicit recommendation

- agent proposals open with **auto-run OFF**
- user may toggle it ON before accepting

---

## Interaction with existing marker logic

## `PLAN_SET`
Current behavior:
- activate immediately

New behavior:
- create/replace `proposed_plan`
- open review modal
- do not activate yet

## `PLAN_ADD`
Recommended behavior depends on state:

### If there is an active accepted plan
- either:
  - queue as a proposed amendment requiring approval, or
  - append automatically with visible confirmation

Preferred behavior:
- treat it as a **plan amendment proposal** and ask for approval

### If there is only a proposed plan under review
- append to the proposal draft

## `STEP_DONE` / `STEP_FAILED`
These should only affect the **active accepted** plan.

They must not modify a merely proposed plan.

---

## Parsing source of truth

## Requirement

Plan proposals should be extracted primarily from:

- `Incoming::AgentEnd { messages }`

not from:

- last `Entry::Assistant` in transcript only

## Reason

The agent-end payload is the authoritative final turn output.
Transcript reconstruction is a derived UI artifact and may drift.

## Fallback

If needed:
- parse transcript text only as a compatibility fallback

---

## Visibility requirements

When a plan is proposed, the app should visibly signal it.

Required:

1. open the Plan Review modal automatically
2. push a transcript info row:
   - `agent proposed a plan: 4 steps`
3. flash a clear status message:
   - `review proposed plan`

When a plan is accepted:

1. compact plan card appears above the editor
2. transcript info row confirms acceptance
3. optional flash:
   - `plan accepted`

When denied:

1. no compact plan card
2. transcript info row confirms rejection
3. optional flash:
   - `plan rejected`

---

## Marker visibility policy

Choose one policy and keep it consistent.

## Recommended policy

- parse plan markers
- strip them from visible assistant text
- preserve the semantic effect in transcript info rows

Example:

Visible assistant text before strip:

```text
I'll break this into steps.
[[PLAN_SET: inspect repo | trace flow | write review]]
```

Visible assistant text after strip:

```text
I'll break this into steps.
```

Transcript then also gets:

- `agent proposed a plan: 3 steps`

This is cleaner than exposing protocol markers in the transcript.

---

## Suggested state transitions

## Case A: agent proposes new plan

```text
Idle/no plan
  -> receive PLAN_SET
  -> store proposed_plan
  -> open PlanReview modal
  -> wait for user decision
```

## Case B: user accepts

```text
proposed_plan
  -> Accept
  -> active_plan = Plan::set_all(...)
  -> proposed_plan cleared
  -> optional auto-run enabled if chosen
```

## Case C: user denies

```text
proposed_plan
  -> Deny
  -> proposed_plan cleared
  -> no active plan change
```

## Case D: user edits then accepts

```text
proposed_plan
  -> Edit
  -> mutate draft
  -> Accept
  -> active_plan created from edited draft
```

---

## Integration with existing `/plan` commands

## Keep
- `/plan`
- `/plan set`
- `/plan add`
- `/plan done`
- `/plan fail`
- `/plan next`
- `/plan clear`
- `/plan auto on|off`

## Suggested semantics

### `/plan set ...`
Because this is user-authored, it may still activate immediately.

Reason:
- the user is the proposer and approver in one action

### agent `PLAN_SET`
Must go through review modal.

This distinction is important and intuitive.

---

## Acceptance criteria

This feature is complete only when all are true:

1. An agent-emitted `PLAN_SET` does **not** immediately become active.
2. An agent-emitted `PLAN_SET` opens a review modal automatically.
3. The user can **Accept**, **Deny**, or **Edit** the proposal.
4. No auto-run occurs before acceptance.
5. Only an accepted plan affects `wrap_with_plan()`.
6. `STEP_DONE` / `STEP_FAILED` operate only on active accepted plans.
7. Plan proposal/accept/reject produces visible transcript feedback.
8. Parsing uses `agent_end.messages` as the primary source.
9. Marker visibility is consistent with docs and implementation.

---

## Recommended implementation order

### P0
1. add `proposed_plan` state
2. parse `PLAN_SET` into proposal, not active plan
3. create review modal with Accept/Deny
4. block auto-run until acceptance

### P1
5. add Edit mode for proposed steps
6. allow auto-run toggle in review/edit modal
7. strip plan markers from visible assistant text

### P2
8. support proposal/amendment flow for `PLAN_ADD`
9. add richer editing like reorder

---

## Test plan

Add reducer/integration tests for:

1. `PLAN_SET` creates proposal, not active plan
2. proposal opens review modal
3. accepting proposal activates plan
4. denying proposal discards it
5. edited proposal activates edited content
6. auto-run does not fire before accept
7. `wrap_with_plan()` ignores unaccepted proposals
8. `STEP_DONE` advances only accepted active plan
9. `PLAN_SET` parsed from `agent_end.messages` even if transcript tail differs

---

## Final recommendation

The right mental model is:

- **Agent plans are proposals**
- **User plans are commands**

So:

- user `/plan set ...` may activate immediately
- agent `[[PLAN_SET: ...]]` must require **accept / deny / edit**

That change would make plan mode much more understandable, much safer, and much more trustworthy.
