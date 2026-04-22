# rata-pi — the one-page pitch

> A Ratatui terminal UI for the [Pi coding
> agent](https://github.com/mariozechner/pi-coding-agent). Keyboard-first,
> zero-Electron, ships as a single ~5 MiB binary.

## The problem

The `pi` CLI is great at driving an LLM, running tools, and keeping a
session — but every long exchange has the same friction:

- The scrollback becomes a wall of text. You lose your place.
- Multi-step plans just run. You don't get to veto step 3 before it
  `rm -rf`s anything.
- Toggles like theme, steering mode, thinking level, auto-retry live
  behind a stack of flags you have to remember.
- Prompts you keep re-typing have no home. "Review this PR" goes into
  your notes, or into your mental cache, or into nothing.

## What rata-pi gives you

A full-screen TUI that wraps `pi` without changing it:

- **A transcript that stays readable.** Cards for user / thinking /
  assistant / tool calls. Markdown + syntect fenced-code highlighting
  that tracks a 7-theme palette. Per-entry render cache so long
  sessions don't lag.
- **Plan approval, not plan execution.** When the agent emits
  `[[PLAN_SET: …]]`, rata-pi opens a Review modal. Accept, Edit (add /
  delete / rewrite steps), or Deny. Accepted plans auto-run if you
  leave the toggle on — but *nothing* runs until you've seen the list.
- **Agent interview forms.** When the agent needs structured input,
  `[[ASK_*]]` markers become a modal with Tab navigation, required
  fields, defaults, and a single JSON response block.
- **`/settings` and `/shortcuts`.** Every tunable and every keybinding
  in one scrollable pane — no flag cheat sheet.
- **Transcript search** (`/search`) — live overlay, `n`/`N` walks
  hits, `Enter` focuses the match.
- **Composer templates** (`/template`) — save, reuse, delete named
  prompts. Persisted to disk.
- **Git from the composer** — `/status`, `/diff`, `/log`, `/branch`,
  `/commit`, `/stash`, in-app diff viewer with syntect highlighting.
- **Mouse where it makes sense, keyboard everywhere else.** Click
  modal rows, click Plan Review chips, click outside to close. `Ctrl`
  +`P` for the file finder. `Ctrl+R` for prompt history. `Ctrl+Z`
  undoes in the composer.
- **Resilience.** Per-call RPC timeouts (3 s / 1 s / 10 s). Terminal
  panic hook that restores your shell and writes a crash dump to the
  platform state dir. Graceful pi shutdown on `Ctrl+C`.

## What it is *not*

- Not a pi fork — rata-pi spawns `pi --mode rpc` as a child and
  speaks its existing JSONL RPC.
- Not a chat app for anything other than pi.
- Not Electron. ~5 MiB release binary, one dependency on `pi` from
  `$PATH`.

## Install in one command

```bash
brew install TheSamLePirate/rata-pi/rata-pi
npm install -g @mariozechner/pi-coding-agent
rata-pi
```

Or `cargo install rata-pi`, or a prebuilt tarball from the
[Releases page](https://github.com/TheSamLePirate/rata-pi/releases).

## Who it's for

- You drive pi daily and want your hands to stay on the keyboard.
- You want the agent to propose plans but refuse to let them run
  unreviewed.
- You live in tmux / kitty / WezTerm / Ghostty and don't want another
  desktop app.
- You've been copying the same "review this PR" prompt into a notes
  file for six months. Save it once as `/template save review-pr`.

## Where to go next

- Full manual: [`docs/USER_GUIDE.md`](USER_GUIDE.md).
- Illustrated tour with ASCII mockups: [`docs/FEATURES.md`](FEATURES.md).
- `v1.0.0` announcement: [`docs/ANNOUNCEMENT.md`](ANNOUNCEMENT.md).
- Changelog: [`CHANGELOG.md`](../CHANGELOG.md).
