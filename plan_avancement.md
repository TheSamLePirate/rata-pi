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

## M4 — Extension UI protocol (Phase 4)

- [ ] `extension_ui_request` router → dialog components
- [ ] `select` / `confirm` / `input` / `editor` with timeout countdown
- [ ] `notify` → toast stack
- [ ] `setStatus` / `setWidget` / `setTitle` / `set_editor_text`
- [ ] Correctly send `extension_ui_response` with matching `id`

**Deviations / notes:** _(to fill in)_

---

## M5 — Pro polish (Phase 5)

- [ ] Mouse: drag-select copy, click-to-expand, wheel scroll
- [ ] Kitty keyboard protocol
- [ ] Image paste + render (feature `images`)
- [ ] Theme hot-reload; TOML themes
- [ ] Fuzzy command palette (`Ctrl+P`), prompt history search (`Ctrl+R`)
- [ ] Transcript → markdown export

**Deviations / notes:** _(to fill in)_

---

## M6 — Advanced (Phase 6)

- [ ] Split view
- [ ] Plan-mode widget parity
- [ ] Inline diff preview for `@path` references
- [ ] Scripted macros
- [ ] Cross-platform QA (Windows + Termux)

**Deviations / notes:** _(to fill in)_

---

## Cross-milestone issues / backlog

_(log anything discovered mid-flight that doesn't fit the current milestone)_
