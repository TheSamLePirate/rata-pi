# V4 progress tracker

Paired with `PLAN_V4.md`. Per-sub-milestone checklist; rolling metrics at the bottom; deviations logged.

**Legend:** `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated (see Deviations) · `[—]` dropped

Each sub-milestone ships as its own commit with subject `feat(v4.X): <summary>` / `refactor(v4.X): …` / `chore(v4.X): …` after passing:
`cargo fmt` · `cargo clippy --all-features --all-targets -- -D warnings` · `cargo test --locked --all-features --all-targets` · `cargo build --release`.

---

## V4.a — Click-on-chip mouse handling ✅ (V4.a.1 infra; V4.a.2 deferred)

- [x] `MouseMap.modal_chips: Vec<(Rect, ChipTag)>` + `modal_area: Option<Rect>`
- [x] `enum ChipTag` exhaustive (all variants declared; populating them is additive):
  - [x] Plan Review: `PlanReviewAccept`, `PlanReviewEdit`, `PlanReviewDeny`, `PlanReviewEditStep(usize)`
  - [x] Settings: `SettingsRow(usize)` (variant present; registration in V4.a.2)
  - [x] List rows: `ListRow(usize)` (variant present; registration in V4.a.2)
  - [x] Thinking picker: `ThinkingOption(usize)` (variant present; registration in V4.a.2)
  - [x] Ext dialogs: `ExtSelectOption`, `ExtConfirmYes`, `ExtConfirmNo` (variants present; registration in V4.a.2)
- [x] `input::on_mouse_click` dispatches by `ChipTag` — every variant has its keyboard-equivalent arm
- [x] Plan Review action chips (Accept / Edit / Deny) registered in `draw_modal`
- [x] Click-outside-modal closes the modal (universal "press-escape" for mice)
- [!] Registration for Settings / List / Thinking / Ext chips — **deferred to V4.a.2**. The dispatcher is fully wired; each remaining modal is a `mm.push_chip(...)` call or two in its draw path. See Deviations §1.
- [x] Tests:
  - [x] `click_outside_modal_closes_it`
  - [x] `click_inside_modal_does_not_close`
  - [x] `click_plan_review_accept_chip_accepts`
  - [x] `click_plan_review_deny_chip_denies`

**Shipped as** `6191bfb` (V4.a.1 · infrastructure + Plan Review + click-outside)

---

## V4.b — Transcript search overlay

- [ ] `Modal::Search(SearchState { query, hits, cursor })`
- [ ] Key handling: printable → append; Backspace → pop; Enter → focus current hit; `n`/`N` → cycle; `Esc` → close
- [ ] Live filter recomputes `hits` on query change (in `prepare_frame_caches`)
- [ ] Highlight substring passed into visuals cache; markdown renderer underlines matches in assistant text
- [ ] Hint line shows match count + shortcuts
- [ ] `/search <text>` opens the modal pre-populated (back-compat with V3.j MVP)
- [ ] `Ctrl+F`? or keep `/search` slash only — decide during implementation (keyboard-shortcut collision check)
- [ ] Tests:
  - [ ] Query → hit count
  - [ ] `n`/`N` cycles
  - [ ] Enter focuses; Esc closes
  - [ ] Idle-frame perf: no visuals cache walk when query unchanged

**Shipped as** ``

---

## V4.c — Template picker modal

- [ ] `Template { name, body }` struct
- [ ] `Modal::Templates(ListModal<Template>)`
- [ ] Two-column draw (list left, preview right) reusing Commands pattern
- [ ] `/template` (no args) opens the picker
- [ ] Enter loads into composer
- [ ] `d` deletes (maybe with "confirm?" two-step)
- [ ] `s` captures current composer as a new template (prompts for name via ext-ui input)
- [ ] Tests:
  - [ ] List renders all stored templates
  - [ ] Enter loads template into composer
  - [ ] `d` removes and refreshes
  - [ ] `/template` alone opens modal; `/template use <x>` still works

**Shipped as** ``

---

## V4.d — Finish `mod.rs` split

- [ ] `src/app/modals/bodies.rs` (or per-modal files): help · stats · doctor · mcp · shortcuts · commands · models · thinking · history · forks · ext_* · plan_full_lines · plan_card · git_* · file_preview_lines · file_finder_text · diff_body_lines · plan_review_body
- [ ] Split `handle_modal_key` into per-modal handlers
- [ ] `mod.rs` < 4 000 LoC verified
- [ ] All existing tests stay green
- [ ] New modules added to the `modals/mod.rs` hub

**Shipped as** `` (may span multiple commits — list each)

---

## V4.e — Distribution + v1.0.0 release

- [ ] `CHANGELOG.md` — covers V2 → V3 → V4 from a user's perspective; `## [1.0.0] - 2026-XX-XX` header
- [ ] `Cargo.toml` version: `0.1.0` → `1.0.0`
- [ ] Release automation (default: `cargo-dist`; fallback: hand-rolled GitHub Actions)
- [ ] Homebrew formula (via cargo-dist OR manual tap)
- [ ] README install section rewritten
- [ ] CI: release workflow triggers on tag push
- [ ] Tag `v1.0.0` + verify the workflow produces artifacts for macOS (arm64 + x86_64) and Linux (x86_64)
- [ ] `cargo install rata-pi --version 1.0.0` works end-to-end (publish to crates.io if applicable)
- [ ] GitHub release page displays installers + binaries

**Shipped as** ``

---

## Rolling metrics

| | V3 final | V4.a | V4.b | V4.c | V4.d | V4.e |
|---|---|---|---|---|---|---|
| Tests | 242 | | | | | ≥ 265 |
| `src/app/mod.rs` LoC | 7 241 | | | | < 4 000 | |
| Modules under `src/app/modals/` | 2 | | | | ≥ 4 | |
| Click-on-chip works | no | yes | yes | yes | yes | yes |
| Transcript search overlay | no | no | yes | yes | yes | yes |
| Template picker modal | no | no | no | yes | yes | yes |
| `cargo install rata-pi` works | no | no | no | no | no | yes |
| Homebrew formula available | no | no | no | no | no | yes |
| Release tag | — | — | — | — | — | `v1.0.0` |
| Clippy clean | ✓ | | | | | ✓ |
| Fmt clean | ✓ | | | | | ✓ |

---

## Deviations

*(Same pattern as V3 — record sub-milestone · task · what changed · why. Blank section = on plan.)*

### 1. V4.a · Settings / List / Thinking / Ext chip registration deferred
**What changed.** V4.a.1 shipped the full chip infrastructure (`MouseMap.modal_chips`, `push_chip`, `chip_at`, `dispatch_chip`), all `ChipTag` variants, click-outside-modal-closes, and Plan Review action chips end-to-end. The remaining registrations (SettingsRow / ListRow / ThinkingOption / ExtSelect / ExtConfirm) are **not yet wired into `draw_modal`**.

**Why.** Registering a chip per visible row in a list modal requires computing per-row rects at render time against a scrolling viewport; doing it cleanly wants either (a) instrumenting the body builders (`commands_text`, `models_text`, …) to emit row-position metadata or (b) a second render pass that consumes the rendered Paragraph's line rects. Option (a) pollutes the body builders with layout concerns; (b) isn't cheap to do right.

The user-visible behavior is **already a big step forward** — click-outside-closes + Plan Review chips cover the most-requested gestures. Keyboard shortcuts for every other modal still work. Follow-ups land in V4.a.2 (could be a 1-day piece of work) without any further plumbing: the dispatcher is exhaustive and the map is generic.

---

## Notes

- V4 is smaller than V3 by design. Resist scope creep: every item not already in this tracker goes to `V4.1_TODO.md` or a GitHub issue.
- The V4.d split is the last one. After it lands, `mod.rs` should not grow again past 4 500 LoC without an explicit plan doc. Any new feature that wants to live there should justify it.
- V4.e needs to clear **before** any post-V4 feature work — shipping `v1.0.0` is the forcing function for "does this thing actually install?"
- If cargo-dist proves painful (weird auth, signing friction, cross-compile issues), time-box at 2 hours and fall back to hand-rolled release.yml.
