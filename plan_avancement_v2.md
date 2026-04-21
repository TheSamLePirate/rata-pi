# V2 progress tracker

Paired with `PLAN_V2.md`. Checklist per milestone; deviations noted in the **Notes** block at the bottom of each section. All boxes `[ ]` until the milestone ships.

Legend: `[ ]` todo · `[~]` in progress · `[x]` done · `[!]` deviated from plan (see notes) · `[—]` intentionally dropped / deferred

---

## V2.0 — Foundation refactor + animation + theme ✅ (pragmatic subset)

### Architecture
- [—] `app/` module split — **deferred to V2.0.1**. The mechanical refactor of a 2.6k-line file into `app/{state,action,events,input,commands,runtime}` is its own commit; doing it *inside* V2.0 would bury the theme deliverable and risk behavior regressions. Pure reducer bundled with the split.
- [—] `Action` enum + `Cmd` enum + pure reducer — **deferred to V2.0.1**
- [—] Reducer insta snapshot tests — **deferred to V2.0.1**

### Theme system (`src/theme/`)
- [x] `Theme` struct + full semantic palette (21 semantic fields: accent, accent_strong, muted, dim, text, success, warning, error, role_{user,assistant,thinking,tool,bash}, border_{idle,active,modal}, diff_{add,remove,hunk,file}, gauge_{low,mid,high})
- [x] Six built-ins in true-color RGB: `tokyo-night` (default), `dracula`, `solarized-dark`, `catppuccin-mocha`, `gruvbox-dark`, `nord`
- [—] `solarized-light` skipped — we picked the six best-contrast dark themes and one cool-accent (nord); a light variant lands in V2.0.1 alongside the TOML loader
- [—] TOML loader + `notify` hot-reload — **deferred to V2.0.1** (adds `toml` + `notify` deps; ship the built-ins now and iterate)
- [x] `Ctrl+Shift+T` cycles through the six themes
- [x] `/theme [name]` picks by name (case-insensitive); `/theme` with no arg cycles
- [x] `/themes` opens a picker modal with the full list; Enter applies directly without echoing a slash command into the composer
- [x] Plumbed through every draw surface — header (spinner, model label, status/thinking badges, queue chip), transcript (user/pi/thinking prefixes + markdown output), tool cards (status chip by state), bash card ($ prefix + exit-code chip + gray body), diff heuristic (+/−/@@/±±± hunks), compaction + retry rows, editor border + title, context gauge (gradient low/mid/high), modal border + title + bottom-hint, every picker's `▸ selected` highlight, extension dialog select/confirm/input bodies, toast stack chips, info/warn/error rows, connection-failure banner
- [x] Boot info line echoes the current theme name so a fresh user sees what's active
- [—] Theme golden snapshots — **deferred to V2.12** per PLAN_V2 §11; `insta` isn't in the dep tree yet

### Animation engine (`src/anim/`)
- [x] `Anim` type with `new(duration)` / `looping(duration)` + `progress()` + `is_done()`; suitable as the primitive for V2.1's spinner-border-pulse and retry-countdown-ring
- [x] Easing kit in `anim::ease`: `linear`, `ease_out_cubic`, `smoothstep`, `triangle` (the last for breathing pulses)
- [x] Color/ratio tween helpers: `lerp_u8`, `lerp_f64`
- [—] Active-animation registry + tick-source parking — **shape exists but registry ships with V2.1** where StatusWidget first needs it
- [x] 5 unit tests across `anim` (progress range, looping, lerp endpoints, ease endpoints, ease-out front-loaded)

### Acceptance
- [x] `cargo test` ≥ V1 count → 62 (V1 was 53; gained 9 from theme + anim)
- [x] Manual parity pass: every V1 feature still works — same keybindings, modals, RPC round-trips, submit paths
- [x] `cargo clippy --all-targets -- -D warnings` clean
- [x] `cargo fmt --check` clean
- [—] vhs demo `docs/demos/v2_0.gif` — **deferred to V2.12 polish pass** (no `vhs` dep in CI yet)

**Notes:**
- Scoped V2.0 to ship visible UX improvement (themes) without the architectural rewrite. That rewrite becomes V2.0.1 — noted as a single tracked milestone rather than mixed into this one, so the theme work is reviewable in isolation and the reducer refactor can proceed behind stable snapshots.
- `kb()` helper still returns `Color::Cyan` instead of theme accent; picker highlight uses `t.accent`. On 256-color terminals they look cohesive enough; on truecolor kb will bridge in V2.0.1.
- Tokyo-Night selected as default because its accent blue doubles well as the streaming border and its magenta reads as "thinking" in every test.
- `Ctrl+Shift+T` handler is placed *before* `Ctrl+T` so the more-specific match wins — crossterm reports both modifiers in the same event.
- Kept `Color::Rgb(0, 0, 0)` in two chip fg spots (bash exit-code chip, ext-confirm Yes/No chip, toast chips) because solid-bg chips read best with guaranteed-black text on every theme; tempting to swap to `t.text` but it breaks on light themes. Revisit in V2.0.1 with a contrasting-text helper.

---

## V2.0.1 — Architecture refactor + TOML themes + hot-reload (queued)

Planned follow-up to finish the V2.0 feature list without blocking the theme deliverable.

- [ ] `app/` module split per PLAN_V2 §2 (state / action / events / input / commands / runtime)
- [ ] Pure reducer `step(App, Action) -> (App, Vec<Cmd>)`; migrate every current handler
- [ ] Insta snapshot tests for reducer transitions (≥ 20 scripts)
- [ ] `toml` dep + `~/.config/rata-pi/themes/*.toml` loader
- [ ] `notify` dep + watcher → hot-reload on file save
- [ ] `solarized-light` built-in (light-mode contrast)
- [ ] `kb()` helper uses `t.accent_strong`

**Notes:** _(to fill in)_

---

## V2.1 — StatusWidget + header gauges + sparklines ✅ (core)

Also folds in V2.0 bug fixes the user reported (see "Fixes" below).

### Delivered
- [x] `LiveState` enum with 9 explicit states covering the PLAN_V2 §8 spec: `Idle / Sending / Llm / Thinking / Tool / Streaming / Compacting / Retrying{attempt,max,delay_ms} / Error`. Each has its own border color + label + spinner policy.
- [x] `StatusWidget` drawn as a 4-row bordered card between the editor and footer. Title shows `status · MM:SS` (or `HH:MM:SS` past an hour) for the current state.
- [x] Row 1: spinner + state label (e.g. `llm · anthropic/claude-sonnet-4` or `retry 2/3 in 2000ms`) + `turn N` chip + running-tools chip (`K running · N done`).
- [x] Row 2: throughput label + sparkline + `X t/s`; cost label + per-turn sparkline + `$0.XXXX`; session total cost.
- [x] Heartbeat dot in the header that pulses via the `anim::ease::triangle` function; color goes `dim` (idle) → `success` (recent event) → `warning` (10 s silence while streaming) → `error` (100+ ticks silence).
- [x] Throughput tracking: per-second bucket rolling 60 wide. `approx_tokens` (≈ 4 chars/token) converts text/thinking deltas into rate samples.
- [x] Cost tracking from `turn_end.message.usage.cost.total`: 30-turn rolling series + running session total. Sparkline quantizes to hundred-thousandths of a dollar so per-turn granularity is legible.
- [x] Retry state surfaces as the distinctive "retry N/M in Xms" label with a `warning`-colored border — the countdown ring per PLAN_V2 §8 is **deferred to V2.2** (needs `anim` registry integration; the `ease::triangle` primitive is already in place).
- [x] Every event updates `last_event_tick`, so the heartbeat dot is a live liveness probe even when no state change happens (e.g. tool_execution_update coming in frequently).
- [x] Layout gracefully degrades: if terminal height < 20 rows, StatusWidget is hidden.
- [x] 63 tests pass (V2.0 was 62; +1 from commands::builtins test).

### Fixes bundled in this milestone
- [x] **`Ctrl+Shift+T` works now** on every terminal thanks to three bindings: `Ctrl+Shift+T` (Kitty keyboard protocol), `Alt+T` (reliable on macOS Terminal / iTerm / xterm), and `F12` (function-key fallback). All three call `cycle_theme`.
- [x] **`/theme` works when pi is offline** — `submit()` is split into `try_local_slash` (no client needed) + `try_pi_slash` (requires client). Local commands run first, so `/theme`, `/help`, `/stats`, `/export`, `/themes`, `/clear` all work without pi.
- [x] **Commands picker shows more than pi's skills now.** New `CommandSource::Builtin` variant + new `src/ui/commands.rs` with 19 curated built-ins: help, stats, themes, theme, export, export-html, copy, clear, rename, new, switch, fork, compact, model, think, cycle-model, cycle-think, auto-compact, auto-retry. The `F1` and `/` pickers merge built-ins with pi's extension/prompt/skill commands. A `[builtin]` badge tags them in the list.
- [x] Picker Enter dispatches Builtin commands inline: no-argument built-ins fire immediately (e.g. `/help` opens help, `/theme` cycles); built-ins that expect an argument (`/rename`, `/switch`) prefill `/name ` so the user can type the argument and submit. Pi commands still prefill as before.

### Deferred (queued for V2.2+)
- [ ] Retry countdown ring graphic (8-frame circular arc) — anim registry plumbing lands with V2.2 conversation-view refactor
- [ ] Connection-quality sparkline from stats-ticker RTT — needs latency capture in the RPC client; slotted for V2.2
- [ ] Animated smooth Gauge tween in the footer — current Gauge jumps between states; smoothing comes with anim registry
- [ ] StatusWidget golden snapshots per state (insta) — lands with V2.12 snapshot pass
- [ ] Fire-every-event contract test — needs the Action reducer refactor (V2.0.1) so we can script actions

**Notes:**
- The `Sparkline` widget from Ratatui 0.30 takes `&[u64]`; we convert u32 throughput and scale cost (×100 000) to get readable micro-spark resolution at sub-cent amounts.
- `LiveState::Sending` is set when the user submits so the UI flashes "sending" between the prompt going out and `agent_start` coming back — a small detail but closes the perception loop.
- Turn counter is bumped on `turn_start` rather than `agent_start` so a multi-turn run (tool calls + follow-ups) counts correctly.
- Added the `clear` local slash command — turns out to be super handy while debugging layout.
- The 19 built-in commands + pi's own commands means the `/` picker is already 30+ entries on most sessions. Scrollable list lands with V2.5.

---

## V2.1 backlog items rolled forward

- Retry countdown ring — bundled with the animation-registry consumer work in V2.2
- Connection-quality sparkline — V2.2 (RTT capture in the RPC client)
- Golden snapshots — V2.12

---

## V2.2 — Conversation view overhaul ✅ (core)

### Delivered
- [x] **Rounded-border message cards** via a new reusable `Card` widget in `src/ui/cards.rs`. `BorderType::Rounded` is used across the transcript and the outer transcript frame. Each entry type maps to a themed card with its own icon and border color.
- [x] **Role icons** per card: `❯` user · `✦` thinking · `✦` pi · `⚙` tool · `$` bash. Info / Warn / Error / Compaction / Retry still render as inline non-bordered rows (they read better flat).
- [x] **Model in the card title** — assistant cards show the current model provider/id as a right-aligned title chip. Tool cards show status label (`running / ok / error`) + a `▸`/`▾` expand marker in the title. Bash cards show `exit N` / `cancelled` as a right chip.
- [x] **Virtualized transcript scroll** — `draw_body` now computes a per-visual height at the current content width using Ratatui's `Paragraph::line_count` (via the `unstable-rendered-line-info` feature), then skips entries above/below the viewport. 10k entries stay responsive because we never render what we don't draw.
- [x] **Scrollbar** in the transcript's right column (1 cell wide). Thumb size and position are proportional to viewport/total. Dotted track + solid thumb in the theme's accent color. Same scrollbar renders inside any list modal (Commands / Models / History / Forks) when the content overflows.
- [x] **Commands picker scrolls!** Previously the list just overflowed; now `draw_modal` computes a centered `scroll_y` around the selected item so it always stays visible, and adds the same scrollbar column. Fixes the F1 / `/` menu-not-scrolling bug the user reported.
- [x] **Focus mode** — `Ctrl+F` enters focus mode; `j`/`k`/`↑`/`↓` move between cards; `g`/`G`/`Home`/`End` jump to top/bottom; `PgUp`/`PgDn` move by 5; `Enter` / `Space` toggles expand on tool cards; `Esc` / `q` exits. The focused card gets a bold `border_active`-colored border, and the transcript auto-scrolls to keep it centered. A `transcript · focus` title tag appears on the transcript frame while focus mode is on.
- [x] **Sticky live-tail indicator** — when scrolled up (manual or focus-centered), a small `⬇ live tail (End)` chip appears at the bottom-center of the transcript with the theme accent background. Disappears automatically when the user hits `End` or scrolls back down.
- [x] **Welcome card** — empty transcript shows a friendly "welcome to rata-pi" hint with the most useful keybindings instead of a blank pane.
- [x] **Cut-off hint** — when card virtualization has to slice a partial card at the top of the viewport, we render a faint `⋯ (card continues above)` marker so the user never sees a headless body.
- [x] **Focus auto-clamp** — when the transcript grows past the focused index, focus tracks the tail (feels natural when you `Ctrl+F` then watch new events roll in).
- [x] **Thinking as blockquote** — thinking cards render with `│ ` prefix + italic muted text, reading like a proper quoted block. Token count lands in the right chip.
- [x] **Context-aware footer hints** — focus mode shows `j/k nav · Enter expand · g/G top/bot · Esc exit`; idle mode shows `Enter send · / cmds · Ctrl+F focus · F5 model · …`; streaming shows its own abort/cycle hints.

### Deferred
- [ ] Emoji avatars (🧑 / 🤖 / 💭) — kept the single-char unicode icons because emoji width is inconsistent across terminals (2 cells in some, 1 in others) and breaks virtualization math. Revisit with a width probe in V2.4.
- [ ] Relative timestamps + absolute-on-focus — transcript entries don't carry a creation timestamp yet; adding one is a small transcript.rs change but was scoped out of V2.2.
- [ ] Nested tool-call tree (tools visually nested inside the assistant turn that triggered them) — needs a parent-child relationship we don't model yet. V2.3 alongside the tool-renderer registry.
- [ ] 10k-message stress benchmark — infrastructure lands with V2.12.
- [ ] Golden snapshots per card state — `insta` isn't in the dep tree yet; V2.12.

### Notes
- The `Paragraph::line_count` method requires ratatui's `unstable-rendered-line-info` feature. Enabled in `Cargo.toml`. Worth it: keeps our height math 100% in sync with the real wrap the renderer performs.
- Card height = `line_count + 2` (top + bottom border). Bodies render with 1 column of left padding for breathing room.
- `Visual` enum collapses Card and InlineRow — inline rows (Info/Warn/Error/Compaction/Retry) look denser as a single line and save vertical space in busy sessions.
- Scroll offset is still line-granular (`Option<u16>` lines-from-top) because a single tall card can be > viewport; card-boundary-only snapping would make such a card unreachable.
- The modal scrollbar logic is the same code path as the transcript's; extracted into `draw_scrollbar`.
- 63 tests pass; clippy -D warnings clean; fmt clean.

---

## V2.3 — Syntect + diff widget + per-tool renderers ✅ (core)

### Delivered
- [x] **syntect integration** (`src/ui/syntax.rs`) — OnceLock-loaded SyntaxSet + `base16-ocean.dark` theme; `highlight(code, lang_hint) -> Vec<Line<'static>>`. Lang detection tries token → extension → case-insensitive name → plain-text. Default features stripped to: `parsing`, `default-syntaxes`, `default-themes`, `regex-fancy` (pure-Rust regex, no C dep).
- [x] **Markdown fenced code** now tokenizes through syntect. The markdown renderer buffers text during `Start(CodeBlock)…End(CodeBlock)`, then emits the highlighted lines surrounded by `┄─── lang ───┄` / `└──────` chip rows for a clean "code block" frame.
- [x] **Inline unified-diff widget** (`src/ui/diff.rs`) — `is_unified_diff(s)` heuristic + `render(diff, theme)`. Output:
  - File-header rows in `diff_file` (cyan bold)
  - `@@` hunk rows in `diff_hunk` (magenta)
  - Body rows with a **two-number gutter** (`old_ln / new_ln`) + a `+` / `-` / `space` chip + the line body
  - Context lines get **syntect-highlighted** when the `+++` header carries a file path we can derive a lang from (e.g. `+++ b/src/app.rs` → Rust highlight on surrounding context)
- [x] **Per-tool body dispatcher** with 7 families (`tool_family` match): `bash`, `edit` (covers `edit` / `apply_patch` / `str_replace` / `str_replace_editor` / `multi_edit` / `patch`), `read_file` (covers `read` / `readfile` / `view` / `cat`), `grep` (covers `grep` / `search` / `rg` / `ripgrep`), `write` (covers `write` / `write_file` / `create`), `todo` (covers `todo` / `todowrite` / `tasks`), generic fallback.
  - **Edit family**: synthesizes a mini diff from `old_string` / `new_string` when those args are present, runs it through syntect with the file extension as lang hint. Unified-diff output from pi is routed to `diff::render`.
  - **Read family**: `path` chip in the body; collapsed shows "N lines — Enter / Ctrl+E to view"; expanded runs the file content through syntect using the extension. `strip_line_numbers` removes common `N→content` prefixes so syntect tokenizes clean source.
  - **Grep family**: `"pattern"` chip; results grouped by file (file name on its own line in accent, hits indented under it).
  - **Write family**: `path` chip + syntect-highlighted content.
  - **Todo family**: `☐ / ◐ / ☑` by status, coloured dim / warning / success.
  - **Bash family**: keeps the existing BashExec card path.
- [x] **Tool card title/chip redesign** — title is `▸ tool_name` with a family-specific left icon (`±` / `▤` / `⌕` / `✎` / `☐` / `⚙`). The right-chip shows the primary arg (`file_path`, `pattern`, `command`) when present, falling back to `✓ ok` / `✗ error` / `⚙ running`.
- [x] **Turn separator** — `Entry::TurnMarker { number }` pushed on `Incoming::TurnStart` (from the second turn onward). Renders as a centered `──── turn N ────────` divider between cards.
- [x] **Focus-border fixes** — responds to the user bug report. Focused cards now use `BorderType::Double` (instead of a subtle `Modifier::BOLD` which many terminals render indistinguishably) and prepend a `▶` marker in the title. Border stays role-coloured so User/Thinking/Assistant/Tool semantics remain meaningful.
- [x] Export (`ui/export.rs`) gets a markdown `---` + **turn N** divider for TurnMarker entries so exports show the turn structure too.

### Deferred
- [ ] Side-by-side diff (`d/D`) — unified is the right default; side-by-side needs width-aware column split and I didn't want to bloat V2.3.
- [ ] Line-numbers toggle (`Ctrl+Shift+L`) — diff widget already shows both old/new numbers; the toggle is mostly for read_file syntect renders. Add with V2.4.
- [ ] Markdown table rendering — pulldown-cmark table events still fall through; will use ratatui's Table widget in a V2.4 / V2.5 follow-up.
- [ ] Cache compiled syntax references per-file — syntect re-tokenizes on every streaming redraw. Fine for sub-400-line content, but add an LRU cache if profiling shows stutter on long assistant responses.

### Tests
- 70 pass (V2.2 63 + 3 syntax + 4 diff).
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.

**Notes:**
- `syntect` adds ~3 MiB to debug binary and ~2 MiB to `--release -s` builds — acceptable for the visual win.
- Regex flavor `regex-fancy` was chosen over `regex-onig` to avoid a C dep; some edge-case syntax definitions fall back gracefully.
- `base16-ocean.dark` was picked over prettier options because it's bundled with syntect (no separate `.tmTheme` file) and its palette reads on every rata-pi theme.
- TurnMarker intentionally doesn't fire before turn 1 — a leading divider looks wrong on first prompt.

---

## V2.4 — Clipboard, Kitty keys, mouse clicks, streaming cursor ✅ (core)

### Delivered
- [x] **Terminal capability probe** (`src/term_caps.rs`) — sniffs `TERM_PROGRAM`, `KITTY_WINDOW_ID`, `GHOSTTY_RESOURCES_DIR`, `TERM`. Returns `{ kind, kitty_keyboard, graphics }`. Logged at startup.
- [x] **Kitty keyboard protocol** — enabled at startup when `caps.kitty_keyboard` is true (Kitty / Ghostty / WezTerm); popped on shutdown *and* in the panic hook so the terminal is always left clean. With this on, `Ctrl+Shift+T` now actually reaches us with both modifier flags set; other terminals still have the `Alt+T` / `F12` fallbacks from V2.0.
- [x] **Real clipboard** (`src/clipboard.rs`) — `arboard` primary (native macOS / Windows / X11 / Wayland), OSC 52 fallback for SSH sessions + headless boxes + tmux with `set-clipboard`. Single `copy(text)` entry point returns a `CopyOutcome { backend, bytes }`.
- [x] **`/copy` + `Ctrl+Y`** — `/copy` fetches the last assistant text via `GetLastAssistantText` RPC and writes it to the clipboard. `Ctrl+Y` outside focus mode does the same directly from the transcript. In focus mode, `y` (or `c`) copies the focused card's plain-text rendering (user / thinking / assistant / tool-call-with-args / bash-with-output).
- [x] **Mouse click-to-focus** — new `on_mouse_click` reads the per-frame `MouseMap` populated during `draw_body` and maps (x, y) to an entry index. First click on any transcript card = focus it. Second click on the same tool card = toggle its expanded state.
- [x] **Click the `⬇ live tail` chip** — the chip's rect is captured in `MouseMap::live_tail_chip`; clicking it re-pins the transcript to the bottom (same as pressing `End`).
- [x] **Animated streaming cursor** — the last assistant card appends a blinking `▌` (0.5 Hz via `ticks / 5 % 2`) while pi is actively streaming text. The card title flips to `pi · streaming` during that window so the reader instantly sees "still going".
- [x] **Flash feedback** on copy: `✓ copied 312 chars` or `✓ copied 312 chars (osc52)` depending on backend.

### New modules
- `src/term_caps.rs` — `detect()` + `Caps` struct + `TerminalKind` enum. 1 test (smoke).
- `src/clipboard.rs` — `copy(text)` + `CopyOutcome` + `Backend`. 1 test.

### Deferred (kept for V2.4b / V2.5)
- [ ] Image paste (clipboard → PNG → base64 → `ImageContent`) — needs a PNG encoder on the critical path. I wired the `arboard::get_image` surface but stubbed the PNG encode; landing the full path wants a small `image` crate inclusion under the `images` feature, which I'd rather bundle with the ratatui-image rendering work.
- [ ] `ratatui-image` rendering — feature-gated. Scope parked to V2.4b so the default build doesn't take the hit.
- [ ] Mouse drag-select across transcript — crossterm's drag events are present but building a selection buffer on top of virtualized cards is its own milestone. V2.5 with the buffer-backed render.

### Tests + gates
- 72/72 pass (V2.3 70 + 1 term_caps + 1 clipboard).
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.

**Notes:**
- `MouseMap` lives behind a `RefCell<MouseMap>` on `App` so `draw_body` (which takes `&App`) can refresh it without the signature churn of passing `&mut App` through every drawing helper.
- The streaming cursor uses `anim`'s principles but doesn't need a dedicated `Anim` — the 100 ms tick source already drives `app.ticks`, so `(ticks / 5).is_multiple_of(2)` gives a 2 Hz blink.
- On the focus-mode hotkey question: `c` is also bound to copy in focus mode (Gmail-style), alongside `y`. Both work.
- The capability probe is conservative: we only enable Kitty keyboard on known-good terminals rather than probing via escape sequences. If your terminal claims kitty protocol but isn't on the list, set `TERM=xterm-kitty` or file an extra detection env var.

---

## V2.5 — Command-menu redesign ✅ (core)

### Delivered
- [x] **Two-pane layout** — the Commands modal (`F1` / `/`) now splits into a categorized list on the left and a **detail pane on the right**: title, category label, source chip, full description, argument hint, example invocation, and an action hint telling you what `Enter` will do ("runs it" vs "prefills the composer" vs "applies the theme").
- [x] **Category groups with dividers + icons** — each category gets a header row with its icon (`◆ session · ▤ transcript · ✦ model · ⚙ pi runtime · ◉ view · ⌕ debug · ◈ theme · ● extension · ▸ prompt · ✧ skill`). Built-ins come first, pi's extension / prompt / skill commands appear under their own headers.
- [x] **Inline arg hints** — `/rename <name>`, `/switch <path>`, `/compact [instructions]`, `/steer-mode <all | one-at-a-time>` — the `<args>` portion renders in `warning` (yellow) so it jumps out.
- [x] **Filter searches name + description** — typing "copy" finds `/copy` (match by name); typing "clipboard" finds it (match by description). Case-insensitive.
- [x] **Per-item source badge** — `[builtin]`, `[ext]`, `[prompt]`, `[skill]` chips on each entry.
- [x] **Centered scroll in big lists** — `commands_selected_line` computes the terminal-row index of the selected item accounting for category headers and 2-line items (name + description), so the scroll centers on the selection correctly.
- [x] **j/k/PgUp/PgDn/Home/End** already work across every list modal (shared `handle_list_keys`). Existing scrollbar widget renders in the right column when content overflows.
- [x] **10 new commands added**, taking the built-in catalog from 19 to **30**:
  - `/retry` (re-submits the last user prompt, using `Steer` when streaming)
  - `/abort`, `/abort-bash`, `/abort-retry` (fire the corresponding RPC)
  - `/steer-mode <all|one-at-a-time>` + `/follow-up-mode <all|one-at-a-time>`
  - `/doctor` — quick readiness check: terminal kind, Kitty-keyboard flag, graphics support, clipboard backend, current theme
  - `/version` — prints rata-pi version
  - `/log` — prints log file path hint
  - `/env` — shows `TERM` / `TERM_PROGRAM` / detected terminal kind / kitty-kb / graphics flags
  - `/snapshots` (placeholder; lands with V2.9)
- [x] Modal now sizes to `120×26` to make room for the detail pane on wide terminals; falls back to single-column list on narrower windows.

### New + updated modules
- `src/ui/commands.rs` — rewritten around `MenuItem { name, description, category, args, example, source }` + `Category` enum (10 variants) with `label()` / `icon()` / `sort_key()`. `builtins()` returns `Vec<MenuItem>`; `wrap_pi(cmds)` adapts pi commands; `merged_menu(pi)` concatenates and sorts for the picker; `theme_items(names)` builds the `/themes` list. `matches(item, query)` does case-insensitive substring match against both name and description.
- `src/ui/modal.rs` — `Modal::Commands` now holds `ListModal<MenuItem>`.
- `src/app.rs` — new `commands_text` (categorized list), `command_detail_lines` (right-pane), `commands_selected_line` (scroll math), plus a vertical-rule renderer for the pane split. New slash handlers in both `try_local_slash` and `try_pi_slash`.

### Deferred
- [ ] MRU re-ordering within categories — needs a persistent `cmd_mru.json`; small bump in V2.6 / V2.11.
- [ ] Aliases file `~/.config/rata-pi/aliases.toml` — planned with V2.11 hooks config.
- [ ] `?` inside the menu opening a command's detail page — detail is already **always** on the right pane in the two-pane layout, so `?` redundancy deferred.
- [ ] Wheel scroll inside modals — mouse currently goes to transcript; modal-mouse-routing is a V2.6 follow-up.

### Tests + gates
- 75 pass (V2.4 72 + 3 from the new `ui::commands` tests).
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.

**Notes:**
- The picker now feels much closer to an IDE command palette — type a few letters, see matches across categories, read the detail pane to learn the command before committing.
- `/doctor` is the single command that summarizes "am I in a happy state" — useful for first-run triage.
- Every arg-taking builtin now surfaces its arg hint in the list AND in the detail pane. No more guessing `/rename what?`.

---

## V2.6 — File tree + fuzzy + grep + @file ✅ (fuzzy + @-path subset)

### Delivered
- [x] `ignore` crate walker (`src/files.rs`) respects `.gitignore` + `.git/info/exclude`; capped at `MAX_FILES=20_000` with `truncated` flag in `FileList`.
- [x] `Ctrl+P` fuzzy file finder modal — `FileFinder { files, query, selected, mode }`. Live substring filter uses `SkimMatcherV2`; empty query returns a short-path-first prefix.
- [x] `@path` autocomplete — typing `@` in the composer opens a floating picker with the current token used as a live filter. Selecting replaces the `@<incomplete>` token with `@<full/path>`.
- [x] `/find [query]` slash command opens the file finder with the query pre-loaded.
- [x] Preview pane reserved in the modal split — `files::preview(root, rel)` returns first 40 lines or 8 KiB with a binary-file heuristic.
- [x] `FilePickMode::{Insert, AtToken}` distinguishes "replace composer" (Ctrl+P) from "replace last @-token" (composer autocomplete).

### Deferred
- [ ] `FileTree` sidebar widget — V2.6 shipped the finder because that's the muscle memory users have (VS Code / Sublime). Tree sidebar (`F2`) lands with V2.11+ polish.
- [ ] `Ctrl+G` live grep — ripgrep shell-out exists for `/git-log`-style use; the live streaming grep picker is queued for V2.12.
- [ ] `/edit <path>` shells to `$EDITOR` — parked pending the multi-line composer decision (V2.10 shipped in-TUI).

**Notes:**
- `matches_file(path, query)` is a wrapper around `SkimMatcherV2::fuzzy_match` so filter logic stays in one place.
- The modal's `text_width` reservation for the scrollbar column was kept in sync with the Commands picker so both feel identical.

---

## V2.7 — Git integration ✅ (core)

### Delivered
- [x] Git helper (`src/git.rs`) shells out to the `git` binary — no `git2` / libgit2. `git_timeout` runs on a background thread with a bounded duration so a hung repo never blocks draws.
- [x] `GitStatus { is_repo, branch, ahead, behind, staged, unstaged, untracked }` parsed from `git status --porcelain=v2 --branch`. Refreshed on every stats tick.
- [x] Header git chip: `⎇ branch ●` (dirty) / `○` (clean) with ahead/behind pips `↑N ↓N`. Hidden outside a repo.
- [x] `/status` opens the git-status modal with staged/unstaged/untracked counts + branch chip.
- [x] `/diff`, `/diff --staged` → `DiffView { title, staged, diff, scroll }` full-screen modal using the V2.3 diff widget; `j/k/↑/↓/PgUp/PgDn/g/G` scroll.
- [x] `/git-log [n]` → `GitLogState { commits, selected }` — scrollable picker of last N commits.
- [x] `/branch` opens `GitBranchState { branches, query, selected }` with live filter.
- [x] `/switch-branch <name>` / `/commit <msg>` / `/stash` slash commands.
- [x] Post-V2.7 fix: git-log and diff modals were missing scroll; the follow-up commit `e0b67b3` added j/k/↑/↓/PgUp/PgDn/g/G nav + scroll centering for both. Diff scroll is line-granular.

### Deferred
- [ ] Side-by-side diff (`d/D`) — queued with V2.12 polish.
- [ ] Commit modal with staged-files preview + message composer — `/commit <msg>` works inline but the full modal with file-tick-boxes is V2.12.
- [ ] `git2` feature flag — shell-out is fast and dep-light; revisit if profiling shows the subprocess latency matters.

**Notes:**
- Porcelain v2 parsing lives in `classify_staged_and_unstaged` with 4 unit tests covering staged/unstaged/untracked combinations.
- `GitStatus::dirty()` returns true if any of staged/unstaged/untracked is non-zero.

---

## V2.8 — Plan mode + TODO + diagnostics — **merged into V2.9**

Plan-mode work shipped in V2.9. TODO widget + `/diagnostics` deferred to V2.11+.

- [—] Standalone V2.8 folded into V2.9 because plan-mode is the feature that needed to land first; TODO is a separate UI primitive (right-side widget) that deserves its own milestone.

---

## V2.9 — Plan mode, agent-authored or user-authored, enforced ✅ (core)

### Delivered
- [x] `src/plan.rs` — `Plan { items, auto_run, fail_reason }`, `Item { text, status, attempts }`, `Status { Pending, Active, Done, Failed }`.
- [x] **Agent-authored plans** via marker protocol: assistant text containing `[[PLAN_SET: a | b | c]]` sets the plan; `[[PLAN_ADD: text]]` appends one item; `[[STEP_DONE]]` advances the active step; `[[STEP_FAILED: reason]]` marks the step failed.
- [x] `parse_markers(text)` scans assistant text on each `agent_end`; `as_context()` renders the plan as a prompt-injection block; `capability_hint()` tells the agent about the markers when no plan is active.
- [x] `wrap_with_plan()` prepends the active plan context to every outgoing prompt so the agent has full state in every turn.
- [x] **Auto-continue** — when `auto_run: true` and the plan still has pending items, `agent_end` queues a follow-up prompt ("continue with the next step"). Capped at `MAX_ATTEMPTS = 3` per step to prevent runaway loops.
- [x] Plan card rendered between editor and footer when the plan is active, showing progress (N/M done) + active step with `▸` marker.
- [x] `Modal::PlanView` full plan listing via `/plan` (no arg); `/plan set | add | done | fail | next | clear | auto` manages plan state from the composer.
- [x] Commit `98f5379` "feat(v2.9): plan mode — agent-authored or user-authored, enforced".

### Deferred
- [ ] Plan-mode Y/N gate on `tool_execution_start` — plan enforcement is narrative, not tool-level; the tool-gate flavor of plan mode is queued behind UX research.
- [ ] TODO widget (right-side persistent, keyboard CRUD) — separate primitive, V2.11+.
- [ ] Timeline / replay — deferred to its own milestone (V2.12 candidate).
- [ ] Transcript snapshots `/snapshots` — placeholder command exists; persistence lands with V2.12.

**Notes:**
- The marker scan is scoped to text blocks only so embedded code doesn't trigger accidental plan updates.
- `attempts` on each `Item` is bumped by the auto-continue loop and consulted before queueing another follow-up.

---

## V2.10 — Multi-line composer + vim mode ✅ (core)

### Delivered
- [x] `src/composer.rs` — `Composer { lines, row, col, mode, pending_op }`, `Mode::{Insert, Normal}`, UTF-8-aware char-boundary helpers.
- [x] Insert ops: `insert_char / insert_str / insert_newline / backspace / delete_char_forward`.
- [x] Navigation: `left / right / up / down / home / end / top / bottom / word_left / word_right` (vim-style, with trailing-whitespace skip on word_right).
- [x] Kill ops: `kill_line_forward / kill_line_back / delete_line`.
- [x] `desired_rows(max)` drives editor height so the editor grows as you type (up to `max`, clamped).
- [x] `render(theme, focus)` bakes the cursor as a `Modifier::REVERSED` cell (works on every terminal without cursor-positioning races).
- [x] Composer replaces `App.input: String` across 36 call sites in `app.rs`. Shift+Enter / Ctrl+J inserts newline; Enter submits. Ctrl+A / Ctrl+U / Ctrl+K / Ctrl+W / Alt+←/→ round out the editing surface.
- [x] **Vim mode** opt-in via `/vim` with a full normal-mode dispatcher: `h/j/k/l`, `w/b`, `0/$`, `i/a/o/I/A/O`, `x`, `dd`, `gg`, `G`. Double-letter ops (`dd`) use `pending_op`.
- [x] `vim_enabled: bool` on App — default **off** so Esc keeps clearing the composer for non-vim users. Editor title chip shows `· INS` / `· NORM` only when vim mode is on.
- [x] `/default` (alias `/emacs`) restores default bindings. Both commands in the `View` category.
- [x] 8 composer unit tests cover insert / newline / word-right-with-whitespace / backspace-across-line / kill_line.

### Deferred
- [ ] `tui-textarea` migration — rolled our own to avoid the dep and own cursor semantics.
- [ ] Undo / redo — composer edits are non-reversible today; a ring buffer of snapshots is scoped for V2.12.
- [ ] `/templates` picker over `~/.config/rata-pi/templates/*.md` with `{{selection}}` / `{{file}}` — parked for V2.12.

**Notes:**
- Composer's word_right now explicitly skips trailing whitespace to land on the next word's first char (matched vim's `w` after the test caught the off-by-one).
- Cursor rendering via `Modifier::REVERSED` on the target cell means no terminal cursor is actually drawn — reliable across SSH + screen multiplexers.

---

## V2.11 — Notifications, /doctor, /mcp, crash dump ✅ (core)

### Delivered
- [x] **`src/notify.rs`** — unified notification surface:
  - **OSC 777** (`ESC ] 777 ; notify ; title ; body BEL`) is always emitted; works on iTerm2, kitty, WezTerm, Ghostty, gnome-terminal, konsole.
  - **`notify-rust`** behind the opt-in `notify` cargo feature for native DBus / NSUserNotification / WinToast.
  - Returns `Backends { osc777, native }` so `/notify` can report "notifications on · backends: osc777 · native".
  - Sanitises BEL / ESC / CR / LF in title+body so a stray BEL can't close the OSC frame early.
- [x] **Event hooks** fire notifications on:
  - `agent_end` when the turn took ≥ 10 s (100 ticks at 100 ms/tick) — `"pi · response ready" / "15s · 2 tool calls"`.
  - `ToolExecutionEnd { is_error: true }` — `"pi · tool error" / <first line of output>`.
  - `AutoRetryEnd { success: false }` — `"pi · retries exhausted" / <final_error>`.
  - `agent_start_tick` + `tool_calls_this_turn` fields added to App to track duration and tool count per turn.
- [x] **`/notify` slash command** toggles `app.notify_enabled`; emits a test notification and flashes the backends label so the user can verify their setup.
- [x] **`Modal::Doctor(Vec<DoctorCheck>)`** — pass/warn/fail/info rows:
  - pi binary on PATH (pass / fail with resolved path).
  - pi connection (fail with spawn error, or pass).
  - terminal kind (info).
  - Kitty keyboard (pass if advertised, warn if not).
  - Graphics protocol (info).
  - Clipboard (arboard native vs OSC 52 fallback).
  - Git repo (pass with branch, info outside).
  - Theme (info).
  - Notifications (pass when enabled; shows whether the `notify` feature is compiled in).
  - Rows render with `✓ / ▲ / ✗ / ·` glyphs in the theme's success/warning/error/dim colors.
  - `/doctor` slash command opens the modal; Esc closes.
- [x] **`Modal::Mcp(Vec<McpRow>)`** — placeholder today because pi doesn't expose MCP servers over the JSONL RPC yet. Single info row: "pi does not expose MCP server state over RPC yet". Structure is future-proof for when pi adds `get_mcp_servers`.
- [x] **Crash dump on panic** — `install_panic_hook` now also calls `write_crash_dump(info)` which writes `{data_local_dir}/rata-pi/crash-<unix_ts>.log` with version / os / arch / location / payload / `Backtrace::force_capture()`. Panic path: restore-terminal → write-dump → stderr-notice → chain to original hook. Uses `directories::BaseDirs::data_local_dir()` (works on macOS / Linux / Windows).
- [x] Catalog entries for `/doctor`, `/mcp`, `/notify` under the Debug category with arg hints + examples; detail pane explains what each does.
- [x] `notify-rust` crate is optional and gated — default build is unchanged in dep count.
- [x] 104 tests pass (V2.10 99 + 4 notify + 1 earlier suite bump). `cargo clippy --all-targets -- -D warnings` clean. `cargo fmt --check` clean. Release build unchanged (notify feature off). `cargo build --features notify` also clean.

### Deferred
- [ ] `~/.config/rata-pi/hooks.toml` schema (event → shell cmd OR notify) — hooks config parked pending a real use-case; the built-in notifications cover the common cases (agent_end, tool_error, retry_exhausted).
- [ ] `/doctor` actions (click a failing row to open docs / restart pi) — currently read-only.
- [ ] Real MCP listing — blocked on pi exposing the info over RPC; when it does, `mcp_rows(app)` swaps to a live reader.
- [ ] `color-eyre`-style formatted panic report — current dump uses a plain `Backtrace::force_capture()`; the colorized eyre report is nice-to-have for V2.12.

**Notes:**
- Agent-end notification threshold is deliberately 10 s so sub-conversational turns stay silent.
- `Backends { osc777: true, native: true }` lights both in iTerm2 + macOS Notification Center when built with `--features notify`.
- `Modal::Doctor` / `Modal::Mcp` reuse the same close-on-Esc pattern as `Help` / `Stats`.
- Crash dump uses unix-seconds for the filename so sort order matches chronology without any locale fuss.

---

---

## V2.7 — Git integration

- [ ] `git2` feature flag (`git`, default-on); shell-out fallback
- [ ] Header `git` chip: branch + dirty dot (●/○)
- [ ] `/diff` (unstaged), `/diff --staged`
- [ ] `/log [n]` last N commits with author/date/subject
- [ ] `/commit <msg>` modal with staged-files preview
- [ ] `/branch` (list); `/switch-branch <n>`; `/stash`
- [ ] Inline diff view uses V2.3 widget
- [ ] Works in non-git dirs (chip hidden)

**Notes:** _(to fill in)_

---

## V2.8 — Plan mode + TODO + diagnostics

- [ ] `/plan` toggles plan-mode
- [ ] Plan-mode intercepts `tool_execution_start`; shows preview card; Y/N gate
- [ ] Y → forward the tool call; N → send steer "stop, let's reconsider"
- [ ] TODO widget (right-side persistent; keyboard CRUD)
- [ ] `/todo add|done|rm|list`
- [ ] TODO persists to `~/.local/share/rata-pi/todos/<session>.md`
- [ ] Diagnostics panel stretch (`/diagnostics` shells `cargo check` for Rust projects)

**Notes:** _(to fill in)_

---

## V2.9 — Timeline + replay + snapshots

- [ ] `/timeline` modal — branch graph of every session under pi's session dir
- [ ] Fork edges rendered git-log style
- [ ] `--replay <file>` CLI — steps events with n/p/N/P
- [ ] `/save [name]` snapshots transcript to `snapshots/<name>.{md,jsonl}`
- [ ] `/load <name>` restores as read-only overlay
- [ ] `/snapshots` lists saved snapshots
- [ ] Replay progress bar + speed controls (1x/2x/4x)

**Notes:** _(to fill in)_

---

## V2.10 — Multi-line + vim/emacs + templates

- [ ] `tui-textarea` composer with undo/redo + word motions
- [ ] Shift+Enter newline; Enter submit
- [ ] `/vim` mode (normal/insert; `i`/`a`/`o`/`hjkl`/`wb`/`dw`/`yy`/`p`/`:w`/`:q`)
- [ ] `/emacs` mode (Ctrl-heavy bindings)
- [ ] `/default` restores
- [ ] Current mode indicator in editor border
- [ ] `/templates` — picker over `~/.config/rata-pi/templates/*.md` (template vars `{{selection}}`, `{{file}}`)
- [ ] Undo history snapshot tests

**Notes:** _(to fill in)_

---

## V2.11 — (see above — merged into the delivered V2.11 section)

---

## V2.12 — app.rs module split + reducer tests ✅ (partial, staged)

Closes the P2 audit item (#9, #10, #11) for the high-value chunks: reducer-style state tests, directory layout, visuals cache extraction, and main-screen drawing extraction. The remaining modal-draw / input / slash / runtime / events extractions are tracked for V2.13+ — they're mechanical and each would be its own commit.

### V2.12.a · reducer tests for `App::on_event` (commit `fe4f216`)
- [x] **18 tests** driving the full state machine through scripted `Incoming` events. `App::on_event` is already pure (`(&mut self, Event) -> State`), so we construct a fresh App, feed events, and assert against public state.
- [x] Coverage: Agent lifecycle (start/end, tool counters), Turn bookkeeping (counter + divider + cost), Message deltas (text/thinking/error), Tool lifecycle (counters, LLM↔Tool flips), Auto-retry (state transitions, success vs exhausted), Compaction (state transitions), Queue updates, Extension errors, Liveness probe (`last_event_tick`), End-to-end full-turn transcript shape.
- [x] `app()` helper disables notifications so OSC 777 doesn't leak into test output.

### V2.12.b · directory layout (commit `cf045c4`)
- [x] `src/app.rs` → `src/app/mod.rs`. File move only, no code changes. Unlocks incremental submodule extraction.

### V2.12.c · extract visuals cache (commit `6783ed5`)
- [x] `src/app/visuals.rs` (350 lines). Contains:
  - `Visual` enum + `height` / `render` / `render_clipped`
  - `render_card_clipped` (partial-card renderer for clipped scrolling)
  - `fingerprint_entry` (cache-key function hashing mutable bits)
  - `CachedVisual` + `VisualsCache` + `update_visuals_cache`
- [x] `build_one_visual` and the body-builder helpers (tool_card, bash_card, build_*_body, …) **stay** in mod.rs — too tangled with mod.rs helpers (`truncate_preview`, `args_preview`, `markdown::render`, syntect) to move cleanly in one pass. Follow-up extraction will tease them apart.
- [x] `pub(super)` on the items consumed by mod.rs (`Visual`, `VisualsCache`, `update_visuals_cache`, `CachedVisual` fields). Child module sees parent's private `App` / `LiveState` / `Entry` for free (Rust descendant-privacy rule).
- [x] `fingerprint_entry` re-imported under `#[cfg(test)]` only so clippy doesn't warn about an "unused import" in non-test builds.

### V2.12.d · extract main-screen drawing (commit `40d5045`)
- [x] `src/app/draw.rs` (890 lines). Contains:
  - `draw` (the top-level frame entry), `draw_widgets`, `draw_toasts`
  - `draw_status` + `fmt_elapsed`
  - `draw_header`
  - `draw_body` (reads the visuals cache — zero markdown/syntect work in this hot path)
  - `render_cutoff_hint`, `draw_scrollbar`
  - `draw_editor`, `draw_footer`
  - `kb` (themed keybinding chip)
  - `SPINNER` constant
- [x] Modal drawing (`draw_modal` + ~20 per-modal body builders) **stays** in mod.rs for now — another ~1500-line chunk that gets its own future extraction.
- [x] Three `pub(super)` markers on exports: `draw` (called from `ui_loop`), `draw_scrollbar` + `kb` (called from modal body builders still in mod.rs).
- [x] `draw.rs` imports `NotifyKind` (used by `draw_toasts`); mod.rs sheds the now-unused `Constraint`/`Layout`/`BorderType`/`Gauge`/`Sparkline`/`NotifyKind` imports.

### Metrics
| | V2.11.3 | V2.12 |
|---|---|---|
| `src/app.rs` (or mod.rs) | 6 267 lines | **5 815 lines** |
| Tests | 114 | **132** |
| `src/app/` files | 1 | **3** (mod, visuals, draw) |
| Reducer-test coverage | 0 state transitions | **18** state transitions |

### Gates
- **132 tests pass** (V2.11.3 was 114; +18 reducer tests).
- `cargo clippy --all-targets -- -D warnings` clean across all four commits.
- `cargo fmt --check` clean.
- Each commit builds green in isolation (bisectable).

### Not done — deferred to V2.13+ (mechanical, each its own commit)
- [ ] **V2.13.a** — extract modal drawing (`draw_modal` + all `*_text` / `*_body` helpers + `filtered_*` iterators). ~1500 lines → `src/app/draw_modal.rs`.
- [ ] **V2.13.b** — extract body-builder helpers (`build_one_visual`, `tool_card`, `bash_card`, `build_*_body`, `ToolFamily`, `tool_family`, `primary_arg_chip`, `plain_paragraph`, `thinking_body`, `compaction_lines`, `retry_lines`, `strip_line_numbers`, `add_output_body`, `body_or_ellipsis`, `diff_body_line`). ~700 lines → `src/app/builders.rs` or fold into `visuals.rs`.
- [ ] **V2.13.c** — extract input (`handle_crossterm`, `handle_modal_key`, `handle_focus_key`, `on_mouse_click`, `handle_list_keys`, `handle_vim_normal`). ~800 lines → `src/app/input.rs`.
- [ ] **V2.13.d** — extract slash (`try_local_slash`, `try_pi_slash`, `handle_plan_slash`, `submit`, `insert_file_ref`, `do_copy`). ~600 lines → `src/app/slash.rs`.
- [ ] **V2.13.e** — extract runtime (`run`, `run_inner`, `ui_loop`, `bootstrap`, `refresh_stats`, `spawn_git_refresh`, `spawn_file_walk`, `prepare_frame_caches`, `ensure_file_preview`, `build_preview_lines`, `offline_events`). ~250 lines → `src/app/runtime.rs`.
- [ ] **V2.13.f** — extract events (`handle_incoming`, `handle_ext_request`, `import_messages`, `user_content_text`, `parse_bash_result`). ~300 lines → `src/app/events.rs`.

After all V2.13 extractions `mod.rs` should land around 2000 lines — holding just the `App` struct, `SessionState`, enum `LiveState` / `ComposerMode`, `MouseMap`, `install_panic_hook` + `write_crash_dump` + `which_pi`, the impl blocks on App, and the top-level `pub async fn run(args)`.

### Notes
- Reducer tests were the highest-ROI item of the whole V2.12 block — they make future state-machine changes safe. The module split is incremental maintainability.
- The parent → child privacy rule (descendants see ancestor privates) made extraction cheap: only *exports* from child to parent need `pub(super)`.
- `#[cfg(test)] use visuals::fingerprint_entry;` pattern is how we avoid clippy "unused import" on items used only from a test module — worth remembering for the remaining V2.13 extractions.
- Each V2.13 subcommit should mirror V2.12.c / V2.12.d: verify build + tests between steps, stay bisectable, fix imports as a second pass.

---

## V2.11.3 — Cleanup sweep: RefCell kill, themed kb, state_dir, small fixes ✅

Closes the P3 + P4 items from the audit plus several smaller items (#8, #15, #16, #19, #23).

### P3 · #7 · Kill `RefCell<MouseMap>`
- [x] `MouseMap` is now a plain `MouseMap` field on App (no interior mutability).
- [x] `draw(f, app, &mut mm)` and `draw_body(f, area, app, &mut mm)` take the map as an explicit out-argument — the render produces it.
- [x] `ui_loop`: `let mut mm = MouseMap::default(); terminal.draw(|f| draw(f, app, &mut mm))?; app.mouse_map = mm;`. Clean frame-scoped data flow.
- [x] `on_mouse_click` reads fields directly: `app.mouse_map.live_tail_chip`, `app.mouse_map.entry_at(x, y)` — no borrow dance.
- [x] `std::cell::RefCell` import dropped from app.rs.

### P3 · #13 · Theme the `kb()` helper
- [x] `kb(s: &str, t: &Theme)` now uses `t.accent_strong` + BOLD instead of hardcoded `Color::Cyan`. Light themes + Solarized now read correctly.
- [x] 44 call sites updated. `draw_footer` aliases `let t = &app.theme;` at the top so the kb invocations read cleanly.

### P3 · #14 · Crash dump to `state_dir`
- [x] `write_crash_dump` now prefers `BaseDirs::state_dir()` (Linux XDG `~/.local/state/`) and falls back to `data_local_dir` elsewhere. Matches the PLAN_V2 §11 spec verbatim; cosmetic on macOS/Windows where `state_dir` isn't populated.

### Smaller audit items bundled
- [x] **#8 · composer unwraps** — `word_right` replaces `.chars().next().unwrap()` with `let Some(c) = ... else break/return` patterns. Same behavior, no panic risk. `history.rs:100` likewise.
- [x] **#15 · crossterm error handling** — the `Some(Ok(ev))` pattern in `tokio::select!` is replaced with a full match: `Some(Ok(ev))` dispatches, `Some(Err(e))` logs + sets `app.quit = true`, `None` (EOF) logs + quits gracefully. A dying TTY (SIGHUP / container detach) no longer spins forever on ticks.
- [x] **#16 · history load cap** — `History::load()` caps in-memory entries to the last `MAX_HISTORY = 5_000`. The JSONL file on disk is untouched; future writes continue to append. Prevents unbounded startup memory growth after months of use.
- [x] **#19 · dead code removal** — deleted `git::is_repo` (never called; `git::status` does the work). Removed stale `#[allow(dead_code)]` on `FileList::empty` (now actually used by `open_file_finder`) and `RpcClient::send_ext_ui_response` (used by ext-UI dispatch in 8 places).
- [x] **#23 · `dummy_events` → `offline_events`** — renamed with a proper doc explaining why the dead-channel placeholder is correct. No behavior change.

### P4 · #18 · Release opt-level 3
- [x] `Cargo.toml` flipped `opt-level = "s"` → `opt-level = 3`. Binary grew from **4.2 MB → 4.9 MB** (+730 KB). Acceptable for a TUI (nothing near a size budget). Expected CPU wins in the markdown/syntect hot paths.

### Not done (scoped out, justified)
- [—] **#17 · cache `merged_commands`** — on reflection, this runs once per modal-open (user action), not per frame. The cost is bounded and caching adds complexity without a measurable win.
- [—] **#20 · `eprintln!` in panic hook** — replaced the write would happen after tracing's file writer may be disposed. Keeping `eprintln!` is correct; the audit flagged style, not correctness.
- [—] **P2 · module split** — deferred to its own milestone (V2.12 candidate). Too large and mechanical to mix with behavioral fixes.

### Gates
- **114 tests pass** (no new tests — all fixes are surgical).
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.
- Release build: **5 172 400 bytes** (was 4 429 792 — +17%).

### Notes
- Eliminating `RefCell<MouseMap>` removes the last piece of interior mutability on `App`. The render path is now a truly pure fn of `(State, &mut MouseMap)`: `draw(f, app, mm)` can be snapshot-tested by constructing a mock frame and a fresh MouseMap.
- The `kb()` change means every keybinding hint now respects the active theme. Test manually by switching to Solarized via `/theme solarized-dark` — hints are now the same hue as the accent border, not bare cyan.
- Dead-code sweep kept all `#[allow(dead_code)]` that document genuine speculative surface (`HistoryEntry::path`, `Mode::label`, `McpStatus::{Connected,Disconnected}`, etc.) — these are future-wired intentionally.

---

## V2.11.2 — Render hot-path: visuals + heights cache, parallel bootstrap, async history ✅

Closes the P0 + P1 items from the full codebase audit.

### P0 · per-entry visuals + heights cache
- [x] **`fingerprint_entry(entry, show_thinking) -> u64`** — fingerprints the mutable bits of every Entry variant so two equal fingerprints imply equal renders under the same theme. Strings use `.len()` (our transcript only grows at the tail, so len is a strict monotonic identity proxy). Structured entries (ToolCall, BashExec, Compaction, Retry) hash their mutable fields explicitly.
- [x] **`CachedVisual { fingerprint, visual, width, height }`** — one slot per Entry. `VisualsCache { theme_key, entries }` on App.
- [x] **`update_visuals_cache(app, content_w)`** — rebuilds only slots whose fingerprint changed; recomputes `height` only when the width changed. Wipes everything on theme change. Truncates on transcript shrink (`/clear`, session switch).
- [x] **Live-streaming tail is forced to rebuild each frame** so the blinking cursor + "pi · streaming" title stay animated without polluting the fingerprint space.
- [x] **`build_one_visual(entry, app, is_live_tail) -> Visual`** — extracted from the old `build_visuals`. Per-entry render path.
- [x] **`draw_body` reads from cache** directly: `let cache = &app.visuals_cache.entries;`. No markdown/syntect/line_count calls inside `draw()` any more. If content_w disagrees (mid-frame resize), the draw falls back to a per-entry `height()` for correctness on that one frame; the next `prepare_frame_caches` cycle re-syncs.
- [x] Plumbed via `prepare_frame_caches(app, terminal.size()?)` — `content_w` is derived from `terminal.width - 3` (2 borders + 1 scrollbar column). The body always spans full terminal width so the math is exact without duplicating the Layout chain.
- [x] **5 unit tests** on `fingerprint_entry`: equal-content, appended-text, variant distinction, show_thinking flag, and ToolCall status/expanded transitions. First tests on `src/app.rs`.

### P1 · focus out of Card
- [x] Removed `focused: bool` from `Card`. Made `render` / `render_to_buffer` take `focused` as an argument.
- [x] `Visual::render` no longer clones the Card — it looks up focus via `app.focus_idx == Some(idx)` and passes it to `Card::render`. The cached Card stays immutable across focus toggles.

### P1 · parallel bootstrap
- [x] `bootstrap()` now fires `GetState / GetMessages / GetCommands / GetAvailableModels / GetSessionStats` concurrently via `tokio::join!`. Round-trip count unchanged; latency at startup drops from Σ RTTs to max RTT.

### P1 · async `History::record`
- [x] `append_file(path, entry)` extracted as a free function and wrapped in `spawn_blocking` when a tokio runtime is present (falls back to sync for unit tests). The submit keypath is no longer blocked on filesystem writes.

### Gates
- **114 tests pass** (V2.11.1 had 109; +5 fingerprint tests).
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.
- `cargo build --features notify` clean.

### Expected perf delta (qualitative)
- 100-entry session with 10 assistant cards containing fenced code blocks, 30 fps redraw: before — 10 pulldown-cmark parses + N syntect runs × 30 fps. After — 0 per frame; rebuilds only when content changes. Orders of magnitude.
- Per-card `Paragraph::new(body.clone())` in `height()` and `render_to_buffer()` still occurs inside ratatui but once per visible card rather than once per cached card (the untouched slot already has a cached `height`).
- Focus navigation (`j`/`k` in focus mode): previously triggered a full Card clone per frame for every visible card. Now: zero allocations from the focus toggle — focus is an index lookup at render time.
- Boot latency: was Σ(5 × RTT); now max(5 × RTT) + parse. On 50 ms RTT that's ~250 ms → ~50 ms.
- Prompt submission on slow disks: no longer stalls on `history.jsonl` append.

### Notes
- The cache size is O(transcript-length). Each slot holds a `Visual` (Card body = `Vec<Line<'static>>`). For a 10 000-entry session that can reach tens of MB — acceptable for a dev tool, but worth remembering. If it bites, a tiered cache (recent 500 full, older summarized) is straightforward.
- `content_w` is captured at `prepare_frame_caches` time. A resize between prepare and draw (rare — both fire synchronously in the same tick) triggers the per-entry fallback branch in draw_body for one frame. The next frame re-syncs.
- Live-tail treatment leaks no cost when idle: the "is streaming" gate is exact (`app.is_streaming && LiveState ∈ {Streaming, Llm}`). When idle, the last entry's fingerprint is cached and the body is reused.
- `is_last_assistant` is gone — the live-tail index is computed once per prepare in `update_visuals_cache`, inlined.
- `History::record` falls back to sync I/O when no tokio runtime is present, so unit tests (no runtime) still exercise the file code path.

---

## V2.11.1 — Perf pass: responsiveness audit ✅

Responding to a deep architectural review that identified five bottlenecks.
The fixes uphold the principle that `terminal.draw(&App)` is a pure function
of state and never touches the filesystem, runs fuzzy matching, spawns
processes, or re-lexes source code.

### 1 · Async git (5-second stutter eliminated)
- [x] `src/git.rs` now uses `tokio::process::Command` with `tokio::time::timeout`. Every helper is async (`status / diff / log / branches / commit_all / stash / switch / is_repo`).
- [x] `refresh_git` is gone. In its place, `spawn_git_refresh(app, &tx)` fires a `tokio::spawn` task that awaits `git::status()` and delivers the result via an `mpsc` channel; a new select branch drains that channel. The 5-second `stats_tick` arm now only *spawns* the child — it doesn't await it — so crossterm events keep flowing.
- [x] `app.git_refresh_inflight` prevents stacked git children on slow disks: if the previous `git status` hasn't returned yet, the next tick is a no-op.
- [x] A `git` child is fired once right after channel setup so the header chip lights up immediately on boot instead of after the first 5 s.
- [x] All user-triggered git slash commands (`/status`, `/diff`, `/git-log`, `/branch`, `/switch-branch`, `/commit`, `/stash`) now await the async helpers directly — they *want* to block the composer until the output arrives, but the executor itself is no longer stalled, so the event loop keeps polling and partial UI repaints (flash toasts, spinners) still happen during long commands.
- [x] `try_local_slash` is now `async fn` to accommodate the awaits; both call sites (`submit` and the Commands-modal Enter path) are already in async context.

### 2 · Fuzzy filter cached in `FileFinder`
- [x] New fields: `filtered: Vec<(String, i64)>` + `filter_query: Option<String>`. The key handler calls `ff.refresh_filter(FILES_CAP)`; the method short-circuits when the query hasn't changed since last build.
- [x] Fresh method `current_path(&self) -> Option<&str>` replaces three ad-hoc re-filter call sites that each re-ran `SkimMatcherV2::fuzzy_match` over 20 000 paths.
- [x] Query-change and selection-change bookkeeping in the key handler: before/after snapshots decide whether to rebuild the filter or drop the preview cache.
- [x] 5 new unit tests in `ui::modal::tests` cover cache keying, short-circuit on unchanged query, current_path-follows-selection, set_files-clears-caches, and invalidate_preview.

### 3 · Preview cache (disk read + syntect)
- [x] New `PreviewCache { path, lines }` on `FileFinder`. `ensure_file_preview(ff)` reads the selected file and runs syntect exactly **once per selection change**. Subsequent frames reuse `cache.lines`.
- [x] `build_preview_lines(root, path)` centralises the `files::preview` + `syntax::highlight` pipeline and returns a plain `Vec<Line<'static>>` without theme chrome — the chrome (title row, footer hint) is baked on in the themed draw helper, which now does zero I/O and zero matcher work.
- [x] `file_preview_lines` dropped from ~60 lines of logic to a ~15-line pure read from the cache.

### 4 · Async filesystem walk
- [x] `open_file_finder` now opens the modal **instantly** in `loading: true` state with an "indexing files…" placeholder, then spawns the walk via `tokio::task::spawn_blocking(|| files::walk_cwd())` (the walker is CPU-bound + blocking I/O, exactly what `spawn_blocking` is for).
- [x] The completed `FileList` is posted through `file_walk_tx` and picked up by a new select arm that calls `FileFinder::set_files` if the Files modal is still open. (If the user closed the modal mid-walk, the message is simply dropped.)
- [x] `FileList::empty()` is now routinely used for the placeholder state.
- [x] The filter / preview draw helpers render gracefully against an empty list — the left pane shows "walking repo (respects .gitignore)…", the right pane shows "(waiting for index…)".

### 5 · Frame-level cache prep
- [x] `prepare_frame_caches(app)` runs once per frame from `ui_loop`, before `terminal.draw(&App)`. Currently it calls `ff.refresh_filter(FILES_CAP)` + `ensure_file_preview(ff)` for an open Files modal. The draw closure itself takes `&App` and does zero caching writes — the architectural invariant (render is a pure fn of state) is preserved.
- [x] `FILES_CAP = 500` centralises the viewport cap previously sprinkled as the magic number 500 across three call sites.

### Gates
- **109 tests pass** (V2.11 was 104; +5 for FileFinder cache behavior).
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.
- `cargo build --features notify` clean.

### Expected perf improvement (qualitative — profile per-repo)
- Stats tick no longer blocks the runtime. Before: 200–800 ms freeze every 5 s in a large repo. After: imperceptible; git child runs on a worker thread.
- Typing in the file-finder: fuzzy matcher ran up to 40 k comparisons per keystroke × N frames. After: one refresh per query change.
- Holding arrow-down through the file list: syntect re-ran every frame. After: syntect runs exactly once per selection.
- Opening the file finder in a 15k-file repo: previously froze the UI for ~300 ms before the modal appeared. After: modal paints immediately; the walk finishes in the background.

### Notes
- The `git_timeout` helper is kept on `status()` only, with a 1-second ceiling, because that's the hot-path call driven by the ticker. One-off commands (`diff`, `log`, etc.) don't need a timeout — if pi-the-user asks to see the diff of a wedged repo they'd rather see the error.
- `spawn_blocking` vs `tokio::spawn`: `walk_cwd` is synchronous and CPU/IO blocking, so `spawn_blocking` is correct. `git::status()` is properly async (child process), so plain `tokio::spawn` is right.
- The file-walk channel buffer is 1 — we only ever have one pending walk per open modal, and if the user triggers a second finder before the first walk returns, the new modal will overwrite the previous Files modal so the old `tx` drops and the old walk's delivery is a no-op.
- `git_refresh_inflight` is on App, not scoped to the channel, because we want the flag to survive task boundaries (it resets only when the channel delivery lands).

---

---

## V2.12 — Release polish

- [ ] `docs/` — manual, keybinding reference, theme gallery, troubleshooting
- [ ] vhs recipes for every major feature; gifs committed to `docs/demos/`
- [ ] Shell completions via clap: bash, zsh, fish
- [ ] Man page via clap_mangen
- [ ] `cargo-dist` CI pipeline for mac/linux/windows artifacts
- [ ] GitHub Pages site with theme gallery + recorded demos
- [ ] CHANGELOG.md keep-a-changelog format
- [ ] Insta snapshots for all widgets (≥ 60)
- [ ] End-to-end test with scripted mock pi (≥ 10 scenes)
- [ ] Benchmarks under `benches/`: stream deltas, render 10k, fuzzy 50k files
- [ ] Semver tag v2.0.0

**Notes:** _(to fill in)_

---

## Cross-milestone backlog

_Discovered mid-flight. Pull into the current or next milestone when triaged._

- [ ] _(empty — populate as we go)_

---

## Rolling metrics

Kept current at each milestone boundary.

| | V1 final | V2.0 | V2.1 | … |
|---|---|---|---|---|
| Tests | 53 | — | — | |
| LOC (bin) | 5907 | — | — | |
| Binary size (release-stripped) | ~2.5 MiB | — | — | |
| Cold start to first draw | — | ≤ 200 ms | | |
| Idle CPU | — | 0.0 % | | |
| Snapshot count | 0 | — | — | |
