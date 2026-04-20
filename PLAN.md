# rata-pi — Full Implementation Plan

A pro-grade Ratatui TUI client for the Pi coding agent, communicating over its JSONL RPC protocol on stdin/stdout. The goal is feature-parity with Pi's own TUI plus a first-class, mouse-friendly, resizable, animated Rust interface.

Reference sources (local paths, via `pi-doc.md`):
- RPC spec: `~/.nvm/versions/node/v24.14.0/lib/node_modules/@mariozechner/pi-coding-agent/docs/rpc.md`
- Example TUI client: `.../examples/rpc-extension-ui.ts`
- Pi TUI component doc: `.../docs/tui.md`
- Pi settings / themes / models / session / skills / compaction / prompt templates: `.../docs/*.md`
- Ratatui examples: `/Users/olivierveinand/Documents/DEV/rataPi/ratatui/examples/apps`

---

## 1. Vision

- **Headless agent, native UI.** Spawn `pi --mode rpc` as a child process; all UX lives in Rust/Ratatui.
- **Every RPC feature exposed.** Nothing hidden, nothing degraded — including the extension UI sub-protocol (select/confirm/input/editor/notify/setStatus/setWidget/setTitle/set_editor_text).
- **Pro-grade feel.** Smooth streaming, real scrollback, mouse drag-select, resizable panels, animated status, modals with focus trapping, theme switching, syntax highlighting, image rendering where the terminal supports it.
- **Robust under concurrency.** Typed state machine; never blocks the UI; survives partial JSONL, backpressure, agent crashes, resize storms, paste bombs.

---

## 2. Tech stack

Crate additions to `Cargo.toml`:

| Crate | Purpose |
|-------|---------|
| `ratatui` (already) | rendering |
| `crossterm` (already) | terminal backend, mouse, bracketed paste, Kitty keyboard |
| `color-eyre` (already) | error reports |
| `tokio` (rt-multi-thread, macros, process, io-util, sync, time, signal) | async core |
| `tokio-util` (codec) | newline-delimited framing |
| `serde`, `serde_json` | RPC types |
| `serde_repr` | stringly-typed enums |
| `thiserror` | typed errors |
| `tracing`, `tracing-subscriber`, `tracing-appender` | logging to file (NEVER stdout/stderr — they're the RPC pipe and the TTY) |
| `tui-textarea` (or hand-rolled) | multi-line editor with paste, history, word motions |
| `tui-scrollview` or custom | virtual scrollback |
| `syntect` (with `default-themes` feature) | syntax highlighting for code blocks, diffs, tool output |
| `pulldown-cmark` | markdown parsing → styled Ratatui `Text` |
| `ansi-to-tui` | render ANSI-escape output (bash tool) as styled `Text` |
| `unicode-width`, `unicode-segmentation` | cell-accurate wrapping |
| `arboard` | clipboard integration (copy-out) |
| `clap` (derive) | CLI flags |
| `directories` | XDG paths for cache/logs/config |
| `notify` | optional: watch pi settings files |
| `viuer` or `ratatui-image` (feature-gated) | inline image rendering for Kitty/iTerm/WezTerm/Ghostty |
| `base64` | image payloads |
| `humansize`, `humantime` | formatting |
| `once_cell` / `arc-swap` | theme / config hot reload |

Optional (Phase 3+): `tui-big-text`, `throbber-widgets-tui`, `tui-tree-widget`.

Rust edition `2024` (already). MSRV: Rust 1.83+.

---

## 3. Process & concurrency model

```
                    ┌──────────────── input events ────────────────┐
                    ▼                                              │
 ┌─────────┐   mpsc (UiEvent)                                      │
 │crossterm├─────────────────┐                                     │
 │ reader  │                 ▼                                     │
 └─────────┘          ┌──────────────┐                             │
                      │  App (Tokio  │   mpsc (UiEvent)            │
 ┌─────────┐          │  select! loop│◀────────────────── timers ──┘
 │ child   │          │  = state +   │
 │  pi     │          │  reducer)    │
 │ stdout  │──┐       └──────┬───────┘
 │ stderr  │  │              │ draw via Ratatui `Terminal<T>`
 │ stdin   │◀─┼────── cmd ───┤
 └─────────┘  │              ▼
              │        ┌──────────┐   crossterm output (mouse,
              │        │  frame   │   bracketed paste, alt-screen,
              │        │  render  │   Kitty keyboard flag set)
              │        └──────────┘
              └── JSONL framed stream → RpcEvent enum → mpsc (UiEvent::Rpc)
```

- **One Tokio runtime** on the main thread; UI draw happens in a `tokio::task::spawn_blocking` or on the main task via `Terminal::draw` between events — Ratatui is sync, so we keep draws off the async select.
- **`select!` over four sources:** RPC events, key/mouse/resize events, timers (spinner ticks, animation frames), and internal commands (user issued RPC writes enqueue via the same channel for auditing).
- **Strict JSONL framing.** Use `tokio_util::codec::LinesCodec` configured with max-length guard; strip `\r`; never use any reader that splits on U+2028/U+2029 — the RPC doc explicitly warns about this.
- **Backpressure.** Bounded mpsc (e.g. 1024). If RPC floods faster than we can redraw, coalesce `message_update` deltas (keep only the latest partial assistant message per id; keep the accumulated text we've appended).
- **Writer task** owns `ChildStdin`; all commands go through it so we never interleave partial writes. Each write is `serde_json::to_vec` + `\n` in one `write_all`.
- **Graceful shutdown.** On `Ctrl+C`/quit: send `abort`, drain events until `agent_end` (or 500 ms), then `kill()` + `wait()`. Close alt-screen and restore terminal even on panic (`color_eyre` panic hook + `ratatui::crossterm::execute!` cleanup guard).

---

## 4. RPC coverage matrix

Every command and event in `rpc.md` is implemented. Outgoing commands modeled as `enum RpcCommand`, each a serde-tagged struct with an auto-generated `id`. Responses correlate via a `HashMap<RequestId, oneshot::Sender<RpcResponse>>` so UI code `await`s typed results.

### Commands (stdin)

- **Prompting:** `prompt` (with images & `streamingBehavior`), `steer`, `follow_up`, `abort`, `new_session`.
- **State:** `get_state`, `get_messages`.
- **Model:** `set_model`, `cycle_model`, `get_available_models`.
- **Thinking:** `set_thinking_level`, `cycle_thinking_level`.
- **Queue modes:** `set_steering_mode`, `set_follow_up_mode`.
- **Compaction:** `compact` (with custom instructions), `set_auto_compaction`.
- **Retry:** `set_auto_retry`, `abort_retry`.
- **Bash:** `bash`, `abort_bash`.
- **Session:** `get_session_stats`, `export_html`, `switch_session`, `fork`, `get_fork_messages`, `get_last_assistant_text`, `set_session_name`.
- **Commands catalog:** `get_commands`.
- **Extension UI responses:** `extension_ui_response` (value / confirmed / cancelled variants).

### Events (stdout)

- Lifecycle: `agent_start`, `agent_end`, `turn_start`, `turn_end`.
- Messages: `message_start`, `message_update` (with 12 delta sub-types: `start`, `text_start/delta/end`, `thinking_start/delta/end`, `toolcall_start/delta/end`, `done`, `error`), `message_end`.
- Tools: `tool_execution_start`, `tool_execution_update` (accumulated `partialResult`), `tool_execution_end`.
- Queue: `queue_update`.
- Compaction: `compaction_start`, `compaction_end` (reason `manual|threshold|overflow`, willRetry, errorMessage).
- Retry: `auto_retry_start`, `auto_retry_end`.
- Extension: `extension_error`.
- Extension UI requests: `extension_ui_request` with methods `select|confirm|input|editor|notify|setStatus|setWidget|setTitle|set_editor_text`.

### Types to port

Straight serde port of: `Model`, `UserMessage`, `AssistantMessage` (with `text` / `thinking` / `toolCall` content blocks), `ToolResultMessage`, `BashExecutionMessage`, `Attachment`, `ImageContent`, `TextContent`, `Usage`, `Cost`, `ThinkingLevel`, `SteeringMode`, `FollowUpMode`, `StopReason`, `CompactionResult`, `SessionStats`, `CommandInfo` (source enum extension|prompt|skill, location enum user|project|path), `State`.

All enums use `#[serde(rename_all = "snake_case")]` where the protocol uses snake, and string-literal enums where the protocol uses them literally (e.g. stop reasons).

---

## 5. Crate / module layout

Single-crate binary (split crates later if RPC client is ever reused):

```
src/
├── main.rs                 # boot: parse args, tracing, spawn pi, run App
├── app.rs                  # top-level App, event loop, reducer
├── cli.rs                  # clap definitions (--pi-bin, --provider, --model, --session-dir, --no-session, --theme, --log-level, etc.)
├── rpc/
│   ├── mod.rs
│   ├── process.rs          # spawn, stdio wiring, graceful shutdown
│   ├── codec.rs            # JSONL framing (LF only, CR-trim, max line guard)
│   ├── client.rs           # request/response correlation, writer task
│   ├── commands.rs         # RpcCommand enum + constructors
│   ├── events.rs           # RpcEvent enum + deserializer
│   └── types.rs            # shared domain types
├── state/
│   ├── mod.rs              # SessionState, Transcript, StreamingBuffers
│   ├── transcript.rs       # append-only message log, indexed by entryId
│   ├── streaming.rs        # per-turn assistant buffer (text/thinking/toolcalls)
│   ├── tools.rs            # ToolExecution state (running, partial, result)
│   └── queue.rs            # steering / follow-up pending
├── ui/
│   ├── mod.rs              # draw(frame, app)
│   ├── theme.rs            # semantic color palette, dark/light/custom loader
│   ├── layout.rs           # responsive layout calculator (breakpoints)
│   ├── widgets/
│   │   ├── header.rs       # title bar: session name, model, branch, tokens
│   │   ├── footer.rs       # status line, cost, context %, spinner, mode badges
│   │   ├── transcript.rs   # scrollable message list (virtualized)
│   │   ├── message.rs      # render User/Assistant/ToolResult/Bash
│   │   ├── thinking.rs     # collapsible thinking block
│   │   ├── toolcall.rs     # collapsible tool call + streaming output
│   │   ├── markdown.rs     # pulldown-cmark → ratatui::text::Text
│   │   ├── code_block.rs   # syntect-highlighted fenced code
│   │   ├── diff.rs         # unified diff renderer (for edit/patch tools)
│   │   ├── editor.rs       # input composer (multi-line, history, slash autocomplete, paste, images)
│   │   ├── mode_bar.rs     # thinking level, steering/follow-up mode chips
│   │   ├── queue_panel.rs  # pending steering/follow-up messages
│   │   ├── commands_panel.rs # `/` autocomplete dropdown (extensions/prompts/skills)
│   │   ├── stats_panel.rs  # tokens/cost/context gauge
│   │   ├── session_list.rs # switch_session picker
│   │   ├── fork_picker.rs  # fork entry list
│   │   ├── model_picker.rs # provider/model chooser
│   │   ├── help.rs         # keybindings cheat sheet
│   │   ├── modal.rs        # generic modal / dialog frame
│   │   ├── toast.rs        # ephemeral notifications
│   │   ├── spinner.rs      # animated working indicator
│   │   ├── gauge.rs        # context-window bar
│   │   └── image.rs        # ratatui-image wrapper (feature-gated)
│   └── syntax.rs           # syntect loader + cache
├── input/
│   ├── mod.rs              # KeyMap, keybinding config
│   ├── bindings.rs         # named actions → keys
│   ├── clipboard.rs        # paste normalization, image paste detection
│   └── mouse.rs            # hit-testing, scroll, drag selection
├── ext_ui/
│   ├── mod.rs              # extension_ui_request router
│   ├── dialogs.rs          # select / confirm / input / editor components
│   └── sinks.rs            # notify → toast; setStatus/setWidget/setTitle state
├── config.rs               # XDG config + keybindings.toml + theme loader
├── history.rs              # prompt history persistence
├── log.rs                  # tracing-appender file sink (no stdout writes)
└── util/
    ├── wrap.rs             # Unicode-aware wrap + truncate
    ├── fuzzy.rs            # command/setting fuzzy matcher
    └── time.rs             # millis → "2s ago"
```

Tests live under `src/**/tests.rs` and `tests/` (integration — spawn a mock pi that speaks JSONL from a scripted fixture).

---

## 6. UI design

### 6.1 Responsive layout (breakpoints by cols)

| Width | Layout |
|-------|--------|
| ≥ 140 | 3 panes: left sidebar (sessions/queue/stats), transcript, right inspector (tool detail) |
| 100–139 | 2 panes: transcript + right inspector; sidebar becomes a drawer (toggle `F2`) |
| < 100 | Single column; sidebar/inspector become modals |

Height: header (1), transcript (flex), editor (3–10, grows with content to `max_height = h/3`), footer (1), optional mode bar (1).

### 6.2 Visual language

- **Header bar** (Bold on accent bg): `● session-name  ·  model:thinking  ·  branch  ·  turn 3/∞  ·  ⏳ 01:23`.
- **Footer**: `tokens 62k/200k [████████░░░] 31%  $0.45  ·  steering:one-at-a-time  ·  auto-compact ●  ·  retry ●`.
- **Turn separators**: thin rule with timestamp & usage for the just-finished assistant message.
- **User messages**: green accent prefix `You ›`, bg `userMessageBg`, right-aligned timestamp, markdown rendered.
- **Assistant messages**: blue accent prefix `Pi ›`, markdown + code blocks with syntect, tool-call collapsibles.
- **Thinking blocks**: dim italic quote, collapsed by default; toggle with `Ctrl+T` or click.
- **Tool calls**: bracketed card `▾ bash$ ls -la` with:
  - colored border by state (pending grey, running amber, success green, error red),
  - streaming output pane with `tool_execution_update` accumulated content (ANSI-aware via `ansi-to-tui`),
  - final result collapsed to first/last N lines, full-view on expand.
- **Bash (RPC) executions**: distinct `$` prefix with exit code chip and truncation marker; open full output file path if `fullOutputPath` set.
- **Queue chips**: `⟶ steering (2)` / `⤳ follow-up (1)` clickable; opens queue panel.
- **Compaction events**: timeline entry `⟲ compaction (threshold) · 150k → 32k`.
- **Auto-retry**: amber inline row with countdown, cancel button (`Esc` or click).
- **Extension notifications**: toast stack in bottom-right, auto-dismiss (info 3s, warning 6s, error until click).

### 6.3 Editor / composer

- Multi-line tui-textarea-like widget with:
  - history (↑/↓ when empty; Ctrl+R fuzzy history search),
  - bracketed paste (detected via crossterm `PasteStart/End`) with image detection: base64 data URI or local path → attached as `ImageContent`,
  - slash autocomplete: typing `/` opens `commands_panel` populated from `get_commands` (source badge: ext/prompt/skill), fuzzy-filtered,
  - `@path` file reference autocomplete (ripgrep walker),
  - `Shift+Enter` = newline, `Enter` = submit, `Ctrl+J` = force newline even on single-line,
  - `Esc` = (streaming) abort / (idle) clear input,
  - `Ctrl+Space` = toggle steering mode: drafts a `steer` instead of `prompt` during streaming; shows yellow border when in steer mode, purple when `follow_up`.
- Widgets-above/below support for `setWidget` — extensions render persistent lines above or below the editor.
- `set_editor_text` RPC request prefills editor non-destructively (confirmation if user has pending text).

### 6.4 Modals & overlays

Centered, shadowed, focus-trapped; Esc to cancel, Tab/Shift+Tab to cycle, Enter to submit. Never steal scroll from transcript; scroll within modal only.

- **Session switcher** (`F3`): recent sessions with fuzzy search; opens `.pi/sessions/*.jsonl` list.
- **Fork picker** (`F4`): populated from `get_fork_messages`, preview pane on right.
- **Model picker** (`F5`): grouped by provider, shows context window / price / reasoning badge; Enter = `set_model`.
- **Thinking picker** (`F6`): radio list off/minimal/low/medium/high/xhigh with budget tooltip.
- **Settings** (`,`): toggles for auto-compaction, auto-retry, steering mode, follow-up mode, theme.
- **Stats** (`F7`): full `get_session_stats` detail: in/out/cache tokens, cost, context gauge.
- **Commands browser** (`F1`): list + description + source + path; Enter inserts `/name ` in editor.
- **Help** (`?`): keybindings cheat sheet, scrollable.
- **Confirm** / **Input** / **Editor** extension dialogs: route from `extension_ui_request`; support `timeout` with countdown ring; on timeout the agent auto-resolves — we just close the modal.

### 6.5 Theming

- Semantic palette (`theme.fg("accent")`, `theme.bg("userMessageBg")`, etc.) mirroring Pi's theme keys from `tui.md` so porting syntax highlighting themes feels familiar.
- Dark + light built-in; loader for TOML themes from `$XDG_CONFIG_HOME/rata-pi/themes/*.toml`.
- `Ctrl+Shift+T` cycles themes live (invalidate caches, redraw).

### 6.6 Animations

- 10 fps spinner task (Braille frames `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`).
- Fade-in of tool-call borders over 2 frames when `tool_execution_start` arrives (simple color interpolation between muted → accent).
- Toast slide-up: newborn toast renders at offset 1, animates to 0 over 2 frames.
- Token gauge animates smoothly between `contextUsage.percent` updates (tween over 4 frames).
- All animations driven by a single 16 ms Tokio interval that pushes `Tick` events only while something is animating (otherwise idle → zero CPU).

### 6.7 Mouse

- Scrollwheel → transcript virtual-scroll (with auto-follow mode disabled when user scrolls up; a `● Live` button re-enables).
- Click on tool call card → toggle expanded.
- Click on queue chip → open queue panel.
- Click on `● Live` indicator → re-pin to bottom.
- Drag-select across transcript → copy-to-clipboard (arboard); selection survives one redraw.
- Click on modal backdrop → close (where allowed).
- Middle-click paste via crossterm paste events.

### 6.8 Resize

- Respond to `Event::Resize` immediately; re-layout; transcript recalculates its virtualized window (store cell-accurate heights per message keyed by `(entryId, width)` so the cache invalidates only on width change, matching Pi's `cachedWidth` pattern).
- Narrow mode (< 100 cols) collapses sidebars; a banner flashes "Narrow layout".
- Minimum supported size: 40×12; below that, show `Terminal too small — resize to continue` centered.

---

## 7. Feature phasing

### Phase 0 — Foundations (1–2 days)
- Cargo deps, tracing to file, crossterm alt-screen + panic restore guard, tokio runtime, clap CLI.
- Spawn `pi --mode rpc` with args; stdio plumbing; JSONL codec; logging raw RPC to `$XDG_STATE_HOME/rata-pi/rpc.log` behind `--debug-rpc`.
- Hello-world Ratatui frame proving the split works.

### Phase 1 — MVP chat (3–5 days)
- Port all RPC types (commands, events, domain types).
- `RpcClient` with request/response correlation + typed helpers (`client.prompt(msg).await`).
- Single-pane transcript: user + assistant text streaming via `text_delta`.
- Editor with Enter submit, Esc abort, bracketed paste (plain text).
- Spinner driven by `agent_start`/`agent_end`.
- Header (model), footer (working state).
- Graceful shutdown.

### Phase 2 — Full message model (3–4 days)
- Thinking blocks (collapsible), tool calls (streaming args + output via `tool_execution_update`), tool results.
- Markdown rendering + syntect code blocks.
- Diff renderer for edit/patch tools.
- Bash (RPC) command UI: `:bash` editor prefix runs `bash` RPC, streams output, inserts `BashExecutionMessage` rendering.
- Auto-retry inline row; compaction timeline entries.

### Phase 3 — Session & state (3–4 days)
- `get_state` / `get_messages` bootstrap on start → restore transcript.
- Session switcher, fork picker, session name setter, export_html, new_session.
- Stats panel + animated context gauge.
- Commands panel (`/` autocomplete) from `get_commands`.
- Queue panel; `steer` / `follow_up` composition with live mode indicator.

### Phase 4 — Extension UI protocol (2–3 days)
- `extension_ui_request` router → dialog components (select/confirm/input/editor) with timeout countdown.
- `notify` → toast stack; `setStatus` → footer status map keyed by `statusKey`; `setWidget` → above/below editor panel keyed by `widgetKey` with `widgetPlacement`; `setTitle` → OSC 0/2 terminal title write; `set_editor_text` → composer prefill.
- Respect fire-and-forget vs dialog semantics; always reply dialog with matching `id`.

### Phase 5 — Pro polish (4–6 days)
- Mouse: drag-select copy, click-to-expand, wheel scroll.
- Kitty keyboard protocol enabled for precise modifier + key-release detection.
- Image paste + render (feature `images`, via `ratatui-image` + `viuer` fallback; on non-capable terminals fall back to `[image attached: 245 KB png]`).
- Theme hot-reload; light/dark; custom TOML themes.
- Fuzzy everything (`fuzzy-matcher`): command palette (`Ctrl+P`), settings, sessions.
- Undo/redo in editor; word motions; multi-cursor-free selection.
- Prompt history with search (`Ctrl+R`), persisted at `$XDG_DATA_HOME/rata-pi/history.jsonl`.
- Export current transcript to markdown (local, independent of `export_html`).
- Crash telemetry opt-in off by default; `--debug-rpc` dumps raw JSONL for bug reports.

### Phase 6 — Advanced (optional, 3–5 days)
- Split view: two transcripts (e.g. while comparing forks).
- Plan-mode widget parity (render extension `setWidget` special-cased for `plan-mode`).
- Inline syntax-highlighted live diff preview while editor contains a `@path` reference.
- Scripted macros: record a sequence of prompts and replay.
- Optional `--json-log` subcommand for headless CI usage.
- Windows + Termux support parity (crossterm already covers both; test matrix).

---

## 8. Keybindings (defaults; overridable via `$XDG_CONFIG_HOME/rata-pi/keys.toml`)

| Key | Action |
|-----|--------|
| `Enter` / `Ctrl+Enter` | Submit prompt (or steer / follow-up depending on mode) |
| `Shift+Enter` | Newline |
| `Esc` | Abort if streaming, else clear input; double-Esc opens fork picker (config) |
| `Ctrl+C` | Global abort + prompt confirm-quit on second press |
| `Ctrl+D` | Quit |
| `Ctrl+L` | Clear transcript view (not history) |
| `Ctrl+T` | Toggle thinking blocks |
| `Ctrl+Space` | Cycle composer mode: prompt → steer → follow-up |
| `Ctrl+P` | Command palette |
| `Ctrl+R` | Prompt history search |
| `Ctrl+U` / `Ctrl+D` (modal) | Half-page scroll |
| `PgUp/PgDn` | Transcript scroll |
| `Home/End` | Scroll to top/bottom |
| `F1` | Commands browser |
| `F2` | Toggle sidebar |
| `F3` | Session switcher |
| `F4` | Fork picker |
| `F5` | Model picker |
| `F6` | Thinking level picker |
| `F7` | Session stats |
| `F8` | Compact now |
| `F9` | Toggle auto-compaction |
| `F10` | Toggle auto-retry |
| `,` | Settings |
| `?` | Help |
| `Ctrl+Shift+T` | Cycle theme |
| Mouse wheel | Scroll |
| Click tool card | Expand/collapse |

---

## 9. State model sketch

```rust
struct App {
    rpc: RpcClient,
    session: SessionState,      // model, thinkingLevel, steeringMode, followUpMode,
                                // auto_compact, session_file, session_id, session_name, pending counts
    transcript: Transcript,     // Vec<RenderedMessage> indexed by entryId
    streaming: Option<StreamingTurn>,  // live assistant message under construction
    tools: HashMap<ToolCallId, ToolExecution>, // running tool state
    queue: QueueState,          // steering[], followUp[]
    stats: Option<SessionStats>,
    retry: Option<AutoRetryState>,
    compaction: Option<CompactionState>,
    ext_ui: ExtUiState,         // open dialogs, statuses by key, widgets by key
    editor: Composer,
    focus: Focus,               // Editor | Transcript | Modal(ModalId) | Sidebar
    modal: Option<Modal>,       // typed modal stack
    toasts: VecDeque<Toast>,
    layout: LayoutBreakpoint,
    theme: Theme,
    keymap: KeyMap,
}
```

Reducer is a pure function `(App, AppEvent) -> Vec<Command>` where `Command = SendRpc | DrawRequest | StartAnim | ... ` to keep it testable.

---

## 10. Testing

- **Unit:** JSONL codec (LF-only, CR-trim, oversized-line cap, malformed JSON), deserializers against fixtures captured from a real `pi --mode rpc` session, reducer transitions.
- **Golden snapshots:** per-widget render tests (`insta` crate) at 80×24 and 140×40 for each message kind.
- **Integration:** `tests/` spawns a mock pi binary (a tiny Rust bin that reads stdin JSONL and emits scripted events from a `.jsonl` fixture) — verifies streaming coalescing, abort, compaction, extension UI round-trips.
- **Chaos:** a fuzzer writes randomly chunked JSONL to the codec to assert no panics.
- **Manual QA checklist:** keyboard-only, mouse-only, paste bomb (100k chars), 10 MB tool output, resize storm, agent crash mid-stream, extension UI timeout, CJK IME input.

CI (extend `.github/workflows/ci.yml`):
- add `cargo clippy -- -D warnings`, `cargo test --all-features`, `cargo deny check` (advisories + licenses), `cargo msrv verify`.
- Matrix already covers macOS + Windows; add `ubuntu-latest` running headless integration tests against a stub pi.

---

## 11. Code-quality guardrails

- `#![deny(rust_2024_compatibility, unused_must_use)]`, `#![warn(clippy::pedantic, clippy::nursery)]` with curated allow-list.
- No `.unwrap()` / `.expect()` outside `main` / tests; `color_eyre::Result` everywhere else.
- All user-visible strings go through a small `strings.rs` module to ease future i18n.
- No blocking I/O on the async runtime; use `tokio::fs` / `spawn_blocking` for clipboard, syntect load, file exports.
- Tracing spans for every RPC request (`rpc.request.type`, `rpc.request.id`) with latency.
- Logs go to a file only — stdout is the RPC pipe, stderr is the child's pipe; accidentally writing to either corrupts the protocol.

---

## 12. Milestones & rough timeline

| Milestone | Scope | Demo |
|-----------|-------|------|
| **M0** (day 2) | Phase 0 done | Hello-world TUI + pi handshake in logs |
| **M1** (day 7) | Phase 1 | Streaming chat with abort & resize |
| **M2** (day 11) | Phase 2 | Markdown, thinking, tool calls, bash, retry, compaction UI |
| **M3** (day 15) | Phase 3 | Session/fork/model/thinking/stats/commands/queue |
| **M4** (day 18) | Phase 4 | Full extension UI protocol |
| **M5** (day 24) | Phase 5 | Mouse, images, themes, palette, polish |
| **M6** (day 29) | Phase 6 | Splits, macros, cross-platform, v1.0 release |

---

## 13. Open questions to resolve before coding

1. `pi` binary discovery — `$PATH` or configurable `--pi-bin` (default to `pi`); surface a friendly error with install hint if missing.
2. Image-protocol detection — query `TERM`/`TERM_PROGRAM` + `KITTY_WINDOW_ID` env, fall back gracefully.
3. How aggressively to coalesce `message_update` deltas — target ≤ 60 fps frame budget under sustained streams.
4. Session file parsing on `switch_session` — do we read the jsonl ourselves for the picker preview, or rely only on `get_messages` after switch? (Preview-only parse keeps the picker snappy.)
5. Clipboard on headless Linux — `arboard` requires X11/Wayland; fall back to OSC 52 for over-SSH copy.

---

## 14. First PRs after this plan lands

1. `chore(deps)`: add tokio + serde stack, wire panic-safe terminal restore.
2. `feat(rpc)`: JSONL codec + `RpcClient` with request/response correlation + fixture tests.
3. `feat(ui)`: skeleton layout (header/transcript/editor/footer) + Phase 1 streaming chat.
4. `feat(state)`: full type port + reducer + snapshot tests.

Each PR ≤ ~600 lines, each behind the same CI gate, each ships a demo gif recorded with `vhs`.
