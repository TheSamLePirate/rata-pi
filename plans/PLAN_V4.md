# rata-pi — V4 Master Plan

**Baseline:** commit `ab26245` (V3 shipped · 242 tests · clippy clean · fmt clean · 5.3 MiB release).
**Theme:** "Finish what V3 started + make it shippable."
**Target:** `v1.0.0` release at the end of V4.
**Cadence:** sub-milestones (V4.a → V4.e). Each green-gates and commits independently.

V4 is deliberately smaller than V3. V3 delivered big feature + architecture passes; V4 closes the V3 deferrals, drops the transcript search MVP for a real overlay, makes `mod.rs` actually hit the plan-V3 split target, and turns the project into something you can `brew install`.

---

## Scope locked

1. **Click-on-chip mouse handling** — generalize `MouseMap` to modal action chips. Unblocks "click Accept on Plan Review" and every Enter-chip in every modal.
2. **Full transcript search overlay** — upgrade the V3.j MVP to a live modal with inline match highlighting + `n`/`N` navigation.
3. **Template picker modal** — real list modal with a preview pane. `/template` (no args) opens it; Enter loads; `d` deletes.
4. **Finish `mod.rs` split** — the V3.d stretch target. Extract modal body renderers + split `handle_modal_key`. Target `src/app/mod.rs` **< 4 000 LoC**.
5. **Distribution + v1.0.0** — `CHANGELOG.md`, cargo-dist release automation, Homebrew formula, tagged `v1.0.0`.

Out of scope: session sidebar, cost/budget controls, inline diff preview on write tools, crash-dump replay, plan edit-mode reorder. Candidates for V4.1 / V5.

---

## Sub-milestones at a glance

| ID | Theme | Effort | Ships |
|---|---|---|---|
| V4.a | Click-on-chip mouse | ~2 days | Generalized `MouseMap` action-chip registration · click dispatch in every modal with action chips (Plan Review, Settings, Commands, History, Forks, Files, Models) · tests |
| V4.b | Transcript search overlay | ~2 days | `Modal::Search(SearchState)` · live-filter · inline highlighting in transcript virtualization · `n`/`N`/`Enter`/`Esc` · match-count chip |
| V4.c | Template picker modal | ~1 day | `Modal::Templates(ListModal<Template>)` with preview pane · `/template` with no args opens picker · delete / load / new-from-composer via keys |
| V4.d | Finish `mod.rs` split | ~3 days | Modal body renderers → `src/app/modals/bodies.rs` (or per-modal files) · `handle_modal_key` split by variant · target `mod.rs` < 4 000 LoC |
| V4.e | Distribution + v1.0.0 | ~2 days | `CHANGELOG.md` (0.1 → 1.0 scope) · cargo-dist release workflow · Homebrew formula · GitHub release with signed artifacts · tagged `v1.0.0` · README install section updated |

Total: ~10 working days. Realistic calendar: 2–3 weeks.

---

## V4.a — Click-on-chip mouse handling

**Goal:** every interactive element a user can focus with the keyboard is also clickable.

### Shape
- Today: `MouseMap` records transcript card rects only (`entry_at(x, y)` + `live_tail_chip`).
- V4: add a generalized chip registry. During `draw_modal`, each interactive chip/row registers its rect with a lightweight tag (e.g. `enum ChipTag { PlanReviewAccept, PlanReviewEdit, PlanReviewDeny, SettingsRow(usize), ListRow(usize), ThinkingOption(usize), … }`).
- `on_mouse_click` consults the map in order: live-tail → modal chip → transcript entry → noop.

### Tasks
- [ ] Extend `MouseMap`: `modal_chips: Vec<(Rect, ChipTag)>`.
- [ ] Add `ChipTag` enum.
- [ ] Teach each modal's draw path to register its chips. Keep it in draw-only code (no state mutation in `MouseMap`).
- [ ] Dispatcher in `input::on_mouse_click` maps `ChipTag` to the same action the keyboard binding would trigger.
- [ ] Tests: synthetic click on Plan Review Accept → proposal accepted; Settings row → toggle flips; Commands row → selection + Enter.

### Acceptance
- Every modal listed above reacts to clicks the same way it does to Enter on the focused row.
- No new `Rc<RefCell<_>>` on `App` — chips stay in the frame-scoped `MouseMap` like transcript entries already do.

---

## V4.b — Transcript search overlay

**Goal:** real search, not a flash.

### Shape
- `Modal::Search(SearchState { query, hits: Vec<usize>, cursor: usize })`.
- Live-filter as the user types; highlight matches inside the transcript virtualization pass (via a per-frame "highlight range" passed to the visuals cache).
- Match-count chip in the modal hint line (`"12 matches · Enter to focus · n/N next/prev · Esc close"`).

### Tasks
- [ ] `Modal::Search` variant + `SearchState`.
- [ ] Key handling: printable → query; Backspace → pop; Enter → focus current hit; `n`/`N` → cycle; `Esc` → close.
- [ ] Live filter recomputes `hits` on every query change (debounced at the frame level — runs in `prepare_frame_caches`).
- [ ] Highlight pass: visuals cache gains an optional "highlight substring" input; markdown renderer underlines matches in assistant text.
- [ ] Tests: query/cursor round-trip; hit count; `n`/`N` navigation; Esc closes.

### Acceptance
- Search covers user / assistant / thinking / tool output / bash output — same fields the V3.j MVP already scanned.
- Highlighting visually distinct from focus (default: reverse-video span on matching text).
- No perf regression on idle frames (visuals cache still short-circuits when query unchanged).

---

## V4.c — Template picker modal

**Goal:** `/template` without args opens a real picker, not a flash.

### Shape
- `Modal::Templates(ListModal<Template>)` reusing the existing `ListModal` pattern (Commands / History / Forks / Models all use it).
- Left pane: name list. Right pane: body preview (first ~20 lines).
- Keys: `Enter` loads, `d` deletes (with confirm?), `s` saves current composer as a new template (prompt for name — maybe a light ext-ui input dialog?).

### Tasks
- [ ] `Template` struct (name + body) and `impl From<(String, String)>`.
- [ ] `Modal::Templates` variant.
- [ ] Draw path with two-column layout (same pattern as Commands).
- [ ] `/template` (no args) opens the picker; existing `/template save|use|list|delete` slash aliases stay.
- [ ] Tests: list renders; Enter loads into composer; `d` deletes and refreshes the list.

### Acceptance
- Flash-only `/template list` replaced with the modal.
- Existing tests for `/template save|use|delete` still green.

---

## V4.d — Finish `mod.rs` split

**Goal:** hit the PLAN_V3 target of `src/app/mod.rs` < 4 000 LoC by extracting the last two big blocks: modal body renderers and `handle_modal_key`.

### Tasks
- [ ] `src/app/modals/bodies.rs` (or per-modal files under `modals/`): `help_text`, `stats_text`, `doctor_body`, `mcp_body`, `shortcuts_body`, `commands_text`, `models_text`, `thinking_text`, `history_text`, `forks_text`, `ext_select_text`, `ext_confirm_text`, `ext_input_text`, `plan_full_lines`, `plan_card`, `git_status_body`, `git_log_body`, `git_branch_body`, `file_preview_lines`, `file_finder_text`, `diff_body_lines`, `plan_review_body`.
- [ ] Split `handle_modal_key` into per-modal handlers in the module owning each modal's state. `mod.rs` keeps only the top-level dispatcher.
- [ ] Verify `src/app/mod.rs` line count < 4 000.
- [ ] All 242+ tests stay green through the split (same discipline as V3.d).

### Acceptance
- `mod.rs` < 4 000 LoC.
- No behavior change. Lift-and-shift only.
- Per-modal module owns: state struct, draw body, key handler, (async) dispatcher if any.

---

## V4.e — Distribution + v1.0.0 release

**Goal:** make rata-pi installable without cloning the repo.

### Tasks
- [ ] `CHANGELOG.md` — user-facing, `## [1.0.0] - 2026-XX-XX` section covering the V2 → V3 evolution + everything new in V4.
- [ ] `Cargo.toml` version bump: `0.1.0` → `1.0.0`.
- [ ] Release automation. Pick one:
  - **`cargo-dist`** — Rust-native, generates GitHub Actions workflow, Homebrew formula, shell installer, signed artifacts.
  - **Plain GitHub Actions** — hand-written release.yml that runs `cargo build --release` on Ubuntu + macOS + Windows, uploads artifacts.
  - Default: cargo-dist (it's the path of least resistance for a single-binary Rust CLI).
- [ ] Homebrew formula — either via cargo-dist or hand-written in a `homebrew-rata-pi` tap repo.
- [ ] README install section rewritten — `brew install TheSamLePirate/rata-pi/rata-pi` + `cargo install rata-pi` + "or build from source".
- [ ] Tag `v1.0.0`, push, verify the release workflow produces installable artifacts.

### Acceptance
- `cargo install rata-pi --version 1.0.0` works.
- `brew install TheSamLePirate/rata-pi/rata-pi` works OR a documented formula is checked into a `Formula/` dir for manual tap.
- GitHub release page shows binaries for at least macOS (arm64 + x86_64) and Linux (x86_64).
- CHANGELOG renders cleanly on GitHub.

---

## Cross-cutting principles

Same as V3. Repeated here so V4 can be read standalone.

- **Green at every commit.** `cargo fmt -- --check` · `cargo clippy --all-features --all-targets -- -D warnings` · `cargo test --locked --all-features --all-targets` · `cargo build --release` — all four pass before every commit.
- **No backwards-compat shims.** Rename / reorder / remove freely.
- **No over-engineering.** A field is dead code → delete it. Three similar lines → don't abstract.
- **Lift-and-shift for refactors.** V4.d is strictly no-behavior-change.
- **Commit subject format:** `feat(v4.X): <summary>` · `refactor(v4.X): <summary>` · `chore(v4.X): <summary>`.
- **Always reference vendor source.** Ratatui examples at `/Users/olivierveinand/Documents/DEV/rataPi/ratatui/examples/`; pi at `/Users/olivierveinand/.nvm/versions/node/v24.14.0/lib/node_modules/@mariozechner/pi-coding-agent`. cargo-dist docs for V4.e.

---

## Per-step workflow

For **every** task under every sub-milestone:

1. Read related code + reference + any relevant V3 deviation notes.
2. Write the change — tight diff, no drive-by refactors.
3. Test. Unit or reducer test for behavior; rely on existing coverage for pure refactors.
4. Gate: fmt + clippy `-D warnings` + `cargo test` + `cargo build --release`.
5. Tick the box in `track_v4_progression.md`. Any deviation → **Deviations** section.
6. Commit.

Mark each sub-milestone `✅` in the tracker with the final commit hash. Sub-commit hashes listed alongside.

---

## V1.0.0 success criteria

All four must be true before the tag lands:

1. All 5 sub-milestones `✅` in the tracker.
2. All gates green on `main` and in CI across Ubuntu + macOS + Windows.
3. `cargo install` from the published crate, OR the GitHub release page, installs a working binary on macOS and Linux.
4. `CHANGELOG.md` accurately describes the V2 → V3 → V4 evolution as visible to a new user opening the project today.

---

## Rolling targets

| | V3 final | V4 target |
|---|---|---|
| Tests | 242 | ≥ 265 |
| `src/app/mod.rs` LoC | 7 241 | **< 4 000** |
| Modules under `src/app/modals/` | 2 | ≥ 4 (+ bodies, + per-modal key handlers) |
| Click-on-chip works in every action-chip modal | no | yes |
| Transcript search is an overlay, not a flash | no | yes |
| Template picker is a modal, not a flash | no | yes |
| `cargo install rata-pi` produces a working binary | no | yes |
| Homebrew formula available | no | yes |
| Published GitHub release with artifacts | no | yes |
| Release version tag | `(none)` | `v1.0.0` |

---

## Open risks

- **cargo-dist vs hand-rolled release.** If cargo-dist proves too opinionated, fall back to hand-written `release.yml` + manual Homebrew formula. Time-box the choice at 2 h.
- **Chip rect registration cost.** Per-frame allocation of a `Vec<(Rect, ChipTag)>` is ~nothing for typical modal sizes, but we should verify it doesn't show up in a profile under heavy click.
- **Search overlay regression on perf.** Highlighting pass touches the visuals cache; must not break the V3.b O(1) idle short-circuit. Add a test.
- **`mod.rs` split churn.** Splitting `handle_modal_key` means threading `client: Option<&RpcClient>` through each per-modal handler. Same pattern V3.d already used for interview/settings.

---

## Out of scope (explicit deferrals for V4.1 / V5)

- Session sidebar + autosave.
- Cost/budget controls (soft cap warnings, pre-submit forecast).
- Plan edit-mode reorder / merge / split.
- Inline before/after diff preview on write-tool cards.
- Ghost autocomplete for `@path` and `/slash` in the composer.
- TODO widget (V3.j deferred, still no real demand).
- Crash-dump replay.
- Theme-per-session override (currently global).

Track these somewhere — probably a `V4.1_TODO.md` file if we accumulate demand, or a GitHub issues board once V4.e is done.

---

Ship V4.a first.
