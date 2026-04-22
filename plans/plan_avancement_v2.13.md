# V2.13 progress tracker

Paired with `PLAN_V2.13.md`. Per-sub-commit checklist; notes at the bottom.
Legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated · `[—]` dropped

---

## V2.13.a — `/shortcuts` modal ✅

- [x] `Modal::Shortcuts { scroll: u16 }` variant
- [x] `Modal::Settings(SettingsState)` variant landed early so both modals ship together
- [x] `/shortcuts` + `/keys` + `/hotkeys` slash aliases
- [x] `/settings` + `/prefs` + `/preferences` slash aliases (V2.13.b handles dispatch)
- [x] Catalog entries in `src/ui/commands.rs` under Session
- [x] `shortcuts_body(theme)` generator with 8 grouped sections
- [x] Modal key handler: ↑↓/j/k/PgUp/PgDn/Home/End/Esc
- [x] Regression test `shortcuts_body_has_every_section`

**Shipped as `a46bb8b`.**

## V2.13.b — `/settings` modal ✅

- [x] `SettingsState { selected, scroll, user_scrolled }` in `src/ui/modal.rs`
- [x] `SettingsRow::{Header, Info, Toggle, Cycle}` + `ToggleAction` + `CycleAction` enums
- [x] `build_settings_rows(&App)` rebuilds live on every draw — 8 sections (Session, Model, Behavior, Appearance, Live state, Capabilities, Paths, Build)
- [x] `settings_body(app, state, theme)` — two-column layout, focus ▶ marker, section rules, coloured value column
- [x] `settings_row_source_line(rows, selected)` for focus-follow scroll
- [x] `settings_modal_key(code, mods, app)` — ↑↓/j/k skipping Headers, Home/g/End/G, PgUp/PgDn ±5, Enter/Space/→ forward, ← backward, Esc close
- [x] `dispatch_settings_action(app, client, action)` async dispatcher:
      — Toggle local flags (ShowThinking, Notify, Vim, PlanAutoRun)
      — Toggle + RPC (AutoCompact, AutoRetry)
      — Cycle local (Theme)
      — Cycle + RPC (ThinkingLevel, Model, SteeringMode, FollowUpMode)
- [x] 5 regression tests pin the row coverage + dispatch plumbing
- [x] `fmt_elapsed` promoted to `pub(super)` and re-imported from `draw.rs`

**Shipped as `f857223`.**

## V2.13.c — header + footer slim-down ✅

- [x] `draw_header` — removed thinking badge, session-name chip, explicit "pi offline" label (folded into heartbeat color), default zero-queue text. Kept: wordmark, heartbeat, spinner, model (shortened), live-state chip, queue chips (only when non-zero), git chip, turn counter.
- [x] Model label shortened via `short_model_label` — drops provider prefix when full label > 24 chars.
- [x] `draw_footer` — single row. Context gauge on left, right-aligned `? help · /settings · /shortcuts` hint chip. Flash messages replace the chip for ~1.5 s then revert.
- [x] Footer Layout::Constraint reduced from Length(2) to Length(1).
- [x] User-guide "Screen layout" diagram updated to reflect the new chrome.

**Shipped as `5368673`.**

## V2.13.d — status widget + docs + tracker closeout ✅

- [x] Status widget reshaped: 4 rows → 3 rows (top-rule only instead of full bordered box). `status_h` constant updated.
- [x] `Block::default().borders(Borders::TOP)` with inline title. Border color still encodes live state.
- [x] User-guide sections added/updated:
      — New "Settings panel" section with sections table, row-kinds, full keyboard reference
      — New "Shortcuts panel" section summarising the groups
      — "Screen layout" diagram rewritten for the 1+1+3+1 chrome shape
      — "Status widget and header" section rewritten per-row with the new semantics
      — `/settings` and `/shortcuts` added to the slash-commands Session sub-list
- [x] Rolling metrics table below filled in.

**Ready for the closeout commit.**

---

## Rolling metrics

| | V2.12 | V2.13.a | V2.13.b | V2.13.c | V2.13.d |
|---|---|---|---|---|---|
| Tests | 188 | 189 | 194 | 194 | 194 |
| Header rows | 1 | 1 | 1 | 1 | 1 |
| Footer rows | 2 | 2 | 2 | **1** | 1 |
| Status widget rows | 4 | 4 | 4 | 4 | **3** |
| Chrome rows total | **7** | 7 | 7 | 6 | **5** |
| Transcript rows gained vs V2.12 | 0 | 0 | 0 | +1 | **+2** |
| `src/app/mod.rs` LOC | 5 815 | +300 | +700 | +130 | (docs only) |

## Notes

- The `SettingsAction::Cycle` carries a direction field that's currently dead code (forward and backward cycle both advance; pi has no `previous_model` RPC). Kept for forward-compat and documented inline with `#[allow(dead_code)]`.
- Flash messages replacing the footer hint chip is intentional: they auto-expire in ~1.5s, so the hint is back before the user can forget about it. No state lost.
- `/settings` rebuilds its row list on every draw (one alloc of ~30 rows). Negligible — the build function is ~100 µs on release. If it ever shows up in a profile, cache via `update_..._cache` like the visuals/heights work in V2.11.2.
- The "Paths" section in /settings intentionally shows only `history file` and `crash dumps`; the log file path would need hooking through the `log` module (`tracing-appender` keeps it internal). Small follow-up in a future milestone.
- `Mcp` section not shown in /settings Capabilities because pi doesn't expose MCP state yet. The `/mcp` modal still renders the placeholder.
