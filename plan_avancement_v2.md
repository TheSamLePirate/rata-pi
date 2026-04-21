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
