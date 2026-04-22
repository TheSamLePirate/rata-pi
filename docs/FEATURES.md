# rata-pi — Illustrated feature tour

This doc walks through the shipped surfaces with ASCII mockups. For
the precise keybinding reference, see
[`USER_GUIDE.md`](USER_GUIDE.md).

---

## 1. The chrome

Launch `rata-pi` and you get a three-band layout: header (1 row) →
transcript + composer → status widget + footer.

```text
 rata-pi ● claude-sonnet-4-6 · llm · ⎇ main ● ↑2 · t3
───────────────────────────────────────────────────────
 you
   let's clean up the settings reducer

 thinking
   I'll read the current module first…

 🛠 Read src/app/modals/settings.rs (124 lines)

 assistant
   I see three kinds of rows — info, toggle, and cycle.
   Worth extracting a dispatcher?

 > _                                                   (INS)
───────────────────────────────────────────────────────
 ── status · 00:14 ──────────────────────────────────
   llm · claude-sonnet-4-6 · turn 3 · 0 running · 1 done
   throughput ▂▄█▆ 82 t/s · cost ▂▃▅ $0.014 · session $0.14
 ctx 14.2k/200k ▇▁ $0.14      ? · /settings · /shortcuts
```

**Header** — wordmark, heartbeat dot (green / yellow / red), spinner,
model, live state, queue chips, git chip, turn counter.

**Status widget** — divider + spinner + throughput / cost
sparklines. Hidden when the terminal is < 20 rows tall.

**Footer** — left: context gauge + session cost. Right: hint chip
that flashes toasts in warning color on events like theme changes.

---

## 2. The plan approval flow

When the agent emits `[[PLAN_SET: …]]`, rata-pi opens a Review modal
instead of running anything. You're the approver.

```text
 ┌ plan · review ─────────────────────────────────────┐
 │ auto-run ✓ (t toggles)                             │
 │                                                    │
 │ [ ] 1. extract cycle dispatch into its own fn      │
 │ [ ] 2. rewire theme row to the new dispatcher      │
 │ [ ] 3. update snapshot tests                       │
 │                                                    │
 │  [ Accept (a) ]  [ Edit (e) ]  [ Deny (d) ]        │
 └────────────────────────────────────────────────────┘
```

Press `e` to switch to Edit mode: `↑↓` picks a step, `Enter` / `i`
edits it, `a` inserts a blank step below focus, `x` / `Del` deletes,
`Ctrl+S` commits the edited list.

On Accept, the first step's prompt is staged automatically (toggle
off with `t` beforehand if you want to hand-drive). `[[STEP_DONE]]`
and `[[STEP_FAILED: …]]` from the agent advance or halt the plan;
`[[PLAN_ADD: …]]` opens the Review modal again with the amended
list.

---

## 3. The agent interview

When the agent wants structured answers, markers like `[[ASK_TEXT: …]]`,
`[[ASK_PICK: …]]`, `[[ASK_YESNO: …]]`, `[[ASK_NUM: …]]` become a form.

```text
 ┌ Project setup ─────────────────────────────────────┐
 │ Short description shown above the fields           │
 │                                                    │
 │ Basics                                             │
 │   Project name     *  [ rata-pi-example        ]   │
 │   Stack               ( ) Node ( ) Deno (•) Rust   │
 │   Include tests       [x] yes                      │
 │   Max memory (MB)     [ 512                    ]   │
 │                                                    │
 │              [ Create (Ctrl+Enter) ]               │
 └────────────────────────────────────────────────────┘
```

`Tab` / `Shift+Tab` moves between fields, `Space` toggles yes/no and
multi-picks, `Ctrl+Enter` / `Ctrl+S` submits. rata-pi returns a
single JSON payload as a user message so the agent parses
deterministically.

---

## 4. `/settings` — every tunable, one pane

```text
 ┌ settings ──────────────────────────────────────────┐
 │ Session                                            │
 │    session name      my-session                    │
 │    connection        connected                     │
 │                                                    │
 │ Model                                              │
 │ ▶  model             claude-sonnet-4-6          ▸  │
 │    thinking          medium                     ▸  │
 │    steering mode     one-at-a-time              ▸  │
 │    follow-up mode    all                        ▸  │
 │                                                    │
 │ Behavior                                           │
 │    show thinking     [x] yes                       │
 │    notifications     [ ] no                        │
 │    auto-compaction   [x] yes                       │
 │    auto-retry        [x] yes                       │
 │    plan auto-run     [x] yes                       │
 │                                                    │
 │ Appearance                                         │
 │    theme           ◂ tokyo-night                ▸  │
 │    vim mode         [ ] no                         │
 │    show raw markers [ ] no                         │
 │    focus marker    ◂ both                       ▸  │
 │                                                    │
 │ …                                                  │
 └────────────────────────────────────────────────────┘
```

`↑↓ / jk` walks interactive rows (Info rows are skipped). `Enter` /
`Space` toggles; `→` cycles forward; `←` cycles backward on
theme. Clickable with the mouse in V4.a.

Preferences that persist across launches: `theme`, `notify`, `vim`,
`show_thinking`, `show_raw_markers`, `focus_marker`. File lives at
`<config_dir>/rata-pi/config.json`.

---

## 5. `/shortcuts` — the always-up-to-date keybinding reference

```text
 ┌ shortcuts ─────────────────────────────────────────┐
 │ Global                                             │
 │   Ctrl+C / Ctrl+D   quit                           │
 │                                                    │
 │ Editor (no modal)                                  │
 │   Enter             submit                         │
 │   Shift+Enter       newline                        │
 │   Ctrl+F            focus mode                     │
 │   Ctrl+P            file finder                    │
 │   Ctrl+R            prompt history                 │
 │   Ctrl+Z / …+Shift  undo / redo                    │
 │   F1 / /            commands                       │
 │   F5 / F6 / F7      model / think / stats          │
 │   Alt+T / F12       cycle theme                    │
 │                                                    │
 │ Modal                                              │
 │   ↑ ↓ / j k         move selection                 │
 │   Enter             activate                       │
 │   Esc               close                          │
 │ …                                                  │
 └────────────────────────────────────────────────────┘
```

Regression tests pin down every section header — the panel can
never drift away from the real key handler.

---

## 6. Transcript search

```text
 ┌ search ────────────────────────────────────────────┐
 │ find: insufficient credit▏                         │
 │                                                    │
 │  3 hits · n/N to walk · Enter focuses              │
 │                                                    │
 │   [assistant · turn 2]  … ran into an              │
 │      "insufficient credits" error calling …        │
 │ ▶ [tool     · turn 4]  stderr: insufficient        │
 │      credits (402) — retry_after=120s              │
 │   [assistant · turn 5]  … retrying after the       │
 │      insufficient-credits bounce …                 │
 └────────────────────────────────────────────────────┘
```

`/search [query]` opens the overlay. No arg → empty query.
With an arg → pre-populates and jumps to the most recent match.
`n` / `↓` / `Tab` next, `N` / `↑` / `Shift+Tab` prev, `Enter`
focuses the hit, `Esc` cancels.

---

## 7. Composer templates

```text
 ┌ templates ─────────────────────────────────────────┐
 │ ↑↓ nav · Enter load · d delete · Esc close         │
 │                                                    │
 │   review-pr        │ Please review this PR.        │
 │ ▶ release-notes    │ Draft release notes from the  │
 │   bug-triage       │ commits since the last tag,   │
 │   plan-refactor    │ grouped into user-visible     │
 │                    │ changes + internal changes.   │
 │                    │                               │
 │                    │ Focus on the diff, not the    │
 │                    │ commit messages.              │
 └────────────────────────────────────────────────────┘
```

`/template save <name>` snapshots the composer. `/template` (or
`/tpl`) opens this two-pane picker. `Enter` loads the body into the
composer; `d` deletes the focused template. Stored in
`<config_dir>/rata-pi/templates.json`.

---

## 8. Git integration

```text
 ┌ git status ────────────────────────────────────────┐
 │ branch: main ● dirty · ↑2 ↓0                       │
 │                                                    │
 │ staged:                                            │
 │   M  src/app/mod.rs                                │
 │                                                    │
 │ unstaged:                                          │
 │   M  src/ui/modal.rs                               │
 │                                                    │
 │ untracked:                                         │
 │   ?  docs/FEATURES.md                              │
 └────────────────────────────────────────────────────┘
```

And the diff viewer (`/diff` or `/diff --staged`):

```text
 ┌ diff · unstaged ───────────────────────────────────┐
 │ --- a/src/ui/modal.rs                              │
 │ +++ b/src/ui/modal.rs                              │
 │ @@ … @@  fn modal_keys …                           │
 │ 123 123   pub fn handle(key: KeyCode) {            │
 │ 124 124       match key {                          │
 │ 125       -       KeyCode::Char('q') => close(),   │
 │     125  +       KeyCode::Char('q') |              │
 │     126  +       KeyCode::Esc      => close(),     │
 │ 126 127           _ => {}                          │
 │ 127 128       }                                    │
 │ 128 129   }                                        │
 └────────────────────────────────────────────────────┘
```

Syntect-highlighted context lines when the `+++ b/…` header lets us
derive a file extension.

---

## 9. File finder

```text
 ┌ files · 13,842 indexed ────────────────────────────┐
 │ @src/app/mod▏                                      │
 │                                                    │
 │ ▶ src/app/mod.rs              │ //! V4.a · chip    │
 │   src/app/modal_keys.rs       │ //! registration   │
 │   src/app/modals/bodies.rs    │                    │
 │   src/app/modals/interview.rs │ use super::*;      │
 │   src/app/modals/settings.rs  │ …                  │
 └────────────────────────────────────────────────────┘
```

`Ctrl+P` opens it; `@` in the composer opens it in AtToken mode.
`Enter` inserts `@<path>` at cursor. Preview pane is syntect-
highlighted by extension, bounded to 8 KiB / 40 lines.

---

## 10. Doctor

```text
 ┌ doctor ────────────────────────────────────────────┐
 │ PASS  pi binary          /usr/local/bin/pi         │
 │ PASS  pi connection      connected, turn 3         │
 │ INFO  terminal           kitty                     │
 │ PASS  kitty keyboard     advertised                │
 │ INFO  graphics           kitty protocol            │
 │ PASS  clipboard          arboard (native)          │
 │ PASS  git                main                      │
 │ INFO  theme              tokyo-night               │
 │ PASS  notifications      on (native + osc777)      │
 └────────────────────────────────────────────────────┘
```

One-look readiness. `/doctor` anytime.

---

## 11. Plan view

Any time you want to see the active plan:

```text
 ┌ plan ──────────────────────────────────────────────┐
 │ auto-continue ✓                                    │
 │                                                    │
 │ [x] 1. extract cycle dispatch into its own fn      │
 │ [→] 2. rewire theme row to the new dispatcher      │
 │ [ ] 3. update snapshot tests                       │
 │                                                    │
 │ 1/3 done · 1 active · 1 pending                    │
 └────────────────────────────────────────────────────┘
```

`/plan` (or `/plan show`) opens it read-only. User-authored plans
via `/plan set a | b | c` activate immediately (no review); agent
plans always go through the Review modal.

---

## 12. Themes

Seven built-ins. `Alt+T`, `Ctrl+Shift+T` (Kitty-keyboard
terminals), or `F12` cycles:

1. **tokyo-night** (default)
2. **dracula**
3. **solarized-dark**
4. **catppuccin-mocha**
5. **gruvbox-dark**
6. **nord**
7. **high-contrast** — CVD-friendly / low-color fallback

All 21 semantic color slots (borders, roles, diff, gauge) swap
atomically; markdown + syntect fenced-code highlighting tracks the
active palette. Theme choice persists across launches.

---

## 13. Notifications

- **OSC 777** always on. Supported: iTerm2, kitty, WezTerm, Ghostty,
  gnome-terminal, konsole.
- **Native** behind the `notify` feature flag (`cargo build
  --release --features notify`) — DBus on Linux, NSUserNotification
  on macOS, WinToast on Windows.

Fires on: `agent_end` ≥ 10 s (`"15s · 2 tool calls"`),
`tool_execution_end` with error, `auto_retry_end` with failure.
Toggle with `/notify` — includes a test-fire so you see it work.

---

## 14. Resilience

- **Timeouts** — bootstrap 3 s, stats 1 s, user actions 10 s.
  Timeouts flash a non-fatal toast; the UI never hangs waiting on a
  degraded pi.
- **Panic hook** — disables Kitty keyboard, leaves the alt screen,
  writes `crash-<unix_ts>.log` to the platform state dir, chains to
  the original `color-eyre` hook.
- **Graceful shutdown** — `Ctrl+C` sends a clean abort to pi and
  waits on the child before exit.
- **No-pi offline mode** — `/settings`, `/shortcuts`, themes, git,
  file finder all work. RPC-backed toggles flash `offline — applies
  next session`.

---

## 15. Under the hood

- **Rust 2024**, ratatui 0.30, crossterm 0.29, tokio 1.47.
- **JSONL RPC** over pi's stdin/stdout — one framed codec, one
  actor, `RpcClient::TestHarness` for unit tests that assert the
  serialized payload of every settings / interview dispatch.
- **Per-entry render cache** — transcript virtualizes on scroll and
  skips entries that haven't mutated.
- **10 focused modules** under `src/app/` — `draw`, `events`,
  `input`, `cards`, `modal_keys`, `modals/{bodies,interview,settings}`,
  `helpers`, `visuals`. `mod.rs` is the dispatcher.
- **255 tests**, clippy `-D warnings` clean, `cargo fmt` clean.
- **5.3 MiB release binary** with LTO + opt-level=3.

---

## Where next

- User manual with every keystroke: [`USER_GUIDE.md`](USER_GUIDE.md).
- One-page pitch: [`PITCH.md`](PITCH.md).
- `1.0.0` announcement: [`ANNOUNCEMENT.md`](ANNOUNCEMENT.md).
- Release history: [`CHANGELOG.md`](../CHANGELOG.md).
