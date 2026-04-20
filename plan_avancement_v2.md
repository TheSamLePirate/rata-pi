# V2 progress tracker

Paired with `PLAN_V2.md`. Checklist per milestone; deviations noted in the **Notes** block at the bottom of each section. All boxes `[ ]` until the milestone ships.

Legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated from plan (see notes) · `[—]` intentionally dropped / deferred

---

## V2.0 — Foundation refactor + animation + theme ✅ (pragmatic subset)

### Architecture
- [—] `app/` module split — **deferred to V2.0.1**. The mechanical refactor of a 2.6k-line file into `app/{state,action,events,input,commands,runtime}` is its own commit; doing it *inside* V2.0 would bury the theme deliverable and risk behavior regressions. Pure reducer bundled with the split.
- [—] `Action` enum + `Cmd` enum + pure reducer — **deferred to V2.0.1**
- [—] Reducer insta snapshot tests — **deferred to V2.0.1**

### Theme system (`src/theme/`)
- [x] `Theme` struct + full semantic palette (21 semantic fields: accent, accent_strong, muted, dim, text, success, warning, error, role_{user,assistant,thinking,tool,bash}, border_{idle,active,modal}, diff_{add,remove,hunk,file}, gauge_{low,mid,high})
- [x] Six built-ins in true-color RGB: `tokyo-night` (default), `dracula`, `solarized-dark`, `catppuccin-mocha`, `gruvbox-dark`, `nord`
- [—] `solarized-light` skipped — we picked the six best-contrast dark themes and one cool-accent (nord); a light variant lands in V2.0.1 alongside the TOML loader
- [—] TOML loader + `notify` hot-reload — **deferred to V2.0.1** (adds `toml` + `notify` deps; ship the built-ins now and iterate)
- [x] `Ctrl+Shift+T` cycles through the six themes
- [x] `/theme [name]` picks by name (case-insensitive); `/theme` with no arg cycles
- [x] `/themes` opens a picker modal with the full list; Enter applies directly without echoing a slash command into the composer
- [x] Plumbed through every draw surface — header (spinner, model label, status/thinking badges, queue chip), transcript (user/pi/thinking prefixes + markdown output), tool cards (status chip by state), bash card ($ prefix + exit-code chip + gray body), diff heuristic (+/−/@@/±±± hunks), compaction + retry rows, editor border + title, context gauge (gradient low/mid/high), modal border + title + bottom-hint, every picker's `▸ selected` highlight, extension dialog select/confirm/input bodies, toast stack chips, info/warn/error rows, connection-failure banner
- [x] Boot info line echoes the current theme name so a fresh user sees what's active
- [—] Theme golden snapshots — **deferred to V2.12** per PLAN_V2 §11; `insta` isn't in the dep tree yet

### Animation engine (`src/anim/`)
- [x] `Anim` type with `new(duration)` / `looping(duration)` + `progress()` + `is_done()`; suitable as the primitive for V2.1's spinner-border-pulse and retry-countdown-ring
- [x] Easing kit in `anim::ease`: `linear`, `ease_out_cubic`, `smoothstep`, `triangle` (the last for breathing pulses)
- [x] Color/ratio tween helpers: `lerp_u8`, `lerp_f64`
- [—] Active-animation registry + tick-source parking — **shape exists but registry ships with V2.1** where StatusWidget first needs it
- [x] 5 unit tests across `anim` (progress range, looping, lerp endpoints, ease endpoints, ease-out front-loaded)

### Acceptance
- [x] `cargo test` ≥ V1 count → 62 (V1 was 53; gained 9 from theme + anim)
- [x] Manual parity pass: every V1 feature still works — same keybindings, modals, RPC round-trips, submit paths
- [x] `cargo clippy --all-targets -- -D warnings` clean
- [x] `cargo fmt --check` clean
- [—] vhs demo `docs/demos/v2_0.gif` — **deferred to V2.12 polish pass** (no `vhs` dep in CI yet)

**Notes:**
- Scoped V2.0 to ship visible UX improvement (themes) without the architectural rewrite. That rewrite becomes V2.0.1 — noted as a single tracked milestone rather than mixed into this one, so the theme work is reviewable in isolation and the reducer refactor can proceed behind stable snapshots.
- `kb()` helper still returns `Color::Cyan` instead of theme accent; picker highlight uses `t.accent`. On 256-color terminals they look cohesive enough; on truecolor kb will bridge in V2.0.1.
- Tokyo-Night selected as default because its accent blue doubles well as the streaming border and its magenta reads as "thinking" in every test.
- `Ctrl+Shift+T` handler is placed *before* `Ctrl+T` so the more-specific match wins — crossterm reports both modifiers in the same event.
- Kept `Color::Rgb(0, 0, 0)` in two chip fg spots (bash exit-code chip, ext-confirm Yes/No chip, toast chips) because solid-bg chips read best with guaranteed-black text on every theme; tempting to swap to `t.text` but it breaks on light themes. Revisit in V2.0.1 with a contrasting-text helper.

---

## V2.0.1 — Architecture refactor + TOML themes + hot-reload (queued)

Planned follow-up to finish the V2.0 feature list without blocking the theme deliverable.

- [ ] `app/` module split per PLAN_V2 §2 (state / action / events / input / commands / runtime)
- [ ] Pure reducer `step(App, Action) -> (App, Vec<Cmd>)`; migrate every current handler
- [ ] Insta snapshot tests for reducer transitions (≥ 20 scripts)
- [ ] `toml` dep + `~/.config/rata-pi/themes/*.toml` loader
- [ ] `notify` dep + watcher → hot-reload on file save
- [ ] `solarized-light` built-in (light-mode contrast)
- [ ] `kb()` helper uses `t.accent_strong`

**Notes:** _(to fill in)_

---

## V2.1 — StatusWidget + header gauges + sparklines

- [ ] `LiveState` struct tracking: current op, per-tool status, turn tokens, session cost, heartbeat
- [ ] `StatusWidget` per PLAN_V2 §8 with all 10 states
- [ ] Retry countdown ring (800 ms per sec, 8-frame arc)
- [ ] Heartbeat dot top-right (green pulse per event, red if 10 s silence while streaming)
- [ ] Sparkline widget (60-bucket rolling)
- [ ] Throughput sparkline (tok/s) from message deltas
- [ ] Cost sparkline from `turn_end.usage.cost.total`
- [ ] Connection-quality sparkline from stats-ticker RTT
- [ ] Animated smooth Gauge (tween fill, pulse red ≥85%)
- [ ] StatusWidget golden snapshots for each state
- [ ] Fire-every-event contract test (each `Incoming` variant changes status)

**Notes:** _(to fill in)_

---

## V2.2 — Conversation view overhaul

- [ ] Message card widget (rounded borders `╭╮╰╯`, role-themed bg tint)
- [ ] Role avatars (emoji or ASCII per role)
- [ ] Relative timestamp + absolute on focus
- [ ] Virtualized transcript (tui-scrollview or custom)
- [ ] Focus mode (`Esc` toggles; `j/k/↑/↓` navigate; `Enter` expand; `c` copy)
- [ ] Nested tool-call tree (tool calls visually nested inside assistant turn)
- [ ] Sticky "live tail" indicator while scrolled up (+ `End` to re-pin)
- [ ] Per-message collapse/expand (keyboard + click)
- [ ] 10k-message stress test: maintains 60 fps
- [ ] Golden snapshots of every message-card state (idle/focused/collapsed)

**Notes:** _(to fill in)_

---

## V2.3 — Syntect + diff widget + extended markdown

- [ ] `syntect` integration (default-on via `syntax` feature)
- [ ] Markdown fenced code → syntect-highlighted lines
- [ ] `diff.rs` widget: unified by default, side-by-side via `d/D`
- [ ] Auto-detect lang from diff header path or fence tag
- [ ] Line-numbers toggle (`Ctrl+Shift+L`)
- [ ] Tool renderer registry: bash / read_file / edit / apply_patch / str_replace / grep / todo / unknown
- [ ] Each tool renderer owns its card body
- [ ] Markdown table rendering (ratatui Table widget)
- [ ] Syntax-theme fallback when user's theme doesn't define one

**Notes:** _(to fill in)_

---

## V2.4 — Images + Kitty keyboard + OSC 52

- [ ] Feature-gated `images` (ratatui-image + viuer fallback + text fallback)
- [ ] Terminal capability probe (`TERM_PROGRAM`, `KITTY_WINDOW_ID`, `TERM`)
- [ ] Image paste: clipboard → base64 → `ImageContent` attachment
- [ ] Kitty keyboard protocol enable/disable
- [ ] OSC 52 clipboard write (SSH)
- [ ] Mouse drag-select across transcript → OS clipboard
- [ ] `arboard` primary; OSC 52 fallback
- [ ] Drag-select region preserved across redraws

**Notes:** _(to fill in)_

---

## V2.5 — Scrollable slash menu + 20 new commands

- [ ] `CommandMenu` widget with virtualized list + right-pane help
- [ ] Category groups with dividers (Session / Model / Git / View / Tools / Extensions / Debug)
- [ ] MRU re-ordering within categories
- [ ] Per-item key-hint badge
- [ ] Aliases file `~/.config/rata-pi/aliases.toml`
- [ ] Inline arg hints (`/rename <name>`)
- [ ] Filter searches name + description (not just name)
- [ ] All commands from PLAN_V2 §6 wired (40+)
- [ ] Slash-menu scroll works with wheel + PgUp/PgDn + j/k
- [ ] `?` inside menu opens that command's detail page

**Notes:** _(to fill in)_

---

## V2.6 — File tree + fuzzy + grep + @file

- [ ] `ignore` crate walker; respects .gitignore + .git/info/exclude
- [ ] `FileTree` widget (tui-tree-widget)
- [ ] `F2` toggles sidebar; `/files` focuses it
- [ ] `Ctrl+P` fuzzy file finder modal
- [ ] `Ctrl+G` live grep modal (uses ripgrep shell-out; pipes as stream)
- [ ] `@path` autocomplete in composer with floating picker + live preview pane
- [ ] `/find <q>` and `/grep <q>` slash commands
- [ ] Preview pane shows first 40 lines with syntect highlighting
- [ ] Open file in default editor via `/edit <path>` (stretch)

**Notes:** _(to fill in)_

---

## V2.7 — Git integration

- [ ] `git2` feature flag (`git`, default-on); shell-out fallback
- [ ] Header `git` chip: branch + dirty dot (●/○)
- [ ] `/diff` (unstaged), `/diff --staged`
- [ ] `/log [n]` last N commits with author/date/subject
- [ ] `/commit <msg>` modal with staged-files preview
- [ ] `/branch` (list); `/switch-branch <n>`; `/stash`
- [ ] Inline diff view uses V2.3 widget
- [ ] Works in non-git dirs (chip hidden)

**Notes:** _(to fill in)_

---

## V2.8 — Plan mode + TODO + diagnostics

- [ ] `/plan` toggles plan-mode
- [ ] Plan-mode intercepts `tool_execution_start`; shows preview card; Y/N gate
- [ ] Y → forward the tool call; N → send steer "stop, let's reconsider"
- [ ] TODO widget (right-side persistent; keyboard CRUD)
- [ ] `/todo add|done|rm|list`
- [ ] TODO persists to `~/.local/share/rata-pi/todos/<session>.md`
- [ ] Diagnostics panel stretch (`/diagnostics` shells `cargo check` for Rust projects)

**Notes:** _(to fill in)_

---

## V2.9 — Timeline + replay + snapshots

- [ ] `/timeline` modal — branch graph of every session under pi's session dir
- [ ] Fork edges rendered git-log style
- [ ] `--replay <file>` CLI — steps events with n/p/N/P
- [ ] `/save [name]` snapshots transcript to `snapshots/<name>.{md,jsonl}`
- [ ] `/load <name>` restores as read-only overlay
- [ ] `/snapshots` lists saved snapshots
- [ ] Replay progress bar + speed controls (1x/2x/4x)

**Notes:** _(to fill in)_

---

## V2.10 — Multi-line + vim/emacs + templates

- [ ] `tui-textarea` composer with undo/redo + word motions
- [ ] Shift+Enter newline; Enter submit
- [ ] `/vim` mode (normal/insert; `i`/`a`/`o`/`hjkl`/`wb`/`dw`/`yy`/`p`/`:w`/`:q`)
- [ ] `/emacs` mode (Ctrl-heavy bindings)
- [ ] `/default` restores
- [ ] Current mode indicator in editor border
- [ ] `/templates` — picker over `~/.config/rata-pi/templates/*.md` (template vars `{{selection}}`, `{{file}}`)
- [ ] Undo history snapshot tests

**Notes:** _(to fill in)_

---

## V2.11 — Hooks + notifications + MCP

- [ ] `~/.config/rata-pi/hooks.toml` schema (event → shell cmd OR notify)
- [ ] `on_agent_end`, `on_tool_error`, `on_compaction`, `on_retry_exhausted`
- [ ] `notify-rust` desktop notifications (feature `notify`)
- [ ] OSC 777 emit
- [ ] Crash dump to `~/.local/state/rata-pi/crash-<ts>.log` with color-eyre report
- [ ] `/mcp` modal — list MCP servers pi exposes (from extended `get_state` if present)
- [ ] `/doctor` modal — checks: pi on PATH, clipboard, image protocol, git, terminal capabilities

**Notes:** _(to fill in)_

---

## V2.12 — Release polish

- [ ] `docs/` — manual, keybinding reference, theme gallery, troubleshooting
- [ ] vhs recipes for every major feature; gifs committed to `docs/demos/`
- [ ] Shell completions via clap: bash, zsh, fish
- [ ] Man page via clap_mangen
- [ ] `cargo-dist` CI pipeline for mac/linux/windows artifacts
- [ ] GitHub Pages site with theme gallery + recorded demos
- [ ] CHANGELOG.md keep-a-changelog format
- [ ] Insta snapshots for all widgets (≥ 60)
- [ ] End-to-end test with scripted mock pi (≥ 10 scenes)
- [ ] Benchmarks under `benches/`: stream deltas, render 10k, fuzzy 50k files
- [ ] Semver tag v2.0.0

**Notes:** _(to fill in)_

---

## Cross-milestone backlog

_Discovered mid-flight. Pull into the current or next milestone when triaged._

- [ ] _(empty — populate as we go)_

---

## Rolling metrics

Kept current at each milestone boundary.

| | V1 final | V2.0 | V2.1 | … |
|---|---|---|---|---|
| Tests | 53 | — | — | |
| LOC (bin) | 5907 | — | — | |
| Binary size (release-stripped) | ~2.5 MiB | — | — | |
| Cold start to first draw | — | ≤ 200 ms | | |
| Idle CPU | — | 0.0 % | | |
| Snapshot count | 0 | — | — | |
