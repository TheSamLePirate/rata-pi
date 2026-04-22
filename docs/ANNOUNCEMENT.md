# rata-pi 1.0.0 — a terminal UI for the Pi coding agent

*2026-04-22*

Today I'm tagging **rata-pi 1.0.0**. It's a Ratatui-based terminal UI
for [`@mariozechner/pi-coding-agent`](https://github.com/mariozechner/pi-coding-agent)
— the `pi` CLI. You install it with `brew install olivvein/rata-pi/rata-pi`,
`cargo install rata-pi`, or a platform tarball from the
[Releases page](https://github.com/olivvein/rata-pi/releases).

## Why it exists

pi does a lot in a terminal already: streams model output, runs tools,
manages a session, retries on error. rata-pi is the opinion that once
the session gets longer than a single question-and-answer, you want:

- a **chat surface that doesn't lose your place** as the buffer
  scrolls;
- an **approval step** between an agent proposing a multi-step plan and
  that plan actually running;
- **everything you might want to tweak** — model, thinking level,
  auto-compact, auto-retry, steering mode, notifications, theme —
  behind a single keystroke, not a stack of flags;
- a place to **save prompts you re-use** and **search the transcript**
  you just produced;
- git where the cursor is, not in another pane.

That's what rata-pi delivers. It is not a pi fork — it spawns
`pi --mode rpc` as a child process and speaks the existing JSONL RPC.

## What shipped in `1.0.0`

**Chat / transcript**
- Streaming transcript with per-entry render cache and virtualized
  scrolling. Long sessions stay responsive.
- Focus mode (`Ctrl+F`) for card-level navigation; second click on a
  tool card toggles expanded output.
- Markdown + syntect fenced-code highlighting that tracks the active
  rata-pi theme.
- Seven built-in themes including `high-contrast` for CVD-friendly /
  low-color terminals.

**Plan approval**
- Agent-emitted `[[PLAN_SET: …]]` opens a **Plan Review modal**.
  Accept / Edit (add / delete / rewrite steps) / Deny. Nothing runs
  until you say so.
- `[[PLAN_ADD: …]]` on an accepted plan opens an amendment review
  preserving completed steps.
- Raw-marker visibility is a setting — strip the brackets from the
  visible transcript, or keep them for debugging.

**Agent interview mode**
- `[[ASK_*]]` markers (one per line, plan-mode grammar) become a form
  with required fields, defaults, sections, and a single JSON
  response block on submit.

**Modals and UI**
- `/settings` — every tunable plus live state in one scrollable pane.
- `/shortcuts` — keybinding reference pinned by regression tests.
- `/doctor` — readiness checks (pi, terminal, clipboard, git, theme,
  notifications).
- `/stats`, `/diff`, `/log`, `/branch`, `/commit`, `/stash` — git +
  session inspection.
- **Transcript search** (`/search`) — live overlay with per-hit
  snippets, `n` / `N` navigation, `Enter` focuses the match.
- **Composer templates** (`/template`) — two-pane save / load / delete
  picker. `/template save <name>` snapshots the composer.
- **File finder** — `Ctrl+P` or `@path` in the composer, bounded
  syntect preview, respects `.gitignore`.

**Mouse**
- Click outside any modal closes it.
- Click Plan Review action chips, Settings rows, list-modal rows,
  Thinking picker options, and Ext confirm / select chips.
- Wheel scroll routes into the open modal.

**Composer**
- Undo / redo (`Ctrl+Z` / `Ctrl+Shift+Z`) with a 64-snapshot ring.
- Composer draft auto-saved on quit; restored on next launch.
- `Ctrl+Enter` submit (symmetric with the Interview and Plan Review
  modals).

**Preferences persistence**
- Theme, notifications, vim mode, thinking visibility, raw-marker
  visibility, and focus-marker style persist to
  `<config_dir>/rata-pi/config.json`.

**Reliability**
- Per-call RPC timeouts (bootstrap 3 s, stats 1 s, user actions 10 s).
  Timeouts are non-fatal flashes; the UI never hangs waiting on a
  degraded pi.
- `RpcClient::call` no longer leaks pending entries on send failure.
- Per-frame I/O in `/settings` reduced from 3+ probes to 0 — cached
  on `AppCaps` at startup.
- File preview reads bounded — `metadata()` rejects > 50 MiB before
  any read.

**Distribution**
- `cargo build --release --locked --target <triple>` on Ubuntu, macOS
  arm64, macOS x86_64, and Windows x86_64 in the release workflow.
- Homebrew formula at `Formula/rata-pi.rb` (fill the SHA256 fields
  post-release and point a tap at it).
- CHANGELOG, MIT license, Cargo metadata ready for `cargo publish`.

**Numbers**
- 255 tests, clippy `-D warnings` clean, fmt clean.
- **~5.3 MiB** release binary on macOS arm64 with LTO + opt-level=3.
- `src/app/mod.rs` split across ten focused modules; the reducer
  (`events.rs`), per-modal key dispatcher (`modal_keys.rs`), and
  transcript card bodies (`cards.rs`) each live in their own file.

## Known deviations

Tracked transparently in [`track_v3_progression.md`](../track_v3_progression.md)
and [`track_v4_progression.md`](../track_v4_progression.md). Highlights:

- Config file is JSON, not TOML — keeps the dep surface smaller.
- Inline highlight-on-match inside the transcript is not shipped; the
  search overlay's per-hit snippet + jump-to-focus covers the use case.
- `src/app/mod.rs` sits at ~5 375 lines vs. the V4.d < 4 000 target.
  Residue is ~2 100 lines of in-file unit tests — refactoring the
  runtime code fully succeeded.
- Commands modal rows are not mouse-clickable; category-grouped layout
  would need a dedicated hit-test pass.
- TODO widget dropped from V3.j — no user demand surfaced.

## Getting started

```bash
# install pi (the engine rata-pi drives)
npm install -g @mariozechner/pi-coding-agent

# install rata-pi
brew install olivvein/rata-pi/rata-pi    # macOS / Linux
# or
cargo install rata-pi

# run
rata-pi
```

Offline mode is supported: if `pi` isn't on `$PATH`, rata-pi still
starts so you can inspect `/settings`, `/shortcuts`, themes, git, and
the file finder. RPC-backed toggles flash `offline — applies next
session`.

## Where to read more

- One-page pitch: [`docs/PITCH.md`](PITCH.md)
- Illustrated feature tour: [`docs/FEATURES.md`](FEATURES.md)
- Full user manual: [`docs/USER_GUIDE.md`](USER_GUIDE.md)
- Full changelog: [`CHANGELOG.md`](../CHANGELOG.md)

## Thanks

To [Ratatui](https://ratatui.rs) for the TUI framework, and to
[`@mariozechner/pi-coding-agent`](https://github.com/mariozechner/pi-coding-agent)
for the engine rata-pi drives. Every modal, persistence feature, and
piece of test harness that landed between V2 and V4 is documented
above. `1.0.0` is the point where "rata-pi" stops being a local
checkout and starts being something you install.
