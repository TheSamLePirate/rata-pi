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

## M1 — MVP chat (Phase 1)

- [ ] Port all RPC types (commands + events + domain) with serde
- [ ] `RpcClient` with request/response correlation via oneshot map
- [ ] Transcript widget — append-only, auto-scroll, user + assistant streaming
- [ ] Editor widget (single-line first, then multi-line) with Enter submit / Esc abort
- [ ] Header (model) + footer (working state + spinner)
- [ ] Bracketed paste (plain text)
- [ ] Graceful shutdown: abort → drain → kill

**Deviations / notes:** _(to fill in)_

---

## M2 — Full message model (Phase 2)

- [ ] Thinking blocks (collapsible)
- [ ] Tool calls: streaming args + output via `tool_execution_update`
- [ ] Tool results (success/error styling)
- [ ] Markdown rendering (pulldown-cmark → Ratatui Text)
- [ ] Code blocks via syntect
- [ ] Diff renderer for edit/patch tools
- [ ] Bash RPC command UI with `BashExecutionMessage`
- [ ] Auto-retry inline row; compaction timeline entries

**Deviations / notes:** _(to fill in)_

---

## M3 — Session & state (Phase 3)

- [ ] Bootstrap `get_state` + `get_messages` on start, restore transcript
- [ ] Session switcher (F3), fork picker (F4)
- [ ] Session name setter, `export_html`, `new_session`
- [ ] Stats panel + animated context gauge
- [ ] Commands panel (`/` autocomplete) from `get_commands`
- [ ] Queue panel; `steer` / `follow_up` composition with live mode indicator

**Deviations / notes:** _(to fill in)_

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
