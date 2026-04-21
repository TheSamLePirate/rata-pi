# rata-pi — User Guide

A Ratatui terminal UI for the [Pi coding agent](https://www.npmjs.com/package/@mariozechner/pi-coding-agent) (`@mariozechner/pi-coding-agent`). rata-pi spawns `pi --mode rpc` as a child process, speaks the JSONL RPC protocol over stdin/stdout, and renders the conversation, tool calls, plan state, and session stats as a full-screen TUI.

This guide documents **every feature**: command-line arguments, key bindings, slash commands, modals, themes, plan mode, notifications, file persistence, terminal capabilities, and troubleshooting.

---

## Table of contents

1. [Installation and launch](#installation-and-launch)
2. [Command-line arguments](#command-line-arguments)
3. [First-run sanity check](#first-run-sanity-check)
4. [The screen layout](#the-screen-layout)
5. [Composer](#composer)
6. [Submit, steering, and follow-up](#submit-steering-and-follow-up)
7. [Focus mode and scroll](#focus-mode-and-scroll)
8. [Slash commands (full reference)](#slash-commands-full-reference)
9. [Keyboard shortcuts (full reference)](#keyboard-shortcuts-full-reference)
10. [Mouse](#mouse)
11. [Modals](#modals)
12. [Plan mode](#plan-mode)
13. [Interview mode](#interview-mode)
14. [File finder and `@path`](#file-finder-and-path)
14. [Git integration](#git-integration)
15. [Themes](#themes)
16. [Vim mode](#vim-mode)
17. [Notifications](#notifications)
18. [Clipboard](#clipboard)
19. [Transcript export](#transcript-export)
20. [Status widget and header](#status-widget-and-header)
21. [Files rata-pi reads and writes](#files-rata-pi-reads-and-writes)
22. [Terminal capabilities](#terminal-capabilities)
23. [Crash reports](#crash-reports)
24. [Troubleshooting](#troubleshooting)

---

## Installation and launch

### Prerequisites
* `pi` (the `@mariozechner/pi-coding-agent` CLI) installed and on `PATH`. Install via `npm i -g @mariozechner/pi-coding-agent` or equivalent.
* A recent stable Rust toolchain (edition 2024 compatible). For source builds only.
* A terminal with at least 256 colors and mouse support. Most modern terminals qualify; see [Terminal capabilities](#terminal-capabilities).

### From source
```sh
cargo build --release
./target/release/rata-pi
```

Binary lives at `target/release/rata-pi`. Enable native desktop notifications at compile time with:
```sh
cargo build --release --features notify
```

### What happens on launch
1. rata-pi installs a panic hook that restores the terminal on crash and writes a dump to disk.
2. It probes terminal capabilities (Kitty keyboard protocol, graphics support).
3. It enters the alternate screen, enables raw mode, mouse capture, and bracketed paste.
4. It spawns `pi --mode rpc` as a child (pipes on stdin/stdout/stderr).
5. A one-time bootstrap fires **five RPC calls concurrently**: `get_state`, `get_messages`, `get_commands`, `get_available_models`, `get_session_stats`.
6. The UI starts ticking at 10 Hz with a 30 fps redraw cap.

If `pi` cannot be spawned (binary not found, permission error), rata-pi runs in **offline mode**: the transcript shows the error, local-only slash commands (themes, exports, `/help`, `/doctor`, git, etc.) still work, and pi-requiring commands flash a "needs pi (offline)" message.

---

## Command-line arguments

All arguments are optional.

| Flag | Default | Description |
|---|---|---|
| `--pi-bin <path>` | `pi` | Path to the pi binary. Use this to test against a local build. |
| `--provider <name>` | — | LLM provider passed through to pi: `anthropic`, `openai`, `google`, etc. |
| `--model <id>` | — | Model pattern or ID passed through to pi. |
| `--session-dir <dir>` | — | Custom session storage directory for pi. |
| `--no-session` | `false` | Disable pi session persistence. |
| `--debug-rpc` | `false` | Log every RPC line (both directions) to the log file. Verbose. |
| `--log-level <lvl>` | — | Override log level: `trace`, `debug`, `info`, `warn`, `error`. Also honors `RUST_LOG`. |
| `--help` | — | Print usage and exit. |
| `--version` | — | Print the rata-pi version and exit. |

### Examples
```sh
rata-pi
rata-pi --model claude-sonnet-4-6 --provider anthropic
rata-pi --pi-bin ~/src/pi/bin/pi --debug-rpc --log-level debug
rata-pi --no-session
```

All flags not recognized by rata-pi are **not** forwarded to pi; only the explicit ones above shape the pi argv.

---

## First-run sanity check

After launch, open the readiness modal with `/doctor`. You'll see a check list:

* `pi binary` — `PASS` with resolved path, or `FAIL` if not on `PATH`.
* `pi connection` — `PASS` once bootstrap completes; `FAIL` with the spawn error if pi crashed.
* `terminal` — info row showing the detected terminal kind.
* `kitty keyboard` — `PASS` if the Kitty keyboard protocol is advertised (enables `Ctrl+Shift+T` and precise modifiers), `WARN` otherwise.
* `graphics` — `PASS` if an image protocol is detected, `INFO` otherwise (no images today).
* `clipboard` — `PASS` (native arboard) or `WARN` (OSC 52 fallback for SSH / tmux).
* `git` — `PASS` with branch name in a repo, `INFO` if launched outside a repo.
* `theme` — info row with the active theme name.
* `notifications` — `PASS` when enabled, and whether the `notify` feature is compiled in.

Hit `Esc` to close.

---

## The screen layout

From top to bottom:

```
┌─ header ────────────────────────────────────────────────┐
│ rata-pi · model · thinking chip · state chip · git chip │
├─ transcript ────────────────────────────────────────────┤
│                                                         │
│  ❯ user card      ← rounded-border card with role icon │
│  ✦ thinking card  ← italic muted, blockquote style     │
│  ✦ pi · streaming ← assistant card, live cursor at tail │
│  ⚙ tool · running ← bash / edit / read / grep / write  │
│  · info row       ← inline meta rows (no border)       │
│                                                ⬇ live   │ ← sticky tail chip
├─ widgets strip (optional, ext-UI) ──────────────────────┤
├─ plan card (optional, when /plan is active) ────────────┤
├─ editor ────────────────────────────────────────────────┤
│ > │compose here — Shift+Enter for newline              │
├─ widgets strip (optional, below the editor) ────────────┤
├─ status widget ─────────────────────────────────────────┤
│ ⠋ status · MM:SS · turn N · K running · N done         │
│   throughput ▂▄█▆ · cost ▂▃▅ · session total           │
├─ footer ────────────────────────────────────────────────┤
│ context gauge: 42% · 52k / 200k tok · $0.14            │
│ Enter send · / cmds · Ctrl+F focus · ? help            │
└─────────────────────────────────────────────────────────┘
```

* **Header** — one row. Shows model label, thinking level, live state, heartbeat dot, turn counter, queue sizes, and the git chip (branch · dirty dot · ahead/behind arrows).
* **Transcript** — virtualized scroll. Only visible cards are rendered; a per-entry cache keeps markdown/syntect work out of the draw hot path.
* **Widgets strip** — space for `set_widget` requests emitted by pi extensions (above or below the editor depending on placement).
* **Plan card** — appears when a plan is active; collapses when cleared.
* **Editor** — the composer. Grows from 1 row to 8 rows as you type.
* **Status widget** — 4 rows. Hidden when terminal height < 20.
* **Footer** — two rows: context gauge + keybinding hints. Hints adapt to the current state (streaming / focus / idle).

Toasts from pi extensions appear in the top-right corner.

---

## Composer

rata-pi's composer is a first-class multi-line editor with UTF-8-aware cursor movement.

### Basics
* `Enter` submits. `Shift+Enter` or `Ctrl+J` inserts a newline.
* `Esc` while the composer has text clears it. When empty: aborts an in-flight stream (if streaming) or quits the app.
* `Ctrl+C` / `Ctrl+D` always quit immediately.
* The editor grows with your content (min 1 row, max 8 rows — it scrolls internally above that).

### Cursor motion
| Key | Action |
|---|---|
| `←` / `→` | Character left/right |
| `Home` / `Ctrl+A` | Start of line |
| `End` | End of line (when composer has text); otherwise re-pins live tail |
| `Alt+←` / `Alt+→` | Word left/right |
| `↑` / `↓` | Walk prompt history (oldest → newest). See [History](#prompt-history). |

### Editing
| Key | Action |
|---|---|
| `Backspace` | Delete char before cursor (joins lines at start) |
| `Delete` | Delete char under cursor |
| `Ctrl+U` | Kill to start of line |
| `Ctrl+K` | Kill to end of line |
| `Ctrl+W` | Kill word back |
| Paste | Bracketed paste works; newlines are turned into spaces (use Shift+Enter for intentional newlines) |

### Prompt history
Every submitted prompt is appended to `history.jsonl` (see [Files](#files-rata-pi-reads-and-writes)). Walk history with `↑` / `↓` in the composer, restore the in-progress draft by walking past the newest entry. The last 5 000 entries load on startup; older entries stay on disk but aren't walkable.

`Ctrl+R` opens a filterable history picker modal (type to filter; Enter restores).

### `@path` autocomplete
Type `@` in the composer and the file finder modal opens immediately, filter-seeded with whatever token you typed after `@`. Enter replaces the `@<partial>` with `@<full/path>`. See [File finder](#file-finder-and-path).

---

## Submit, steering, and follow-up

### Submit
Press `Enter` with pi connected → sends the prompt. The composer clears, the submitted text becomes a `you` card in the transcript, and the live state flips to `sending` → `llm`.

### Streaming controls
While the agent is running:
* `Esc` aborts the current run (`abort` RPC) — you'll see an `aborted` warn entry.
* `Ctrl+Space` cycles the composer's **intent**: `Prompt` → `Steer` → `FollowUp` → `Steer`. The editor title reflects the current mode.
  * **Steer** — injects a message into the running turn. Delivery mode is controlled by `/steer-mode`.
  * **Follow-up** — queues a follow-up to fire after the current turn ends.

The header shows live queue sizes as chips: `steer: N` and `follow-up: N`.

### Retry
`/retry` re-submits the last user prompt. If the agent is currently streaming, it uses `Steer` to inject the prompt into the live turn; otherwise it submits fresh.

---

## Focus mode and scroll

### Live tail (default)
With no modal open and no focus selection, the transcript auto-scrolls to the bottom as new content arrives.

### Manual scroll
* `PgUp` / `PgDn` scroll the transcript by ~5 rows.
* Mouse wheel scrolls the transcript (4 rows per step).
* `End` on an empty composer re-pins the live tail and hides the "⬇ live tail (End)" sticky chip at the bottom.

### Focus mode
`Ctrl+F` enters focus mode: the most recent card becomes "focused" and gets a double-border + `▶` marker. Then:

| Key | Action |
|---|---|
| `j` / `↓` | Next card |
| `k` / `↑` | Previous card |
| `g` / `Home` | First card |
| `G` / `End` | Last card |
| `PgUp` / `PgDn` | Move ±5 cards |
| `Enter` / `Space` | Toggle expand on a tool card |
| `y` / `c` / `Ctrl+Y` | Copy the focused card's plain text to the clipboard |
| `Esc` / `q` | Exit focus mode |
| `Ctrl+C` | Quit |

The transcript auto-centers on the focused card.

### Tool card expand / collapse
Tool cards collapse their output body to "N lines — Enter / Ctrl+E to view" when long. `Ctrl+E` (outside focus mode) toggles the *last* tool card's expansion. Inside focus mode, `Enter` / `Space` toggles the focused tool card.

---

## Slash commands (full reference)

Type `/` on an empty composer to open the Commands picker (or `F1`). The list merges rata-pi built-ins with pi's own extension / prompt / skill commands. Every built-in has an arg hint, an example, and a category. Filter by typing; select with `↑` / `↓`; apply with `Enter`.

Enter semantics:
* **No-arg built-in** — runs immediately (e.g. `/help` opens help; `/theme` cycles).
* **Arg-taking built-in** — prefills the composer with `/name ` so you can finish typing the argument.
* **Pi command** — prefills the composer too.

### Session
* `/help` — open the help modal with keybindings + commands.
* `/stats` — open the session stats modal.
* `/rename <name>` — set the session's display name.
* `/new` — start a fresh pi session (keeps rata-pi running).
* `/switch <path>` — load a session from a JSONL path.
* `/fork` — open the fork picker (resume from an earlier user turn).
* `/snapshots` — placeholder for saved-transcript snapshots (feature deferred).

### Transcript / export
* `/export` — write the transcript to a local markdown file. Path is echoed via flash.
* `/export-html` — ask pi to export the session to HTML.
* `/copy` — copy the last assistant message to the clipboard.
* `/clear` — clear the transcript view only (pi state intact).
* `/retry` — re-submit the last user prompt (uses steer if streaming).

### Model / thinking
* `/model` — open the model picker (F5).
* `/think` — open the thinking-level picker (F6). Levels: off / minimal / low / medium / high / xhigh.
* `/cycle-model` — advance to the next configured model.
* `/cycle-think` — advance to the next thinking level.

### Pi runtime toggles / controls
* `/compact [instructions]` — compact context now. Optional custom instructions.
* `/auto-compact` — toggle auto-compaction (F9).
* `/auto-retry` — toggle auto-retry on transient errors (F10).
* `/abort` — abort the current streaming run (Esc).
* `/abort-bash` — abort the currently running bash command.
* `/abort-retry` — cancel an in-progress auto-retry.
* `/steer-mode <all | one-at-a-time>` — set steering delivery mode.
* `/follow-up-mode <all | one-at-a-time>` — set follow-up delivery mode.

### View / UI
* `/theme [name]` — cycle if no arg; apply by name otherwise (case-insensitive).
* `/themes` — open the theme picker. Enter applies.
* `/find [query]` — fuzzy file finder. Pre-seeds the filter.
* `/vim` — enable vim normal/insert composer mode.
* `/default` (alias `/emacs`) — disable vim mode (Esc clears composer again).
* `/plan [subcommand]` — open the full plan view (no arg) or manage steps. Subcommands:
  * `/plan set a | b | c` — set a new plan (items separated by `|`).
  * `/plan add <text>` — append an item.
  * `/plan done` — mark the active step done; advance.
  * `/plan fail <reason>` — mark the active step failed; halt.
  * `/plan next` — skip the active step (marks it done without executing).
  * `/plan clear` — clear the plan entirely.
  * `/plan auto on|off` — toggle auto-continue follow-ups.
  * `/plan show` — same as no-arg.

### Git (all require a working `git` binary on `PATH`)
* `/status` — open the git-status summary.
* `/diff [--staged]` — full-screen diff viewer (unstaged by default).
* `/git-log [n]` — last N commits (default 30).
* `/branch` — list + switch branches.
* `/switch-branch <name>` — checkout a branch directly.
* `/commit <message>` — `git commit -am <message>`.
* `/stash` — `git stash push`.

### Debug
* `/doctor` — readiness modal (pi, terminal, clipboard, git, theme, notifications).
* `/mcp` — list MCP servers exposed by pi (currently shows a placeholder; pi doesn't expose MCP state over RPC yet).
* `/notify` — toggle desktop notifications. Emits a test notification and flashes which backends fired.
* `/version` — echo the rata-pi version.
* `/log` — echo a hint about the log directory.
* `/env` — echo relevant env vars: `TERM`, `TERM_PROGRAM`, detected terminal kind, kitty-kb flag, graphics flag.

### Pi commands
Extensions, prompts, and skills registered by pi appear in the Commands picker under their own categories. Selecting one runs or prefills per pi's convention. Available pi commands vary per session — open the Commands picker (`F1` or `/`) to browse.

---

## Keyboard shortcuts (full reference)

### Global (always active)
| Key | Action |
|---|---|
| `Ctrl+C` | Quit |
| `Ctrl+D` | Quit |

### Editor (when no modal is open and not in focus mode)
| Key | Action |
|---|---|
| `Enter` | Submit |
| `Shift+Enter` / `Ctrl+J` | Insert newline |
| `Esc` | (If streaming) abort. (Else if composer has text) clear. (Else) quit. |
| `Ctrl+F` | Enter focus mode |
| `Ctrl+T` | Toggle thinking visibility |
| `Alt+T` / `Ctrl+Shift+T` / `F12` | Cycle theme |
| `Ctrl+E` | Toggle expand on the last tool card |
| `Ctrl+P` | Open file finder |
| `Ctrl+Y` | Copy the last assistant message |
| `Ctrl+Space` | Cycle composer intent (streaming only: Prompt/Steer/FollowUp) |
| `Ctrl+R` | Open prompt-history picker |
| `Ctrl+S` | Export the transcript to markdown |
| `F1` | Commands modal |
| `F5` | Model picker |
| `F6` | Thinking-level picker |
| `F7` | Stats modal |
| `F8` | Compact context now |
| `F9` | Toggle auto-compaction |
| `F10` | Toggle auto-retry |
| `?` | Help modal |
| `/` | Commands modal (on empty composer) |
| `↑` / `↓` | Prompt history (prev / next) |
| `PgUp` / `PgDn` | Scroll transcript ±5 rows |
| `End` (empty composer) | Re-pin live tail |

### Composer editing
| Key | Action |
|---|---|
| `←` / `→` | Cursor left / right |
| `Alt+←` / `Alt+→` | Word left / right |
| `Home` / `Ctrl+A` | Start of line |
| `End` | End of line (composer non-empty) |
| `Backspace` | Delete char before cursor |
| `Delete` | Delete char under cursor |
| `Ctrl+U` | Kill to start of line |
| `Ctrl+K` | Kill to end of line |
| `Ctrl+W` | Kill word back |

### Focus mode
| Key | Action |
|---|---|
| `j` / `↓` | Next card |
| `k` / `↑` | Previous card |
| `g` / `Home` | First card |
| `G` / `End` | Last card |
| `PgUp` / `PgDn` | Move ±5 cards |
| `Enter` / `Space` | Toggle expand on a tool card |
| `y` / `c` / `Ctrl+Y` | Copy focused card to clipboard |
| `Esc` / `q` | Exit focus mode |

### Modal (any modal)
| Key | Action |
|---|---|
| `↑` / `↓` | Move selection |
| `PgUp` / `PgDn` | Page ±5 |
| `Home` / `End` | First / last |
| `Enter` | Apply selection |
| `Esc` | Close modal |
| (printable key) | Append to filter query |
| `Backspace` | Delete from filter query |

Modals with viewer bodies (Diff, GitLog) also accept `j/k/g/G` and scroll pages with `PgUp/PgDn`.

### Vim mode (opt-in via `/vim`)
While in Normal mode, the composer intercepts these keys instead of inserting:

| Key | Action |
|---|---|
| `h` / `←` | Left |
| `j` / `↓` | Down one row |
| `k` / `↑` | Up one row |
| `l` / `→` | Right |
| `w` | Word right |
| `b` | Word left |
| `0` / `Home` | Start of line |
| `$` / `End` | End of line |
| `i` | Insert mode (at cursor) |
| `a` | Insert mode (after cursor) |
| `A` | Insert mode at end of line |
| `I` | Insert mode at start of line |
| `o` | New line below, Insert |
| `O` | New line above, Insert |
| `x` | Delete char under cursor |
| `dd` | Delete current line |
| `gg` | Top of composer |
| `G` | Bottom of composer |

`Esc` in Insert mode → Normal. `/default` or `/emacs` disables vim mode.

---

## Mouse

rata-pi captures mouse events. Bracketed paste is also enabled.

| Gesture | Action |
|---|---|
| Wheel up / down | Scroll transcript |
| Left click on a card | Focus that card |
| Double left click on a tool card | Toggle expand |
| Left click on the `⬇ live tail` chip | Re-pin live tail |

Mouse-drag selection is not wired to rata-pi's rendering. Terminals often allow holding `Shift` (or equivalent) to bypass the app and use the terminal's native selection.

---

## Modals

Modals overlay the transcript and steal keyboard focus. Only one can be open at a time.

### `Help` (`?` or `/help`)
Cheat sheet of keybindings and commands. `Esc` closes.

### `Stats` (`F7` or `/stats`)
Session statistics: user/assistant message counts, tool calls, token breakdown (input / output / cache read / cache write / total), total cost, context window usage.

### `Commands` (`F1` or `/` on empty composer)
Two-pane picker (single-column on narrow terminals). Left: categorized command list with arg hints. Right: detail pane showing the description, source (builtin / ext / prompt / skill), argument hint, example, and an "Enter" hint.

### `Models` (`F5`)
Pick a model from those pi reports via `get_available_models`. Shows provider, context window, and whether the model is tagged `reasoning`.

### `Thinking` (`F6`)
Radio list: `off / minimal / low / medium / high / xhigh`.

### `History` (`Ctrl+R`)
Prompt history picker. Type to filter; Enter restores.

### `Forks` (`/fork`)
Picker over pi's fork-eligible prior turns. Enter forks from the chosen turn.

### `Files` (`Ctrl+P` or `/find`)
Fuzzy file finder. See [File finder](#file-finder-and-path) for details.

### `Diff` (`/diff [--staged]`)
Full-screen diff viewer with gutter showing old/new line numbers, +/- markers, and syntect-highlighted context.

### `GitStatus` (`/status`)
Branch + dirty dot + ahead/behind counts + staged/unstaged/untracked counts.

### `GitLog` (`/git-log [n]`)
Scrollable commit list (hash · author · date · subject).

### `GitBranch` (`/branch`)
Filterable branch picker. Enter switches.

### `PlanView` (`/plan`)
Full plan list with progress markers.

### `Doctor` (`/doctor`)
Readiness checks (see [First-run sanity check](#first-run-sanity-check)).

### `Mcp` (`/mcp`)
Placeholder showing "pi does not expose MCP server state over RPC yet" until pi adds that capability.

### `Interview` (agent-initiated)
Agent-authored structured form. Opens when the agent emits `[[INTERVIEW: …json…]]` in assistant text. See [Interview mode](#interview-mode) for the full field-type reference, keyboard shortcuts, and JSON schema.

### Extension UI dialogs
When a pi extension calls `ext_ui_select`, `ext_ui_confirm`, `ext_ui_input`, or `ext_ui_editor`, a dialog modal opens. `↑↓` (select), `Y/N/←→` (confirm), type-to-fill (input/editor). Enter submits, Esc cancels.

---

## Plan mode

Plan mode turns rata-pi into a step-tracking harness for multi-step tasks. The agent itself can author a plan via markers in its assistant text, OR you can hand-author one.

### Status glyphs
* `[ ]` pending
* `[→]` active
* `[x]` done
* `[✗]` failed

### Commands
| Command | Effect |
|---|---|
| `/plan` or `/plan show` | Open the PlanView modal |
| `/plan set a | b | c` | Replace plan with three steps |
| `/plan add <text>` | Append one step |
| `/plan done` | Mark the active step done and advance |
| `/plan fail <reason>` | Mark the active step failed and halt auto-run |
| `/plan next` | Skip the active step (marks it done) |
| `/plan clear` | Clear the plan |
| `/plan auto on\|off` | Toggle auto-continue follow-ups |

### Agent-authored markers
When the agent emits any of these in its assistant text, rata-pi parses them on `agent_end`:

| Marker | Effect |
|---|---|
| `[[PLAN_SET: a \| b \| c]]` | Replace the plan with those items |
| `[[PLAN_ADD: text]]` | Append one item |
| `[[STEP_DONE]]` | Mark the active step done; advance |
| `[[STEP_FAILED: reason]]` | Mark the active step failed; halt |

These markers are **hidden** from the visible transcript rendering (they still pass through pi).

### Prompt injection
While a plan is active, every outgoing prompt is wrapped with the plan state as context so the agent sees the full list and which step is active. When no plan is active, a short capability hint is appended to prompts so the agent knows the marker grammar exists.

### Auto-continue
With `/plan auto on`, every `agent_end` with pending steps auto-queues a "continue with the next step" follow-up. The loop halts when:
* A step is marked failed (agent or user).
* All steps are done.
* The per-step attempt counter hits `MAX_ATTEMPTS = 3`.

You can always interrupt: `Esc` aborts; `/plan fail <reason>` halts.

---

## Interview mode

Interview mode is the agent's way of asking you **structured** questions instead of a wall of text. When the agent needs multiple related answers — names, toggles, choices, numbers — it emits a single marker describing a form; rata-pi parses it and opens a full-screen modal with labeled fields, sections, required-field markers, defaults, and keyboard-first controls. You fill it in, press `Ctrl+Enter`, and rata-pi sends your answers back as a single JSON payload so the agent can parse it deterministically.

### How it starts

The agent can emit the form in any of three shapes — rata-pi's detector tries them in order:

1. **Canonical marker** (preferred, renders cleanest):
   ```
   [[INTERVIEW: { "title": "...", "fields": [ ... ] }]]
   ```
2. **Fenced code block** (commonly used by chat-trained agents):
   ```
   ```json
   { "title": "...", "fields": [ ... ] }
   ```
   ```
   Any language tag works (`json`, `interview`, none).
3. **Bare JSON object** in the message body. Must deserialize as a valid Interview and carry at least one interactive field — accidental JSON snippets are rejected.

On detection rata-pi:
* **Strips** the form JSON (including the surrounding markers / fences) from the visible assistant card — you're not left staring at raw JSON.
* **Opens** the Interview modal with defaults hydrated.
* **Pushes** an `Info` row into the transcript: `✍ agent opened an interview: "Title" — answer and Ctrl+Enter to submit (Esc cancels)`.
* **Flashes** `interview · Title` in the status bar.

The capability hint describing all three shapes is automatically appended to your outgoing prompts (when no plan is active), so the agent knows the feature exists without you prompting for it.

**Validation** (prevents accidental triggering on unrelated JSON):
* `title` must be a non-empty string.
* `fields` must be a non-empty array.
* At least one field must have a recognized interactive `type` (not just `section` / `info`).

### Field types

| Type | Render | How to edit |
|---|---|---|
| `section` | `── Title ──────────` + optional description | non-interactive (grouping only) |
| `info` | `ℹ note text` | non-interactive |
| `text` | `> │value_` (with placeholder when empty) | type to edit, Alt+←/→ word motion, Ctrl+A/E/U/K/W kill bindings |
| `text` + `multiline: true` | multi-row box | same as text; `Shift+Enter` inserts a newline |
| `toggle` | `[x] yes` / `[ ] no` | `Space` / `Enter` toggles; `←` / `→` set false/true explicitly |
| `select` | `(●) A   ( ) B   ( ) C` | `←` / `→` cycle; `1..9` numeric shortcut |
| `multiselect` (alias `checkboxes`) | `[x] A   [ ] B   [x] C` — the cursor highlights one option | `←` / `→` move cursor between options; `Space` / `Enter` toggle the current option |
| `number` | `> │123_` | only digits / sign / decimal / `e` are accepted; auto-clamped to `min` / `max` if set |

### Required fields

Any `text`, `select`, or `number` can declare `"required": true`. Required fields show a red `*` after the label; `Ctrl+Enter` on a form with missing required answers refuses to submit and jumps focus to the first missing field plus a red validation row at the top of the modal. The submit button also flips from the accent color to a dim gray while validation fails.

### Defaults and hydration

Every field supports a `default`:

* `text` / `number` — seeded string (number gets formatted as `5173`, not `5173.0`, for whole integers).
* `toggle` — `true` / `false` (defaults to false when omitted).
* `select` — a string that must be one of the `options`; if unmatched, falls back to the first option.
* `multiselect` — an array of pre-checked option names.

### Keyboard reference (Interview modal)

| Key | Action |
|---|---|
| `Tab` | Next interactive field (skips sections) |
| `Shift+Tab` | Previous interactive field |
| `↓` / `↑` | Same as Tab / Shift+Tab (unless in a multiline text field where ↓/↑ still move focus — one field per field) |
| `←` / `→` | Field-specific: cycle select, move multiselect cursor, set toggle, move text cursor |
| `Alt+←` / `Alt+→` | Word-left / word-right inside a text or number field |
| `Home` / `Ctrl+A` | Start of the current text field |
| `End` / `Ctrl+E` | End of the current text field |
| `Space` | Toggle a checkbox / boolean / multiselect option |
| `1..9` | On a select, jump to option *N* |
| `Backspace` / `Delete` | Delete char before / after cursor |
| `Ctrl+U` | Kill to start of the current text field |
| `Ctrl+K` | Kill to end of the current text field |
| `Ctrl+W` | Kill word back |
| `Shift+Enter` | Insert newline (in a `multiline` text field only) |
| `Ctrl+Enter` or `Ctrl+S` | Submit — send answers to the agent |
| `Esc` | Cancel — close the modal without sending |

### What pi receives on submit

A normal user message is dispatched with a structured block inside:

```
<interview-response>
{
  "title": "Project setup",
  "answers": {
    "name": "my-app",
    "framework": "Vue",
    "typescript": true,
    "features": ["router", "testing"],
    "port": 5173,
    "notes": ""
  }
}
</interview-response>
```

* Values are typed (booleans, numbers, arrays, strings) — not stringified.
* `section` and `info` fields produce no keys.
* Empty optional fields still emit the key with an empty value so the agent doesn't have to guess.
* A summary line is also pushed into the transcript as a user card: `(interview) interview submitted · name=my-app · typescript=yes · features=[router,testing]`.

If you submitted while the agent was still streaming, rata-pi uses `steer` or `follow_up` depending on the composer's current intent (the same rule as regular submits). Otherwise it's a fresh `prompt`.

### Full example the agent can emit

```json
{
  "title": "Project setup",
  "description": "Let's scaffold a new app.",
  "submitLabel": "Create",
  "fields": [
    { "type": "section", "title": "Basics" },
    { "type": "text", "id": "name", "label": "Project name",
      "placeholder": "my-app", "required": true },
    { "type": "select", "id": "framework", "label": "Framework",
      "options": ["React", "Vue", "Svelte", "None"], "default": "Vue" },

    { "type": "section", "title": "Options" },
    { "type": "toggle", "id": "typescript", "label": "Use TypeScript?", "default": true },
    { "type": "multiselect", "id": "features", "label": "Include features",
      "options": ["router", "store", "testing", "i18n"],
      "default": ["router", "testing"] },
    { "type": "number", "id": "port", "label": "Dev-server port",
      "min": 1024, "max": 65535, "default": 5173 },

    { "type": "section", "title": "Extra" },
    { "type": "info", "text": "We'll add more scaffolding later." },
    { "type": "text", "id": "notes", "label": "Additional notes", "multiline": true }
  ]
}
```

### Tips for agents

* Prefer **one interview** over several free-form questions when the questions are related — a single form round-trip is much cheaper for the user than five messages.
* Use `section` generously; grouping makes long forms scannable.
* Set `default`s whenever a sensible choice exists — a user who just presses `Ctrl+Enter` on a mostly-filled form is a happy user.
* Never emit more than one `[[INTERVIEW]]` per agent turn; only the first is parsed.
* The markers are stripped from the visible transcript, but the `<interview-response>` block you receive IS a normal user message — don't try to "hide" it from the conversation history.

### Tips for users

* You can always back out with `Esc` — nothing is sent.
* If you see a red validation row, jump straight to the red `*` field (focus already moved there).
* For long forms, the modal scrolls naturally with the content; `Tab` / `Shift+Tab` moves the cursor and the viewport follows.
* `Ctrl+S` submits too (same as `Ctrl+Enter`) — handy on terminals that intercept Ctrl+Enter.

---

## File finder and `@path`

### Opening
* `Ctrl+P` — opens the finder, composer unchanged. Selecting a path replaces the composer with `@<path>`.
* `@` in the composer — opens the finder in **AtToken** mode. Selecting a path replaces the `@<partial>` token at cursor with `@<full/path>`.
* `/find [query]` — opens with the filter pre-seeded.

### Walk
The walk respects `.gitignore`, `.git/info/exclude`, and `.git/info/exclude`-style rules via the `ignore` crate. Hidden files (dotfiles) are included unless gitignored. The walk is capped at **20 000 files**; an overflow is flagged in the header hint.

### Performance
The walk runs on a blocking thread when the modal opens — the UI shows `indexing files…` until it finishes, then the filter + preview go live. Typing in the filter box never blocks on I/O: the fuzzy match runs once per keystroke against the in-memory list, and the selected-file preview is cached by path.

### Preview pane
When the terminal is wide enough for a two-pane layout, the right pane shows the first ~40 lines / 8 KiB of the selected file, syntect-highlighted by extension. Binary files and files larger than 50 MB show `(preview unavailable)`.

---

## Git integration

Hidden when launched outside a repo.

### Header chip
Shows: `⎇ <branch> ●/○ ↑N ↓N`. Filled dot = dirty, hollow dot = clean. Ahead/behind pips appear only when non-zero. Updated every 5 seconds on a background task (doesn't block the UI).

### Commands
See the [Git section of slash commands](#git-all-require-a-working-git-binary-on-path). All shell out to the `git` binary — no libgit2 dependency.

### Diff viewer
Two-column gutter (old_line / new_line). `+` / `-` / `space` chip. `@@` hunk headers in a distinct color. Context lines syntect-highlighted when the `+++ b/…` header lets us derive a file extension.

---

## Themes

Built-in themes:

1. **tokyo-night** (default)
2. **dracula**
3. **solarized-dark**
4. **catppuccin-mocha**
5. **gruvbox-dark**
6. **nord**

### Switching
* `Alt+T`, `Ctrl+Shift+T` (on Kitty keyboard terminals), or `F12` cycles through the six themes.
* `/theme <name>` applies by name (case-insensitive): `/theme dracula`, `/theme nord`, etc.
* `/theme` with no arg cycles.
* `/themes` opens a picker.

### Coverage
Each theme drives 21 semantic color slots: accent / accent_strong / muted / dim / text / success / warning / error, role colors for user / assistant / thinking / tool / bash, border colors (idle / active / modal), diff colors (add / remove / hunk / file), and context-gauge gradient colors (low / mid / high). Everything in the UI pulls from these — including the keybinding chips, the focus border, the diff viewer, the gauge, the modal borders, and the toast stack.

### Persistence
Theme choice is **not** persisted across launches today (resets to `tokyo-night`). A TOML theme loader with hot-reload is a planned follow-up.

---

## Vim mode

Default is "emacs-style" — arrow keys + kill-line-style shortcuts. `/vim` opts in to modal editing.

### Turning it on
`/vim` — the editor border gains an ` · INS` / ` · NORM` suffix showing the current mode. `Esc` in Insert leaves Insert for Normal; `i` / `a` / `o` / `I` / `A` / `O` in Normal re-enter Insert.

### Turning it off
`/default` or `/emacs` — mode chip disappears, `Esc` goes back to "clear composer / quit" semantics.

### Coverage
hjkl / wb / 0$ / x / dd / gg / G. See the [Vim mode keybindings](#vim-mode-opt-in-via-vim). No `p`/`y`/`/`/`.` yet; undo/redo isn't implemented.

---

## Notifications

### Default (OSC 777)
Every desktop-notification call emits an OSC 777 `notify` escape sequence. Supported terminals pick these up and render a native notification: iTerm2, kitty, WezTerm, Ghostty, gnome-terminal, konsole. Terminals that don't support it silently ignore the sequence.

### Native (optional)
Building with `--features notify` adds the `notify-rust` crate and tries a native backend (DBus on Linux, NSUserNotification on macOS, WinToast on Windows) in addition to OSC 777. `/doctor` reports whether the feature is compiled in.

### When notifications fire
* **`agent_end`** — only for turns that took ≥ 10 s. Body shows duration and tool-call count: `"15s · 2 tool calls"`.
* **`tool_execution_end` with error** — body shows the first line of the error output.
* **`auto_retry_end` with failure** — body shows the final error.

### Toggle
`/notify` flips the `notify_enabled` flag at runtime. It also fires a test notification and flashes which backends answered.

---

## Clipboard

`arboard` (native clipboard) is tried first. Falls back to OSC 52 when arboard is unavailable (SSH sessions, tmux without `set-clipboard`, containers). `/doctor` reports which backend is active.

### Copy
* `Ctrl+Y` (outside focus mode) — copies the last assistant message.
* `y` / `c` / `Ctrl+Y` (inside focus mode) — copies the focused card's plain-text rendering (user prompts, thinking, assistant text, tool-call args + output, bash command + stdout).
* `/copy` — same as `Ctrl+Y`.

Flash feedback: `✓ copied 312 chars` or `✓ copied 312 chars (osc52)`.

---

## Transcript export

* `Ctrl+S` or `/export` — writes the current transcript to a timestamped markdown file in the platform's data directory (`~/Library/Application Support/rata-pi/exports/` on macOS, `~/.local/share/rata-pi/exports/` on Linux, `%LOCALAPPDATA%\rata-pi\exports\` on Windows). Flash shows the resulting path.
* `/export-html` — asks pi to export the session to HTML (pi's export format — not rata-pi's).

Exports include turn dividers, user / thinking / assistant sections, tool call details, and bash command output.

---

## Status widget and header

### Header (1 row)
Left → right:
1. `rata-pi` wordmark + heartbeat dot (green when recent event; yellow at 10 s silent during streaming; red at 100 ticks).
2. Model label (`anthropic/claude-sonnet-4`).
3. Thinking-level chip (`think: medium`).
4. Session name if set via `/rename`.
5. Connection state (`connected` / `offline`).
6. Queue chips — `steer: N`, `follow-up: N` when non-zero.
7. Git chip — `⎇ branch ●/○ ↑N ↓N` when in a repo.

### Status widget (4 rows, between editor and footer)
Hidden when terminal height < 20. Shows:
* Row 1 — spinner + state label (`llm / thinking / tool / streaming / compacting / retry N/M in Xms / error / idle`) + turn chip + running-tools chip.
* Row 2 — throughput sparkline + t/s, cost sparkline + per-turn cost, session total.
* Rows 3-4 — reserved for future use.

Border color follows live state (idle / error / warning for retry / accent during streaming / accent_strong during compacting).

### Footer (2 rows)
* Row 1 — context gauge + label: `42% · 52k / 200k tok · $0.14`.
* Row 2 — contextual keybinding hints (idle / streaming / focus).

---

## Files rata-pi reads and writes

| Path (Linux shown; other OSes use platform-equivalent dirs) | Purpose |
|---|---|
| `~/.local/share/rata-pi/history.jsonl` | Prompt history, append-only |
| `~/.local/share/rata-pi/exports/transcript-<ts>.md` | Markdown exports |
| `~/.local/state/rata-pi/crash-<unix_ts>.log` | Crash dumps (XDG state dir on Linux; `data_local_dir/rata-pi/` elsewhere) |
| `<log dir>/rata-pi.log` | Trace log — path logged to stderr at startup |

rata-pi does not write to the pi session files directly — pi owns those. rata-pi speaks only through the RPC protocol.

Platform-specific data directories are resolved via the `directories` crate:
* **macOS** — `~/Library/Application Support/rata-pi/`
* **Linux** — `~/.local/share/rata-pi/` (data) + `~/.local/state/rata-pi/` (state)
* **Windows** — `%LOCALAPPDATA%\rata-pi\`

---

## Terminal capabilities

rata-pi probes terminal capabilities at launch and logs them. `/doctor` reports them visibly.

### Kitty keyboard protocol
Enabled when the terminal advertises Kitty keyboard support: Kitty, Ghostty, WezTerm. Lets `Ctrl+Shift+T` (and other multi-modifier combos) arrive with full modifiers. Popped on exit and in the panic hook.

On terminals *without* Kitty keyboard, `Alt+T` and `F12` are the theme-cycle fallbacks.

### Image graphics protocols
Probed but not consumed today — image rendering is a planned follow-up.

### Clipboard
`arboard` is tried first (native macOS NSPasteboard / Windows clipboard / X11 / Wayland). Falls back to OSC 52 for SSH / tmux / headless boxes.

### Mouse
Mouse capture is enabled on launch. Some terminals require holding `Shift` (or equivalent) to bypass the app and use terminal-native text selection.

### Bracketed paste
Enabled on launch. Multi-line pastes arrive as a single event; newlines are converted to spaces to avoid accidental submits. Use `Shift+Enter` for intentional newlines.

### Alternate screen
rata-pi uses the alternate screen — when it quits, your terminal returns to the state it had before launch. Crashes also restore the terminal via the panic hook.

---

## Crash reports

On panic, rata-pi:

1. Disables Kitty keyboard enhancements, bracketed paste, mouse capture, raw mode.
2. Leaves the alternate screen.
3. Writes a dump to the platform state dir:
   * Linux — `~/.local/state/rata-pi/crash-<unix_ts>.log`
   * macOS — `~/Library/Application Support/rata-pi/crash-<unix_ts>.log`
   * Windows — `%LOCALAPPDATA%\rata-pi\crash-<unix_ts>.log`
4. Prints `rata-pi: crash dump written to <path>` to stderr.
5. Chains to the original `color-eyre` panic hook.

The dump contains: rata-pi version, OS, architecture, panic location, payload string (if any), and a forced backtrace.

---

## Troubleshooting

### `pi: command not found`
Install pi: `npm i -g @mariozechner/pi-coding-agent` (or wherever you install npm globals). Then re-launch rata-pi. Or point rata-pi at a custom path: `rata-pi --pi-bin /usr/local/bin/pi`.

### `Ctrl+Shift+T` doesn't cycle theme
Your terminal isn't advertising Kitty keyboard protocol. Use `Alt+T` or `F12` instead. On macOS Terminal.app, Alt-keys need "Use Option as Meta key" enabled in Terminal → Preferences → Profiles → Keyboard.

### Clipboard doesn't work
Check `/doctor`: if it reports `OSC 52 fallback`, your terminal needs to opt in:
* **tmux** — `set -g set-clipboard on` in `~/.tmux.conf`.
* **SSH + iTerm2** — enable "Applications in terminal may access clipboard" under Preferences → General → Selection.
* **screen** — OSC 52 is unsupported. Use rata-pi inside tmux or directly without a multiplexer.

### Transcript is slow on a long session
Verify with `/doctor` that you're on a release build (`cargo run --release`). V2.11.2+ includes a per-entry visuals cache — if you see CPU pegged on `markdown::render` in a profiler, file a bug.

### Headers clobbered / garbled output
Usually means the previous instance didn't clean up. Reset the terminal: `stty sane && clear`, or close the tab and reopen.

### Long bash output freezes the UI
Ticks are capped at 30 fps and events are drained in bursts of up to 64 per frame. If you still see freezes, try `--debug-rpc` + `--log-level debug` and compare the log timestamps.

### Notifications don't appear
Check `/doctor`: if `notifications` is `on` but you see nothing, your terminal may not support OSC 777. Build with `--features notify` for native desktop notifications. Disable the leak of escape codes in CI: run rata-pi with `/notify` off.

### Something went wrong — how do I report it?
1. Check `/log` for the log file path. Re-run with `--log-level debug --debug-rpc`.
2. Reproduce the issue. Look at the new log tail.
3. If the app crashed, check the crash dump directory (`/doctor` or the "crash dump written to" stderr line).
4. Open an issue with the log excerpt and the crash dump.

---

## Quick cheat sheet

```
quit                 Ctrl+C or Ctrl+D
submit               Enter
newline              Shift+Enter or Ctrl+J
abort stream         Esc (while streaming)
clear composer       Esc (when not streaming, composer has text)

commands             F1 or / (on empty composer)
model picker         F5
thinking picker      F6
stats                F7
compact              F8
auto-compact         F9
auto-retry           F10

help                 ?
theme cycle          Alt+T / Ctrl+Shift+T / F12
focus mode           Ctrl+F
file finder          Ctrl+P
copy last assistant  Ctrl+Y
history picker       Ctrl+R
export markdown      Ctrl+S
thinking visibility  Ctrl+T
expand last tool     Ctrl+E
composer intent      Ctrl+Space (while streaming)
scroll transcript    PgUp / PgDn / mouse wheel
re-pin live tail     End (empty composer) / click the chip

git status           /status
git diff             /diff [--staged]
git log              /git-log [n]
git branches         /branch

plan view            /plan
plan set             /plan set a | b | c
plan done            /plan done
plan auto-run        /plan auto on|off

doctor               /doctor
notifications        /notify
```
