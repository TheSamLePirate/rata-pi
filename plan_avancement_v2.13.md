# V2.13 progress tracker

Paired with `PLAN_V2.13.md`. Per-sub-commit checklist; notes at the bottom.
Legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated · `[—]` dropped

---

## V2.13.a — `/shortcuts` modal

- [ ] `Modal::Shortcuts` variant (holds a `scroll: u16`)
- [ ] `/shortcuts` slash command handler opens it
- [ ] Catalog entry in `src/ui/commands.rs`
- [ ] Draw function with sections + key/action pairs
- [ ] Modal key handler: ↑↓/j/k/PgUp/PgDn/Home/End/Esc
- [ ] User-guide section "Shortcuts modal"
- [ ] Regression test that construction + rendering doesn't crash

## V2.13.b — `/settings` modal

- [ ] `Modal::Settings(SettingsState)` variant
- [ ] `SettingsState { selected: usize, scroll: u16, user_scrolled: bool }`
- [ ] `build_settings_rows(app) -> Vec<SettingsRow>` rebuilds live every frame
- [ ] `SettingsRow` enum: Header / Info / Toggle / Cycle
- [ ] Actions: toggle flips App state (firing RPCs where needed);
      cycle steps through a fixed list (theme, thinking level, etc.)
- [ ] Draw function: two-column layout, focus marker, section dividers,
      value column colored by kind
- [ ] Key handler: ↑↓/j/k skipping Headers, Enter/Space dispatch,
      ←/→ for cycle step, PgUp/PgDn for ±5, Home/End for first/last,
      Esc to close
- [ ] `/settings` slash command opens it
- [ ] Catalog entry in `src/ui/commands.rs`
- [ ] User-guide section "Settings modal"
- [ ] Regression tests: row assembly covers every App flag,
      toggle dispatch flips flags, cycle dispatch advances indexes

## V2.13.c — header + footer slim-down

- [ ] `draw_header` drops thinking chip, session-name chip,
      explicit connection label (folds into heartbeat color)
- [ ] Model label shortened (provider/id) when terminal narrow
- [ ] `draw_footer` becomes 1 row: context gauge + right-aligned
      `?/help · /settings · /shortcuts` hint
- [ ] Flash overlay moved to 1-row above the footer (toast-style);
      drops when empty
- [ ] Layout math (composer rows + widgets + status + footer) updated
- [ ] User-guide "Screen layout" diagram updated

## V2.13.d — status widget + docs + tracker closeout

- [ ] Status widget reduced to 3 rows (spinner/state row + two
      metric rows). Hidden rule unchanged.
- [ ] User guide: new "Settings modal" + "Shortcuts modal" sections,
      updated "Screen layout" + "Quick cheat sheet"
- [ ] Tracker rolling metrics updated
- [ ] Close-out commit

---

## Rolling metrics

| | V2.12 | V2.13.a | V2.13.b | V2.13.c | V2.13.d |
|---|---|---|---|---|---|
| Tests | 188 | — | — | — | — |
| Chrome rows (header + footer + status) | 1+2+4=7 | — | — | 1+1+4=6 | 1+1+3=5 |

## Notes

_(Filled in per sub-commit as we go.)_
