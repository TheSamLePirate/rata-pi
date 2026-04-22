# V4 progress tracker

Paired with `PLAN_V4.md`. Per-sub-milestone checklist; rolling metrics at the bottom; deviations logged.

**Legend:** `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated (see Deviations) · `[—]` dropped

Each sub-milestone ships as its own commit with subject `feat(v4.X): <summary>` / `refactor(v4.X): …` / `chore(v4.X): …` after passing:
`cargo fmt` · `cargo clippy --all-features --all-targets -- -D warnings` · `cargo test --locked --all-features --all-targets` · `cargo build --release`.

---

## V4.a — Click-on-chip mouse handling ✅

- [x] `MouseMap.modal_chips: Vec<(Rect, ChipTag)>` + `modal_area: Option<Rect>`
- [x] `enum ChipTag` exhaustive; every variant has a dispatch arm in `input::dispatch_chip`:
  - [x] Plan Review: `PlanReviewAccept` · `PlanReviewEdit` · `PlanReviewDeny` · `PlanReviewEditStep(usize)`
  - [x] Settings: `SettingsRow(usize)`
  - [x] List rows: `ListRow(usize)` (Models / History / Forks / Files / GitLog)
  - [x] Thinking picker: `ThinkingOption(usize)`
  - [x] Ext dialogs: `ExtSelectOption(usize)` · `ExtConfirmYes` · `ExtConfirmNo`
- [x] `input::on_mouse_click` dispatches by `ChipTag` — every variant has its keyboard-equivalent arm
- [x] Plan Review action chips registered in `draw_modal`
- [x] V4.a.2 · chip rects registered for Settings / list modals / Thinking / ExtSelect / ExtConfirm
- [x] Click-outside-modal closes the modal
- [—] Commands modal row click — **dropped**. Commands has category-group headers interleaved with rows; mapping a clicked Y back to an item index needs the full `commands_selected_line` walk. Keyboard shortcuts still work. Followup candidate if users ask.
- [x] Tests:
  - [x] `click_outside_modal_closes_it`
  - [x] `click_inside_modal_does_not_close`
  - [x] `click_plan_review_accept_chip_accepts`
  - [x] `click_plan_review_deny_chip_denies`
  - [x] `click_list_row_sets_selection` (Thinking modal, covers the whole ListRow dispatch path)
  - [x] `click_settings_row_selects_and_resets_scroll_flag`

**Shipped as** V4.a.1 `6191bfb` · V4.a.2 `de8a9e9`

---

## V4.b — Transcript search overlay ✅

- [x] `Modal::Search(SearchState { query, query_cursor, hits, hit_idx })`
- [x] Key handling: printable → append at cursor; Backspace/Delete → edit; ←→/Home/End → cursor motion; Enter → focus current hit + close; `n`/`N` → cycle (wraps); Down/Tab / Up/BackTab aliases; Esc → close
- [x] Live filter recomputes `hits` on every query edit (synchronously in the key handler — cheap substring scan, no framework-level debounce needed)
- [—] Inline highlight in transcript via visuals cache — **dropped**. The modal shows a preview of every hit with snippet + context; jumping to focus via Enter already directs the user to the card. Visuals-cache-wide highlighting adds a lot of plumbing for modest marginal value. Logged in Deviations §3.
- [x] Hint line shows `N of M · n/N next/prev · Enter focus · Esc close`
- [x] `/search <text>` opens the modal pre-populated; starts focused on the last hit (matches V3.j.3 MVP behaviour); `/search` alone opens empty
- [—] `Ctrl+F` binding — skipped; it's already "focus mode". `/search` is the canonical entry.
- [x] Tests:
  - [x] `transcript_hits_finds_matches_case_insensitive`
  - [x] `slash_search_opens_modal_prefilled`
  - [x] `search_modal_live_filter`
  - [x] `search_modal_navigate_and_enter_focus`

**Shipped as** `25d4d5a`

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
| Tests | 242 | 248 | **252** | | | ≥ 265 |
| `src/app/mod.rs` LoC | 7 241 | 7 394 | 7 581 | | < 4 000 | |
| Modules under `src/app/modals/` | 2 | 2 | 2 | | ≥ 4 | |
| Click-on-chip works | no | yes (except Commands) | yes | yes | yes | yes |
| Click-outside-modal closes | no | yes | yes | yes | yes | yes |
| Transcript search overlay | no | no | **yes** | yes | yes | yes |
| Template picker modal | no | no | no | yes | yes | yes |
| `cargo install rata-pi` works | no | no | no | no | no | yes |
| Homebrew formula available | no | no | no | no | no | yes |
| Release tag | — | — | — | — | — | `v1.0.0` |
| Clippy clean | ✓ | ✓ | ✓ | | | ✓ |
| Fmt clean | ✓ | ✓ | ✓ | | | ✓ |

---

## Deviations

*(Same pattern as V3 — record sub-milestone · task · what changed · why. Blank section = on plan.)*

### 3. V4.b · inline match highlighting in transcript dropped
**What changed.** `PLAN_V4` called for the transcript search overlay to push a highlight substring through the visuals cache so matches underline/reverse inside the rendered assistant markdown. The search modal ships **without** inline highlighting.

**Why.** The modal's preview panel already shows, per hit: transcript index, kind (user / assistant / …), and a 60-char context snippet centred on the match. Pressing Enter on a hit focuses the card. That covers the two things users actually want ("did it say X?" → snippet answers; "show me where" → Enter jumps). Inline highlighting requires wiring a highlight substring into `build_one_visual` and through the markdown renderer — both would have to learn to produce extra spans conditionally, invalidating the visuals cache on every query keystroke. The marginal value vs. the V3.b perf-sensitive paths wasn't worth it.

If user feedback asks for it, the hook is obvious: `markdown::render` already takes a `&Theme`; add an optional `&SearchHighlight` too. Leave cache invalidation keyed on the query hash. Not hard — just not shipped.

### 2. V4.a.2 · Commands modal row click dropped
**What changed.** Every other list-style modal (Models / History / Forks / Files / GitLog) got click-to-select rows in V4.a.2. Commands did not.

**Why.** Commands has category-group headers (Session / Git / Model / …) interleaved with the rows; the source-line → item-index map goes through `commands_selected_line`, which walks the filtered list grouping on each frame. Running that walk in reverse to convert a clicked Y back to an item index is a 30-line helper that duplicates display logic. The keyboard path still works; user can arrow to a row. If it comes up as a user request, revisit.

### 1. V4.a.1 · Settings / List / Thinking / Ext chip registration deferred
*(Resolved by V4.a.2 — deviation closed.)*

V4.a.1 shipped the dispatcher + infra + Plan Review chips; V4.a.2 added chip registrations for Settings, list modals (Models / History / Forks / Files / GitLog), Thinking picker, ExtSelect, ExtConfirm. Every modal except Commands (see §2) now routes mouse clicks to the same code path as the keyboard shortcut.

---

## Notes

- V4 is smaller than V3 by design. Resist scope creep: every item not already in this tracker goes to `V4.1_TODO.md` or a GitHub issue.
- The V4.d split is the last one. After it lands, `mod.rs` should not grow again past 4 500 LoC without an explicit plan doc. Any new feature that wants to live there should justify it.
- V4.e needs to clear **before** any post-V4 feature work — shipping `v1.0.0` is the forcing function for "does this thing actually install?"
- If cargo-dist proves painful (weird auth, signing friction, cross-compile issues), time-box at 2 hours and fall back to hand-rolled release.yml.
