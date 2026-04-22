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

**`v1.0.0`** — first tagged release. See
[`CHANGELOG.md`](CHANGELOG.md) for the full feature list and
[`PLAN_V4.md`](PLAN_V4.md) for what shipped between V3 and 1.0.

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

## Install

### Homebrew (macOS, Linux)

```bash
brew install olivvein/rata-pi/rata-pi
```

*(tap repo will be populated once `v1.0.0` is published and the
formula in [`Formula/rata-pi.rb`](Formula/rata-pi.rb) has its
SHA256 fields filled.)*

### cargo install

```bash
cargo install rata-pi
```

### Build from source

```bash
git clone https://github.com/olivvein/rata-pi
cd rata-pi
cargo build --release   # target/release/rata-pi
cargo run --release     # run without installing
```

### Prebuilt binaries

Every `v*` tag triggers a [release workflow](.github/workflows/release.yml)
that builds for:

- macOS arm64 (`aarch64-apple-darwin`)
- macOS x86_64 (`x86_64-apple-darwin`)
- Linux x86_64 (`x86_64-unknown-linux-gnu`)
- Windows x86_64 (`x86_64-pc-windows-msvc`)

Download the tarball / zip for your platform from the
[Releases page](https://github.com/olivvein/rata-pi/releases).

## Running

```bash
rata-pi                        # uses `pi` from $PATH
rata-pi --pi-bin /path/to/pi   # explicit path
```

If `pi` isn't available the app still starts in offline mode so you
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

- [`CHANGELOG.md`](CHANGELOG.md) — user-facing release notes.
- [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md) — every feature with
  keybindings, settings, and the plan / interview flows in detail.
- [`PLAN_V3.md`](PLAN_V3.md) / [`PLAN_V4.md`](PLAN_V4.md) — master
  plans, design decisions, sub-milestone structure.
- [`track_v3_progression.md`](track_v3_progression.md) /
  [`track_v4_progression.md`](track_v4_progression.md) — live
  progression trackers with commit hashes, rolling metrics, and
  deviations.
- [`pi-doc.md`](pi-doc.md) — pointers into the pi coding agent source
  and docs used while building rata-pi.

## License

Copyright (c) olivvein <parcouru_epoque.9b@icloud.com>

Licensed under the MIT license ([LICENSE](./LICENSE) or
<http://opensource.org/licenses/MIT>).
