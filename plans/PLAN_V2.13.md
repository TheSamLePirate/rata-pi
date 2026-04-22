# V2.13 — Settings + Shortcuts panels · UI density pass

## Goal

Add a `/settings` modal that exposes **every** tunable setting and observable
state of the app, and a `/shortcuts` modal that lists **every** keyboard
action. Then use those two modals as the excuse to slim the perma-visible
chrome (header, footer, status widget) and free up vertical space for the
transcript.

## Why now

Five milestones of chips and hints have accumulated at the top and bottom of
the screen:

* Header: rata-pi + model + thinking + session-name + connection + queue +
  git + heartbeat — eight things on one row.
* Footer row 2: a keybinding hint row that rotates through three different
  states (idle / streaming / focus) but is never sufficient and always
  crowded.
* Status widget: 4 rows, 2 of which carry live metrics; the other 2 are
  mostly title + spinner.

A dedicated `/settings` modal means we don't have to expose every stateful
detail in the chrome; a dedicated `/shortcuts` modal means we don't need a
hint row at all. That frees ~2–3 rows of transcript on every run.

## Out of scope

* No changes to the RPC protocol.
* No edit of keybindings at runtime (`/shortcuts` is read-only).
* No settings persistence across launches (future: V2.14 TOML config + hot reload).

---

## Design

### `/shortcuts` modal (read-only)

Grouped reference card. Each section is a short table with two columns
(`key`, `action`). Sections:

1. **Global** — Ctrl+C, Ctrl+D
2. **Editor (idle)** — Enter, Shift+Enter, Esc semantics, F-keys, Alt+T,
   Ctrl+F, Ctrl+T, Ctrl+E, Ctrl+P, Ctrl+Y, Ctrl+R, Ctrl+S, Ctrl+Space, etc.
3. **Composer editing** — Alt+←/→, Home/End, Ctrl+A/E/U/K/W, Backspace, Delete
4. **Focus mode** — j/k, g/G, Enter, y/c, Esc
5. **Modal (any)** — ↑↓, PgUp/PgDn, Home/End, Enter, Esc, type-to-filter
6. **Vim normal mode** — hjkl, wb, 0$, iaoIAO, x, dd, gg, G
7. **Interview modal** — Tab navigation, submit button, scrolling keys, etc.
8. **Mouse** — wheel, click, live-tail chip

The modal is scrollable (the list is long); the generic scroll widget
already handles this. Esc closes.

### `/settings` modal (interactive)

Two-column layout: label on the left, value + action-hint on the right.
Each row is one of four kinds:

* **Info**   — read-only string (e.g. "connected to /usr/local/bin/pi")
* **Toggle** — boolean; Enter/Space flips it
* **Cycle**  — enum/next-in-list; Enter/Space advances; ←/→ also cycles
* **Header** — section divider (non-interactive, skipped by ↑↓)

Sections, in rendering order:

1. **Session**
   - session name (read-only, set via `/rename`)
   - session id · session file path (read-only)
   - connection status (read-only — connected/offline)
   - pi binary path (read-only)

2. **Model**
   - model (cycle) — reuses the `/cycle-model` RPC
   - thinking level (cycle) — reuses `/cycle-think`
   - steering mode (cycle `all` / `one-at-a-time`)
   - follow-up mode (cycle `all` / `one-at-a-time`)

3. **Behavior**
   - show thinking (toggle; mirrors Ctrl+T)
   - notifications (toggle; mirrors `/notify`)
   - auto-compaction (toggle; mirrors F9)
   - auto-retry (toggle; mirrors F10)
   - plan auto-run (toggle)

4. **Appearance**
   - theme (cycle through built-ins)
   - vim mode (toggle; mirrors `/vim` / `/default`)

5. **Live state** (read-only)
   - live status + elapsed time
   - turn count
   - tools running / tools done
   - queue sizes (steering / follow-up)
   - context window usage
   - session cost

6. **Capabilities** (read-only, from term_caps + build features)
   - terminal kind
   - kitty keyboard protocol
   - graphics support
   - clipboard backend
   - notify-rust feature compiled in

7. **Paths** (read-only)
   - log file path
   - history file path
   - crash-dump directory
   - exports directory

8. **Build** (read-only)
   - rata-pi version
   - OS · arch

Navigation:

* `↑` / `↓` or `j` / `k` — move selection (skips Headers)
* `Home` / `g`, `End` / `G` — jump to first / last
* `PgUp` / `PgDn` — ±5
* `Enter` / `Space` — toggle / cycle selection
* `←` / `→` — on cycle rows, step back / forward
* `Esc` — close

Focused row shows a `▶` marker on the left; value spans on the right stay
aligned in a compact right column. The modal is scrollable (auto-follow
the focused row). Same scrollbar widget as every other list modal.

---

### Header + footer + status slim-down

Now that settings live in `/settings` and shortcuts in `/shortcuts`:

**Header (was 1 row, stays 1 row — less packed)**

Drop from the live header:
- thinking chip (move to /settings)
- session name chip (move to /settings)
- explicit "connected" label (turn into a color cue on the heartbeat dot)

Keep:
- `rata-pi`
- model label (shortened: `<provider>/<id>`)
- live state chip (brief — `idle / llm / tool / retry …` with color)
- turn counter (short: `t3`)
- queue chips when non-zero (`↻2 ▸1`)
- git chip when in a repo
- heartbeat dot (green/yellow/red tick)

**Footer (was 2 rows, becomes 1)**

Drop row 2 entirely (the rotating keybinding hints). Row 1 keeps the
context gauge on the left and gains a tiny right-aligned hint chip that
just says `?/help · /settings · /shortcuts`.

If the user has an active flash message it renders as a 1-row overlay
above the footer (same pattern we use for toasts).

**Status widget (was 4 rows, becomes 3)**

Drop the reserved future-use rows 3–4. Keep rows 1–2. When the terminal
is tall enough we draw it; otherwise hide (unchanged rule).

Net gain: **2 rows** of transcript screen real-estate on typical sessions,
**3 rows** when the status widget is hidden.

---

## Staging

| Sub | What | Est. LOC |
|-----|------|----------|
| V2.13.a | `/shortcuts` modal (Modal variant, draw, key handler, catalog entry) | ~300 |
| V2.13.b | `/settings` modal (SettingsState, row model, draw, key handler, action dispatch, catalog entry) | ~700 |
| V2.13.c | Header + footer slim-down | ~200 |
| V2.13.d | Status widget compaction + doc updates + tracker close | ~150 |

Each sub-commit stands on its own and keeps all gates green (tests, clippy,
fmt). Test count expected to grow by ~15 (row-rendering + toggle-cycle
regression tests).

## Success criteria

- `/settings` and `/shortcuts` modals open and render correctly on a narrow
  (80-col) and wide (140-col) terminal.
- Every App / SessionState flag has a row in `/settings`.
- Every keypress the app responds to appears in `/shortcuts`.
- Header is visibly less crowded; footer is exactly 1 row.
- Transcript has at least 2 more rows on a 30-row terminal compared to
  V2.12.x (measured by `terminal.draw` layout math).
- All existing tests pass; new tests pin down toggle dispatch and section
  ordering.

## Rolling tracker

See `plan_avancement_v2.13.md` for per-sub-commit state.
