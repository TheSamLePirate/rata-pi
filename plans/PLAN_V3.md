# rata-pi — V3 Master Plan

**Baseline:** commit `2651d8a` (V2.13 shipped · 194 tests · clippy clean · fmt clean · 5.3 MiB release).
**Sources consolidated:** `code_review.md` (ChatGPT), `code_ux_review.md` (Claude), `plan_mode_review.md` (ChatGPT), `plan_approval_flow.md` (proposed spec).
**Cadence:** sub-milestones (V3.a → V3.j). Each is independently shippable, green-gated (tests · clippy `-D warnings` · fmt · release build), and committed on its own.

---

## Design decisions locked for V3

| # | Decision | Rationale |
|---|---|---|
| 1 | Interview mode stays as-is — **no** accept/deny/edit shell around it. | Interview is already interactive; the plan-mode critique does not transfer. |
| 2 | Plan-edit mode is **minimum**: edit text, add step, delete step, toggle auto-run. Nothing richer in V3. | Escalate only if users ask. Reorder/merge/split deferred. |
| 3 | After user accepts a plan, **auto-run goes YOLO** (ON). Acceptance is explicit consent. | User called it: accept = go full speed. Aligns with the "user plans are commands" principle. |
| 4 | Protocol markers (plan + interview) are **stripped from transcript by default**, with a `/settings → "show raw markers"` toggle for debugging. | Best of both: clean default, transparent when needed. |
| 5 | Sub-milestone cadence (V3.a → V3.j). Each ships green and commits separately. | Matches V2 workflow; bisectable history. |
| 6 | Module split is **medium**: extract the two modals (`interview`, `settings`), plus `events.rs` (reducer) and `input.rs` (key handling). Target `mod.rs` ≈ 5 000 lines. | Full 8-module split would stall other work. |
| 7 | RPC timeouts are **per-call hardcoded**: bootstrap 3 s, stats 1 s, user actions 10 s. Timeouts surface as non-fatal flashes. | Different calls want different bounds; single-global-timeout is a blunt tool. |
| 8 | **Defer nothing** from the reviews — all P0/P1/P2/P3 items are in V3 scope. | User directive. |

---

## Sub-milestones at a glance

| ID | Theme | Effort | Ships |
|---|---|---|---|
| V3.a | RPC reliability | ~1 day | Pending-leak fix · per-call timeouts · offline flash · error surfacing audit |
| V3.b | Perf regressions | ~1 day | `build_settings_rows` cache · term_caps cache · History path cache · file-preview bounded reads · incremental visuals cache |
| V3.c | CI hardening | ~15 min | Linux in matrix · clippy `-D warnings` · `--features notify` build · release smoke |
| V3.d | Module split (medium) | ~2 days | `app/modals/interview.rs` · `app/modals/settings.rs` · `app/events.rs` · `app/input.rs` · `app/helpers.rs` |
| V3.e | UX quick wins | ~1 day | `/help` refresh · Tab in /settings · `FlashKind` color-coded · slash aliases in catalog · Ctrl+Enter composer · modal close-key consistency · cycle-row arrow cleanup · narrow-hint responsiveness |
| V3.f | Plan approval flow | ~3 days | `ProposedPlan` state · review modal · edit mode · parse from `agent_end.messages` · marker strip + `/settings` toggle · `PLAN_ADD` amendment flow · lifecycle tests |
| V3.g | Theme + docs | ~1 day | Theme-aware markdown · per-theme syntect palette · README refresh · guide updates |
| V3.h | Testing hardening | ~2 days | Mock-pi integration harness · plan lifecycle tests · settings/interview dispatch tests · shortcuts-scroll regression |
| V3.i | Accessibility + polish | ~1 day | Mouse in modals · high-contrast theme · `ignore()` helper · flash icons · state-aware sparkline colors · focus redundancy knob |
| V3.j | P3 features | ~3 days | Undo/redo composer · composer templates · transcript search · theme persistence TOML · TODO widget · composer draft auto-save |

**Order rationale.** Quick correctness + perf + CI first (V3.a–c) — they are fast and unblock everything. Module split next (V3.d) so the plan-approval work in V3.f lands in cleaner files. UX quick wins (V3.e) ship steady improvements. V3.f is the star product change and lands on a solid foundation. V3.g–j round out the review backlog.

---

## V3.a — RPC reliability

**Goal:** the UI never freezes waiting on pi. Pending state never leaks.

### Tasks
- [ ] `RpcClient::call()` — on `send()` failure, **remove** the pending entry before returning `Err(RpcError::Closed)`. Ref `code_review.md §3`.
- [ ] Introduce `RpcClient::call_timeout(cmd, Duration) -> Result<Value, RpcError>`. Implement via `tokio::time::timeout`.
  - Keep `call()` as `call_timeout(cmd, Duration::from_secs(10))` so existing sites get a default bound.
- [ ] Wrap the three hot paths explicitly:
  - `bootstrap()` — each sub-call at 3 s.
  - `refresh_stats()` — 1 s (called every stats tick from `ui_loop`).
  - User-initiated slash/modal RPC — 10 s.
- [ ] Surface `RpcError::Timeout` as a non-fatal flash (`"pi didn't answer in {}s"`) — don't poison the session.
- [ ] **Offline flash**: in `dispatch_settings_action` for `AutoCompact`/`AutoRetry` toggles, when `client.is_none()` add `app.flash("offline — change applies next session")`. Ref `code_ux_review.md P0 #6`.

### Tests
- [ ] Unit: dropped writer channel → `call()` returns `Closed` **and** pending map is empty (regression for the leak).
- [ ] Unit: `call_timeout` returns `Timeout` when the oneshot never resolves within the bound.
- [ ] Reducer: offline toggle flips local state and pushes a flash.

### Acceptance
- `cargo test --locked --all-features --all-targets` green.
- `cargo clippy --all-features --all-targets -- -D warnings` clean.
- Killing pi mid-session does not freeze the TUI; flashes appear instead.

### References
- `/Users/olivierveinand/.nvm/versions/node/v24.14.0/lib/node_modules/@mariozechner/pi-coding-agent/docs/rpc.md` — expected response times.
- `src/rpc/client.rs` — current `call()`.
- `src/app/mod.rs` — bootstrap, `refresh_stats`, `dispatch_settings_action`.

---

## V3.b — Perf regressions

**Goal:** eliminate the V2.13.b perf debt. No disk reads, clipboard opens, or env walks per frame.

### Tasks
- [ ] **Cache static settings rows.** Move clipboard availability, history file path, state dir, build info, `term_caps` behind a `AppCaps` struct computed once in `App::new()` and accessed via `&app.caps`. Only volatile rows (queue counts, turn counter, live cost, theme name) rebuild per frame. Ref `code_ux_review.md P0 #1`.
- [ ] `History::default_path() -> Option<PathBuf>` — cheap static accessor that does **not** load the JSONL. Use from `/doctor` and `/settings`. Ref `code_ux_review.md P1 #11`.
- [ ] `term_caps::detect()` called exactly once at startup; cached result re-read everywhere. Ref `code_ux_review.md P1 #10`.
- [ ] **File preview bounded reads.** In `src/files.rs::preview()`:
  - Open with `BufReader`, call `metadata()` first.
  - If size > 50 MiB, bail before reading content.
  - Otherwise read only up to the needed byte cap (≈ 8 KiB) via `.take(cap)`.
  - Offload via `spawn_blocking` like the file walk already does.
  Ref `code_review.md §4`.
- [ ] **Incremental visuals cache.** In `src/app/visuals.rs::update_visuals_cache`, stop re-fingerprinting the whole transcript. Track a `dirty_from: Option<usize>` on `App` that mutations bump; cache recompute starts at that index. Width/theme changes invalidate all. Ref `code_review.md §6`.

### Tests
- [ ] Unit: `AppCaps` populated once; second call does not re-probe clipboard (track via a test-only counter or behind `#[cfg(test)]` injection).
- [ ] Unit: `files::preview` on a fake 100 MiB sparse file returns `None`/early without reading content.
- [ ] Unit: `update_visuals_cache` with `dirty_from = None` short-circuits (no fingerprint calls).
- [ ] Existing visuals cache regression tests still pass.

### Acceptance
- Opening `/settings` and mashing arrow keys does not hit disk (verify with `strace -e openat` on Linux, or `dtruss -t open` on macOS).
- File-finder selection over a huge file stays snappy.
- Long transcript (> 500 entries) frame cost stays flat.

### References
- `src/app/mod.rs:5660-5738` — current `build_settings_rows`.
- `src/files.rs::preview`.
- `src/app/visuals.rs::update_visuals_cache`.

---

## V3.c — CI hardening

**Goal:** CI catches everything a local `cargo …` catches.

### Tasks
- [ ] `.github/workflows/ci.yml` — add `ubuntu-latest` to the test matrix. Ref `code_review.md §8`, `code_ux_review.md P0 #7`.
- [ ] Clippy job: pass `clippy_flags: -- -D warnings` explicitly (don't rely on action defaults). Ref `code_ux_review.md P0 #8`.
- [ ] New `build-notify` job: `cargo build --features notify` on Ubuntu. Ref `code_ux_review.md P1 #13`.
- [ ] New `build-release` job: `cargo build --release` on Ubuntu as a smoke test. Ref `code_ux_review.md P1 #14`.

### Tests
- Not applicable (CI config); verify via a throwaway PR on a branch if needed.

### Acceptance
- Green CI on all four jobs across Ubuntu + macOS + Windows for tests.

---

## V3.d — Module split (medium)

**Goal:** `src/app/mod.rs` drops from 8 266 → ~5 000 lines. Each extracted file has one responsibility.

### Tasks
- [ ] `src/app/modals/mod.rs` — public re-export hub.
- [ ] `src/app/modals/interview.rs` — move interview modal key handling, draw body, field focus math, dispatch. ~500 lines.
- [ ] `src/app/modals/settings.rs` — move `SettingsRow`, `ToggleAction`, `CycleAction`, `build_settings_rows`, `settings_body`, `settings_modal_key`, `dispatch_settings_action`, `settings_row_source_line`. ~700 lines.
- [ ] `src/app/events.rs` — move `App::on_event`, transcript mutators, bootstrap import, live-state transitions. ~1 200 lines.
- [ ] `src/app/input.rs` — move `App::handle_key` (global composer key handling). ~600 lines.
- [ ] `src/app/helpers.rs` — cross-cutting helpers: `truncate_preview`, `args_preview`, `approx_tokens`, `on_off`. `fmt_elapsed` stays in `draw.rs` (already `pub(super)`). Ref `code_ux_review.md P2 #20`.
- [ ] Keep reducer tests, visuals tests, and interview tests alongside the modules they cover (move out of the mega `#[cfg(test)] mod tests` block in `mod.rs`).

### Discipline
- **No behavior change.** Every `cargo test` between sub-steps should stay green.
- Split into 5 commits, one per new file, so bisect points at any regression immediately.
- After each extraction, run `cargo fmt` + `cargo clippy -- -D warnings` before committing.

### Tests
- All 194 existing tests continue to pass across the split.
- Where a test moves files, verify it still runs via `cargo test <name>`.

### Acceptance
- `src/app/mod.rs` < 5 100 lines.
- Extracted modules each < 1 500 lines.
- CI green across the full split.

### References
- V2.12's extraction of `draw.rs` and `visuals.rs` is the pattern to follow.
- Ratatui example `user_input.rs` (`/Users/olivierveinand/Documents/DEV/rataPi/ratatui/examples/apps/user_input`) for the input-handling module shape.

---

## V3.e — UX quick wins

**Goal:** a tight batch of user-visible improvements that each take minutes to write.

### Tasks
- [ ] **`/help` refresh.** Replace the stale 10-command body with a compact essentials block + pointers to `/shortcuts` and `/settings`. Ref `code_ux_review.md P0 #3`.
- [ ] **Tab / Shift+Tab in `/settings`.** Bind Tab = Down-skipping-Headers, Shift+Tab = Up-skipping-Headers in `settings_modal_key`. Ref `code_ux_review.md P0 #2`.
- [ ] **`FlashKind`.** Introduce `enum FlashKind { Info, Success, Warn, Error }`; change `App::flash(..)` signature to take a kind; add `App::flash_info/success/warn/error` helpers. Update ~40 call sites. Footer renders with theme color per kind. Ref `code_ux_review.md P0 #4`.
- [ ] **Slash aliases in catalog.** Add `/keys`, `/hotkeys`, `/prefs`, `/preferences` as catalog entries (or as `aliases: &[&str]` on the existing entries — prefer the latter, extend the picker filter to match aliases). Ref `code_ux_review.md P0 #5`.
- [ ] **Cycle-row arrow cleanup.** In `/settings`, rows whose action is `CycleModel` / `CycleThinkingLevel` ignore `←` (pi has no backward RPC). Remove the `◂` visual affordance on those rows; keep `▸` only. Theme still supports real backward cycling. Ref `code_ux_review.md P1 #15`.
- [ ] **Modal close-key consistency.** Adopt: read-only viewers (`Help`, `Shortcuts`, `Stats`, `Doctor`, `Mcp`, `Diff`) accept `Esc` **and** `q`. Interactive modals (`Files`, `Settings`, `Commands`, `Interview`, `PlanView`, `PlanReview`) accept only `Esc`. Ref `code_ux_review.md P1 #16`.
- [ ] **Ctrl+Enter composer = submit.** Symmetric with Interview. Ref `code_ux_review.md P1 #17`.
- [ ] **Narrow interview hint.** Detect `frame.area().width < 90` and render a shorter hint row (drop the parenthetical notes). Ref `code_ux_review.md P1 #12`.

### Tests
- [ ] Settings key: Tab moves selection forward, skipping Headers (new test).
- [ ] Flash: `flash_success("x")` yields a `FlashKind::Success` entry whose render color is `theme.success`.
- [ ] Help body: regex-matches the new essentials list (pin the pointer mentions).
- [ ] Alias: picker filter with input `ke` surfaces `/shortcuts` via its `keys` alias.
- [ ] Close-key: `q` closes `Modal::Help`, does **not** close `Modal::Settings`.

### Acceptance
- All five categories behave as documented.
- No flash call site left on the deprecated single-color API (compile-time enforced by changing the signature).

---

## V3.f — Plan approval flow (product)

**Goal:** deliver the full `plan_approval_flow.md` spec. Agent-proposed plans are reviewed before becoming active. User-authored `/plan set` still activates immediately.

### State model
- [ ] `enum PlanOrigin { Agent, User }`
- [ ] `struct ProposedPlan { items: Vec<String>, origin: PlanOrigin, raw_text: Option<String>, suggested_auto_run: bool, created_tick: u64 }`
- [ ] `App.proposed_plan: Option<ProposedPlan>`
- [ ] `App.active_plan: Plan` (renamed from `App.plan` to eliminate ambiguity)
- [ ] Invariant: `wrap_with_plan()` reads **only** `active_plan`. `proposed_plan` never influences prompt wrapping, auto-run, or marker parsing.

### Parsing source of truth
- [ ] `apply_plan_markers_on_agent_end` takes `&AgentEnd` and parses from `agent_end.messages` directly — concatenate assistant-message texts, not the transcript tail. Ref `plan_mode_review.md §2`, `plan_approval_flow.md` "Parsing source of truth".
- [ ] Transcript tail remains a fallback for compatibility.

### Plan Review modal
- [ ] New `Modal::PlanReview(PlanReviewState)` variant.
- [ ] `PlanReviewState { mode: ReviewMode, selected: usize, scroll: u16, editing: Option<EditingStep>, auto_run_pref: bool }`
  - `enum ReviewMode { Review, Edit }`
  - `struct EditingStep { index: usize, buffer: String, cursor: usize }`
- [ ] Draw:
  - Title: `agent proposed a plan`
  - Intro: `The agent proposed a N-step plan. Review it before execution.`
  - Step list with checkbox placeholders.
  - Auto-run row: `auto-run after accept: ON / OFF` (togglable).
  - Action row: `[ Accept ]   [ Edit ]   [ Deny ]` with focus chip.
- [ ] Key handling (Review mode):
  - `↑↓` / `j k` → move focus between action chips / step list.
  - `a` → Accept. `e` → Edit. `d` → Deny. `t` → toggle auto-run.
  - `Enter` → activate focused action.
  - `Esc` → Deny (explicit; closes with rejection).
- [ ] Key handling (Edit mode):
  - `↑↓` → move between steps.
  - `Enter` / `i` → enter text editor on current step.
  - `a` → add step below. `x` / `Del` → delete current.
  - `t` → toggle auto-run.
  - `Ctrl+S` → accept edited plan (flush current edit buffer if any).
  - `Esc` → back to Review mode (or Deny if buffer clean).

### Lifecycle
- [ ] On `Incoming::AgentEnd` with `PLAN_SET`:
  1. Parse `PLAN_SET` from `agent_end.messages` — if present, construct `ProposedPlan { origin: Agent, suggested_auto_run: true, .. }` (**YOLO default**: once accepted, auto-run starts).
  2. Store as `proposed_plan`, open `Modal::PlanReview`.
  3. Push `flash_info("review proposed plan")`.
  4. Push transcript row `Entry::Info("agent proposed a plan: N steps")`.
  5. **Do not** stage any auto-prompt.
- [ ] On Accept:
  - `active_plan = Plan::set_all(items)`; `active_plan.auto_run = proposed_plan.suggested_auto_run`.
  - Clear `proposed_plan`.
  - Push `Entry::Info("plan accepted: N steps")` and `flash_success("plan accepted")`.
  - **YOLO kick-off**: if `auto_run` is true, immediately stage `Proceed with step 1: …` (matches current active-plan behavior).
- [ ] On Deny:
  - Clear `proposed_plan`.
  - Push `Entry::Info("plan proposal rejected")` and `flash_info("plan rejected")`.
- [ ] On Edit → Accept:
  - Build `Plan::set_all(edited_items)`, follow Accept path.
- [ ] `PLAN_ADD` while active plan exists:
  - Treat as an amendment proposal. Build a synthetic `ProposedPlan` whose items = existing active items + new item, open the same review modal.
  - Existing `/plan add <text>` user command appends directly (user-authored, no review needed).
- [ ] `STEP_DONE` / `STEP_FAILED` operate **only** on `active_plan`. If only `proposed_plan` exists, silently ignore (or log-debug).
- [ ] User `/plan set a | b | c` still activates immediately with `PlanOrigin::User` — no review modal.

### Marker visibility (answer 4)
- [ ] Strip plan markers from visible assistant transcript text. Mirror the interview stripping logic in `src/interview.rs::strip_ranges` — add a `plan::strip_ranges` and apply during the transcript rewrite that already happens for interview.
- [ ] Continue to strip interview markers (already done).
- [ ] New setting: `ShowRawMarkers` (bool, default OFF). Added to `/settings → Appearance` section as a `SettingsRow::Toggle` with `ToggleAction::ShowRawMarkers`. When ON, the strip passes are skipped.

### Tests
- [ ] `PLAN_SET` on agent_end creates `proposed_plan`, opens review modal, does **not** activate.
- [ ] `proposed_plan` exists → `wrap_with_plan` returns unchanged prompt.
- [ ] Accept → `active_plan` populated; `proposed_plan` cleared; auto-run kicks off when flagged.
- [ ] Deny → `proposed_plan` cleared; no transcript mutation beyond the info row.
- [ ] Edit mode: delete step, add step, edit text, then Accept → resulting `active_plan` matches the edited content.
- [ ] Parsing: `PLAN_SET` in `agent_end.messages[i].text` is detected even when transcript tail is a different assistant entry.
- [ ] `PLAN_ADD` on active plan creates an amendment proposal.
- [ ] `STEP_DONE` with only `proposed_plan` is a no-op.
- [ ] User `/plan set` bypasses review (no modal opened).
- [ ] Plan markers are stripped from rendered transcript; with `ShowRawMarkers = true` they appear.

### Acceptance criteria (from `plan_approval_flow.md`)
1. Agent `PLAN_SET` does **not** immediately become active. ✓
2. Agent `PLAN_SET` opens review modal automatically. ✓
3. User can Accept / Deny / Edit. ✓
4. No auto-run before acceptance. ✓
5. Only accepted plan affects `wrap_with_plan`. ✓
6. `STEP_DONE`/`STEP_FAILED` only on active accepted plans. ✓
7. Propose/accept/deny produces visible transcript feedback. ✓
8. Parsing uses `agent_end.messages` as primary source. ✓
9. Marker visibility policy consistent (strip + settings toggle). ✓

### References
- `plan_approval_flow.md` — the source spec.
- `src/plan.rs` — state model.
- `src/app/mod.rs::apply_plan_markers_on_agent_end`.
- `src/interview.rs::strip_ranges` — template for marker stripping.
- Ratatui example `list.rs` / `form.rs` (`/Users/olivierveinand/Documents/DEV/rataPi/ratatui/examples/`) for the edit-mode list + text input patterns.

---

## V3.g — Theme consistency + docs

**Goal:** no more hardcoded colors in rendering code; README reflects reality.

### Tasks
- [ ] **Theme-aware markdown.** `src/ui/markdown.rs` takes `&Theme` (or the relevant semantic slots) and renders via `theme.muted`, `theme.accent`, `theme.keyword`, etc. Remove the hardcoded `Color::DarkGray`, `Color::Cyan`, `Color::Blue`, `Color::Yellow` instances. Ref `code_review.md §5`, `code_ux_review.md P2 (theme)`.
- [ ] **Theme-aware syntax.** `src/ui/syntax.rs` chooses a syntect palette per built-in theme:
  - `dracula` → `base16-eighties.dark`
  - `gruvbox-dark` → `Solarized (dark)` or bundled gruvbox
  - `catppuccin-mocha` → dark-contrast fallback
  - `solarized-light` → `Solarized (light)`
  - etc.
  Ship a `theme.syntect_name: &'static str` field on `Theme` and use it. Ref `code_review.md §5`.
- [ ] **README refresh.** Replace the Hello-World blurb with:
  - What rata-pi is (TUI for pi-coding-agent)
  - Install / run (with pi dependency note)
  - Feature bullets (plan mode, interview mode, file finder, git integration, notifications, etc.)
  - Screenshot (or ASCII layout)
  - Links to `docs/USER_GUIDE.md`, `PLAN_V3.md`
  Ref `code_review.md §7 §9`, `code_ux_review.md P0 doc drift`.
- [ ] **USER_GUIDE updates** for V3 features:
  - New "Plan approval" section (propose → review → accept/deny/edit).
  - `/settings → Appearance → show raw markers` row documented.
  - Heartbeat-color legend added. Ref `code_ux_review.md P2 #25`.
  - Marker visibility policy restated (matches implementation).

### Tests
- [ ] Markdown rendering test: swapping theme changes rendered spans' `fg`.
- [ ] Syntax rendering test: `theme = Dracula` uses eighties palette; `theme = SolarizedLight` uses solarized.
- [ ] Doc tests: `cargo test --doc` (if any) still green.

### Acceptance
- No `Color::Rgb` / `Color::Named` literals in `src/ui/markdown.rs` or `src/ui/syntax.rs` outside the theme-mapping tables.
- `cargo doc --no-deps --all-features` builds clean.

---

## V3.h — Testing hardening

**Goal:** behaviour regressions get caught even when the code paths are async or involve mock pi.

### Tasks
- [ ] **Mock-pi harness.** `tests/common/mock_pi.rs`: a task-local stdio pair that speaks JSONL with a scripted event list. Can be fed from test fixtures. Ref `code_ux_review.md P1 #18`.
- [ ] **Integration test: prompt lifecycle.** Send a prompt → mock pi replies `agent_start` · `assistant text` · `agent_end` → assert transcript contains the assistant entry, stats updated, live-state back to `Idle`.
- [ ] **Integration test: insufficient-credits.** Mock pi replies `agent_end` with `stop_reason: "error"`, error text in `messages[0].error_message` → assert a visible error row is rendered (regression for the V2.12.f fix).
- [ ] **Plan lifecycle reducer tests** (cover the V3.f acceptance matrix).
- [ ] **Settings dispatch end-to-end.** Build a mock `RpcClient` (same scaffold) → dispatch each `ToggleAction` and `CycleAction` → assert the right RPC command is sent. Ref `code_ux_review.md test-coverage gap §1`.
- [ ] **Interview dispatch end-to-end.** Same pattern. Ref §2.
- [ ] **Shortcuts modal scroll.** `KeyCode::PageDown` bumps scroll by 10; `Home` resets to 0. Ref §3.

### Tests
- Every listed item is itself a new test. Target: +25 tests (194 → ~220).

### Acceptance
- `cargo test --locked --all-features --all-targets` green.
- New harness lives under `tests/` (cargo integration test layout).

---

## V3.i — Accessibility + polish

**Goal:** the app feels good even on unusual setups.

### Tasks
- [ ] **Mouse in modals.** Wheel → scroll. Click on an action chip → activate. Reuse the existing `selected_line` → `scroll_y` math from the draw module. Ref `code_ux_review.md P2 #21`.
- [ ] **High-contrast theme.** Add `high-contrast` to the built-in list. Pure black/white/bold; semantic slots use only bold + underline for emphasis. Ref `code_ux_review.md P2 #22`.
- [ ] **`ignore()` helper.** `pub fn ignore<T, E: std::fmt::Debug>(r: Result<T, E>, reason: &'static str)`; when `tracing::enabled!(Debug)` emit `tracing::debug!(?e, reason, "ignored")`. Replace the 63 `let _ = ...` sites that are fire-and-forget RPC calls. Ref `code_ux_review.md P2 #24`.
- [ ] **Flash icons.** `FlashKind` renders with a leading glyph: `ℹ`, `✓`, `⚠`, `✗`. Width-aware (drop the icon on terminals < 60 cols). Ref `code_ux_review.md P2 #26`.
- [ ] **State-aware sparkline.** Throughput/cost sparklines in the status widget tint toward `theme.error` when `LiveState::Error`. Ref `code_ux_review.md visual polish`.
- [ ] **Focus redundancy knob.** `/settings → Appearance → focus_marker` cycle row: `Triple (default) / Border+Marker / Border only / Marker only`. Ref `code_ux_review.md visual polish`.

### Tests
- [ ] Mouse event on a modal region updates scroll.
- [ ] `high-contrast` theme passes existing theme render tests.
- [ ] Flash icon rendering drops correctly under 60 cols.

### Acceptance
- All six items shipped; no regressions elsewhere.

---

## V3.j — P3 features

**Goal:** clear the P3 backlog from `code_ux_review.md`.

### Tasks
- [ ] **Undo/redo composer.** Ring buffer of ~32 edits; `Ctrl+Z` / `Ctrl+Shift+Z` bindings.
- [ ] **Composer templates.** `/template list`, `/template save <name>`, `/template use <name>`. Persist to `directories::state_dir() / "rata-pi/templates.json"`.
- [ ] **Transcript search.** `Ctrl+R` opens a search overlay; `/` for forward, `?` backward (vim mode only); matches highlight and the view jumps to each hit.
- [ ] **Theme persistence (TOML).** `~/.config/rata-pi/config.toml` — load on startup, write on theme change or on any `/settings` action the user wants persisted. Schema: `theme`, `notify`, `vim`, `show_thinking`, `auto_compact`, `auto_retry`, `show_raw_markers`, `focus_marker`.
- [ ] **TODO widget.** Simple per-session TODO list, bound under `Ctrl+T` or a status-widget row. Persisted with transcript.
- [ ] **Composer draft auto-save.** On Esc-quit with non-empty composer, write to `state_dir/draft.txt`; on next launch offer to restore.

### Tests
- [ ] Undo/redo: snapshot after 3 edits, undo twice, assert buffer matches snapshot-1.
- [ ] Template save + use round-trips through the templates file.
- [ ] Search: seeded transcript with 3 hits, `Ctrl+R foo Enter` navigates to hit 1; `n` moves to 2.
- [ ] Config: write `theme = "dracula"`, restart (simulate via `App::load_config`), app comes up with dracula theme.
- [ ] TODO: add two items, toggle one, reload session, state preserved.
- [ ] Draft: `Esc`-quit with content, launch again, draft restored.

### Acceptance
- All six features usable end-to-end.
- Config file schema documented in USER_GUIDE.

---

## Cross-cutting principles

- **Green at every commit.** `cargo test --locked --all-features --all-targets` + `cargo clippy --all-features --all-targets -- -D warnings` + `cargo fmt -- --check` + `cargo build --release` before every commit. If any of those fail, fix before committing.
- **No behavior change in refactors.** V3.d is strictly lift-and-shift.
- **No backwards-compat shims.** Rename freely. The project has no public downstream users.
- **Never over-engineer.** A field is dead code → delete it. Three similar lines → don't abstract.
- **No new README/docs unless the task requires it.** Existing doc surface is already large.
- **Always reference vendor source when designing.**
  - Ratatui: `/Users/olivierveinand/Documents/DEV/rataPi/ratatui` — especially `examples/apps/` for widget patterns.
  - Pi: `/Users/olivierveinand/.nvm/versions/node/v24.14.0/lib/node_modules/@mariozechner/pi-coding-agent` — `docs/rpc.md` for the wire protocol, `examples/` for canonical flows.
- **Commit subject format:** `feat(v3.X): <concise summary>`, matching the V2 history.

---

## Per-step workflow

For **every** task under every sub-milestone:

1. **Read** the related code + review excerpt + pi/ratatui reference before writing.
2. **Write** the change. Keep the diff tight — no drive-by refactors.
3. **Test.** If the change has user-visible behavior, add a unit or reducer test. If it's pure refactor, rely on existing coverage but run the full suite.
4. **Gate.**
   ```
   cargo fmt
   cargo clippy --all-features --all-targets -- -D warnings
   cargo test --locked --all-features --all-targets
   cargo build --release
   ```
   All four must pass.
5. **Track.** Tick the box in `track_v3_progression.md`. If you deviated from the plan, record the deviation in that file under **Deviations** with a one-liner `why`.
6. **Commit.** One logical unit per commit. Subject: `feat(v3.X): <summary>`. Co-author line as usual.

Mark each sub-milestone shipped in `track_v3_progression.md` with `✅` and the final commit hash. The tracker should always reflect reality — if the plan changed, the tracker records it.

---

## Rolling targets

| | V2.13 | V3 target |
|---|---|---|
| Tests | 194 | ≥ 220 |
| `src/app/mod.rs` LoC | 8 266 | ≤ 5 100 |
| Clippy (`-D warnings`) | clean | clean |
| Fmt | clean | clean |
| Release binary | 5.3 MiB | ≤ 5.6 MiB (new features budget) |
| CI OS coverage (tests) | mac + win | ubuntu + mac + win |
| Hardcoded `Color::X` literals in `ui/markdown.rs` + `ui/syntax.rs` | many | 0 outside theme tables |
| Per-frame blocking I/O when `/settings` open | 3+ calls | 0 |

---

## Out of scope for V3 (explicit deferrals)

*(None — user directive: "defer nothing".)*

If something genuinely doesn't fit after exploration, move it to a `V4.md` note; do not silently drop.

---

## Open risks / known unknowns

- **`agent_end.messages` shape drift.** Pi may change its payload structure. Mitigate via the parsing fallback to transcript tail.
- **Syntect theme availability.** Per-theme palettes depend on what syntect ships; double-check the bundled list before hardcoding names.
- **Edit-mode text input.** If we hit edge cases with UTF-8 cursor positioning, fall back to Ratatui's `widgets::block` + manual cursor math (see `ratatui/examples/apps/user_input`).
- **Mock-pi harness.** Getting a stable async stdio pair in tests is fiddly. If it takes > 4 h, switch to a simpler channel-based shim and document the limitation.

---

Ship V3.a first.
