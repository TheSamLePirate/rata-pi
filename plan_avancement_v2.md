# V2 progress tracker

Paired with `PLAN_V2.md`. Checklist per milestone; deviations noted in the **Notes** block at the bottom of each section. All boxes `[ ]` until the milestone ships.

Legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated from plan (see notes) · `[—]` intentionally dropped / deferred

---

## V2.0 — Foundation refactor + animation + theme

### Architecture
- [ ] Create `app/` module split (state.rs, action.rs, events.rs, input.rs, commands.rs, runtime.rs)
- [ ] Define `Action` enum (≥ 40 variants; domains: Rpc, Input, Ui, Session, Git, Tools, Anim)
- [ ] Define `Cmd` enum (SendRpc, Fsy*, StartAnim, Notify, …)
- [ ] Implement pure `step(App, Action) -> (App, Vec<Cmd>)` reducer
- [ ] Migrate every handler from V1 `App::on_event` and `handle_*` into Actions
- [ ] Runtime `tokio::select!` at 60 fps max, parks when nothing moves
- [ ] Reducer snapshot tests (insta) against 20+ action scripts

### Theme system (`src/theme/`)
- [ ] `Theme` struct + semantic palette (see PLAN_V2 §9)
- [ ] Built-ins: tokyo-night, dracula, solarized-dark, solarized-light, catppuccin-mocha, gruvbox-dark, nord
- [ ] TOML loader from `~/.config/rata-pi/themes/*.toml`
- [ ] `Ctrl+Shift+T` cycle; `/theme [name]`; `/themes` picker
- [ ] `notify` watcher → hot-reload on file save
- [ ] Plumb theme through every widget (header, footer, editor, transcript, modals, status)
- [ ] Theme golden snapshots at 80×24 for each built-in

### Animation engine (`src/anim/`)
- [ ] Active-animation registry (`HashMap<AnimId, ActiveAnim>`)
- [ ] Tick source drives only while non-empty
- [ ] Ease functions (linear, ease-out-cubic, smoothstep)
- [ ] Transition helpers for color/ratio/offset
- [ ] Spinner type reusable across widgets

### Acceptance
- [ ] `cargo test` ≥ V1 count
- [ ] Manual parity pass: every V1 feature still works identically
- [ ] vhs demo `docs/demos/v2_0.gif`

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
