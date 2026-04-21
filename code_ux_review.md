# rata-pi — Full Code + UX Review

**Audit date**: 2026-04-21
**Audit base**: commit `2651d8a` (V2.13 shipped — /settings, /shortcuts, chrome slim-down)
**Scope**: all 18 343 lines of Rust source, CI config, user-facing behavior.

---

## TL;DR

The project is in **very good shape**. 194 tests pass; clippy with `-D warnings` is clean; fmt is clean; a release binary builds at 5.3 MiB. Architecture has matured through 13 milestones and the V2.11.x perf pass eliminated the main hot-path pathologies.

The remaining issues fall into three buckets:

1. **One lingering architectural weight** — `src/app/mod.rs` is still 8 266 lines. The V2.12 split extracted only `draw` + `visuals`. The rest (events, input, slash, runtime, settings, interview, tests) is still in one file.
2. **Several real perf regressions introduced by V2.13.b** that we haven't noticed because tests don't exercise them — mainly redundant `History::load()`, `arboard::Clipboard::new()`, and `term_caps::detect()` calls inside `build_settings_rows` on every frame the Settings modal is open.
3. **Many small UX rough edges** — stale `/help`, flash-always-warning-colored, missing Tab binding in /settings, discoverability gaps.

None of these block the app. All of them are bounded fixes.

---

## Table of contents

1. [Architecture](#architecture)
2. [Critical perf — hot paths that regressed](#critical-perf)
3. [Code correctness](#code-correctness)
4. [Error handling & edge cases](#error-handling)
5. [Test coverage](#test-coverage)
6. [CI](#ci)
7. [UX — discoverability](#ux-discoverability)
8. [UX — visual polish](#ux-visual-polish)
9. [UX — consistency](#ux-consistency)
10. [UX — accessibility](#ux-accessibility)
11. [Dependencies and footprint](#dependencies-and-footprint)
12. [Prioritised fix list](#prioritised-fix-list)

---

## Architecture

### `src/app/mod.rs` is still the god-module

| File | LoC | Notes |
|---|---|---|
| `src/app/mod.rs` | **8 266** | State, events, input, slash, modals, interview modal key/draw, settings modal key/draw/actions, runtime, 60+ tests |
| `src/app/draw.rs` | 905 | V2.12.d extraction |
| `src/app/visuals.rs` | 350 | V2.12.c extraction |
| `src/interview.rs` | 1 938 | Parser + state + response + tests (self-contained; fine) |
| `src/rpc/types.rs` | 494 | OK |
| `src/ui/commands.rs` | 515 | OK |

The roadmap in `plan_avancement_v2.md` already tracks V2.13+ extractions (modal_draw, builders, input, slash, runtime, events). None shipped yet. `mod.rs` has GROWN by 1 400+ lines through V2.13 because Settings + Interview went into it. Trend is going the wrong way.

**Concrete next move**: Do a single extraction of the interview modal's key/draw/dispatch helpers (~500 lines) plus the settings modal's equivalent (~700 lines) into `src/app/modals/interview.rs` and `src/app/modals/settings.rs`. No behavior change; big navigability win.

### Submodule boundaries are mostly clean

Current submodules have clear single responsibilities: `rpc::{codec, client, commands, events, types}`, `ui::{cards, diff, markdown, syntax, ansi, transcript, ext_ui, export, modal, commands}`, `theme::{mod, builtin}`, `anim::{mod, ease}`. All small, all testable in isolation.

### Cross-cutting helpers sit in the parent

`truncate_preview`, `args_preview`, `approx_tokens`, `on_off`, `fmt_elapsed` all live in `src/app/mod.rs` — some are now also used from `draw.rs` via `pub(super)`. That's fine but worth factoring into `src/app/helpers.rs` next time.

---

## Critical perf

### 🔴 `build_settings_rows` opens the clipboard, walks the filesystem, and detects terminal caps **on every frame** the Settings modal is open

`src/app/mod.rs:5660-5738`:

```rust
pub(crate) fn build_settings_rows(app: &App) -> Vec<SettingsRow> {
    ...
    let caps = crate::term_caps::detect();                     // env lookup
    ...
    rows.push(R::Info {
        label: "clipboard".into(),
        value: if arboard::Clipboard::new().is_ok() {           // OPENS CLIPBOARD HANDLE
            "arboard (native)".into()
        } else {
            "OSC 52 fallback".into()
        },
    });
    ...
    let hist = crate::history::History::load();                 // DISK READ
    rows.push(R::Info {
        label: "history file".into(),
        value: hist.path().map(|p| p.display().to_string())...,
    });
    let state_dir = directories::BaseDirs::new()                // HOME-dir lookup
        .map(|d| d.state_dir().unwrap_or_else(...)...)
    ...
}
```

This function is called from **three** places per frame while the Settings modal is open:
1. `settings_body(...)` in the draw path.
2. `settings_row_source_line(...)` (via the draw selected_line computation).
3. `settings_modal_key(...)` on every keystroke.

So at 30 fps, with the modal open and the user mashing arrow keys, we're opening the clipboard, reading the history JSONL, and running env lookups **dozens of times per second**. On Linux/X11 `arboard::Clipboard::new()` opens an X display connection; on macOS it hits NSPasteboard permissions. This is the same class of bug the V2.11.1 fuzzy-filter cache fixed — we've reintroduced it.

**Fix**: cache the static bits (clipboard availability, history path, state dir, capabilities) on `App` at startup or as `OnceLock`. Only live fields (queue sizes, turn counter, cost) need rebuilding per frame.

### 🟠 `History::load()` called 3× total

`src/app/mod.rs:400` (startup — fine), `:3171` (/doctor draw), `:5707` (settings rows). `/doctor` doesn't even need the entries — only `hist.path()`. Expose a cheap `History::default_path()` static function.

### 🟠 `arboard::Clipboard::new()` called in `/doctor` too

Same pattern as above. Every time the user opens `/doctor` we open a clipboard handle that we immediately close. Some systems surface a permission prompt each time.

### 🟡 `term_caps::detect()` called 4×

Once at startup (ok), then re-probed in `/doctor`, `/settings`, and one other spot. Env-var lookups are fast but the invariant is wrong.

---

## Code correctness

### 🟠 Settings modal ignores `Tab` / `Shift+Tab`

`src/app/mod.rs:5323-5347` — the key match has arms for `Down/j`, `Up/k`, `Home/g`, `End/G`, `PgDown`, `PgUp`, `Enter/Space/Right`, `Left`, and `Esc`. **No `Tab` or `BackTab`**. Users who reflexively press Tab (because Interview modal accepts Tab) get nothing. The user-guide table under "Keyboard" doesn't list Tab either — so the doc accidentally matches reality. But `/shortcuts → Modal — any` mentions Tab flow and users will try it.

### 🟠 Cycle row `←` does the same thing as `→`

`dispatch_settings_action` handles `CycleAction::{ThinkingLevel, Model}` by firing `CycleThinkingLevel` / `CycleModel` RPCs. Pi has no `previous` endpoints, so the `CycleDir` parameter is ignored. The `SteeringMode` / `FollowUpMode` arms read the current value and flip to the other option, ignoring direction too. This means `←` on a cycle row silently advances forward — not what users expect.

**Fix**: for theme (which has a fixed local list), implement real backward cycling. For the RPC-backed cycles, hide the `←` affordance and only respond to `→`/Enter/Space.

### 🟠 Stale `/help` modal content

`src/app/mod.rs:6613` (`help_text`) lists `/help /stats /export /export-html /rename <n> /new /switch <p> /fork /compact /theme` — ten commands. The actual catalog has **40+ commands**. Users pressing `?` get a tiny subset without `/settings`, `/shortcuts`, `/doctor`, `/vim`, `/notify`, `/retry`, `/mcp`, `/find`, `/plan`, `/diff`, `/status`, etc. `/help` should just redirect to `/shortcuts` for the keybinding half and `/settings` for the state half — or be replaced by a one-liner `"press /shortcuts for keys, /settings for state"`.

### 🟡 `.unwrap()` density

63 `.unwrap()` / `.expect()` calls total. **Every one I traced is in `#[cfg(test)]` or at OnceLock init** (syntect theme load, theme crate invariant) — those are safe. Verified by inspection:
- `src/composer.rs:227/245` — fixed in V2.11.3 to `let Some(c) = ... else ...` patterns. No longer present.
- `src/history.rs:100` — replaced with `let-else` in V2.11.3. No longer present.
- `src/ui/syntax.rs:38` — panics once if syntect ships no themes. Documented invariant; acceptable.

No user-input-path unwraps. Good.

### 🟡 `let _ =` density

63 intentional-discards. Most are in fire-and-forget RPC paths (`let _ = c.fire(...).await;`). The pattern is sound but readers have to trust each site. A `fn ignore<T, E>(r: Result<T, E>, reason: &'static str)` helper with an optional `tracing::debug!` would make audits easier.

### 🟢 Error handling has been hardened

V2.12.f fixed the "insufficient credits" silent drop, V2.11.3 added proper terminal-error handling in the select loop, crash dumps write to disk. Nothing obvious to fix here.

### 🟡 `SettingsAction::Cycle` carries unused `CycleDir`

`#[allow(dead_code)]` on the field is honest but if we're not going to use direction, dropping it reduces a layer. Left as-is for forward compat (future pi RPCs).

---

## Error handling

### 🟠 RPC-backed toggles silently discard when offline

`dispatch_settings_action` on AutoCompact / AutoRetry does `if let Some(c) = client { c.fire(...).await; }`. When pi is offline (`client == None`), the local flag flips but the RPC doesn't fire. No flash. User thinks the setting changed; next session it's at the old state.

**Fix**: `if client.is_none() { app.flash("…(offline: change applies next session)"); }`.

### 🟠 `Ctrl+S` submission from composer collides with exports

`Ctrl+S` in the idle composer → export transcript to markdown. `Ctrl+S` in the Interview modal → submit form. Different contexts, different actions — fine, documented, not a bug. But the `/settings` modal doesn't intercept `Ctrl+S` so the behavior falls through to… nothing (because only modal handlers fire when a modal is open). That's fine. Worth noting so we don't accidentally add a submit-in-settings binding later.

### 🟡 Flash messages can stack

`app.flash(...)` overwrites the previous flash. If two actions flash in the same frame (rare), the second wins. Acceptable.

### 🟡 Modal stacking is prevented by replacement

Only one modal can be open at a time. Opening `/settings` while `/help` is open replaces `/help`. No confusion, but no stack-back either. OK for now.

---

## Test coverage

### Summary

**194 tests** across 24 files. Coverage by area (approximate):

| Area | Tests | Grade |
|---|---|---|
| `rpc::codec` (JSONL framing) | 7 | A |
| `rpc::events` / `rpc::types` (serde round-trips) | 11 | A |
| `rpc::commands` | 8 | A |
| `interview` (parser + state + detect) | 37 | A+ |
| `reducer` (App::on_event) | 24 | A |
| `ui::modal` (FileFinder cache) | 5 | B |
| `ui::markdown` | 7 | B |
| `ui::syntax` | 3 | B |
| `ui::transcript` | 3 | B+ |
| `ui::diff` | 4 | B |
| `ui::ext_ui` | 8 | B+ |
| `ui::commands` | 4 | B |
| `theme` | 4 | B |
| `anim` | 5 | A |
| `notify` | 5 | A |
| `plan` | 10 | A |
| `composer` | 8 | A |
| `git::classify` | 2 | C (parser only; no integration) |
| `files::filter / walk` | 4 | B |
| `clipboard` | 1 | C |
| `term_caps` | 1 | C |
| `history` | 2 | C |
| **Settings modal** | **4** | **B** — rows present, enter dispatches, esc closes |
| **Shortcuts modal** | **1** | **C** — section-presence only; no keypress dispatch test |

### Gaps

1. **Settings actions aren't end-to-end tested.** `dispatch_settings_action` requires `&RpcClient` and is async — hard to test without a mock. We verify that Enter returns the right `SettingsAction`, but not that it flips the flag. A lightweight mock client (already done for reducer tests) would cover this in ~50 lines.
2. **Interview dispatch** — same gap. We verify Tab→Submit→Enter returns `submit: true`, but not that `dispatch_interview_response` fires the right RPC.
3. **No `Modal::Shortcuts` key-scroll regression test** — just the body-sections test. A 5-line test that `KeyCode::PageDown` bumps `scroll` by 10 would be trivial.
4. **No test for the agent_end→interview flow** using the flat ASK markers on the **full App state machine** — only the parse side is covered. The reducer tests cover part of this but not the transcript-strip path for all shapes.
5. **No integration test with a scripted mock pi.** The roadmap has this slotted for V2.12 (never shipped). Highest ROI for confidence.

### Test hygiene

`cargo test --all-targets` passes. All tests run in <200 ms. No flakiness observed. Tests are organized into per-file `#[cfg(test)] mod tests` blocks except the giant `app::reducer_tests` / `app::visuals_cache_tests` / `app::interview_tests` blocks in `src/app/mod.rs` (~1 600 lines of test code inside mod.rs). Mega-file but pragmatic.

---

## CI

### `.github/workflows/ci.yml` observations

1. **Linux is missing from the test matrix**. Jobs run on macOS + Windows only. Given our main target is Linux/macOS developers, that's inverted. Add `ubuntu-latest`.

2. **Clippy action doesn't enforce `-D warnings`**. `clechasseur/rs-clippy-check@v3` defaults vary by version. Local development runs `cargo clippy --all-targets -- -D warnings` which is strictly stricter than CI. A warning merged to `main` would pass CI and fail a local clippy run.

3. **No `cargo build --features notify` step**. The `notify-rust` integration is feature-gated but CI never compiles with it enabled. It can silently rot.

4. **No release build verification**. Release mode uses LTO + different codegen; a bug that only surfaces in release (rare but real — e.g. the V2.12.c scroll layout math) would slip. Add a `cargo build --release` smoke step.

5. **No coverage measurement**. Low priority.

---

## UX — discoverability

### 🔴 `?` → `/help` modal is stale and small

Listed above under "Code correctness". The user's first discovery gesture dumps them into an 18-line cheat sheet from V2.1 that misses the last 12 milestones of commands. `/shortcuts` exists but new users don't know that yet.

**Fix**: replace the help modal body with:

```
   Welcome to rata-pi.

   For the full reference:
     /shortcuts    every keybinding, grouped by context
     /settings     every tunable + live state

   Quick essentials:
     Enter           submit prompt
     Esc             abort streaming · clear composer · quit
     Ctrl+F          focus mode
     Ctrl+C          quit
     /               command picker
     ?               this screen
```

Then every command detail is reachable via the picker (F1 / `/`) with its built-in detail pane.

### 🟠 Slash aliases aren't in the catalog

`/keys`, `/hotkeys`, `/prefs`, `/preferences` all work as slash-command names but the picker only lists `/shortcuts` and `/settings`. If a user types `/keys`, the picker filter hides it. Easy fix: add the aliases to the catalog (as alternative rows or mention them in the description).

### 🟠 No heartbeat-color legend

The header dot is green/yellow/red depending on event freshness and pi-health. Nobody knows that. Add the explanation to the "Status widget and header" section of the user guide (partially already there but worth a quick legend chip table).

### 🟡 The "Screen layout" diagram in docs/USER_GUIDE.md is ASCII-drawn and easy to drift from reality

The V2.13.c slim-down required a diagram rewrite. A programmatically-generated layout sketch (snapshot-style) would self-maintain. Low priority.

---

## UX — visual polish

### 🟠 Flash is always `warning`-colored

`src/app/draw.rs` footer fallback code:

```rust
Span::styled(msg.clone(), Style::default().fg(t.warning).add_modifier(Modifier::BOLD)),
```

Even "✓ copied 312 chars" — a success message — shows in yellow/orange. A `FlashKind::{Info, Success, Warn, Error}` enum with a color per kind would make feedback feel right.

Places that flash:
- `✓ copied 312 chars` → success
- `theme → dracula` → info
- `commit failed: ...` → error
- `switched → …` → success
- `new session started` → success
- `fork failed: ...` → error
- `/rename …` → success
- `notifications on/off` → info

About 40 call sites. Mechanical refactor.

### 🟠 Interview modal hint row gets cut off on narrow terminals

```
Tab/↓ next · Shift+Tab/↑ prev · PgUp/PgDn scroll · Space toggle · Enter send (on Submit) · Ctrl+S submit · Esc cancel
```

That's 104 chars. On an 80-col terminal it wraps unpredictably. Either shorten or make the hint line adapt to width.

### 🟠 Settings modal Cycle rows show `◂` arrows even when `←` doesn't do anything (see correctness section)

Visually implies bidirectional cycling; action is forward-only on most rows.

### 🟡 Focus `▶` marker + focused-border double-encoding

In transcript focus mode, the focused card gets both a `▶` title marker AND a `BorderType::Double` (V2.3). Plus a different border color. Triple-encoded focus — redundant but clear. Some users find the double border visually noisy; a config knob could help.

### 🟡 Sparkline colors don't adapt to live state

Throughput sparkline always uses `t.accent`; cost always uses `t.success`. When pi is in `LiveState::Error`, they stay as-is, creating a visual mismatch with the red title rule.

---

## UX — consistency

### 🟠 Modal close keys inconsistent

Most modals close on `Esc`. `Modal::Shortcuts` also closes on `q`. `Modal::Diff` also closes on `q`? — no, only Esc. Let me check again.

Actually the pattern is: read-only / viewer modals accept `q` to close (like less/vim); editable modals don't. `Help` accepts `Esc | Enter` (no q). `Shortcuts` accepts `Esc | q`. `Stats` / `Doctor` / `Mcp` accept only `Esc | Enter`. Inconsistent.

**Fix**: adopt a single rule — read-only modals accept `Esc` or `q`. Interactive modals (`Files`, `Settings`, `Commands`, etc.) accept only `Esc`.

### 🟠 `End` key semantics differ by context

- Empty composer → `End` = re-pin live tail.
- Composer with text → `End` = end of line.
- Modal → `End` = jump to last selection.
- Interview multi-line text → `End` = end of current text field.

All defensible individually, but worth a mental-model sentence in the guide: "End moves to the last interesting thing in the current context."

### 🟡 `Ctrl+Enter` works for Interview submit but not for Composer submit

Composer: plain `Enter` submits, `Shift+Enter` inserts newline. `Ctrl+Enter` does nothing (unmatched in `handle_key`). For symmetry with the Interview modal, `Ctrl+Enter` on the composer could also submit (same as Enter). Trivial to add.

### 🟡 Composer `Ctrl+S` = export, Interview `Ctrl+S` = submit

Context-dependent. Documented. But it's the most-overloaded shortcut in the app.

---

## UX — accessibility

### 🟡 No mouse support in modals

Transcript supports wheel-scroll and click-to-focus. Modals are keyboard-only. Could add wheel scroll to Commands / Files / Settings / Shortcuts / Interview — the scroll math already exists (`selected_line` → scroll offset).

### 🟡 No high-contrast theme

All six built-ins assume 24-bit true color. Monochrome / high-contrast mode would help users on low-color terminals or with accessibility needs. Low priority — the `theme::gauge_color` helper already defines a gradient that could specialize.

### 🟡 Rectangular-character icons (`▶ ⚙ ✦ ✗`) may render poorly in some fonts

No fallback path. `/doctor` reports graphics/kitty-kb but doesn't probe font capabilities. Out of scope realistically.

### 🟢 Color-blind-friendliness

Most themes use theme-assigned semantic slots (`success`, `warning`, `error`) with contrast. `catppuccin-mocha` and `gruvbox-dark` are popular with CVD users already.

---

## Dependencies and footprint

### Direct deps (from Cargo.toml)

21 direct deps. Reasonable for a TUI app of this scope. No obvious bloat.

| Dep | Role | Note |
|---|---|---|
| `ratatui` 0.30 | TUI framework | unstable-rendered-line-info feature used |
| `crossterm` 0.29 | input/output | event-stream feature used |
| `tokio` 1.47 | async runtime | full-ish features |
| `tokio-util` 0.7 | codec / JSONL framing | |
| `syntect` 5 | syntax highlighting | +3 MiB binary, no C deps (regex-fancy) |
| `pulldown-cmark` 0.13 | markdown parser | default-features off |
| `arboard` 3 | clipboard | wayland-data-control feature |
| `fuzzy-matcher` 0.3 | SkimMatcherV2 | |
| `ignore` 0.4 | file walker | |
| `directories` 6 | XDG paths | |
| `base64` 0.22 | OSC 52 | |
| `serde` / `serde_json` | JSONL | |
| `thiserror` 2 | error types | |
| `clap` 4 | CLI parsing | derive feature |
| `color-eyre` 0.6.5 | panic hook | |
| `tracing` / `tracing-subscriber` / `tracing-appender` | logging | |
| `notify-rust` 4 | native desktop notifications | **optional, behind `notify` feature** |

### Build

- **Debug**: unmeasured but typical debug profile.
- **Release**: 5.3 MiB (since V2.11.3 flipped `opt-level=s` → `3`). Down from 4.2 MiB, worth it for speed.
- **LTO**: on. codegen-units = 1. Release build takes 55 s on a modern Mac.

### Cargo.lock

Checked in. Good.

---

## Prioritised fix list

Ranked by (impact × ease) — highest first.

### P0 — real user-visible or correctness issues (<1 day each)

| # | Item | Rough effort | Notes |
|---|---|---|---|
| 1 | **Settings `build_settings_rows` runs clipboard+disk probes every frame** | 1 h | Cache on App at startup; rebuild only volatile rows. Biggest regression since V2.11.1. |
| 2 | **Settings modal missing Tab / Shift+Tab binding** | 5 min | Add to `settings_modal_key`. Users try it first. |
| 3 | **`/help` content is 10-commands-of-40 stale** | 15 min | Replace body with `/shortcuts` + `/settings` pointer + essentials. |
| 4 | **Flash always warning-colored** | 1 h | Introduce `FlashKind`; ~40 sed-style call-site updates. |
| 5 | **Slash aliases missing from the Commands catalog** | 5 min | Add `/keys`, `/hotkeys`, `/prefs`, `/preferences` entries or note aliases in descriptions. |
| 6 | **Offline-mode silently swallows RPC-backed toggle** | 10 min | Add `else { app.flash("offline — applies next session"); }` to the two call sites. |
| 7 | **CI doesn't test on Linux** | 5 min | Add `ubuntu-latest` to matrix. |
| 8 | **CI clippy doesn't enforce `-D warnings`** | 5 min | Pass `clippy_flags: -- -D warnings` to the action. |

### P1 — visible improvement, small risk (0.5–2 days each)

| # | Item | Effort |
|---|---|---|
| 9 | Extract `/interview` and `/settings` modal code from `mod.rs` into `src/app/modals/{interview,settings}.rs` | 2 h |
| 10 | Cache `term_caps::detect()` result on App; expose via `app.caps()` | 30 min |
| 11 | Cache the History path once (don't call `History::load()` from /doctor) | 15 min |
| 12 | Shrink Interview modal hint row on narrow terminals | 30 min |
| 13 | Add `cargo build --features notify` step to CI | 5 min |
| 14 | Add `cargo build --release` smoke step to CI | 5 min |
| 15 | Remove `←` affordance on pi-RPC-cycle rows (or implement real reverse cycle for theme) | 30 min |
| 16 | Standardise modal close keys (Esc for all; `q` for read-only viewers only; document) | 45 min |
| 17 | `Ctrl+Enter` on composer = submit | 10 min |
| 18 | Mock-pi integration test covering prompt → assistant text → agent_end | 3 h |

### P2 — maintainability, deferrable

| # | Item | Effort |
|---|---|---|
| 19 | Continue `mod.rs` extraction (runtime, slash, events) per the V2.13+ roadmap | 2 days total |
| 20 | Factor shared helpers (`truncate_preview`, `approx_tokens`, etc.) into `src/app/helpers.rs` | 30 min |
| 21 | Mouse support in modals (wheel + click) | 4 h |
| 22 | High-contrast theme | 2 h |
| 23 | Modal close-key consistency audit | 1 h |
| 24 | Replace `let _ =` scatter with `fn ignore<T,E>(r, reason)` helper for auditability | 1 h |
| 25 | Heartbeat-color legend in the user guide | 15 min |
| 26 | Flash messages with consistent icon chips (ℹ / ✓ / ⚠ / ✗) | 1 h (if done with #4) |

### P3 — future feature work (not a regression, not blocking)

- Undo/redo in composer
- Composer templates
- Transcript search
- Theme persistence (TOML config)
- Real MCP panel (once pi exposes it)
- TODO widget (deferred from V2.8)
- Session timeline / replay
- Composer draft auto-save across launches
- Snapshot/export improvements

---

## Signals that rata-pi is in a strong place

* **Every major state transition is reducer-tested.** Adding a new Incoming variant is safe: misses are caught.
* **Perf is well-understood.** V2.11.1 + V2.11.2 documented exactly why each hot-path function exists and what it costs.
* **Every commit has bisectable builds.** Tests pass on every milestone in the tracker.
* **User-guide is comprehensive and up-to-date through V2.13.** ~1 400 lines, covers every feature including the new Interview grammar and Settings panel.
* **Extensive marker / capability protocols with models** (plan, interview). The capability_hint pattern means the agent discovers features without the user explaining — a rare and valuable design choice.

---

## Summary

Ship V2.13 as-is; the fixes above are refinements, not show-stoppers.

**Biggest single win available right now**: item #1 — cache the static "capabilities / paths / build" rows in /settings. It turns a real-per-frame overhead into a constant.

**Most architecturally important**: continue the `mod.rs` extraction. The codebase has proven it can sustain 5 sub-commits per milestone with green gates; break off 2-3 focused modules per milestone until `mod.rs` is under 3 000 lines.

**Most UX impact for effort**: refresh `/help` (item #3), colour-code flash (item #4), add aliases to the catalog (item #5). Under an hour of total work. All three shape the user's first impression.
