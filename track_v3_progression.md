# V3 progress tracker

Paired with `PLAN_V3.md`. Per-sub-milestone checklist; rolling metrics at the bottom; deviations logged.

**Legend:** `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated (see Deviations) · `[—]` dropped

Each sub-milestone ships as its own commit with subject `feat(v3.X): <summary>` after passing:
`cargo fmt` · `cargo clippy --all-features --all-targets -- -D warnings` · `cargo test --locked --all-features --all-targets` · `cargo build --release`.

---

## V3.a — RPC reliability ✅

- [x] `RpcClient::call()` removes pending entry on `send()` failure
- [x] `RpcClient::call_timeout(cmd, Duration)` added; `call()` delegates with 10 s default
- [x] `bootstrap()` — each sub-call at 3 s
- [x] `refresh_stats()` — 1 s
- [x] Slash/modal RPC sites — 10 s (inherited via `call()` default)
- [x] `RpcError::Timeout` surfaced as non-fatal flash (refresh_stats site)
- [x] Offline flash in `dispatch_settings_action` (AutoCompact / AutoRetry)
- [x] Tests:
  - [x] pending map empty after send failure (`call_removes_pending_on_send_failure`)
  - [x] `call_timeout` returns `Timeout` when oneshot idle (`call_timeout_returns_timeout_when_idle`)
  - [x] offline toggle flashes and flips local state (`settings_toggle_offline_flashes_for_rpc_backed_flags`)

**Shipped as** `022e358`

---

## V3.b — Perf regressions ✅

- [x] `AppCaps` struct built once in `App::new()` — clipboard / history path / state dir / term_caps / pi binary / platform / package version
- [x] `build_settings_rows` reads static rows from `app.caps` (no re-probe per frame)
- [x] `History::default_path()` cheap static accessor; `/log` and `/settings` use it
- [x] `term_caps::detect()` cached on `App.caps.term`; `/env`, `/doctor`, `/settings` read it
- [x] `files::preview` — metadata-first size check + `BufReader::take(MAX_BYTES)` bounded read
- [!] `spawn_blocking` offload for `files::preview` — **deviated** (see Deviations §1)
- [x] `Transcript::mutation_epoch` + `VisualsCache::last_seen_epoch` — fingerprint walk skipped on no-change frames
- [x] Tests:
  - [x] AppCaps populated and matches direct probes (`app_caps_matches_direct_probes`)
  - [x] `build_settings_rows` reads from `app.caps` (`settings_rows_read_from_app_caps`)
  - [x] `files::preview` rejects over-cap file without reading (`preview_rejects_files_over_size_cap_without_reading`)
  - [x] `files::preview` clips to MAX_LINES (`preview_clips_to_max_lines`)
  - [x] Visuals cache early-outs when epoch unchanged (`visuals_cache_skips_walk_when_epoch_unchanged`)
  - [x] Visuals cache walks after mutation (`visuals_cache_walks_after_mutation`)

**Shipped as** `41bb7f8`

---

## V3.c — CI hardening ✅

- [x] `.github/workflows/ci.yml` — `ubuntu-latest` added to test matrix
- [x] Clippy job — `args: --all-features --all-targets -- -D warnings` passed explicitly
- [x] `build-notify` job — `cargo build --locked --features notify` on Ubuntu (installs libdbus-1-dev)
- [x] `build-release` job — `cargo build --locked --release` smoke on Ubuntu
- [x] Local parity: all three commands pass locally (test matrix will confirm on push)

**Shipped as** `be61ab1`

---

## V3.d — Module split (medium)

- [ ] `src/app/modals/mod.rs` re-export hub
- [ ] `src/app/modals/interview.rs` — key/draw/dispatch extracted (~500 LoC)
- [ ] `src/app/modals/settings.rs` — rows/actions/body/key/dispatch extracted (~700 LoC)
- [ ] `src/app/events.rs` — `on_event`, bootstrap import, live-state transitions (~1 200 LoC)
- [ ] `src/app/input.rs` — `handle_key` global composer path (~600 LoC)
- [ ] `src/app/helpers.rs` — `truncate_preview`, `args_preview`, `approx_tokens`, `on_off`
- [ ] Tests moved alongside their modules; `cargo test` still green across the split
- [ ] `src/app/mod.rs` < 5 100 lines verified

**Shipped as** `` (may span 5 commits — list each below)

- [ ] commit 1: modals/interview — hash:
- [ ] commit 2: modals/settings — hash:
- [ ] commit 3: events — hash:
- [ ] commit 4: input — hash:
- [ ] commit 5: helpers + `mod.rs` cleanup — hash:

---

## V3.e — UX quick wins

- [ ] `/help` body refreshed (essentials + pointers to /shortcuts /settings)
- [ ] Tab / Shift+Tab bound in `settings_modal_key` (skip Headers)
- [ ] `FlashKind { Info, Success, Warn, Error }` introduced; `App::flash_*` helpers; ~40 call sites migrated
- [ ] Slash aliases in catalog — `/keys`, `/hotkeys`, `/prefs`, `/preferences` (picker filter honors aliases)
- [ ] Cycle-row arrow cleanup — RPC-backed cycles render only `▸`
- [ ] Modal close-key consistency — read-only viewers accept Esc+q; interactive modals Esc only
- [ ] Ctrl+Enter on composer = submit
- [ ] Narrow interview hint — width-aware fallback under 90 cols
- [ ] Tests:
  - [ ] Tab skips Headers in /settings
  - [ ] `flash_success` uses `theme.success` color
  - [ ] `/help` body contains the new essentials list
  - [ ] picker filter `ke` surfaces `/shortcuts` via alias
  - [ ] `q` closes `Modal::Help`, not `Modal::Settings`

**Shipped as** ``

---

## V3.f — Plan approval flow

### State model
- [ ] `PlanOrigin { Agent, User }` enum
- [ ] `ProposedPlan` struct
- [ ] `App.proposed_plan: Option<ProposedPlan>`
- [ ] `App.active_plan` (renamed from `App.plan`)
- [ ] Invariant: `wrap_with_plan` reads only `active_plan`

### Parsing
- [ ] `apply_plan_markers_on_agent_end(&AgentEnd)` parses from `agent_end.messages`
- [ ] Transcript tail kept as fallback

### Modal
- [ ] `Modal::PlanReview(PlanReviewState)` variant
- [ ] Review mode: `↑↓ j k Enter Esc a e d t`
- [ ] Edit mode: `↑↓ Enter/i a x/Del t Ctrl+S Esc`
- [ ] Draw body matches spec

### Lifecycle
- [ ] `PLAN_SET` on agent_end → proposal + review modal + info row + `flash_info("review proposed plan")`
- [ ] Accept → `active_plan` populated, YOLO kick-off if `auto_run`
- [ ] Deny → proposal cleared + info row + flash
- [ ] Edit → Accept → edited items become active
- [ ] `PLAN_ADD` on active plan → amendment proposal (review modal)
- [ ] User `/plan set` activates immediately (no review)
- [ ] `STEP_DONE` / `STEP_FAILED` ignored when only `proposed_plan` exists

### Marker visibility
- [ ] `plan::strip_ranges` added
- [ ] Plan markers stripped from transcript (parallel to interview strip)
- [ ] `/settings → Appearance → show raw markers` toggle (`ToggleAction::ShowRawMarkers`)
- [ ] `App.show_raw_markers: bool` (default false) — when true, strip passes skipped

### Tests (cover full acceptance matrix)
- [ ] PLAN_SET on agent_end → proposal, no activation
- [ ] `wrap_with_plan` unchanged while proposal exists
- [ ] Accept path
- [ ] Deny path
- [ ] Edit → delete + add + edit text → Accept path
- [ ] Parsing from `agent_end.messages` wins over transcript tail
- [ ] PLAN_ADD on active plan creates amendment
- [ ] STEP_DONE no-op with only proposal
- [ ] `/plan set` bypasses review
- [ ] Plan markers stripped by default; visible with toggle on

### Acceptance criteria (from `plan_approval_flow.md`)
- [ ] 1. Agent PLAN_SET does not immediately become active
- [ ] 2. Agent PLAN_SET opens review modal automatically
- [ ] 3. Accept/Deny/Edit available
- [ ] 4. No auto-run before acceptance
- [ ] 5. Only accepted plan affects `wrap_with_plan`
- [ ] 6. STEP_DONE/STEP_FAILED only on accepted active plans
- [ ] 7. Visible transcript feedback for all transitions
- [ ] 8. Parsing from `agent_end.messages`
- [ ] 9. Marker visibility policy consistent

**Shipped as** ``

---

## V3.g — Theme + docs

- [ ] `src/ui/markdown.rs` takes `&Theme`; no hardcoded `Color::X` outside mapping tables
- [ ] `src/ui/syntax.rs` — per-theme syntect palette via `theme.syntect_name`
- [ ] `Theme` struct gains `syntect_name: &'static str`
- [ ] README refreshed (what / how to run / features / pi dep / links)
- [ ] USER_GUIDE — Plan approval section added
- [ ] USER_GUIDE — /settings `show raw markers` row documented
- [ ] USER_GUIDE — heartbeat-color legend added
- [ ] USER_GUIDE — marker visibility policy restated
- [ ] Tests:
  - [ ] swapping theme changes markdown rendered spans
  - [ ] syntax palette selection per theme

**Shipped as** ``

---

## V3.h — Testing hardening

- [ ] `tests/common/mock_pi.rs` harness (async stdio pair, scripted events)
- [ ] Integration: prompt lifecycle (assistant text → agent_end → transcript + stats + Idle)
- [ ] Integration: insufficient-credits error surfaced to user
- [ ] Plan lifecycle reducer tests (covers V3.f matrix)
- [ ] Settings dispatch end-to-end tests (each ToggleAction + CycleAction)
- [ ] Interview dispatch end-to-end tests
- [ ] Shortcuts modal scroll regression (PageDown bumps by 10, Home resets)
- [ ] Test count ≥ 220

**Shipped as** ``

---

## V3.i — Accessibility + polish

- [ ] Mouse events in modals (wheel scroll, click-to-activate action chips)
- [ ] `high-contrast` built-in theme
- [ ] `ignore(result, reason)` helper; `let _ =` RPC fire-sites migrated
- [ ] FlashKind icon glyphs (`ℹ ✓ ⚠ ✗`) with <60-col fallback
- [ ] State-aware sparkline tint (errors → `theme.error`)
- [ ] `/settings → Appearance → focus_marker` cycle (Triple / Border+Marker / Border only / Marker only)
- [ ] Tests:
  - [ ] Mouse event on modal updates scroll
  - [ ] `high-contrast` renders
  - [ ] Flash icon drops under 60 cols

**Shipped as** ``

---

## V3.j — P3 features

- [ ] Undo/redo in composer (Ctrl+Z / Ctrl+Shift+Z) — ring buffer ≥ 32
- [ ] Composer templates (`/template list/save/use`) — persisted under state_dir
- [ ] Transcript search (Ctrl+R overlay; `n`/`N` navigate)
- [ ] Theme persistence (TOML config at `~/.config/rata-pi/config.toml`)
- [ ] TODO widget (per-session, persisted with transcript)
- [ ] Composer draft auto-save on Esc-quit; restore on next launch
- [ ] Tests for each feature (6+)

**Shipped as** ``

---

## Rolling metrics

| | V2.13 | V3.a | V3.b | V3.c | V3.d | V3.e | V3.f | V3.g | V3.h | V3.i | V3.j |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Tests | 194 | 197 | 203 | 203 | | | | | ≥ 220 | | |
| `src/app/mod.rs` LoC | 8 266 | 8 311 | 8 348 | 8 348 | ≤ 5 100 | | | | | | |
| Release binary (MiB) | 5.3 | 5.3 | 5.3 | 5.3 | | | | | | | |
| Hardcoded `Color::X` in markdown/syntax | many | many | many | many | | | | | | | |
| CI test OS count | 2 | 2 | 2 | **3** | | | | | | | |
| CI jobs total | 4 | 4 | 4 | **6** | | | | | | | |
| Clippy `-D warnings` enforced in CI | no | no | no | **yes** | | | | | | | |
| Per-frame I/O in /settings | 3+ | 3+ | 0 | 0 | | | | | | | |
| Per-frame transcript hash walk | O(n) | O(n) | O(1) idle | O(1) idle | | | | | | | |
| Clippy clean | ✓ | ✓ | ✓ | ✓ | | | | | | | |
| Fmt clean | ✓ | ✓ | ✓ | ✓ | | | | | | | |

---

## Deviations

*(If any task deviates from `PLAN_V3.md`, record it below with: sub-milestone · task · what changed · why. Blank section = on plan.)*

### 1. V3.b · skipped `spawn_blocking` offload for `files::preview`
**What changed.** The plan called for `files::preview` to be offloaded via `tokio::task::spawn_blocking`. It isn't.

**Why.** The read is now bounded to `MAX_BYTES` (8 KiB) by `BufReader::take` and guarded by a `metadata()` size check that rejects >50 MiB files before opening them. On any reasonable filesystem the actual I/O is <1 ms — well below the UI frame budget — whereas a `spawn_blocking` hop costs a scheduler trip plus a channel round-trip per selection change. The root cause (unbounded `std::fs::read`) is fixed; adding async machinery on top of an already-cheap operation would add overhead for zero user-visible benefit. The plan note "optionally offload" covered this.

---

## Notes

- The V3.d split is lift-and-shift only. Any behavior change caught during extraction pauses the split and gets fixed in a dedicated commit on a prior sub-milestone.
- V3.f is the longest milestone. Expect 3+ commits inside it — split into (1) state model + parser, (2) review modal draw/key, (3) edit mode, (4) amendment flow + marker strip, (5) tests. Each of those sub-commits should still be green.
- If a deferred item from a review turns out to be wrong or obsolete during exploration, mark it `[—]` and note the reason in Deviations — don't silently drop.
- Keep the reviews (`code_review.md`, `code_ux_review.md`, `plan_mode_review.md`, `plan_approval_flow.md`) in the tree — they're the source of truth for what V3 owes.
