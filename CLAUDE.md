# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`rata-pi` is a Rust terminal UI (TUI) built with [Ratatui](https://ratatui.rs), currently at the Hello World scaffold stage (`src/main.rs` renders a single `"hello world"` string and exits on any keypress). The intended direction is a TUI for Pi (the `@mariozechner/pi-coding-agent` CLI) — see `pi-doc.md` for filesystem pointers.

## Commands

- Build: `cargo build` (release: `cargo build --release` — `Cargo.toml` configures LTO, `opt-level = "s"`, `strip = true`)
- Run: `cargo run`
- Test: `cargo test --locked --all-features --all-targets` (matches CI)
- Single test: `cargo test <test_name>` or `cargo test --test <file>`
- Format check: `cargo fmt -- --check` (CI enforces)
- Lint: `cargo clippy` (CI uses `clechasseur/rs-clippy-check`)
- Docs: `cargo doc --no-deps --all-features` (CI runs on nightly with `RUSTDOCFLAGS=--cfg docsrs`)

Edition is `2024`; keep this in mind for syntax and lint expectations.

## Architecture

Entry point is `src/main.rs`. The app uses `ratatui::run(app)` which owns the `DefaultTerminal` lifecycle (setup, cleanup, panic hook). Within `app`, the event loop is the standard Ratatui pattern:

1. `terminal.draw(render)` — stateless render closure receives a `Frame`.
2. `crossterm::event::read()` — blocking event read; currently any key press exits.

Errors flow through `color_eyre::Result` at the `main` boundary; the inner `app` returns `std::io::Result` because that's what `ratatui::run` expects from its closure. Keep that split when adding features — put fallible setup in `main`, keep the frame/event loop on `io::Result`.

When extending beyond the hello-world scaffold, the typical Ratatui pattern is to introduce an `App` struct holding state, split `render` into widget-returning methods, and move key handling into an `App::handle_event` method. Reference implementations live outside this repo at `/Users/olivierveinand/Documents/DEV/rataPi/ratatui/examples/apps` (see `pi-doc.md`).

## External references (from `pi-doc.md`)

- Pi coding agent: `/Users/olivierveinand/.nvm/versions/node/v24.14.0/lib/node_modules/@mariozechner/pi-coding-agent` (including `docs/rpc.md` and `examples/`) — consult for the RPC surface this TUI is expected to drive.
- Ratatui source + examples: `/Users/olivierveinand/Documents/DEV/rataPi/ratatui` and `.../ratatui/examples/apps`.
