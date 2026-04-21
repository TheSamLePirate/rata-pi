# rata-pi

A terminal UI for [Pi](https://github.com/mariozechner/pi-coding-agent), the
`@mariozechner/pi-coding-agent` CLI. Built with
[Ratatui](https://ratatui.rs) on top of a JSONL RPC actor that speaks
to pi over stdio.

rata-pi gives the pi coding agent a fast, keyboard-driven chat surface
with a streaming transcript, a slash-command palette, project file
fuzzy-find, git integration, and an interview / plan approval flow so
the agent never runs a multi-step plan without explicit user consent.

## Status

V3 in progress. The codebase already ships a large feature set
(streaming transcript with per-entry render cache, modal-based UI,
six built-in themes, notifications, crash dumps, RPC timeouts, a
plan-approval review modal, an agent-driven interview form, marker
stripping, and a settings panel for every runtime toggle). See
`PLAN_V3.md` for the active plan and `track_v3_progression.md` for
where each sub-milestone currently stands.

## Features

- **Streaming transcript** — per-entry render/height cache, live tail
  scroll, focus mode (Ctrl+F) for card-level navigation.
- **Plan approval flow** — `[[PLAN_SET: …]]` from the agent opens a
  review modal; you Accept, Edit (add / delete / in-place edit
  steps), or Deny before anything runs. Accepted plans kick off
  auto-run if you left the toggle on.
- **Agent interview** — the agent can emit `[[ASK_TEXT: … ]]`-style
  markers to pop a structured form; answer with Tab navigation and
  Ctrl+S / Ctrl+Enter to submit.
- **Slash picker** — `/` or F1 opens the fuzzy command menu over
  built-ins + pi skills; `/settings`, `/shortcuts`, `/find`,
  `/plan`, `/doctor`, etc.
- **`/settings` panel** — every runtime tunable plus live state:
  model, thinking level, steering / follow-up mode, auto-compact,
  auto-retry, notifications, vim mode, theme, raw-marker visibility.
- **`/shortcuts` panel** — read-only keybinding reference grouped by
  context (global / composer / modal / vim / interview / mouse).
- **Themes** — six built-ins (tokyo-night, dracula, solarized-dark,
  catppuccin-mocha, gruvbox-dark, nord). Markdown rendering and
  syntect fenced-code highlighting track the active theme.
- **Git integration** — `/status`, `/diff`, `/log`, `/branch`,
  `/commit`, `/stash`, in-app diff viewer.
- **File finder** — `Ctrl+P` or `@path` in the composer; bounded
  previews with per-theme syntect highlighting.
- **Notifications** — OSC 777 always on; `notify-rust` native
  desktop notifications behind the `notify` feature flag.
- **Resilience** — per-call RPC timeouts (bootstrap 3 s, stats 1 s,
  user actions 10 s), terminal panic hook with crash dump persisted
  to the platform state dir, graceful shutdown of the pi child.

## Running

```bash
cargo run --release
```

rata-pi spawns `pi` from `$PATH` by default. Override with:

```bash
cargo run --release -- --pi-bin /path/to/pi
```

If pi isn't available the app still starts in offline mode so you
can inspect the chrome, settings, shortcuts, and themes. Any RPC-
backed toggle flips the local flag and flashes a warning that the
change won't persist past this session.

### pi dependency

Install pi via npm:

```bash
npm install -g @mariozechner/pi-coding-agent
```

## Development

```bash
cargo build                  # debug
cargo build --release        # LTO + opt-level=3, ~5 MiB binary
cargo test --locked --all-features --all-targets
cargo clippy --all-features --all-targets -- -D warnings
cargo fmt -- --check
```

Rust edition `2024`. Project layout under `src/app/` is split across
modules: `mod` (state + dispatch + draw_modal), `draw` (chrome),
`visuals` (per-entry render cache), `events` (reducer for
`Incoming` events), `input` (global key handling), `modals/interview`,
`modals/settings`, and `helpers` for cross-cutting utilities.

## Docs

- [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md) — every feature with
  keybindings, settings, and the plan / interview flows in detail.
- [`PLAN_V3.md`](PLAN_V3.md) — V3 master plan, design decisions, and
  sub-milestone structure.
- [`track_v3_progression.md`](track_v3_progression.md) — live V3
  progression tracker with commit hashes, rolling metrics, and
  deviations.
- [`pi-doc.md`](pi-doc.md) — pointers into the pi coding agent source
  and docs used while building rata-pi.

## License

Copyright (c) olivvein <parcouru_epoque.9b@icloud.com>

Licensed under the MIT license ([LICENSE](./LICENSE) or
<http://opensource.org/licenses/MIT>).
