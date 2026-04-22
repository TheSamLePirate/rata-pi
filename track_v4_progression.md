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

## V4.c — Template picker modal ✅

- [x] `Template { name, body }` struct in `ui::modal` + `From<(String, String)>`
- [x] `Modal::Templates(ListModal<Template>)`
- [x] Two-column draw reusing the Commands/Files two-pane path — list on left, body preview on right
- [x] `/template` (no args) opens the picker (alias `/template list` and `/template pick`)
- [x] Enter loads into composer; Esc closes without touching the composer
- [x] `d` / Delete deletes the focused template; auto-closes the modal when the list becomes empty
- [—] `s`-in-modal quick-save from composer — **dropped**. `/template save <name>` already covers it, and adding an ext-ui input dialog inside the picker for the name prompt would layer modal-on-modal state. See Deviations §4.
- [x] ListRow chip registered so mouse clicks select + Enter-to-activate
- [x] Scroll routing: wheel up/down steps selection (ListModal has no separate scroll offset; `selected_line` centres the row)
- [x] Tests:
  - [x] `template_picker_enter_loads_body`
  - [x] `template_picker_delete_removes_and_auto_closes_when_empty`
  - [x] `template_picker_esc_closes_without_loading`

**Shipped as** `cc2c5bc`

---

## V4.d — Finish `mod.rs` split ✅ (target missed, residual documented)

- [x] V4.d.1 · `src/app/modals/bodies.rs` — all 31 modal body renderers + helpers (1 711 lines extracted)
- [x] V4.d.2 · `src/app/cards.rs` — transcript card builders (562 lines; tool/bash/compaction/retry cards + diff row + syntax helpers)
- [x] V4.d.3 · `src/app/modal_keys.rs` — `handle_modal_key` dispatcher (814 lines)
- [!] `mod.rs` < 4 000 LoC target — **missed at 5 375**. Residual is ~2 100 lines of in-file unit tests (`visuals_cache_tests` + `reducer_tests`); prod code alone is ~3 275 lines (under target). See Deviations §5.
- [x] All 255 existing tests stay green across the split
- [x] New modules added: `cards`, `modal_keys`; `modals::bodies` registered in `modals::mod`

**Shipped as** V4.d.1 `ed38a5a` · V4.d.2 `82b70d3` · V4.d.3 `0bcd929`

---

## V4.e — Distribution + v1.0.0 release ✅ (artifacts prepped; tag + push is the user's call)

- [x] `CHANGELOG.md` — user-facing, covers V2 → V3 → V4, `## [1.0.0]` section with feature groups + known deviations
- [x] `Cargo.toml` version: `0.1.0` → `1.0.0`; also added `readme`, `repository`, `homepage`, `keywords`, `categories` for crates.io
- [!] Release automation — **hand-rolled GitHub Actions** in `.github/workflows/release.yml` (cargo-dist deviation, see §6). Triggers on `v*` tag push. Builds macOS arm64 + macOS x86_64 + Linux x86_64 + Windows x86_64; uploads tarballs / zip + SHA256SUMS.txt; creates a draft release with CHANGELOG.md as the body.
- [x] `Formula/rata-pi.rb` — three-platform Homebrew formula with `REPLACE_WITH_SHA256` placeholders + documented fill-in step. Install caveat hints at the `npm install -g @mariozechner/pi-coding-agent` dep.
- [x] README install section rewritten — brew tap + cargo install + build from source + prebuilt binaries + run + pi dependency notes.
- [x] Status blurb in README updated to "v1.0.0 — first tagged release".
- [x] Full gate green at `v1.0.0`: 255 tests, clippy `-D warnings` clean, fmt clean, release build green.

### User-triggered final steps (not executable from this session)

1. Review + merge the V4.e commit onto main.
2. Tag: `git tag -a v1.0.0 -m "1.0.0"`.
3. Push tag: `git push origin v1.0.0` — this fires `.github/workflows/release.yml`.
4. Once the workflow's draft release is up:
   - Download `SHA256SUMS.txt` from the release.
   - Fill the three `REPLACE_WITH_SHA256` placeholders in `Formula/rata-pi.rb` with the matching hashes.
   - Commit + push the Formula update to a `homebrew-rata-pi` tap repo (or keep it local for `brew install --formula Formula/rata-pi.rb`).
5. Publish to crates.io: `cargo publish` (requires `CARGO_REGISTRY_TOKEN`).
6. Publish the GitHub release draft.

**Shipped as** `<tbd>`

---

## Rolling metrics

| | V3 final | V4.a | V4.b | V4.c | V4.d | V4.e |
|---|---|---|---|---|---|---|
| Tests | 242 | 248 | 252 | 255 | 255 | **255** |
| `src/app/mod.rs` LoC | 7 241 | 7 394 | 7 581 | 7 808 | 5 375 | 5 375 |
| Modules under `src/app/` (excl. `mod.rs`) | 5 | 5 | 5 | 5 | 8 | 8 |
| Click-on-chip works | no | yes (except Commands) | yes | yes | yes | yes |
| Click-outside-modal closes | no | yes | yes | yes | yes | yes |
| Transcript search overlay | no | no | yes | yes | yes | yes |
| Template picker modal | no | no | no | yes | yes | yes |
| `cargo install rata-pi` works | no | no | no | no | no | **prep'd** |
| Homebrew formula available | no | no | no | no | no | **in tree** |
| `CHANGELOG.md` | no | no | no | no | no | **yes** |
| Release workflow (`.github/workflows/release.yml`) | no | no | no | no | no | **yes** |
| `Cargo.toml` version | `0.1.0` | `0.1.0` | `0.1.0` | `0.1.0` | `0.1.0` | **`1.0.0`** |
| Release tag | — | — | — | — | — | **pending user push** |
| Clippy clean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Fmt clean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

---

## Deviations

*(Same pattern as V3 — record sub-milestone · task · what changed · why. Blank section = on plan.)*

### 6. V4.e · hand-rolled GitHub Actions release, not cargo-dist
**What changed.** `PLAN_V4` defaulted to `cargo-dist`; the shipped workflow is a plain `.github/workflows/release.yml`.

**Why.** cargo-dist generates its workflow by running `cargo dist init` locally; that needs a network-reachable tool install + an interactive config flow. A hand-written workflow (~70 lines of YAML) does everything cargo-dist would for a single-binary Rust crate with four target triples: matrix build, tar/zip staging, SHA256 emit, and a draft GitHub release with CHANGELOG.md as the body. No new dev-env dep, reviewable in the PR, easy to evolve.

If future versions want Windows code signing, macOS notarisation, or multi-binary installers, cargo-dist is the right move then — the current workflow is a clean baseline to migrate from.

### 5. V4.d · `mod.rs` < 4 000 LoC target missed at 5 375
**What changed.** `PLAN_V4` targeted `src/app/mod.rs` < 4 000 LoC. Final is 5 375 after V4.d.1/.2/.3 pulled 3 087 lines out.

**Why.** The file now contains ~3 275 lines of prod code and ~2 100 lines of in-file unit tests. Every function the plan enumerated was extracted — modal bodies, card builders, modal-key dispatch. Moving the test blocks to `tests/`-style integration files would require making a host of private items public (`App`, its fields, every helper the tests touch), with zero behaviour benefit and a real cost to encapsulation.

If hitting the number matters for a future V5, the right move is to split `reducer_tests` into `src/app/events_tests.rs` + `src/app/input_tests.rs` + `src/app/modals/settings_tests.rs`, etc., under `#[cfg(test)] mod`. That's purely mechanical and can happen any time. Not worth expanding V4.d's scope — V4 has higher-value work remaining (V4.e).

### 4. V4.c · save-from-inside-picker (`s`) dropped
**What changed.** `PLAN_V4` listed `s` inside the picker as a "capture current composer as a new template (prompts for name via ext-ui input)". Not shipped.

**Why.** The save path already exists as `/template save <name>` — one slash away. Adding an in-picker `s` means either stacking a text-input modal on top of the picker (modal-on-modal state isn't supported today) or building a dedicated inline input widget inside the picker row. Both are scope increases for a feature that duplicates an existing one-liner. The picker is now the discoverable entry for load / delete; save stays as a slash. Revisit if users ask.

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
