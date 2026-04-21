# Plan Mode Review — `rata-pi`

Date: 2026-04-21  
Reviewer: ChatGPT

## Scope

This review focuses specifically on **plan mode** as currently implemented in `rata-pi`.

Reviewed areas:

- plan state model
- marker parsing
- prompt wrapping
- agent-end processing
- auto-run behavior
- visibility in the UI
- mismatch between documented behavior and actual code
- UX implications

No code was changed.

---

## Executive summary

Plan mode is **partially implemented and functionally promising**, but it is **not yet a strong or trustworthy user-facing workflow**.

The current implementation does support:

- creating a plan from agent-emitted markers
- manually managing a plan through `/plan ...`
- wrapping future prompts with plan context
- auto-advancing through steps using `[[STEP_DONE]]` / `[[STEP_FAILED: ...]]`
- showing a compact plan card and a `/plan` modal

However, in its current state, plan mode has important product and UX flaws:

1. **Agent-created plans are auto-accepted immediately**.
2. **The user is not asked to approve, reject, or edit the plan before execution starts**.
3. **A newly created plan may be easy to miss because auto-run can continue immediately**.
4. **Plan parsing is tied to the transcript’s last assistant entry rather than directly to authoritative `agent_end.messages`**.
5. **Documentation claims plan markers are hidden, but current code does not appear to strip them**.
6. **Comments and behavior around “follow-up” vs `Prompt` are inconsistent**.

## Bottom line

**Current state:** usable as an internal mechanism  
**Current UX:** not good enough for reliable human-facing planning  
**Main requirement going forward:**

> When a plan is created by the agent, the user should be able to **accept / deny / edit** it before the app starts executing it.

That should be treated as a core workflow requirement, not optional polish.

---

## Actual current behavior

## 1. Data model is simple and sane
**Files:** `src/plan.rs`

The plan model is straightforward:

- `Plan.items: Vec<Item>`
- `Plan.auto_run: bool`
- `Plan.fail_reason: Option<String>`

Each item tracks:

- `text`
- `status` (`Pending`, `Active`, `Done`, `Failed`)
- `attempts`

This part is good. The state model is understandable and small.

### Status behavior
- first plan item becomes `Active`
- later items are `Pending`
- `mark_done()` moves the next pending item to active
- `mark_fail()` sets the current step to failed and disables auto-run

---

## 2. Marker grammar exists and is parsed correctly
**Files:** `src/plan.rs`

Supported markers:

- `[[PLAN_SET: a | b | c]]`
- `[[PLAN_ADD: text]]`
- `[[STEP_DONE]]`
- `[[STEP_FAILED: reason]]`

`parse_markers()` scans assistant text in order and returns multiple markers if present.

This parser is fine for the current protocol.

---

## 3. Plan capability hint is injected when no plan is active
**Files:** `src/app/mod.rs`, `src/plan.rs`

When no plan is active, outgoing prompts are appended with a capability hint describing the marker protocol.

This means the model is taught that it may propose a plan.

This is a reasonable design.

---

## 4. When a plan is active, future prompts are wrapped with plan context
**Files:** `src/app/mod.rs`, `src/plan.rs`

`wrap_with_plan()` prepends:

- current plan list
- current active step
- marker contract for completion/failure

This is one of the strongest parts of the implementation. Once a plan exists, the system gives the model solid context.

---

## 5. Plan markers are applied only at `agent_end`
**Files:** `src/app/mod.rs`

`apply_plan_markers_on_agent_end()` is called on `Incoming::AgentEnd`.

It:

- finds the most recent `Entry::Assistant(...)` in the transcript
- parses plan markers from that text
- mutates `app.plan`
- pushes transcript info/warn entries
- may stage the next automatic prompt in `pending_auto_prompt`

This is the heart of plan mode today.

---

## 6. New agent-created plans are immediately auto-accepted
**Files:** `src/plan.rs`, `src/app/mod.rs`

When the agent emits `[[PLAN_SET: ...]]`, the app does this immediately:

- `self.plan.set_all(items)`
- pushes `Entry::Info("plan set by agent: N steps")`
- if `auto_run` is on, stages execution of the first step

`Plan::set_all()` sets:

- first step active
- `auto_run = true`

So an agent-created plan is not treated as a proposal for review. It is treated as an already accepted executable plan.

This is the single biggest product problem.

---

## 7. Auto-run may continue immediately after plan creation
**Files:** `src/app/mod.rs`

After a plan is created, if plan is active and `auto_run` is true, the app stages:

- `Proceed with step N: ...`

Then `ui_loop()` sends that prompt automatically using `RpcCommand::Prompt`.

Effectively:

1. agent proposes plan
2. app accepts it instantly
3. app may continue execution instantly

This makes plan creation easy to miss and gives the user little control.

---

## 8. There is UI for viewing plans, but not enough for control at creation time
**Files:** `src/app/draw.rs`, `src/app/mod.rs`

Current plan UI:

- compact plan card above the editor while a plan is active or all done
- full `/plan` modal (`Modal::PlanView`)
- slash commands:
  - `/plan`
  - `/plan set`
  - `/plan add`
  - `/plan done`
  - `/plan fail`
  - `/plan next`
  - `/plan clear`
  - `/plan auto on|off`

This is useful after the plan exists.

But it does **not** solve the main UX problem:

- there is no dedicated **approval/edit modal** for agent-proposed plans
- there is no explicit **accept / deny / edit** decision point

---

## Key criticisms

## 1. Agent-created plans should not auto-execute without user approval
**Severity:** High

This is the most important issue.

A plan is not just metadata. It is a commitment about what the app/agent should do next.

Current behavior is effectively:

- the agent proposes the plan
- the app accepts on the user’s behalf
- execution may continue immediately

That is too aggressive.

### Required change
When the agent emits `[[PLAN_SET: ...]]`, the app should open a dedicated review flow where the user can:

- **Accept** the plan
- **Deny** the plan
- **Edit** the plan before activation

Until the user accepts:

- the plan should be stored as **proposed**, not active
- it should **not** wrap future prompts as authoritative plan context
- it should **not** auto-run

### Recommendation
Introduce a two-stage model:

- `proposed_plan`
- `active_plan`

Flow:

1. agent emits `PLAN_SET`
2. app stores it as `proposed_plan`
3. app opens a modal: review / edit / accept / deny
4. only on accept does it become `active_plan`
5. only then may auto-run begin

This should be considered mandatory.

---

## 2. Parsing plan markers from transcript text is fragile
**Severity:** High

`apply_plan_markers_on_agent_end()` currently searches the transcript for the most recent assistant entry and parses markers from that text.

That is weaker than parsing directly from the `Incoming::AgentEnd { messages }` payload.

### Why this is risky
If the transcript tail and the authoritative agent-end payload ever diverge, plan logic can fail silently.

Potential failure cases:

- streaming transcript entry incomplete or missing
- future backend event-shape changes
- assistant content present in `agent_end.messages` but not in the transcript tail
- other assistant-like entries appearing after the actual plan-bearing message

### Recommendation
Plan detection should first parse from `agent_end.messages` directly.

Transcript scanning can be a fallback, not the primary source.

---

## 3. Visibility is too subtle; users can easily miss that a plan was created
**Severity:** High

Current visible effects when the agent creates a plan:

- one transcript info row: `plan set by agent: N steps`
- compact plan card above the editor
- optional auto-run continues immediately

That is not enough.

### Problem
The user may not notice:

- a plan was created
- what the plan contains
- that execution has already started
- whether the agent is still following the plan

### Recommendation
When an agent proposes a plan, do at least one of these:

- open a plan review modal automatically
- show a blocking accept/deny/edit modal
- show a strong toast: `agent proposed a 4-step plan — review now`

Best option: **blocking review modal**.

---

## 4. Documentation says plan markers are hidden, but code does not appear to strip them
**Severity:** Medium

`docs/USER_GUIDE.md` says:

> These markers are hidden from the visible transcript rendering.

But unlike interview markers, I did not find equivalent stripping logic for plan markers.

Interview markers are explicitly:

- detected
- stripped from assistant text
- rewritten in transcript

Plan markers appear to be:

- parsed
- applied
- left as-is in the assistant transcript text

### Recommendation
Pick one and make it true everywhere:

1. **Strip plan markers from visible assistant text**, or
2. **Update docs** to say they may remain visible

Current state is inconsistent.

---

## 5. Auto-run behavior is too eager for fresh plans
**Severity:** Medium/High

The current logic stages automatic prompts when:

- a new plan is created
- a step advances
- a step remains active without completion

This makes sense for an already accepted plan.

It does **not** make sense for a newly proposed plan.

### Recommendation
Differentiate these cases:

- **newly proposed plan** → do not auto-run
- **accepted active plan** → auto-run permitted if enabled

That distinction is essential.

---

## 6. Comments and implementation are inconsistent around “follow-up” vs `Prompt`
**Severity:** Medium

Comments around plan auto-run mention a follow-up being dispatched after `agent_end`.

But the code in `ui_loop()` dispatches:

- `RpcCommand::Prompt`

not `FollowUp`.

That may be intentional, but the code and the comments disagree.

### Recommendation
Clarify which semantics are correct:

- if plan continuation should be a fresh prompt, update comments
- if it should be a follow-up, update behavior

The current ambiguity makes maintenance harder.

---

## 7. End-to-end test coverage looks weaker than it should be
**Severity:** Medium

`src/plan.rs` has parser/state tests.

That is good.

But plan mode would benefit from more reducer/integration-style tests validating:

- agent emits `PLAN_SET` → plan proposal/review flow appears
- accepted plan becomes active
- fresh proposals do not auto-run before approval
- `STEP_DONE` advances active step correctly
- `STEP_FAILED` halts correctly
- max attempts disables auto-run
- plan parsing from `agent_end.messages` works even if transcript tail differs

Given how behavioral/UX-sensitive plan mode is, this deserves explicit coverage.

---

## What is currently good

To be fair, several parts are already good:

### Good 1. Clean minimal state model
The plan state itself is compact and understandable.

### Good 2. Marker parser is simple and practical
The flat marker protocol is realistic for model output.

### Good 3. Plan context injection is strong
Once a plan is active, prompt wrapping is conceptually good.

### Good 4. Manual control exists
`/plan ...` commands are useful for debugging and power users.

### Good 5. Attempt cap exists
`MAX_ATTEMPTS = 3` prevents endless loops.

So this is not a bad foundation. The main issues are around **authority, visibility, and UX control**.

---

## Required product behavior going forward

This should be the target behavior.

## When the agent proposes a plan
Trigger: assistant emits `[[PLAN_SET: ...]]`

### The app should:
1. Parse the proposal.
2. Store it as a **proposed** plan, not active.
3. Open a modal with the proposed steps.
4. Let the user choose:
   - **Accept**
   - **Edit**
   - **Deny**
5. Only after **Accept**:
   - move it to active plan
   - show compact plan card
   - begin wrapping prompts with plan context
   - optionally start auto-run if enabled

## If the user chooses Edit
The user should be able to:

- add/remove/reorder steps
- change text of steps
- toggle auto-run before activation

Even a minimal edit flow is better than auto-accepting.

## If the user chooses Deny
The app should:

- discard the proposed plan
- optionally push an info row like `plan proposal rejected`
- not activate plan mode
- not auto-run anything

---

## Suggested implementation direction

### 1. Split plan state into proposed vs active
Possible structure:

- `proposed: Option<PlanDraft>`
- `active: Plan`

Where `PlanDraft` may contain:

- proposed items
- origin (`agent` / `user`)
- raw source text if useful

### 2. Add a dedicated plan review modal
New modal should support:

- read proposed steps
- accept
- deny
- edit

### 3. Auto-run only from accepted active plans
Never from `PLAN_SET` directly.

### 4. Parse from `agent_end.messages`
Use transcript text only as fallback.

### 5. Decide marker visibility policy explicitly
Either strip or document them honestly.

---

## Priority list

## P0
1. **Require user accept/deny/edit for agent-created plans**.
2. **Do not auto-run a newly proposed plan before acceptance**.
3. **Parse plan markers from `agent_end.messages` directly**.

## P1
4. Open a dedicated plan review modal automatically on agent proposal.
5. Improve visible feedback when a plan is proposed/accepted/rejected.
6. Clarify `Prompt` vs `FollowUp` semantics in comments and code.

## P2
7. Strip plan markers from visible transcript or update docs.
8. Add reducer/integration tests for full plan lifecycle.

---

## Final assessment

Plan mode today is **a decent internal mechanism** but **not yet a good user-facing planning workflow**.

### Current score
- state model: **7/10**
- parser/protocol: **7/10**
- execution control: **4/10**
- visibility/UX: **4/10**
- overall readiness: **5/10**

### Final verdict
The current implementation should be treated as:

- **prototype-quality UX**
- **not acceptable final behavior** for agent-created plans

The most important design rule going forward is simple:

> **A plan proposed by the agent must be reviewed by the user before it becomes active.**

That means the user must be able to:

- **Accept** it
- **Deny** it
- **Edit** it

Without that, plan mode is too automatic and too easy to distrust.
