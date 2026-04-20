# rata-pi V2 — the mind-blowing TUI agent harness

> V1 shipped a correct, pro-grade RPC client with all of Pi's protocol wired into a usable TUI (see `plan_avancement.md`). V2 is about *feel*: responsive visuals, rich state signalling, a conversation view that rivals IDE chat panels, and feature parity with Claude Code / Codex / Pi's own TUI — all inside the terminal.
>
> Target: **the best TUI agent harness anyone has ever seen, period.**

```
┌─ rata-pi v2 ─────────────────────────────────────────────────────────────────┐
│ ● anthropic/claude-sonnet-4.6  ·  myproject  ·  3 turns  ·  00:04:12          │
│ ╭─ context ─────────────────────╮ ╭─ throughput ─────╮ ╭─ cost ─────────────╮ │
│ │ 62k / 200k  ███████░░░░░  31% │ │ ▁▂▅▇▆▄▃▁  82tok/s│ │ $0.45  ▁▂▃▅▇ +$.09 │ │
│ ╰───────────────────────────────╯ ╰──────────────────╯ ╰────────────────────╯ │
├──────────────────────────────────────────────────────────────────────────────┤
│ 14:02 › you                                                            2m ago│
│ ╭────────────────────────────────────────────────────────────────────────╮   │
│ │ fix the off-by-one in `render_frame`                                   │   │
│ ╰────────────────────────────────────────────────────────────────────────╯   │
│                                                                              │
│ 14:02 › pi  ✦ claude-sonnet-4.6  thinking high                               │
│ ▾ thinking ──────────────────────────────────────────────────────── 128 tok  │
│ │ The loop goes 0..=width, but we want 0..width. Off-by-one on the upper…   │
│ ╰────────────────────────────────────────────────────────────────────────    │
│ ╭────────────────────────────────────────────────────────────────────────╮   │
│ │ I'll inspect the file then patch the loop bound.                       │   │
│ ╰────────────────────────────────────────────────────────────────────────╯   │
│ ▾ ✓ read_file  `src/app.rs`                                     18ms · 4.2k  │
│   ...                                                                        │
│ ▾ ✓ apply_patch  `src/app.rs`  +3 −1                            41ms         │
│   @@ -812,3 +812,3 @@                                                        │
│   - for x in 0..=width {                                                     │
│   + for x in 0..width {                                                      │
│                                                                              │
├─[  plan ●  ]─[ todo 3/5 ]─[ diagnostics 0 ]─[ git feat/fix-offbyone ⏀ ]──────┤
│ ╭─ prompt (steer) ──────────────────────────────────── theme: tokyo-night ─╮ │
│ │ run the tests                                                             │ │
│ │ _                                                                         │ │
│ ╰───────────────────────────────────────────────────────────────────────────╯ │
│ ⠋ steer · Ctrl+T think · Ctrl+E tool · / cmds · ?help · Ctrl+C quit          │
│ ╭─ status ──────────────────── 00:01:23 ─╮  [auth:ready]  [watcher:active]   │
│ │ ⠋ LLM (claude-sonnet-4.6 · anthropic)  │                                   │
│ │   ├ turn 3 · tokens 412 in · 118 out   │                                   │
│ │   └ tools 2 running, 4 done            │                                   │
│ ╰────────────────────────────────────────╯                                   │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## Contents

1. [Design goals](#1-design-goals)
2. [Architecture refactor](#2-architecture-refactor)
3. [New dependencies](#3-new-dependencies)
4. [Feature surface](#4-feature-surface)
5. [Milestones V2.0 → V2.12](#5-milestones-v20--v212)
6. [Slash command catalog](#6-slash-command-catalog)
7. [Keybinding map](#7-keybinding-map)
8. [State-signal model](#8-state-signal-model)
9. [Visual language](#9-visual-language)
10. [Performance budget](#10-performance-budget)
11. [Testing & release](#11-testing--release)

---

## 1. Design goals

Three ordered principles, everything else falls out of them:

1. **Always tell the user what pi is doing.** No ambiguous spinners. Every state signal pi emits becomes a dedicated visual element.
2. **The transcript is an IDE chat panel, not a terminal scrollback.** Messages are addressable, selectable, nestable, collapsible, diff-aware, syntax-highlighted, navigable.
3. **The UI is alive.** Spinners are smooth, transitions are tweened, state changes have microanimations. Nothing jumps; nothing blinks. 60 fps target, 0% idle CPU.

Non-goals (explicitly parked):
- Browser-style rendering (images via Kitty protocol is feature-flagged; the TUI must still work without it).
- Replacing an editor — we render code/diffs but don't edit files.
- Network services — rata-pi is a local harness. No cloud, no telemetry by default.

---

## 2. Architecture refactor

V1 worked but `app.rs` is now 2.6k lines and rendering, state, input, and RPC routing all live in it. V2.0 lands a module split before any new features.

```
src/
├── main.rs                     boot (unchanged)
├── cli.rs                      clap args (add --theme, --layout, --replay <file>)
├── log.rs
├── app/
│   ├── mod.rs                  Dispatcher: App owns state + systems
│   ├── state.rs                SessionState, ComposerMode, ExtUiState refs
│   ├── action.rs               enum Action; reducer step(Action) -> Vec<Cmd>
│   ├── events.rs               map RPC events → Actions; no mutation here
│   ├── input.rs                map key/mouse/paste/resize → Actions
│   ├── commands.rs             Cmd enum: SendRpc, Fsy(Export), StartAnim, …
│   └── runtime.rs              tokio select! loop + draw loop @ 60 fps max
├── rpc/                        (unchanged from V1 other than minor types)
├── history.rs                  add search index, pinned entries
├── session.rs                  NEW: session files, branches, timeline
├── git.rs                      NEW: git status / log / diff via `git2` or shelling out
├── tools/                      NEW: typed tool renderers per tool name
│   ├── mod.rs                  registry: name → ToolRenderer
│   ├── bash.rs                 ansi-to-tui style output
│   ├── read_file.rs            line-numbered preview
│   ├── edit.rs                 diff widget with syntect highlighting
│   ├── grep.rs                 grouped hits by file
│   ├── todo.rs                 todo list state (Claude Code parity)
│   └── unknown.rs              fallback = current generic renderer
├── anim/                       NEW
│   ├── mod.rs                  ticker; active-animation registry
│   ├── ease.rs                 cubic-out / smoothstep
│   └── particles.rs            confetti / pulse overlays
├── theme/                      NEW
│   ├── mod.rs                  Theme struct + semantic palette
│   ├── builtin.rs              tokyo-night / dracula / solarized / catppuccin / gruvbox / nord
│   └── toml.rs                 loader for ~/.config/rata-pi/themes/*.toml
└── ui/
    ├── mod.rs
    ├── layout.rs               NEW: responsive layout engine (breakpoints, named layouts)
    ├── widgets/                NEW: split ui/transcript, editor, status, header, footer, sidebar
    │   ├── message.rs          message card renderer (per role)
    │   ├── thinking.rs         tree-style thinking
    │   ├── tool_card.rs        animated border + status chip
    │   ├── diff.rs             unified + side-by-side modes
    │   ├── code.rs             syntect-highlighted fenced code
    │   ├── sparkline.rs        micro-chart
    │   ├── gauge.rs            smooth animated gauge
    │   ├── status.rs           the big StatusWidget (see §8)
    │   ├── header.rs           top bar
    │   ├── footer.rs           bottom bar
    │   ├── toast.rs            stack with slide-in
    │   ├── command_menu.rs     scrollable /-menu
    │   ├── file_tree.rs        sidebar tree
    │   └── timeline.rs         session branch timeline
    ├── markdown.rs             extended: table rendering, syntax-highlighted fences
    ├── ansi.rs
    ├── export.rs
    ├── ext_ui.rs
    ├── modal.rs                generic frame; modals become widgets in widgets/
    └── transcript.rs           pure data model (no rendering)
```

The reducer/dispatcher pattern replaces `on_event → mutate self`. Benefits:

- **Testable UI:** `step(App, Action) → (App, Vec<Cmd>)` is a pure function. We snapshot-test state transitions.
- **Replayable:** a session.jsonl can be replayed into a fresh App for demos and regression tests.
- **Undo-friendly:** actions are discrete; we can stash the last N for a "rewind" feature.
- **Cleaner diffs:** rendering never mutates; inputs never render.

Action: an enum with ~50 variants grouped by domain (Rpc, Input, Ui, Session, Git, Tools, Anim).

---

## 3. New dependencies

| Crate | Why | Feature-gate? |
|---|---|---|
| `syntect` | code + diff syntax highlighting | yes — `syntax` feature, default-on |
| `tui-textarea` | multi-line composer with undo, word motions | no |
| `tui-scrollview` | virtualized scroll for long transcripts | no |
| `tui-tree-widget` | file tree, timeline, thinking tree | no |
| `tui-big-text` | startup banner, hero text in modals | no |
| `throbber-widgets-tui` | library of spinner styles | no |
| `ratatui-image` | Kitty/iTerm/WezTerm/Ghostty/Sixel image rendering | yes — `images` feature |
| `viuer` | fallback image rendering | yes — `images` feature |
| `fuzzy-matcher` (skim) | fuzzy scoring for palette, files, grep | no |
| `ignore` | respect .gitignore when walking file tree | no |
| `git2` | git status/log/diff without shelling out | yes — `git` feature, default-on |
| `arboard` | OS clipboard (copy selection) | no |
| `notify` | watch ~/.config/rata-pi for theme hot-reload | no |
| `notify-rust` | desktop notifications on completion | yes — `notify` feature |
| `insta` | golden snapshot tests | dev-dep |
| `rexpect` or `assert_cmd` | spawn-and-script mock pi for e2e | dev-dep |
| `dirs-next` | fallback paths where `directories` is awkward | no |
| `tokio-stream` | consolidate stream utilities | no |
| `ropey` | rope for composer buffer (optional; `tui-textarea` may suffice) | maybe |

Optional / stretch:
- `base64` + `image` crate for clipboard-image → webp re-encode before sending to pi
- `reqwest` + `serde_yaml` for MCP server discovery / config (if we go that direction)
- `cpal` (audio) — **NO**, out of scope

Total additional binary size target: ≤ 6 MiB release-stripped after these adds (V1 is ~2.5 MiB).

---

## 4. Feature surface

### 4.1 Live state signals (the #1 user-visible win)

Every moment the user looks at the screen they should know, at a glance:
- Is pi idle, thinking, calling a tool, waiting on the LLM, retrying, or compacting?
- Which tool and for how long?
- What's the throughput / cost / context burn rate right now?
- Is my terminal healthy? (resize storms, paste size, key-repeat)
- What's queued (steer / follow-up) and what's the mode?

Delivered by **StatusWidget** (§8) + header gauges + per-turn breadcrumbs.

### 4.2 Conversation view (§9)

- Message cards with rounded borders, avatars, relative timestamps.
- Per-message focus + keyboard nav (`j/k` or `↑/↓` in focus mode).
- Thinking as a collapsible indented quote *or* a live tree of thoughts.
- Tool calls as cards with border-color state + streaming output + diff toggle.
- Syntect code blocks for markdown and tool output.
- Unified or side-by-side diff with `d` / `D` toggle.
- Inline image rendering (feature-gated).
- Virtualized scroll — 100k messages remain snappy.
- Sticky "you are here" separator when scrolled off live tail.

### 4.3 Pi RPC coverage (complete)

Finishing the last corners:
- `abort_bash` wired to a dedicated UI path (currently `abort` only).
- `cycle_model` / `cycle_thinking_level` bound to quick keys.
- `get_last_assistant_text` bound to `/copy` (copies last assistant into clipboard).
- Explicit dialog-timeout countdown ring (deferred from M4).
- `set_steering_mode` / `set_follow_up_mode` pickers.

### 4.4 Claude Code / Codex-inspired features

- **Plan mode** — intercept tool calls, preview, approve (needs a pi extension or an allowlist overlay here).
- **File tree sidebar** with `.gitignore` respect.
- **`@file` autocomplete** in composer with live preview pane.
- **Fuzzy file finder** (`Ctrl+P`).
- **Live grep** (`Ctrl+G`) with Ripgrep.
- **Git** — branch indicator (already in header via session name? — no: new), dirty chip, `/commit` modal, `/diff` modal, `/log` modal.
- **TODO widget** (Claude Code's todo.md parity, but from extensions `setWidget` or a local `/todo` store).
- **Diagnostics panel** — lint/compile errors if a language-server is reachable (stretch).
- **`init`-style agent** intro ceremonies — when you run `rata-pi` in a fresh repo with no pi session, offer to run `/init` which seeds a session-level note.

### 4.5 Session superpowers

- **Timeline view** — every session.jsonl is a branch; render as a git-like graph with fork points.
- **Compare** — select two sessions; side-by-side.
- **Replay** — `--replay <session.jsonl>`: step through with `n/p` (works without live pi).
- **Record** — optional `vhs` recipe bundled for demo gif generation.
- **Snapshot/restore** — freeze transcript to disk anytime (`/save <name>`).

### 4.6 Input polish

- **Multi-line composer** via `tui-textarea`: Shift+Enter newline, word motions, undo, history search inline.
- **Vim mode** (`/vim`) — normal/insert with `h/j/k/l`, `w/b`, `d`, `y`, `p`.
- **Emacs mode** (`/emacs`) — Ctrl-heavy bindings.
- **Paste detection** — auto-wrap pasted code in a fenced block when it looks like code.
- **Image paste** — detect PNG bytes in clipboard and attach as `ImageContent`.
- **Templates** — `/templates` opens a modal of user-defined prompt templates.

### 4.7 Output polish

- **OSC 52 clipboard** — copy works over SSH.
- **Drag-select** on transcript with mouse → OS clipboard.
- **Notifications** — desktop notify on `agent_end` (opt-in).
- **Bell** on error (opt-in).
- **Title bar** — `rata-pi · <session-name> · <branch>`.

### 4.8 Theming

- Six built-ins: `tokyo-night` (default), `dracula`, `solarized-dark`, `solarized-light`, `catppuccin-mocha`, `gruvbox-dark`, `nord`.
- TOML theme loader from `~/.config/rata-pi/themes/*.toml`.
- `notify` watcher → hot-reload on file save.
- `Ctrl+Shift+T` cycles; `/theme <name>` picks.

### 4.9 Hooks & notifications

- Event hooks: `on_agent_end`, `on_tool_error`, `on_compaction`, `on_retry_exhausted`.
- Hook target: shell command (non-blocking) OR desktop notification.
- Config file `~/.config/rata-pi/hooks.toml`.

### 4.10 MCP & extensions

- Show MCP server status if pi reports it (enhance `get_state` data or piggyback on `get_commands` badges).
- `/mcp` modal — list servers, tools per server, health.
- Extension command filter — `/commands` modal gains per-source tabs (extension / prompt / skill / built-in).

---

## 5. Milestones V2.0 → V2.12

Each milestone ends with: build green, clippy clean, tests pass, a short vhs gif committed under `/docs/demos/`, and an update to `plan_avancement_v2.md`.

### V2.0 — Foundation refactor + animation engine + theme system
- Module split per §2.
- `Action` reducer replaces `on_event`.
- `anim/` module with a 60 fps tick source that only wakes when something is animating. Zero CPU when idle.
- `theme/` with six built-in themes, `Ctrl+Shift+T` cycle, `/theme <name>`.
- Everything in V1 keeps working — this is a green-field refactor that ships identical UX but on new bones.
- **Acceptance:** `cargo test` ≥ V1 count; visual diff of V1 vs V2.0 is indistinguishable to the user.

### V2.1 — StatusWidget + header gauges
- Full StatusWidget per §8 — dominant bottom-left card showing the live operation tree.
- Header gauges: context %, throughput (tok/s), cost delta per turn.
- Sparkline widget for throughput + cost (60 s rolling window).
- Smooth animated Gauge replacement (tweened fill + pulse when >85%).
- **Acceptance:** every Pi event type produces a visible status update. Tested by firing the event fixture.

### V2.2 — Conversation view overhaul
- Message cards with rounded corners + role avatars.
- Per-message focus with `j/k`/`↑↓` navigation in a new "focus mode" (`Esc` to toggle).
- Relative timestamps + absolute tooltip on focus.
- Nested tool-call tree rendering (tool calls can themselves spawn sub-turns).
- Sticky "live tail" indicator while scrolled up.
- Virtualized scrolling via `tui-scrollview`.
- **Acceptance:** 10k-message fixture session scrolls at 60 fps.

### V2.3 — Syntect code + inline diff widget
- `syntect` for all fenced code in markdown.
- `ui/widgets/diff.rs` — unified view by default, side-by-side via `d/D`.
- Auto-detect language from first path in diff header or fence lang tag.
- Line numbers optional (`Ctrl+Shift+L`).
- **Acceptance:** every pi tool that returns diffs (`edit`, `apply_patch`, `str_replace_editor`) renders with per-token colors.

### V2.4 — Images + Kitty keyboard + OSC 52 clipboard
- Feature `images`: ratatui-image for Kitty/iTerm/WezTerm/Ghostty, `viuer` fallback, text fallback `[image 245 KB png]`.
- Image paste: clipboard → base64 → attach to outgoing prompt.
- Kitty keyboard protocol enable/disable.
- OSC 52 clipboard path for SSH.
- Drag-select on transcript → copy.
- **Acceptance:** "take a screenshot, paste it, ask about it" works end-to-end.

### V2.5 — Scrollable slash menu + command catalog
- Rewrite `/` menu as `CommandMenu` widget:
  - virtualized scroll list
  - grouped by category (Session / Model / Git / View / Tools / Help)
  - command history (most-recently-used bubbles to top)
  - per-item key-hint badges
  - help pane on the right showing the selected command's description and args
- Add ~20 new commands (see §6).
- **Acceptance:** typing `/` + filtering 200 items still feels responsive.

### V2.6 — File tree + fuzzy finder + grep + @file
- Sidebar file tree (`F2` toggles; cached walk with `ignore`).
- `Ctrl+P` fuzzy file finder (uses `fuzzy-matcher`).
- `Ctrl+G` live grep modal (ripgrep style).
- Composer `@` autocomplete — shows a floating picker with live preview.
- **Acceptance:** opening a 50k-file repo doesn't stall; fuzzy finds ≤ 100 ms.

### V2.7 — Git integration
- Header `git` chip with branch + dirty dot.
- `/diff` (unstaged), `/diff --staged`, `/log`, `/commit <msg>`, `/branch`, `/switch-branch <name>`, `/stash`.
- Uses `git2` when `git` feature is on; shells out otherwise.
- Inline unified diff view reuses V2.3 widget.
- **Acceptance:** common git flows don't need to leave the TUI.

### V2.8 — Plan mode + TODO + diagnostics
- `/plan` toggles plan-mode: intercept every `tool_execution_start`, show preview, `Y/N` prompt, then let it proceed.
- TODO widget — a right-side persistent widget; keyboard CRUD; `/todo add ...`.
- Diagnostics panel (optional LSP bridge via `lspower` or shell-out to `cargo check` for Rust projects) — stretch.
- **Acceptance:** plan mode can block an actual tool call before it runs.

### V2.9 — Session timeline + replay + snapshots
- `/timeline` opens a branch-graph of every session under pi's session dir, with fork edges.
- `--replay <session.jsonl>` CLI mode — steps with `n/p/N/P`.
- `/save <name>` snapshots current transcript to `snapshots/<name>.md` and `.jsonl`.
- `/load <name>` loads a snapshot into the current UI as a read-only overlay.
- **Acceptance:** recording a demo + replaying it matches frame-for-frame.

### V2.10 — Input: multi-line, vim/emacs, templates
- `tui-textarea` composer with undo/redo, word motions.
- `/vim`, `/emacs`, `/default` modes; stored in settings.
- `/templates` — modal of user-defined prompt templates from `~/.config/rata-pi/templates/*.md`.
- **Acceptance:** multi-line prompts work; vim mode passes a small golden test of `iHello<Esc>:w`.

### V2.11 — Hooks, notifications, telemetry opt-in
- `~/.config/rata-pi/hooks.toml` → events trigger shell commands or desktop notify.
- `notify-rust` for completion notifications.
- OSC 777 emit for terminal-supported native notifications.
- Optional crash dump on panic to `~/.local/state/rata-pi/crash-<ts>.log`.
- **Acceptance:** hook fires within 100 ms of the triggering event.

### V2.12 — Release polish
- `docs/` with full manual, keybinding reference, theme gallery.
- `vhs` recipes for every major feature as gifs.
- Shell completions (bash/zsh/fish) via clap.
- Man page.
- `cargo-dist` release pipeline (GitHub Actions) producing tarballs for mac/linux/windows.
- Insta golden-snapshot coverage for all widgets at 80×24 and 140×40.
- End-to-end tests against a scripted mock pi binary (rexpect / assert_cmd).
- **Acceptance:** a new user follows README.md, gets a working TUI in < 60 seconds.

---

## 6. Slash command catalog

Grouped so the scrollable `/` menu can show category dividers.

**Session**
- `/help` `/stats` `/env` `/log` `/save [name]` `/load <name>` `/export` `/export-html`
- `/rename <name>` `/new` `/switch <path>` `/fork` `/timeline` `/snapshots` `/replay <file>`

**Model / thinking**
- `/model [name]` (picker if arg omitted) `/models` (list) `/cycle-model`
- `/think <level>` `/cycle-think`

**Conversation**
- `/clear` (local visual) `/copy` (last assistant → clipboard) `/copy-code` (last fenced block)
- `/retry` (rerun last prompt) `/replay-last` (fork then rerun) `/steer-mode <all|one-at-a-time>` `/follow-up-mode <...>`

**Tools / bash**
- `!cmd` (bash) `/abort-bash` `/abort-retry` `/compact [instr]` `/auto-compact <on|off>` `/auto-retry <on|off>`

**Files / git**
- `/files` (tree) `/find <q>` `/grep <q>` `/diff [--staged]` `/log` `/commit <msg>` `/branch` `/switch-branch <n>` `/stash`

**UI / view**
- `/theme [name]` `/themes` `/layout <name>` `/panel <name>` (toggle) `/vim` `/emacs` `/default`
- `/plan` (toggle) `/todo [add|done|rm] ...` `/notes`

**Extensions / MCP**
- `/mcp` (list servers) `/commands` (picker — same as F1)
- `/templates` (pick template) `/aliases` (list) `/alias <name>=<expansion>`

**Debug / dev**
- `/trace` (open debug-rpc log viewer) `/version` `/log-level <trace|debug|info|warn|error>`
- `/doctor` (checks PATH for pi, clipboard support, image protocol, etc.)

Target: **40+ commands** by end of V2.5. Every one gets a docstring + argument hints shown in the right pane of the scrollable menu.

---

## 7. Keybinding map

Defaults (customizable via `~/.config/rata-pi/keys.toml`):

```
# Global
Enter            submit               Shift+Enter  newline
Esc              abort/clear/quit     Ctrl+C/D     quit
Ctrl+T           toggle thinking      Ctrl+E       expand last tool
Ctrl+Space       cycle steer/followup Ctrl+Shift+T cycle theme
Ctrl+P           fuzzy files          Ctrl+G       live grep
Ctrl+R           history search       Ctrl+S       export md
Ctrl+L           clear screen         Ctrl+K       focus mode toggle

# Modals
F1 commands  F2 files  F3 sessions  F4 forks  F5 model  F6 thinking
F7 stats     F8 compact now  F9 auto-compact  F10 auto-retry
F11 layouts  F12 mcp          ?   help

# Focus mode (j/k on transcript, like gmail)
j/k          next/prev message    Enter  focus into (expand)
d/D          toggle diff view     c      copy message
g/G          top/bottom           /      slash menu with focus context

# Editor
↑/↓          history walk         Ctrl+A/E  line start/end
Alt+←/→      word motion          Ctrl+W    delete word back
Ctrl+U       kill line back       Ctrl+K    kill line fwd
Ctrl+Z/Y     undo/redo
```

---

## 8. State-signal model

The `StatusWidget` is the single source of truth for "what is pi doing right now". It's a small card (min 24×4, grows to 40×10 when something interesting is happening) always visible at bottom-left above the footer.

Possible states (exactly one is active):

| State | Trigger | Visual |
|---|---|---|
| `Idle` | default | muted `·` dot, no spinner |
| `Sending` | we just wrote a prompt | amber arrow ▸ |
| `LLM` | `agent_start` without a tool yet | cyan spinner + model name + elapsed |
| `Thinking` | `thinking_start`…`thinking_end` | magenta spinner + "thinking" + tokens |
| `Tool` | between `tool_execution_start` and `_end` | yellow spinner + tool name + elapsed + partial tokens if known |
| `Streaming` | after any tool, streaming text | blue spinner + "speaking" + tok/s live |
| `Compacting` | `compaction_start` | teal spinner + "compacting" + tokens before |
| `Retrying` | `auto_retry_start` | orange countdown ring + attempt N/M |
| `Blocked` | plan-mode user prompt pending | purple pause icon |
| `Error` | rpc remote error | red ✗ + message |

The widget additionally shows sub-lines when present:
- Turn N · input/output tokens so far
- Tools: `k running · j done · e failed`
- Cost this turn + session total

Animation:
- Spinner ticks at 12 Hz (braille).
- Retry countdown ring ticks every 100 ms, depleting a circular unicode arc (8 frames for an 800 ms second).
- State transitions fade the border color over 4 frames (150 ms).

Data source: the reducer updates a `live: LiveState` struct on every RPC event. `StatusWidget` is a pure function of `live` + animation tick.

Additional micro-signals:
- Top-right **heartbeat dot** pulses green with each received event, red if no event in 10 s while `is_streaming`.
- Footer **throughput sparkline** (60-bucket rolling) from token deltas.
- Footer **cost sparkline** from `turn_end.message.usage.cost.total`.
- **Connection quality** sparkline: round-trip time between our `get_session_stats` ticker and its response.

---

## 9. Visual language

Semantic palette (theme fills it in):

```
accent         primary brand (header title, spinners, focused borders)
accent-strong  hover/pressed, high-emphasis chips
muted          time stamps, secondary hints
dim            tertiary text, separators
success / warning / error
role-user      green family
role-assistant blue family
role-thinking  magenta family
role-tool      yellow family
role-bash      cyan family
diff-add / diff-remove / diff-hunk / diff-file
border-idle / border-active
bg-card        soft fill on message cards (reversed-8% where supported)
```

Typography conventions (unicode only — no ligatures or custom fonts):
- Bold for actor prefixes + command keys in help.
- Italic for thinking + meta.
- Reversed (block) for cursor + chips + banners.
- Dim for nav hints.
- Crossed-out for strikethrough / deleted items.

Symbols kit (pre-curated; theme doesn't change these):
```
Spinner  ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏
Arrows   ▸▾◂▴ ← →  ↑↓
Chips    ● ○ ◉ ◎ ✓ ✗ ⚠ ℹ ↻ ⟲ ⏀ ⏳ ⏸ ⏵
Brackets ╭╮╯╰ ┌┐└┘ ━│
Dividers ┄ ── ──── ═══
Badges   ⌥ ⎇ ⌘ ⏎ ⇧
```

Layout breakpoints (responsive):

| Width | Layout |
|---|---|
| ≥ 160 | 4-zone: L sidebar (files/todo/history) · transcript · R inspector (tool detail / diff) · status column |
| 120–159 | 3-zone: L sidebar · transcript · R inspector |
| 90–119 | 2-zone: transcript · R inspector (sidebar → F2 drawer) |
| < 90 | 1-zone: transcript only; inspectors are modals |

Height-wise: header(1) + body(flex) + widgets(dyn) + editor(3–8) + status(3–5) + footer(1).

Minimum viable: 60 × 16. Below that we show a "terminal too small" placeholder (unchanged from V1).

---

## 10. Performance budget

- **Render**: 60 fps *only when animation is active or state changed*. Otherwise we park the draw. Budget per frame: 4 ms.
- **Reducer**: pure, typically < 200 µs per Action even for large transcripts (thanks to virtualized scroll).
- **Transcript memory**: capped at 10k messages in-memory; older overflow into `sessions/<id>.jsonl` on disk and stream back when scrolled to.
- **Startup**: cold `cargo run --release` from zero to first draw < 200 ms (excluding pi spawn time, which we can't control).
- **Idle CPU**: 0.0 % with `top` — the ticker stops when nothing is moving and nothing is pending.
- **Animation** contracts: every animation declares a duration + ease; the anim registry stops ticking when the last animation completes.

Benchmarks: we add `cargo bench` tests under `benches/` for (a) streaming 1000 deltas into the transcript, (b) rendering 10k messages, (c) fuzzy-finding across 50k files.

---

## 11. Testing & release

Test tiers:

1. **Unit** — reducers, parsers, codecs, fuzzy-matcher, diff heuristic, theme parser. Target: 300+ tests by V2.12.
2. **Golden snapshots** (`insta`) — every widget at 80×24 and 140×40 for every state (idle, streaming, error, wide, narrow). Target: 60+ snapshots.
3. **Integration** — `tests/` spawns a *mock pi* rust bin that plays a `.jsonl` fixture onto stdout and consumes commands from stdin. Assert UI state after each scripted scene.
4. **Benchmarks** — as above; regression alarm if any bench regresses by > 20 %.

Release (V2.12):
- Tag `v2.0.0` cuts a `cargo-dist` release with macOS (x86_64 + aarch64), Linux (gnu + musl), Windows (msvc).
- Homebrew tap generated by dist.
- `docs/` deployed to a static site (GitHub Pages) with the theme gallery and recorded vhs gifs.
- CHANGELOG.md in keep-a-changelog format.
- Semver: V2.x series.

---

## Appendix A — claude-code / codex feature-parity checklist

| Feature | Parity target | Delivered in |
|---|---|---|
| Slash commands | ≥ 40 built-ins | V2.5 |
| `@file` autocomplete | full | V2.6 |
| File tree | yes | V2.6 |
| Fuzzy find | Ctrl+P | V2.6 |
| Live grep | Ctrl+G | V2.6 |
| TODO widget | yes | V2.8 |
| Plan mode | approve/deny per tool | V2.8 |
| Git chip + quick actions | yes | V2.7 |
| Keybinding customization | toml | V2.10 |
| Templates | md files in config | V2.10 |
| Hooks | shell-cmd + notify | V2.11 |
| Session fork | yes (V1) | shipped |
| Session switch | UI picker | V2.9 (upgrade from V1 text-only) |
| Export md + html | yes (V1) | shipped |
| Image paste/render | feature-gated | V2.4 |
| Subagents | if pi exposes them | backlog |
| Custom tool rendering | typed per tool | V2.1 (bash) → V2.3 (diff) → V2.8 (todo) |
| Theme system | six built-ins + TOML + hot-reload | V2.0 |
| Multi-line editor | yes | V2.10 |
| Vim mode | yes | V2.10 |
| MCP visibility | list + status | V2.11 / stretch |
| Remote session | SSH-friendly (OSC 52, Kitty) | V2.4 |
| Recording / replay | yes | V2.9 |

---

## Appendix B — risks & mitigations

| Risk | Mitigation |
|---|---|
| syntect startup cost | load themes lazily + cache compiled syntaxes |
| ratatui 0.30 vs dep ecosystem | pin to 0.30; fork any laggard minor-bump deps if needed |
| image protocol detection false positives | probe via `TERM_PROGRAM` + env; fall back quietly |
| mouse drag-select hit-testing complexity | use tui-scrollview's built-in selection hooks if available |
| `git2` C dep on musl/Windows | feature-flag + shell-out fallback |
| 2.6k-line app.rs churn during refactor | do V2.0 first; lock behavior via snapshot tests before any feature work |
| pi spec drift | keep domain types `#[serde(default)]`; add contract test that feeds a canonical fixture |

---

## Appendix C — non-negotiables

- **Never writes to stdout/stderr directly** — tracing is file-only, always.
- **Every modal is dismissible with Esc.** No UI cul-de-sac.
- **No .unwrap() outside tests/main.**
- **Every `-D warnings` lint stays clean** through every PR.
- **Every milestone ends with a runnable demo gif.**

---

Tracking lives in `plan_avancement_v2.md`. Every milestone commits on completion with the checklist updated.

Let's build it.
