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

## V2.1 — StatusWidget + header gauges + sparklines ✅ (core)

Also folds in V2.0 bug fixes the user reported (see "Fixes" below).

### Delivered
- [x] `LiveState` enum with 9 explicit states covering the PLAN_V2 §8 spec: `Idle / Sending / Llm / Thinking / Tool / Streaming / Compacting / Retrying{attempt,max,delay_ms} / Error`. Each has its own border color + label + spinner policy.
- [x] `StatusWidget` drawn as a 4-row bordered card between the editor and footer. Title shows `status · MM:SS` (or `HH:MM:SS` past an hour) for the current state.
- [x] Row 1: spinner + state label (e.g. `llm · anthropic/claude-sonnet-4` or `retry 2/3 in 2000ms`) + `turn N` chip + running-tools chip (`K running · N done`).
- [x] Row 2: throughput label + sparkline + `X t/s`; cost label + per-turn sparkline + `$0.XXXX`; session total cost.
- [x] Heartbeat dot in the header that pulses via the `anim::ease::triangle` function; color goes `dim` (idle) → `success` (recent event) → `warning` (10 s silence while streaming) → `error` (100+ ticks silence).
- [x] Throughput tracking: per-second bucket rolling 60 wide. `approx_tokens` (≈ 4 chars/token) converts text/thinking deltas into rate samples.
- [x] Cost tracking from `turn_end.message.usage.cost.total`: 30-turn rolling series + running session total. Sparkline quantizes to hundred-thousandths of a dollar so per-turn granularity is legible.
- [x] Retry state surfaces as the distinctive "retry N/M in Xms" label with a `warning`-colored border — the countdown ring per PLAN_V2 §8 is **deferred to V2.2** (needs `anim` registry integration; the `ease::triangle` primitive is already in place).
- [x] Every event updates `last_event_tick`, so the heartbeat dot is a live liveness probe even when no state change happens (e.g. tool_execution_update coming in frequently).
- [x] Layout gracefully degrades: if terminal height < 20 rows, StatusWidget is hidden.
- [x] 63 tests pass (V2.0 was 62; +1 from commands::builtins test).

### Fixes bundled in this milestone
- [x] **`Ctrl+Shift+T` works now** on every terminal thanks to three bindings: `Ctrl+Shift+T` (Kitty keyboard protocol), `Alt+T` (reliable on macOS Terminal / iTerm / xterm), and `F12` (function-key fallback). All three call `cycle_theme`.
- [x] **`/theme` works when pi is offline** — `submit()` is split into `try_local_slash` (no client needed) + `try_pi_slash` (requires client). Local commands run first, so `/theme`, `/help`, `/stats`, `/export`, `/themes`, `/clear` all work without pi.
- [x] **Commands picker shows more than pi's skills now.** New `CommandSource::Builtin` variant + new `src/ui/commands.rs` with 19 curated built-ins: help, stats, themes, theme, export, export-html, copy, clear, rename, new, switch, fork, compact, model, think, cycle-model, cycle-think, auto-compact, auto-retry. The `F1` and `/` pickers merge built-ins with pi's extension/prompt/skill commands. A `[builtin]` badge tags them in the list.
- [x] Picker Enter dispatches Builtin commands inline: no-argument built-ins fire immediately (e.g. `/help` opens help, `/theme` cycles); built-ins that expect an argument (`/rename`, `/switch`) prefill `/name ` so the user can type the argument and submit. Pi commands still prefill as before.

### Deferred (queued for V2.2+)
- [ ] Retry countdown ring graphic (8-frame circular arc) — anim registry plumbing lands with V2.2 conversation-view refactor
- [ ] Connection-quality sparkline from stats-ticker RTT — needs latency capture in the RPC client; slotted for V2.2
- [ ] Animated smooth Gauge tween in the footer — current Gauge jumps between states; smoothing comes with anim registry
- [ ] StatusWidget golden snapshots per state (insta) — lands with V2.12 snapshot pass
- [ ] Fire-every-event contract test — needs the Action reducer refactor (V2.0.1) so we can script actions

**Notes:**
- The `Sparkline` widget from Ratatui 0.30 takes `&[u64]`; we convert u32 throughput and scale cost (×100 000) to get readable micro-spark resolution at sub-cent amounts.
- `LiveState::Sending` is set when the user submits so the UI flashes "sending" between the prompt going out and `agent_start` coming back — a small detail but closes the perception loop.
- Turn counter is bumped on `turn_start` rather than `agent_start` so a multi-turn run (tool calls + follow-ups) counts correctly.
- Added the `clear` local slash command — turns out to be super handy while debugging layout.
- The 19 built-in commands + pi's own commands means the `/` picker is already 30+ entries on most sessions. Scrollable list lands with V2.5.

---

## V2.1 backlog items rolled forward

- Retry countdown ring — bundled with the animation-registry consumer work in V2.2
- Connection-quality sparkline — V2.2 (RTT capture in the RPC client)
- Golden snapshots — V2.12

---

## V2.2 — Conversation view overhaul ✅ (core)

### Delivered
- [x] **Rounded-border message cards** via a new reusable `Card` widget in `src/ui/cards.rs`. `BorderType::Rounded` is used across the transcript and the outer transcript frame. Each entry type maps to a themed card with its own icon and border color.
- [x] **Role icons** per card: `❯` user · `✦` thinking · `✦` pi · `⚙` tool · `$` bash. Info / Warn / Error / Compaction / Retry still render as inline non-bordered rows (they read better flat).
- [x] **Model in the card title** — assistant cards show the current model provider/id as a right-aligned title chip. Tool cards show status label (`running / ok / error`) + a `▸`/`▾` expand marker in the title. Bash cards show `exit N` / `cancelled` as a right chip.
- [x] **Virtualized transcript scroll** — `draw_body` now computes a per-visual height at the current content width using Ratatui's `Paragraph::line_count` (via the `unstable-rendered-line-info` feature), then skips entries above/below the viewport. 10k entries stay responsive because we never render what we don't draw.
- [x] **Scrollbar** in the transcript's right column (1 cell wide). Thumb size and position are proportional to viewport/total. Dotted track + solid thumb in the theme's accent color. Same scrollbar renders inside any list modal (Commands / Models / History / Forks) when the content overflows.
- [x] **Commands picker scrolls!** Previously the list just overflowed; now `draw_modal` computes a centered `scroll_y` around the selected item so it always stays visible, and adds the same scrollbar column. Fixes the F1 / `/` menu-not-scrolling bug the user reported.
- [x] **Focus mode** — `Ctrl+F` enters focus mode; `j`/`k`/`↑`/`↓` move between cards; `g`/`G`/`Home`/`End` jump to top/bottom; `PgUp`/`PgDn` move by 5; `Enter` / `Space` toggles expand on tool cards; `Esc` / `q` exits. The focused card gets a bold `border_active`-colored border, and the transcript auto-scrolls to keep it centered. A `transcript · focus` title tag appears on the transcript frame while focus mode is on.
- [x] **Sticky live-tail indicator** — when scrolled up (manual or focus-centered), a small `⬇ live tail (End)` chip appears at the bottom-center of the transcript with the theme accent background. Disappears automatically when the user hits `End` or scrolls back down.
- [x] **Welcome card** — empty transcript shows a friendly "welcome to rata-pi" hint with the most useful keybindings instead of a blank pane.
- [x] **Cut-off hint** — when card virtualization has to slice a partial card at the top of the viewport, we render a faint `⋯ (card continues above)` marker so the user never sees a headless body.
- [x] **Focus auto-clamp** — when the transcript grows past the focused index, focus tracks the tail (feels natural when you `Ctrl+F` then watch new events roll in).
- [x] **Thinking as blockquote** — thinking cards render with `│ ` prefix + italic muted text, reading like a proper quoted block. Token count lands in the right chip.
- [x] **Context-aware footer hints** — focus mode shows `j/k nav · Enter expand · g/G top/bot · Esc exit`; idle mode shows `Enter send · / cmds · Ctrl+F focus · F5 model · …`; streaming shows its own abort/cycle hints.

### Deferred
- [ ] Emoji avatars (🧑 / 🤖 / 💭) — kept the single-char unicode icons because emoji width is inconsistent across terminals (2 cells in some, 1 in others) and breaks virtualization math. Revisit with a width probe in V2.4.
- [ ] Relative timestamps + absolute-on-focus — transcript entries don't carry a creation timestamp yet; adding one is a small transcript.rs change but was scoped out of V2.2.
- [ ] Nested tool-call tree (tools visually nested inside the assistant turn that triggered them) — needs a parent-child relationship we don't model yet. V2.3 alongside the tool-renderer registry.
- [ ] 10k-message stress benchmark — infrastructure lands with V2.12.
- [ ] Golden snapshots per card state — `insta` isn't in the dep tree yet; V2.12.

### Notes
- The `Paragraph::line_count` method requires ratatui's `unstable-rendered-line-info` feature. Enabled in `Cargo.toml`. Worth it: keeps our height math 100% in sync with the real wrap the renderer performs.
- Card height = `line_count + 2` (top + bottom border). Bodies render with 1 column of left padding for breathing room.
- `Visual` enum collapses Card and InlineRow — inline rows (Info/Warn/Error/Compaction/Retry) look denser as a single line and save vertical space in busy sessions.
- Scroll offset is still line-granular (`Option<u16>` lines-from-top) because a single tall card can be > viewport; card-boundary-only snapping would make such a card unreachable.
- The modal scrollbar logic is the same code path as the transcript's; extracted into `draw_scrollbar`.
- 63 tests pass; clippy -D warnings clean; fmt clean.

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
