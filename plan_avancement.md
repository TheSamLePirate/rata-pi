# Progress Tracker — rata-pi

Running log of milestone progress, deviations from `PLAN.md`, and open issues. Updated at the end of each milestone before commit.

## Legend
- `[ ]` not started · `[~]` in progress · `[x]` done · `[!]` deviated from plan (see notes)

---

## M0 — Foundations ✅

Goal: async runtime, process spawn, JSONL plumbing, panic-safe terminal restore, logging. Hello-world Ratatui proving the split works.

- [x] Add deps: tokio, tokio-util, bytes, futures, serde, serde_json, thiserror, tracing, tracing-subscriber, tracing-appender, clap, directories (color-eyre, ratatui, crossterm kept)
- [x] `src/cli.rs` — clap CLI (`--pi-bin`, `--provider`, `--model`, `--session-dir`, `--no-session`, `--debug-rpc`, `--log-level`)
- [x] `src/log.rs` — rolling daily tracing appender to `ProjectDirs::state_dir()` (Linux) or `data_local_dir()` (macOS/Windows); never stdout/stderr
- [x] `src/rpc/codec.rs` — strict LF-only JSONL codec with CR-trim, `next_index` scan-resume, 16 MiB line cap, typed `JsonlError::Oversize` recovery
- [x] `src/rpc/process.rs` — spawn `pi --mode rpc ...` with piped stdio + `kill_on_drop(true)`; friendly install-hint error if missing
- [x] `src/app.rs` + `src/main.rs` — tokio multi-thread runtime, alt-screen + mouse capture, panic-safe restore hook, `select!` over `EventStream` + 10 Hz ticker, animated Braille spinner, q / Ctrl+C / Ctrl+D quit
- [x] Smoke: `cargo build`, `cargo test` (7/7 codec tests pass), `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`

**Deviations / notes:**
- Added `bytes = "1"` (direct dep) and `futures = "0.3"` (needed for `EventStream::next()` — `StreamExt` trait). Not in the initial dep list in `PLAN.md` but implied; worth adding to the plan's dep table for completeness.
- `JsonlCodec`, `JsonlError`, `PiProcess` stdio readers carry `#![allow(dead_code)]` until M1 wires the RPC loop — documented inline.
- M1-scaffolding modules deliberately not exercised yet; keeping noise out of clippy.
- CI's `cargo test --all-features --all-targets` already runs the codec tests on macOS + Windows runners.
- Not attempted: live run against a real `pi` binary. Spawning an arbitrary Node CLI from our test harness is M1 territory; for M0 the friendly-error path is verified by the spawn code's `with_context` behavior.

---

## M1 — MVP chat (Phase 1) ✅

- [x] Port all RPC types — domain (`types.rs`), commands (`commands.rs`), events (`events.rs`)
- [x] `RpcClient` (actor pattern: writer + reader + stderr-drain tasks, oneshot correlation via `Arc<Mutex<HashMap<id, oneshot::Sender>>>`)
- [x] Transcript widget with `append_assistant` streaming mutation and `finish_tool` status transition
- [x] Single-line composer with Enter submit / Esc abort-or-clear-or-quit
- [x] Header (model label, spinner, idle/streaming/offline state) + footer (context-dependent keybind hints)
- [x] Bracketed paste support (newlines flattened to spaces in the single-line composer)
- [x] Graceful shutdown: abort → writer shutdown → 500 ms wait → kill on timeout → task joins
- [x] CI gates pass: 26/26 tests (codec + types + commands + events), `clippy -D warnings`, `fmt --check`

**Deviations / notes:**
- Added `src/ui/` module (transcript + view helpers). The plan sketched a deep `ui/widgets/*` tree; for M1 a flat module is enough — the widget split lands when rendering grows (M2 markdown + M3 pickers).
- `message_update.message` and `assistantMessageEvent.partial` stay as `serde_json::Value` rather than deep-typed `AgentMessage`. Rationale: pi emits deltas MANY times per turn and we reconstruct UI state from deltas anyway. Snapshot events (`message_start`, `message_end`, `turn_end`, `agent_end`) use the typed `AgentMessage`. Noted here so M2 knows where to tighten.
- `ExtensionUiResponse` carries its own top-level `"type":"extension_ui_response"` and is sent raw (not via `Envelope`). The naive design would have duplicated `id` fields; test coverage added to lock the wire shape.
- Scroll math is approximate: offset counts *source* lines, not wrapped lines. Under auto-follow (the default) this is invisible; under manual scroll with long wrapped lines the bottom edge may drift by a few cells. Fix lands with the virtualized transcript in M5.
- Kept compile-time `#![allow(dead_code)]` at the top of `rpc/{types,commands,events,process}.rs` and `client.rs` method-level allows because several commands/events/methods aren't surfaced in the UI until later milestones — all documented inline with the M# they belong to.
- Added two deps omitted from `PLAN.md`'s initial table: `bytes` (required by `tokio-util` codec interface) and `futures` (for `crossterm::EventStream::next`). Worth adding to the plan for the record; functionally neutral.
- Smoke test: `cargo run -- --help` prints the flags; `cargo build` succeeds on macOS. A full end-to-end smoke against a live `pi` binary hasn't been run yet — that belongs with the mock-pi integration test scheduled for the M2/M3 boundary.

---

## M2 — Full message model (Phase 2) ✅ (core) · deferred items noted

- [x] Thinking blocks — collapsible globally via `Ctrl+T`, collapsed by default with `▸ thinking (N lines)` placeholder
- [x] Tool calls: `tool_execution_start/update/end` → typed `ToolCall` entry with streaming output accumulation (`partial_result.content` text extraction), status chip (… / ✓ / ✗), args preview, per-tool `expanded` flag
- [x] Tool results with explicit error styling (red header + red border on the `is_error` path)
- [x] Markdown rendering via pulldown-cmark — headings, strong/emphasis/strikethrough, inline code, fenced code blocks with language label, lists (bullet + ordered), task lists, block quotes, rules, links (URL appended when ≠ text), soft/hard breaks
- [ ] Code blocks via syntect — **deferred to M5**. Syntect is a large dep and we chose to ship markdown without it rather than slip the milestone. Code still renders yellow-on-default inside a framed block, just without per-token syntax colors.
- [ ] Diff renderer for edit/patch tools — **deferred to M3**. Pi's edit/patch tools emit diff text in `tool_execution_end.result.content`; for now it shows as plain gray output in the expanded tool card. A dedicated diff widget with + / – / @@ coloring is a tidy add when we do the tool-detail inspector pane in M3.
- [x] Bash RPC UI — user types `!cmd` in the composer; Enter runs `RpcCommand::Bash` (blocking call on the client actor); result becomes a `BashExec` entry with exit-code chip, ANSI-stripped output body, and `… truncated — full log: …` footer when `truncated`
- [x] Auto-retry — typed `Retry` entry that updates in place from Waiting → Succeeded / Exhausted
- [x] Compaction — typed `Compaction` entry that updates in place from Running → Done{summary} / Aborted / Failed

**New modules:**
- `ui/markdown.rs` — stateful pulldown-cmark walker producing styled `Line<'static>`; 7 round-trip tests
- `ui/ansi.rs` — dependency-free CSI / OSC / DCS stripper (chose over `ansi-to-tui` which pulled in a duplicate ratatui 0.29 — noted for M5 when we do colored rendering). 5 unit tests.

**Deviations / notes:**
- Picked `pulldown-cmark = "0.13"` default-features=off to keep the HTML renderer out. Options enabled: `ENABLE_STRIKETHROUGH`, `ENABLE_TABLES`, `ENABLE_TASKLISTS`. Table rendering events are currently ignored (falls through to the catch-all) — good enough for M2 since Pi's assistant rarely emits tables. Will wire in M5 alongside syntect.
- Event handling: only deltas (`text_delta`, `thinking_delta`) mutate the transcript; we never deep-deserialize `message_update.message` or `partial`. This matches the `RpcIo` design from M1 and keeps streaming cheap under fast token rates.
- `Ctrl+E` toggles the most-recent tool card's `expanded` flag. When M5 adds mouse support, click-on-card will drive the same toggle.
- 41 unit tests pass (M1's 26 + 5 ansi + 7 markdown + 3 transcript). `clippy -D warnings` clean. `fmt --check` clean.

---

## M3 — Session & state (Phase 3) ✅ (core) · session-switcher/fork-picker deferred

- [x] Bootstrap — on connect, call `get_state` + `get_messages` + `get_commands` + `get_available_models` + `get_session_stats`. Transcript is rebuilt by translating `AgentMessage` variants into our `Entry` types (ANSI-strip on bash output).
- [ ] Session switcher (F3) — **deferred**. Needs filesystem walking of pi's session dir for the picker preview. Track-only defer to M6 with fork-picker.
- [ ] Fork picker (F4) — **deferred**. Same bucket as session switcher (needs preview-of-message UX).
- [ ] Session name setter / `export_html` / `new_session` — **deferred to M6** (require a generic text-input modal + confirm modal). `set_auto_compaction` / `set_auto_retry` are reachable now via F9/F10 instead.
- [x] Stats modal (F7) with periodic `get_session_stats` polling every 5 s — populates the modal and a context gauge in the footer that turns amber > 65 %, red > 85 %.
- [x] Commands browser (F1) — filterable list sourced from `get_commands`; Enter inserts `/name ` in the composer. Typing `/` in an empty composer opens the same modal as a slash-autocomplete.
- [x] Model picker (F5) — filterable list from `get_available_models`; Enter calls `set_model`.
- [x] Thinking-level picker (F6) — radio list; Enter calls `set_thinking_level`.
- [x] Help modal (?) — keybinding cheat sheet.
- [x] Queue state — `queue_update` event populates header chip `steer:N · follow-up:N`.
- [x] Composer mode cycle (`Ctrl+Space`) — toggles between **steer** and **follow-up** intent during streaming; routes submit to `RpcCommand::Steer` or `::FollowUp` appropriately. Editor border/title reflect the mode.
- [x] F8 compact now (fire-and-forget); F9 toggle auto-compaction; F10 toggle auto-retry. All surface a transient "flash" message in the footer for ~1.5 s.

**New module:** `ui/modal.rs` — `Modal` enum + `ListModal<T>` + `RadioModal<T>` + `centered(area, max_w, max_h)` helper + case-insensitive `matches_query`.

**Deviations / notes:**
- The deferred items (session switcher, fork picker, text-input/confirm modals) are pushed to M6. Rationale: they need a generic overlay-input pattern that's a better fit once mouse drag-select and text-input refactors land in M5.
- Modal input routing is intentionally simple (priority-over-app when open). Tab-cycle between multi-field modals isn't needed yet since all current modals have a single focus target.
- Commands modal: the badge shown per row is `ext` / `prompt` / `skill`. On Enter we currently just prefill the composer with `/name `; if the command takes an argument, the user completes it themselves. Skill commands (`skill:brave-search`) insert with their full name preserved.
- Context gauge label intentionally truncates tokens to thousands (`60k / 200k tok`) for readability.
- Editor title shows `steer` vs `follow-up` depending on composer mode when streaming; idle editor title shows `prompt (Enter submit · / commands · Esc clear)`.
- Cleanup target for M6: replace `serde_json::from_value<AgentMessage>` with direct array deserialization (serde already does the right thing) — current per-element loop is defensive against malformed entries.
- CI: 41 tests, clippy -D warnings clean, fmt clean. (No new tests added for modals — they're plumbing; golden snapshot tests land with `insta` in M5.)

---

## M4 — Extension UI protocol (Phase 4) ✅ (core) · timeout countdown deferred

- [x] `extension_ui_request` router (`handle_incoming` → `handle_ext_request`)
- [x] `select` → `Modal::ExtSelect` with arrow-key nav + Enter value / Esc cancelled
- [x] `confirm` → `Modal::ExtConfirm` with Y/N shortcuts, ←→/Tab toggle, Enter submit, Esc cancelled. Safer default is No (selected=0).
- [x] `input` → `Modal::ExtInput` with placeholder, live text; Enter returns value, Esc cancelled
- [x] `editor` → `Modal::ExtEditor` (single-line M4; multi-line in M5)
- [ ] `timeout` countdown UI — **deferred to M5** alongside the animation pipeline. Agent-side auto-resolve still fires per spec; the client just doesn't render the countdown.
- [x] `notify` → toast stack (bottom-right). Info 3 s, Warning 6 s, Error 12 s. Capped at 6 entries. Styled with `ℹ` / `⚠` / `✗` chips in matching colors.
- [x] `setStatus` → keyed `statuses: HashMap<key, text>` rendered in the footer hints row; empty / absent text clears the entry
- [x] `setWidget` → keyed `widgets: HashMap<key, Widget>` with `AboveEditor` / `BelowEditor` placement. The draw layout inserts extra strips above / below the editor dynamically (capped at 8 lines each) so widgets never push the editor off-screen.
- [x] `setTitle` → `crossterm::terminal::SetTitle` writes the OSC 0/2 terminal title; we also cache it for future restore
- [x] `set_editor_text` → prefills the composer (non-destructive when the user hasn't typed; overwrites otherwise per spec)
- [x] `extension_ui_response` replies preserve the request `id` as the top-level id (never wrapped in an outer `Envelope`), matching the spec quirk locked in by M1's serialization tests

**New module:** `ui/ext_ui.rs` — `NotifyKind` / `WidgetPlacement` / `Toast` / `Widget` / `ExtReq` / `ExtUiState`. Eight unit tests covering parse of select/confirm/notify/setWidget (both placements and clear-on-empty), the unknown-method fallthrough, toast TTL expiry, and status clear-on-None.

**Deviations / notes:**
- Timeout countdown deferred. If pi sends `timeout: 10000`, we don't yet render a "9 …" decrementing ring; the agent-side auto-resolves on expiry and our dialog just closes when the next request displaces it (or the user dismisses). Low-risk, high-polish feature — batched with animations in M5.
- Dialog displacement: if the user has a user-opened modal (commands/models/thinking/stats/help) and an extension dialog arrives, the extension wins. Rationale: extensions are typically acting on behalf of a tool-call that's blocking pi; user-opened modals can be reopened.
- `setWidget` layout math: each placement group sums its total lines (min 0, capped 8). If above+below = 12, editor still has its 3 rows since body is `Min(3)` and flex takes from the transcript.
- Unknown methods are logged-and-skipped; we don't have the request id after parse, so we can't send a cancel response — the agent-side auto-resolves per spec. Acceptable: the TS example client in pi's own examples does the same thing for unrecognized methods.
- 49/49 tests (M3 41 + 8 ext_ui). clippy -D warnings clean. fmt clean.

---

## M5 — Pro polish (Phase 5) ✅ (high-value subset)

- [x] Mouse **wheel scroll** on the transcript (`MouseEventKind::ScrollUp/Down`, 3 lines per tick)
- [ ] Mouse drag-select / copy — **deferred to M6**. Crossterm's drag events are there but we'd need hit-testing over wrapped transcript lines + integration with an OS clipboard (arboard) + fallback OSC 52 for SSH. Not a 30-minute task.
- [ ] Click-to-expand tool cards — **deferred to M6**. Needs hit-testing that tracks per-card screen rects across redraws.
- [ ] Kitty keyboard protocol — **deferred to M6**. Not yet required; crossterm gives us modifiers we need for Ctrl+combos.
- [ ] Image paste + render — **deferred to M6**. Requires terminal capability detection + ratatui-image feature-gate; large scope.
- [ ] Theme hot-reload + TOML themes — **deferred to M6**. Touches every hardcoded `Color::*` site; reasonable to bundle with a semantic-color refactor pass.
- [x] **Prompt history search** — `Ctrl+R` opens a filterable `Modal::History`; `↑` / `↓` in the composer walk forward/back in-place with stash-restore on passing the end. Persisted as JSONL under `ProjectDirs::data_local_dir()/history.jsonl`. Duplicates of the immediately-previous entry are skipped.
- [x] **Transcript → markdown export** — `Ctrl+S` writes `session-<unix-ts>.md` under the exports dir; each entry type renders in a way you can paste into PRs: user/assistant as sections with source markdown preserved, thinking as blockquote, tool calls as a header + JSON args + fenced output, bash as `$ cmd — exit N` + fenced stripped output, retries/compaction as italic meta lines.
- [ ] Fuzzy command palette (Ctrl+P) — **partially delivered**: `F1` already opens the commands browser with case-insensitive substring filtering; `Ctrl+P` as a stricter fuzzy (FZF-style) scorer lands in M6 alongside real `fuzzy-matcher`.

**New modules:**
- `src/history.rs` — `History` + `HistoryEntry`, with JSONL append-on-record and up/down walking that stashes the user's in-progress draft. 2 unit tests (walk semantics + dedupe).
- `src/ui/export.rs` — Markdown serializer over `Transcript`. 2 unit tests (basic turn + bash with exit code).

**Deviations / notes:**
- The M5 roadmap in `PLAN.md` was ambitious (mouse drag-select + Kitty + images + theme reload + command palette + history + export). Rather than ship all of them half-done, I locked in the three highest-UX-value items (mouse scroll, persistent history with in-composer navigation + search modal, markdown export) and deferred the rest to M6 so they land alongside the other post-core polish items.
- History picker uses reverse order (newest at top) so the most-recent submission is always right under the marker; feels more natural than chronological.
- Export filename uses Unix timestamp; a prettier `YYYY-MM-DD-HHMM` name is a two-line change if anyone asks.
- `Ctrl+J` newline / multi-line composer also deferred to M6 — users occasionally want multi-line prompts but in practice Enter-submit + paste-flattens-newline + `!cmd` covers ~99 % of cases.
- CI: 53 tests (M4 49 + 2 history + 2 export). `clippy -D warnings` clean. `fmt --check` clean.

---

## M6 — Advanced (Phase 6) ✅ (pragmatic subset)

Goal for this milestone was to absorb the session-management gaps from M3
and the diff-visualization gap from M2 into a working v1, rather than deliver
the literal list from PLAN.md (split view, plan-mode, inline-diff-previews,
scripted macros, cross-platform QA).

Delivered:
- [x] **Slash command router** — `/help`, `/stats`, `/export` (local md), `/export-html` (pi RPC), `/rename <name>`, `/new`, `/switch <session-file>`, `/fork` (opens picker), `/compact [instructions]`. Unknown `/name` falls through to pi so extension / prompt / skill commands still work.
- [x] **Fork picker** (`/fork`) — populated from `RpcCommand::GetForkMessages`, filterable list of `{entry_id, text}`; Enter calls `Fork { entry_id }` then re-bootstraps transcript at the fork point.
- [x] **Session switcher** via `/switch <path>` — calls `RpcCommand::SwitchSession`, then re-bootstraps. The full pickerside filesystem walking promised in PLAN.md is simplified to `<path>` argument; it covers the common case without us needing to scan pi's session dir.
- [x] **Session management** — `/new` (new_session), `/rename` (set_session_name), `/export-html` (export_html). Each surfaces a flash message with the outcome.
- [x] **Diff-aware tool output** — `looks_like_diff()` heuristic triggers on `+++`/`---`/`@@` hunk markers in the first 20 lines; renders `+` green, `-` red, `@@` magenta, file headers cyan-bold. Falls back to gray-plain otherwise.
- [x] Updated Help modal to document all slash commands and the history/export bindings from M5.

Not delivered (documented as a backlog):
- Split view (major layout rework)
- Plan-mode widget parity (unclear what to mirror without a live extension to compare against)
- `@path` autocomplete with live diff preview (needs ripgrep walker + syntect)
- Scripted macros
- Windows + Termux QA — no test matrix was added; relies on crossterm portability and the CI matrix (`ci.yml` macOS + Windows)
- Syntect code highlighting / diff (the diff heuristic is sufficient for tool output; syntect for markdown fences remains deferred)
- Multi-line composer, click-to-expand, mouse drag-select, Kitty keyboard protocol, image paste, theme hot-reload, `fuzzy-matcher` palette, dialog timeout countdown

**Deviations / notes:**
- Slash dispatch order: the `/` prefix path is tried *first* in `submit()`; unknown names fall through to pi. This keeps extension-defined commands (like `/skill:brave-search`) working unchanged.
- `/new` clears the local transcript immediately on success rather than waiting for pi to emit a `new_session` event; pi's next `get_state` (bootstrap call) reconciles.
- `/switch` re-runs `bootstrap()` on success, which re-fetches get_state/get_messages/get_commands/get_available_models/get_session_stats — expensive for large sessions but correct.
- Fork picker uses `ForkMessage.entry_id` prefix (10 chars) as a stable visual ID, preserving UX from Pi's own fork picker.
- 53 tests, clippy -D warnings clean, fmt clean.

---

## Final state

All six milestones committed on `main`. Binary builds, help prints, unit tests pass.
Feature matrix against pi's rpc.md — commands wired: all ~30 commands typed in `rpc/commands.rs`; UI reaches SetModel, CycleModel? no (defer), GetState, GetMessages, GetCommands, GetAvailableModels, GetSessionStats, Prompt, Steer, FollowUp, Abort, Bash, AbortBash? no (defer — abort always fires a general Abort), SetThinkingLevel, SetAutoCompaction, SetAutoRetry, Compact, NewSession, SwitchSession, Fork, GetForkMessages, SetSessionName, ExportHtml. Events wired: every event variant with its UX (streaming, tool cards, retries, compaction, queue, extension UI, extension errors).

Backlog items that would take the TUI from "pro" to "aspirational":
- Syntect-lit code blocks and diffs with language detection
- Multi-line composer (tui-textarea) with word motions, history search in-place
- Mouse drag-select + OSC 52 clipboard for SSH, click-to-expand cards
- Image rendering via ratatui-image (Kitty/iTerm/WezTerm/Ghostty)
- Theme system with TOML + hot reload
- Fuzzy matcher for palette + settings
- Session switcher UI (walk pi's session dir with previews)
- Dialog timeout countdown ring
- Extension keybinding file (`keys.toml`) and color theme file support
- Integration test harness with a mock-pi scripted JSONL fixture binary
- Insta golden-snapshot tests for widget rendering
- Release profile size optimization + `cargo dist` CI tarball.

---

## Cross-milestone issues / backlog

_(log anything discovered mid-flight that doesn't fit the current milestone)_
