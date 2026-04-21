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

## V3.d — Module split (medium) ✅

- [x] `src/app/modals/mod.rs` re-export hub
- [x] `src/app/modals/interview.rs` — key/draw/dispatch extracted (905 LoC)
- [x] `src/app/modals/settings.rs` — rows/actions/body/key/dispatch extracted (618 LoC)
- [x] `src/app/events.rs` — `on_event`, `apply_state`, bootstrap `import_messages` (390 LoC)
- [x] `src/app/input.rs` — `handle_key` + `handle_focus_key` + `on_mouse_click` + `handle_vim_normal` (446 LoC)
- [x] `src/app/helpers.rs` — `truncate_preview`, `args_preview`, `approx_tokens`, `extract_error_detail`, `on_off` (87 LoC)
- [x] Tests moved alongside (or stayed — reducer tests still in mod.rs under `#[cfg(test)]` re-imports). All 203 tests pass across the split.
- [!] `src/app/mod.rs` target ≤ 5 100 lines — **deviated**, ended at 6 132. See Deviations §2.

- [x] commit 1: modals/interview — `b7862c7`
- [x] commit 2: modals/settings — `e00d5f3`
- [x] commit 3: events — `4fb8645`
- [x] commit 4: input — `f32b4ad`
- [x] commit 5: helpers — `32e848a`

---

## V3.e — UX quick wins ✅

- [x] `/help` body refreshed — essentials + pointers to /shortcuts /settings (regression test `help_text_points_at_shortcuts_and_settings`)
- [x] Tab / BackTab bound in `settings_modal_key` (advances / reverses through selectable rows)
- [x] `FlashKind { Info, Success, Warn, Error }` + `App::flash_info/success/warn/error` helpers; ~20 error/success/warn call sites migrated; footer draws each kind in its own color
- [x] Slash aliases surfaced via description text — searching `keys`/`hotkeys` surfaces `/shortcuts`; searching `prefs`/`preferences` surfaces `/settings`. `try_local_slash` already routed the aliases.
- [x] Cycle-row arrow cleanup — only `Theme` shows `◂` + `▸`; pi-backed cycles show `▸` only
- [x] Modal close-key consistency — read-only viewers (Stats, Help, GitStatus, PlanView, Doctor, Mcp, Diff) accept Esc + Enter + q; interactive modals keep Esc only
- [x] Ctrl+Enter on composer = submit (explicit arm, symmetric with Interview)
- [x] Narrow interview hint — compact variant under 95 terminal cols
- [x] Tests:
  - [x] `help_text_points_at_shortcuts_and_settings`
  - [x] `settings_tab_and_shift_tab_step_selection`
  - [x] `flash_helpers_tag_kind_correctly`
  - [x] `settings_toggle_offline_flashes_for_rpc_backed_flags` (updated to assert FlashKind::Warn)
  - [x] `picker_surfaces_rows_via_alias_hints`

**Shipped as** `b6808f0`

---

## V3.f — Plan approval flow ✅

### State model
- [x] `PlanOrigin { Agent, User }` enum
- [x] `ProposedPlan` struct + `ProposalKind { NewPlan, Amendment }`
- [x] `App.proposed_plan: Option<ProposedPlan>`
- [—] `App.active_plan` rename — kept as `App.plan` to avoid 30+ call-site churn. Behavior matches spec.
- [x] Invariant: `wrap_with_plan` reads only `app.plan` (accepted plan). Proposal never participates.

### Parsing
- [x] `apply_plan_markers_on_agent_end(&[AgentMessage])` parses from the payload
- [x] Transcript tail kept as fallback

### Modal
- [x] `Modal::PlanReview(Box<PlanReviewState>)` variant
- [x] Review mode: `↑↓/j/k`, `h/l/←/→` (chip focus), `Enter`, `Esc`, `a`, `e`, `d`, `t`
- [x] Edit mode: `↑↓/j/k`, `Enter/i` edit, `a` add-below, `x/Del` delete, `t` auto-run toggle, `Ctrl+S` accept, `Esc` back
- [x] Text-entry sub-mode inside Edit: printable insert, `←→`, `Home/End`, `Backspace/Delete`, `Enter` commit, `Esc` cancel
- [x] Draw body renders Review chips vs Edit focus list with inline cursor

### Lifecycle
- [x] `PLAN_SET` on agent_end → proposal + review modal + info row + flash
- [x] Accept (new plan) → `plan.set_all` + YOLO kick-off if auto-run
- [x] Accept (amendment) → `plan.merge_amendment` preserving status on matching steps
- [x] Deny → proposal cleared + info row + flash
- [x] Edit → Accept → edited items become active
- [x] `PLAN_ADD` on active plan → amendment proposal (review modal)
- [x] User `/plan set` activates immediately (no review, unchanged)
- [x] `STEP_DONE` / `STEP_FAILED` ignored when only `proposed_plan` exists
- [x] `STEP_DONE` advances when the plan is accepted

### Marker visibility
- [x] `plan::strip_markers` added (drops `PLAN_SET`/`PLAN_ADD`/`STEP_DONE`/`STEP_FAILED`, keeps foreign `[[…]]`)
- [x] Plan markers stripped from transcript tail after each agent_end (when toggle is off)
- [x] `/settings → Appearance → show raw markers` toggle (`ToggleAction::ShowRawMarkers`)
- [x] `App.show_raw_markers: bool` (default false) gates the strip pass

### Tests — V3.f adds 10 new reducer/integration tests:
- [x] `plan_set_from_agent_creates_proposal_not_active_plan`
- [x] `accept_proposed_plan_activates_and_stages_first_step`
- [x] `deny_proposed_plan_clears_it`
- [x] `step_done_ignored_without_accepted_plan`
- [x] `step_done_advances_accepted_plan`
- [x] `plan_set_parses_from_agent_end_messages_not_transcript_tail`
- [x] `plan_review_edit_delete_add_accept` (Edit → delete → add → Ctrl+S)
- [x] `plan_markers_stripped_from_transcript_by_default`
- [x] `show_raw_markers_toggle_preserves_brackets`
- [x] `amendment_acceptance_preserves_step_status`
- [x] Plus 3 plan::strip_markers unit tests

### Acceptance criteria (from `plan_approval_flow.md`)
- [x] 1. Agent PLAN_SET does not immediately become active
- [x] 2. Agent PLAN_SET opens review modal automatically
- [x] 3. Accept/Deny/Edit available
- [x] 4. No auto-run before acceptance
- [x] 5. Only accepted plan affects `wrap_with_plan` (via `plan.is_active` gate)
- [x] 6. STEP_DONE/STEP_FAILED only on accepted active plans
- [x] 7. Visible transcript feedback for all transitions
- [x] 8. Parsing from `agent_end.messages`
- [x] 9. Marker visibility policy consistent

**Shipped as** V3.f.1 `e6c390d` · V3.f.2 `521fddd` · V3.f.3 `2bbfd05`

---

## V3.g — Theme + docs ✅

- [x] `Theme` struct gains `syntect_name: &'static str`; each of the 6 built-ins picks a palette (tokyo-night → ocean.dark · dracula → eighties.dark · solarized-dark → Solarized (dark) · catppuccin-mocha → mocha.dark · gruvbox-dark → eighties.dark · nord → ocean.dark)
- [x] `src/ui/syntax.rs` — `highlight(code, lang, theme)` takes `&Theme`; per-name palette cache via OnceLock<Mutex<HashMap>>; unknown palette names fall back to `base16-ocean.dark`
- [x] `src/ui/markdown.rs` — `render(md, theme)` takes `&Theme`; every previously-hardcoded `Color::DarkGray / Cyan / Blue / Yellow / Magenta / Green` now goes through the semantic slot (dim / accent / accent_strong / warning / success / muted)
- [x] All call sites migrated: `files::preview` cache, `build_one_visual` (assistant markdown), tool card body (file content highlight), `diff_body_line`, diff widget's `highlight_body`
- [x] README refreshed (what / how to run / features / pi dep / links)
- [x] USER_GUIDE — Plan approval section rewritten (authority rule, Review modal, Edit sub-mode, text-entry sub-mode, amendment merge semantics, marker visibility)
- [x] USER_GUIDE — /settings `show raw markers` row documented
- [x] USER_GUIDE — heartbeat-color legend added to "Status widget and header"
- [x] USER_GUIDE — Tab / Shift+Tab added to /settings keyboard table; cycle-row arrow distinction documented
- [x] Tests:
  - [x] `heading_color_tracks_theme_accent` — swapping theme changes markdown span fg
  - [x] `different_themes_produce_different_palettes` — per-theme syntect palette selection
  - [x] `unknown_palette_falls_back_gracefully` — missing palette doesn't crash

**Shipped as** `<tbd>`

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
| Tests | 194 | 197 | 203 | 203 | 203 | 207 | 220 | **223** | ≥ 220 | | |
| `src/app/mod.rs` LoC | 8 266 | 8 311 | 8 348 | 8 348 | 6 132 | 6 204 | 6 769 | 6 772 | | | |
| Modules under `src/app/` | 3 | 3 | 3 | 3 | 8 | 8 | 8 | 8 | | | |
| Agent plans require user accept | no | no | no | no | no | no | yes | yes | | | |
| Plan markers visible in transcript | yes | yes | yes | yes | yes | yes | no (default) | no (default) | | | |
| Markdown / syntax theme-aware | no | no | no | no | no | no | no | **yes** | | | |
| Flash color-coded by kind | no | no | no | no | no | yes | yes | yes | | | |
| Release binary (MiB) | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | | | |
| Hardcoded `Color::X` in markdown/syntax | many | many | many | many | many | many | many | **0** | | | |
| CI test OS count | 2 | 2 | 2 | 3 | 3 | 3 | 3 | 3 | | | |
| CI jobs total | 4 | 4 | 4 | 6 | 6 | 6 | 6 | 6 | | | |
| Clippy `-D warnings` enforced in CI | no | no | no | yes | yes | yes | yes | yes | | | |
| Per-frame I/O in /settings | 3+ | 3+ | 0 | 0 | 0 | 0 | 0 | 0 | | | |
| Per-frame transcript hash walk | O(n) | O(n) | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle | | | |
| Clippy clean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | |
| Fmt clean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | |

---

## Deviations

*(If any task deviates from `PLAN_V3.md`, record it below with: sub-milestone · task · what changed · why. Blank section = on plan.)*

### 3. V3.f · `App.plan` kept its name (not renamed to `active_plan`)
**What changed.** `plan_approval_flow.md` proposed renaming `App.plan` → `App.active_plan`. We kept the existing name.

**Why.** The spec's goal was making the proposal-vs-accepted distinction clear in code. We hit that by introducing `App.proposed_plan: Option<ProposedPlan>` alongside the existing `App.plan` — the pair `(proposed_plan, plan)` already reads as draft-vs-committed. Renaming `plan` → `active_plan` would touch ~30 call sites across mod.rs, events.rs, input.rs, modals/settings.rs, plan slash handling, and existing tests, with no behavior benefit. Kept simple: `plan` is the accepted one; `proposed_plan` is the draft under review.

### 2. V3.d · mod.rs ended at 6 132 lines, not ≤ 5 100
**What changed.** The plan targeted `src/app/mod.rs` ≤ 5 100 after the medium split; final LoC is 6 132.

**Why.** Every function the plan enumerated was extracted: the two modals (interview, settings), the reducer (events), the global input layer (input), and the cross-cutting helpers. Total reduction 8 466 → 6 132 = **-2 334 lines / -27.6%**. The remaining ~1 000 excess sits in two places:
- Modal *body renderer* functions (`doctor_body`, `mcp_body`, `help_text`, `commands_text`, `models_text`, `thinking_text`, `history_text`, `forks_text`, `ext_*_text`, `plan_full_lines`, `plan_card`, `git_*_body`, `file_preview_lines`, `file_finder_text`) — ~700 LoC. Natural home is a new `modals/bodies.rs` or per-modal files extending the pattern from V3.d.1/.2.
- `handle_modal_key` (~500 LoC) — per-variant dispatch for every `Modal::*` kind. Logical home is `modals/mod.rs` once every modal has its own submodule.

Both are clean mechanical extractions — the scope grew beyond what the V3.d milestone's "medium split" framing said to do in one pass. Deferred rather than expand the milestone. Plan for V3.d.* follow-up OR a dedicated V3.d.6 when we revisit the split later in V3 or early V4.

### 1. V3.b · skipped `spawn_blocking` offload for `files::preview`
**What changed.** The plan called for `files::preview` to be offloaded via `tokio::task::spawn_blocking`. It isn't.

**Why.** The read is now bounded to `MAX_BYTES` (8 KiB) by `BufReader::take` and guarded by a `metadata()` size check that rejects >50 MiB files before opening them. On any reasonable filesystem the actual I/O is <1 ms — well below the UI frame budget — whereas a `spawn_blocking` hop costs a scheduler trip plus a channel round-trip per selection change. The root cause (unbounded `std::fs::read`) is fixed; adding async machinery on top of an already-cheap operation would add overhead for zero user-visible benefit. The plan note "optionally offload" covered this.

---

## Notes

- The V3.d split is lift-and-shift only. Any behavior change caught during extraction pauses the split and gets fixed in a dedicated commit on a prior sub-milestone.
- V3.f is the longest milestone. Expect 3+ commits inside it — split into (1) state model + parser, (2) review modal draw/key, (3) edit mode, (4) amendment flow + marker strip, (5) tests. Each of those sub-commits should still be green.
- If a deferred item from a review turns out to be wrong or obsolete during exploration, mark it `[—]` and note the reason in Deviations — don't silently drop.
- Keep the reviews (`code_review.md`, `code_ux_review.md`, `plan_mode_review.md`, `plan_approval_flow.md`) in the tree — they're the source of truth for what V3 owes.
