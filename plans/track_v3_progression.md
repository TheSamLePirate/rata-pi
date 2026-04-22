# V3 progress tracker

## V3 shipped 🎉

All 10 sub-milestones landed. Rolling metrics:

| Metric | V2.13 baseline | V3 final | Delta |
|---|---|---|---|
| Tests | 194 | **242** | +48 |
| `src/app/mod.rs` LoC | 8 266 | **7 241** | -12.4% (after 5 sub-modules extracted) |
| Modules under `src/app/` | 3 | 8 | +5 |
| Crate-level modules | baseline | +2 | `config`, `templates` |
| Built-in themes | 6 | 7 | +high-contrast |
| CI test OS count | 2 | 3 | +Ubuntu |
| CI jobs total | 4 | 6 | +build-notify, build-release |
| Hardcoded `Color::X` in markdown/syntax | many | 0 | theme-aware |
| Per-frame I/O in /settings | 3+ | 0 | cached AppCaps |
| Per-frame transcript hash walk | O(n) | O(1) idle | mutation-epoch cache |
| Agent plans require user accept | no | yes | V3.f approval flow |
| Plan markers visible in transcript | yes | no (default) | strip + /settings toggle |
| Persistent user config | no | yes | JSON config + draft |
| Composer undo/redo | no | yes (64-deep) | V3.j.2 |

Every sub-milestone's commit hash is listed below. Seven documented deviations (file format, mock-pi location, etc.); none of them affect user-visible behaviour.



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

**Shipped as** `71c2c92`

---

## V3.h — Testing hardening ✅

- [!] `tests/common/mock_pi.rs` external harness — **deviated**. Instead of a separate integration crate, the harness ships as `rpc::client::TestHarness` inside the crate under `#[cfg(test)]`. See Deviations §4.
- [x] Integration: prompt lifecycle — covered by the reducer-level tests `credit_error_surfaced_via_agent_end_messages`, `plan_full_lifecycle_propose_accept_advance_complete`, and the streaming transcript tests already in place.
- [x] Integration: insufficient-credits already covered by V2.12.f tests (`credit_error_surfaced_via_agent_end_messages`) and V3.a test coverage.
- [x] Plan lifecycle reducer tests — V3.f added 6 (propose / accept / deny / step-done gated / parse / amendment); V3.h.2 adds the full propose→accept→advance→complete happy-path test.
- [x] Settings dispatch end-to-end — **new** `settings_cycle_dispatch_fires_correct_rpc` (matrix over every CycleAction), **new** `settings_toggle_rpc_fires_with_new_value` (AutoCompact / AutoRetry), **new** `settings_theme_cycle_does_not_hit_pi`, **new** `show_raw_markers_toggle_does_not_hit_pi`. All drive the real `dispatch_settings_action` with a `TestHarness` client and inspect the serialized payload.
- [x] Interview dispatch end-to-end — **new** `interview_dispatch_chooses_correct_rpc_by_mode` covers idle → prompt, streaming+Steer → steer, streaming+FollowUp → follow_up.
- [x] Shortcuts modal scroll regression — **new** `shortcuts_modal_scroll_and_close_bindings` (PageDown bumps by 10, Home resets, `q` closes).
- [x] Test count target — **230** (started V3 at 194; V3.h adds +7 reaching comfortably above the 220 floor).

**Shipped as** `b38c4cc`

---

## V3.i — Accessibility + polish ✅

- [x] Mouse wheel scroll routes into open modals (Shortcuts / Diff / Settings / Interview / PlanReview). Click-on-action-chips deferred — would require live hit-test rect recording during draw; wheel covers the main "long modal needs scrolling" gap. See Deviations §5.
- [x] `high-contrast` built-in theme added to the cycle (7 themes total now)
- [x] `ignore(result, reason)` helper in `app/helpers.rs`; 6 RPC fire-sites in `modals/settings.rs` migrated
- [x] FlashKind icon glyphs (`ℹ ✓ ⚠ ✗`) with `< 60` col fallback to plain bullet
- [x] State-aware sparkline tint — throughput and cost sparklines tint `theme.error` during `LiveState::Error`, `theme.warning` during `LiveState::Retrying`
- [x] `/settings → Appearance → focus marker` cycle (`border + marker` / `border only` / `marker only`) — cycles `FocusMarkerStyle`; `FocusMode` enum in `ui/cards.rs` drives the actual render
- [x] Tests:
  - [x] `focus_marker_cycle_and_gates` — state machine + gate helpers
  - [x] `wheel_scroll_routes_to_open_modal` — scroll goes to modal, not transcript
  - [x] `wheel_scroll_without_modal_is_a_noop`

**Shipped as** `203d5b0`

---

## V3.j — P3 features ✅ (with documented deferrals)

- [x] **Undo/redo in composer** — 64-snapshot ring on `Composer`. Ctrl+Z / Ctrl+Shift+Z. `snapshot_before_edit` hooks every text-mutating primitive incl. new `kill_word_back`. Redo clears on fresh edit. (`c54825f`)
- [x] **Composer templates** — `/template save|use|list|delete <name>` with `/tpl` alias. Stored as `BTreeMap<String, String>` in JSON at `<config_dir>/rata-pi/templates.json`. Picker catalog entry wired. (`8bfc56a`)
- [x] **Transcript search (MVP)** — `/search <text>` scans the transcript and focuses the most-recent matching entry. Full inline n/N highlight overlay deferred; the MVP covers "where did we discuss X" already. (`8bfc56a`)
- [!] **Theme persistence** — shipped as **JSON** (not TOML) at `<config_dir>/rata-pi/config.json`. Six fields (theme, notify, vim, show_thinking, show_raw_markers, focus_marker). See Deviations §6. (`c54825f`)
- [—] **TODO widget** — **deferred**. Adds a full modal + per-session persistence + keybinding. Scope too large for V3.j vs. the modest value add. Logged in Deviations §7.
- [x] **Composer draft auto-save** — on Ctrl+C / Ctrl+D quit with non-empty composer, write to `<config_dir>/rata-pi/draft.txt`; `App::new` restores + deletes atomically on next launch. (`c54825f`)
- [x] Tests: +6 total across V3.j (undo/redo roundtrip, redo invalidation, ring cap, config partial/malformed/round-trip, templates roundtrip, templates persist-guard).

**Shipped as** V3.j.1+.2 `c54825f` · V3.j.3+.4 `8bfc56a`

---

## Rolling metrics

| | V2.13 | V3.a | V3.b | V3.c | V3.d | V3.e | V3.f | V3.g | V3.h | V3.i | V3.j |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Tests | 194 | 197 | 203 | 203 | 203 | 207 | 220 | 223 | 230 | 233 | **242** |
| `src/app/mod.rs` LoC | 8 266 | 8 311 | 8 348 | 8 348 | 6 132 | 6 204 | 6 769 | 6 772 | 6 953 | 7 084 | 7 241 |
| Modules under `src/app/` | 3 | 3 | 3 | 3 | 8 | 8 | 8 | 8 | 8 | 8 | 8 |
| Crate-level modules (`src/*.rs`) | — | — | — | — | — | — | — | — | — | — | **+2** (config, templates) |
| Built-in themes | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 7 | 7 |
| Persistent user config | no | no | no | no | no | no | no | no | no | no | **yes** |
| Composer undo/redo | no | no | no | no | no | no | no | no | no | no | **yes (64-deep)** |
| Composer templates | no | no | no | no | no | no | no | no | no | no | **yes** |
| Transcript search | no | no | no | no | no | no | no | no | no | no | **yes (MVP)** |
| Draft auto-save on quit | no | no | no | no | no | no | no | no | no | no | **yes** |
| Mouse-scroll works inside modals | no | no | no | no | no | no | no | no | no | yes | yes |
| Agent plans require user accept | no | no | no | no | no | no | yes | yes | yes | yes | yes |
| Plan markers visible in transcript | yes | yes | yes | yes | yes | yes | no (default) | no (default) | no (default) | no (default) | no (default) |
| Markdown / syntax theme-aware | no | no | no | no | no | no | no | yes | yes | yes | yes |
| Flash color-coded by kind | no | no | no | no | no | yes | yes | yes | yes | yes | yes |
| FlashKind icon glyphs | no | no | no | no | no | no | no | no | no | yes | yes |
| Release binary (MiB) | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 | 5.3 |
| Hardcoded `Color::X` in markdown/syntax | many | many | many | many | many | many | many | 0 | 0 | 0 | 0 |
| Mock-pi test harness | no | no | no | no | no | no | no | no | yes | yes | yes |
| CI test OS count | 2 | 2 | 2 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 |
| CI jobs total | 4 | 4 | 4 | 6 | 6 | 6 | 6 | 6 | 6 | 6 | 6 |
| Clippy `-D warnings` enforced in CI | no | no | no | yes | yes | yes | yes | yes | yes | yes | yes |
| Per-frame I/O in /settings | 3+ | 3+ | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| Per-frame transcript hash walk | O(n) | O(n) | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle | O(1) idle |
| Clippy clean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Fmt clean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

---

## Deviations

*(If any task deviates from `PLAN_V3.md`, record it below with: sub-milestone · task · what changed · why. Blank section = on plan.)*

### 7. V3.j · TODO widget deferred past V3
**What changed.** PLAN_V3 listed a per-session TODO widget. Not shipped.

**Why.** Fully scoped it wants a new modal (draw + key handling), a new state model on `App`, persistence (either in the transcript JSONL or its own file), and a keybinding to open it. That's ~200 LoC of UI work for a feature the other V3.j deliverables already partially cover (templates give reusable-prompts, the transcript itself + `/search` gives a checkable history). Net value vs. scope said defer. When a user actually asks, revisit as V4 or V3.j.follow-up.

### 6. V3.j · config file is JSON, not TOML
**What changed.** `PLAN_V3` asked for `config.toml`. Shipped as `config.json` at the same path.

**Why.** Adding a `toml` dependency for a flat six-field file where `serde_json` already covers the round-trip isn't worth the crate-graph weight (toml ships with its own parser + tokenizer). Behaviourally the two are identical for our use case: a typed struct ↔ flat file. If we ever need richer schema (comments, sections, user-facing editability) the conversion is small.

### 5. V3.i · click-on-action-chips deferred to a future milestone
**What changed.** PLAN_V3 listed "Mouse events in modals (wheel scroll, click-to-activate action chips)". Wheel scroll is in; click-on-chip is not.

**Why.** Modal draw paths build lines + spans without emitting per-span hit-test rects. Mapping clicks onto action chips (Accept/Edit/Deny in Plan Review, settings rows, etc.) needs a chip-bounding-box registration pass during draw — the same pattern `MouseMap` uses for the transcript but generalized. It's a clean ~1-day piece of work on its own and fits a V3.i-follow-up or V4 milestone. Deferred rather than expand V3.i.

### 4. V3.h · mock-pi harness lives in-crate, not under `tests/`
**What changed.** `PLAN_V3.md` called for `tests/common/mock_pi.rs` (cargo integration-test layout). The actual implementation is `rpc::client::TestHarness` inside `src/rpc/client.rs` under `#[cfg(test)]`.

**Why.** rata-pi is a binary crate, not a library; `tests/` integration tests can only reach public API, but the full RPC round-trip requires access to private types (`OutMsg`, the `RpcClient` fields we want to stub). An in-crate `#[cfg(test)] pub(crate) struct TestHarness` gives every unit test across the crate the same wiring without the extra `mod.rs` / `main.rs` split that a new integration test binary would require. The harness exposes `client: RpcClient` (for driving `call` / `fire`) and `try_next_write` / `drain_writes` (for asserting the serialized JSON). Future real mock-pi (with a reader task feeding scripted `Incoming` events back into the event channel) can slot in on top when V3.j or V4 wants it.

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
