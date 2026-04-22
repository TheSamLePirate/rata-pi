# Changelog

All notable changes to rata-pi are documented here.

The format loosely follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] — 2026-04-22

First tagged release. `1.0.0` consolidates the V2 → V3 → V4 evolution
into something you can `cargo install rata-pi` or
`brew install TheSamLePirate/rata-pi/rata-pi`.

### What rata-pi is

A Ratatui terminal UI for the
[Pi coding agent](https://github.com/mariozechner/pi-coding-agent).
It spawns `pi` over JSONL RPC, streams its output into a scrolling
transcript, and layers keyboard-driven modals, git integration, plan
approval, agent-driven interview forms, and template reuse on top.

### Added — key user-visible features since the Ratatui scaffold

**Chat / transcript**
- Streaming transcript with per-entry render cache and virtualized
  scrolling — long sessions stay responsive.
- Focus mode (`Ctrl+F`) for card-level navigation; second click on a
  tool card toggles expanded output.
- Markdown rendering with fenced-code syntect highlighting. Both
  palette-aware — markdown and code fences track the active rata-pi
  theme.
- Seven built-in themes including `high-contrast` for CVD-friendly /
  low-color terminals.

**Plan approval flow** (V3.f)
- Agent-emitted `[[PLAN_SET: …]]` now opens a review modal instead of
  running immediately. Accept / Edit / Deny.
- Edit mode can add, delete, and rewrite steps before acceptance.
- `[[PLAN_ADD: …]]` against an accepted plan opens an amendment
  review; accepting merges into the active plan preserving completed
  steps.
- Marker visibility toggleable via `/settings → Appearance → show raw
  markers` (default: stripped).
- `/plan set a | b | c` from the user still activates immediately —
  user plans are commands, agent plans are proposals.

**Agent interview mode** (V2.12.g)
- Agents emit `[[ASK_TEXT: …]]` / `[[ASK_SELECT: …]]` etc. to pop a
  structured form. Tab navigates fields, Space / Enter toggles, Ctrl+S
  or Ctrl+Enter submits.

**Modals and UI**
- `/settings` — every tunable + live state (model, thinking level,
  auto-compact, auto-retry, theme, focus-marker, notifications, etc.).
- `/shortcuts` — read-only keybinding reference.
- `/doctor` — readiness checks (pi binary, terminal caps, clipboard,
  git, notifications).
- `/stats`, `/diff`, `/log`, `/branch`, `/commit`, `/stash` — git +
  session inspection.
- **Transcript search** (V4.b) — `/search` opens an overlay with live
  filter, per-hit snippets, `n`/`N` navigation, Enter to focus.
- **Template picker** (V4.c) — `/template` opens a two-pane modal with
  body preview; `/template save <name>` snapshots the composer.
- **File finder** — `Ctrl+P` or `@path` in the composer with bounded
  syntect preview.

**Mouse** (V4.a)
- Click outside any modal closes it.
- Click Plan Review action chips, Settings rows, list-modal rows,
  Thinking picker options, and Ext confirm/select chips.
- Wheel scroll routes into the open modal.

**Composer** (V3.j)
- Undo / redo (`Ctrl+Z` / `Ctrl+Shift+Z`) with a 64-snapshot ring.
- Composer draft auto-saved on `Ctrl+C` / `Ctrl+D` quit; restored on
  next launch.
- `Ctrl+Enter` submit (symmetric with the Interview modal).

**Settings persistence** (V3.j)
- Theme, notification, vim, show-thinking, show-raw-markers, and
  focus-marker preferences saved to `<config_dir>/rata-pi/config.json`.
  Restored on every launch.

**Reliability** (V3.a, V3.b)
- Per-call RPC timeouts — `bootstrap` 3 s, stats 1 s, user actions
  10 s. Timeouts surface as non-fatal flashes; the UI never hangs
  waiting on a degraded pi.
- `RpcClient::call` no longer leaks pending entries on send failure.
- Per-frame I/O in `/settings` reduced from 3+ probes (clipboard, home
  dir, history JSONL) to 0 — all cached on `AppCaps` at startup.
- Transcript fingerprint walk is O(1) on idle frames (mutation epoch
  on `Transcript`).
- File preview reads bounded — `metadata()` rejects > 50 MiB before
  any read; sample is `BufReader::take(8 KiB)`.

**Observability / footer**
- Color-coded flash toasts — `info` / `success` / `warn` / `error` each
  render in their own theme slot with a matching glyph (`ℹ ✓ ⚠ ✗`).
- State-aware status sparklines — throughput and cost lines tint
  `theme.error` during `LiveState::Error`.
- Heartbeat-color legend in the header (green/yellow/red ≈
  healthy/stalled/offline).

**CI** (V3.c)
- Matrix covers Ubuntu, macOS, and Windows.
- Clippy enforced with `-D warnings` explicitly.
- Separate `build --features notify` and `build --release` smoke
  jobs.

### Architecture

- 255 tests, clippy `-D warnings` clean, fmt clean, 5.3 MiB release
  binary.
- `src/app/mod.rs` split across ten focused modules: `draw`, `events`
  (reducer), `input`, `cards` (transcript card bodies),
  `modal_keys` (per-modal key dispatcher), `modals/bodies`,
  `modals/interview`, `modals/settings`, `helpers`, `visuals`.
- In-crate `RpcClient::TestHarness` for unit tests that assert the
  serialized RPC payload of every settings / interview dispatch path.

### Known deviations from the original plans

All tracked in `track_v3_progression.md` and `track_v4_progression.md`
— summarized here for transparency:

- Config file is JSON, not TOML (keeps dep surface smaller).
- Mock-pi harness lives in-crate under `#[cfg(test)]`, not in
  `tests/`.
- `src/app/mod.rs` sits at 5 375 lines vs. the V4.d < 4 000 target;
  residual is ~2 100 lines of in-file unit tests.
- Inline highlight-on-match in the transcript search is not shipped
  — the modal's per-hit snippet + jump-to-focus covers the use case.
- Click-to-activate on list-modal rows sets the selection; Enter
  still activates (intentional — prevents misclick accidents).
- Commands modal rows are not mouse-clickable (category-grouped
  layout would need a dedicated hit-test pass).
- `s` shortcut inside the template picker was dropped in favour of
  the existing `/template save <name>` slash.
- TODO widget dropped from V3.j — no user demand surfaced.

### Thanks

Ratatui for the TUI framework; pi-coding-agent for the engine rata-pi
drives. Every modal, persistence feature, and piece of test harness
that landed between V2 and V4 is listed above. `1.0.0` is the point
where "rata-pi" stops being a local checkout and starts being
something you install.
