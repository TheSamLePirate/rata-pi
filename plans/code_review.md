# Code Review — `rata-pi`

Date: 2026-04-21  
Reviewer: ChatGPT

## Scope

This review covers the current `rata-pi` application in this repository, including:

- Rust application structure
- RPC/process integration with `pi`
- UI architecture and rendering pipeline
- performance and responsiveness risks
- maintainability
- testing/CI
- documentation quality

I did **not** edit application code. I only inspected the repository and ran project checks.

## Validation performed

The following commands were run successfully:

- `cargo test --locked --all-features --all-targets`
- `cargo clippy --all-features --all-targets -- -D warnings`
- `cargo fmt -- --check`

Result:

- **194 tests passed**
- **clippy passed with no warnings**
- **format check passed**

That is a very good baseline and materially increases confidence in the codebase.

---

## Executive summary

This is a **strong, feature-rich TUI application** with notably good test coverage, thoughtful terminal cleanup, a solid RPC abstraction, and visible attention to UX details like transcript virtualization, cached rendering, crash dumps, offline mode, and modal workflows.

The codebase is already far beyond the scaffold stage and shows clear product thinking.

The main weaknesses are not correctness in the small; they are mostly **architecture and operability risks**:

1. **`src/app/mod.rs` is far too large** and currently carries too many responsibilities.
2. **Some RPC paths can block the UI indefinitely** because there are no timeouts around awaited calls in key runtime paths.
3. **There is at least one real RPC bookkeeping bug**: pending request state is not removed if send fails in `RpcClient::call`.
4. **File previewing is more expensive and blocking than it needs to be**.
5. **Theme consistency is incomplete** because markdown/syntax rendering bypasses the app theme system.
6. **Documentation drift is visible**, especially in `README.md`.

Overall assessment: **good app, good engineering instincts, but now at the point where refactoring and operational hardening should be prioritized over adding more features**.

---

## What is working especially well

### 1. Strong test coverage
The project has unusually good unit coverage for a TUI app:

- reducer/event-flow tests
- RPC serialization/deserialization tests
- transcript mutation tests
- markdown/diff/syntax tests
- interview/plan parsing tests
- theme and notify tests

This is a real strength. It reduces regression risk in a codebase with lots of branching UI behavior.

### 2. Good failure hygiene around the terminal
`src/app/mod.rs` handles:

- raw mode enable/disable
- alternate screen entry/exit
- mouse + bracketed paste toggling
- panic cleanup
- crash dump persistence

That is exactly the kind of polish TUI apps often miss.

### 3. Solid RPC layering
The split across:

- `src/rpc/commands.rs`
- `src/rpc/events.rs`
- `src/rpc/types.rs`
- `src/rpc/client.rs`

is good. The wire model is clear, testable, and reasonably tolerant to protocol evolution.

### 4. User-facing capability is already substantial
Features like:

- plan mode
- interview mode
- extension UI
- file finder
- git integration
- status widget
- notifications
- transcript export
- theme support

show that the app is genuinely useful, not just technically interesting.

### 5. Evidence of performance thinking
The app is not naive:

- bootstrap RPCs run concurrently
- transcript rendering is virtualized
- per-entry visuals are cached
- git refresh is backgrounded
- file walk is offloaded

Those are all good decisions.

---

## Key findings

## High priority

### 1. `src/app/mod.rs` is a maintainability bottleneck
**Severity:** High  
**Files:** `src/app/mod.rs`, `src/app/draw.rs`, `src/app/visuals.rs`

`src/app/mod.rs` is currently **~8.2k lines** and contains:

- state model
- event reducer
- input handling
- slash command dispatch
- modal logic
- interview form behavior
- plan behavior
- file finder behavior
- rendering helpers
- git/status bodies
- settings panel logic
- tests

This is the biggest structural problem in the repository.

### Why it matters

- change risk is high because unrelated concerns sit together
- onboarding cost is high
- compile/test cycles are noisier when one file changes constantly
- it becomes harder to reason about state ownership and invariants
- extraction gets harder the longer it waits

### Recommendation
Refactor by responsibility, not by arbitrary size. Suggested split:

- `app/state.rs` — `App`, `SessionState`, live-state helpers
- `app/reducer.rs` — `on_event`, bootstrap import, state transitions
- `app/input.rs` — global key handling
- `app/modal_input.rs` — modal key handling
- `app/commands.rs` — slash command routing
- `app/interview_ui.rs` — interview-specific input/render helpers
- `app/git_ui.rs` — git modal bodies and helpers
- `app/settings.rs` — settings row model + actions
- `app/render_modals.rs` — modal drawing only

Even getting this down from 8k lines to 6-8 focused modules would be a major improvement.

---

### 2. Some RPC calls can freeze the UI because they have no timeout
**Severity:** High  
**Files:** `src/rpc/client.rs`, `src/app/mod.rs`

`RpcClient::call()` has no timeout, and several important UI paths await it directly.

Examples:

- `bootstrap()` in `src/app/mod.rs`
- `refresh_stats()` in `src/app/mod.rs`
- several slash-command and modal actions

The highest-risk case is the periodic stats path:

- `ui_loop()` runs `refresh_stats(c, app).await` inside the `stats_tick` branch
- if `pi` hangs or stops responding without fully dying, this await can stall the loop
- startup can also hang indefinitely during bootstrap if the child process accepts I/O but never responds

### Why it matters
A TUI must stay responsive even if the backend is degraded. Right now some runtime paths rely on backend cooperation.

### Recommendation
Add timeout-aware RPC helpers, for example:

- `call_timeout(command, Duration)`
- or wrap selected `call()` sites with `tokio::time::timeout`

Suggested policy:

- bootstrap calls: 1-3s each, degrade gracefully
- stats polling: short timeout, e.g. 500ms-1s
- user-initiated control commands: slightly longer, but still bounded

Also consider surfacing timeout failures as non-fatal status/toast events rather than blocking the app.

---

### 3. `RpcClient::call()` leaks pending requests on send failure
**Severity:** High  
**File:** `src/rpc/client.rs`

In `RpcClient::call()`:

1. request id is inserted into `pending`
2. the JSON is sent on `self.tx`
3. if `send()` fails, the function returns `Err(RpcError::Closed)`

But the pending entry is **not removed** in that error path.

### Why it matters
If the writer channel is closed and callers keep issuing `call()` attempts, the pending map can accumulate dead waiters. This is a real correctness bug, even if the leak is small in practice.

### Recommendation
On `send()` failure, remove the inserted id from `pending` before returning.

This is a small fix with clear value.

---

## Medium priority

### 4. File preview is synchronous and reads the whole file before enforcing the size guard
**Severity:** Medium  
**Files:** `src/files.rs`, `src/app/mod.rs`

`files::preview()` currently does:

- `std::fs::read(&path)`
- then checks if file size is `> 50 * 1024 * 1024`
- then only uses the first ~8 KiB / 40 lines

This means a very large file is fully read into memory before being rejected.

Separately, preview generation is synchronous and triggered from the UI-side cache preparation path.

### Why it matters
- unnecessary allocation for large files
- avoidable latency on slow disks/network mounts
- selection changes in the file finder can briefly stall the UI

### Recommendation
Use streaming metadata + bounded reads:

- call `metadata()` first for size
- open file with `BufReader`
- read only up to the required sample limit
- optionally offload preview building to `spawn_blocking` the same way the file walk already is

The current logic is correct enough, but it is wasteful.

---

### 5. Theme system is semantically good, but markdown/syntax rendering bypasses it
**Severity:** Medium  
**Files:** `src/theme/mod.rs`, `src/ui/markdown.rs`, `src/ui/syntax.rs`

The app has a clean semantic `Theme` abstraction, but some rendering code still hardcodes colors:

- `ui/markdown.rs` uses `Color::DarkGray`, `Color::Cyan`, `Color::Blue`, `Color::Yellow`, etc.
- `ui/syntax.rs` uses a fixed syntect theme (`base16-ocean.dark`) regardless of the selected app theme

### Why it matters
- visual consistency is weaker than the rest of the design suggests
- syntax colors may clash with some themes
- theming is not truly centralized

### Recommendation
Move markdown/syntax styling behind the semantic theme layer:

- pass `&Theme` into markdown rendering
- define semantic markdown roles if needed (`markdown_heading`, `markdown_quote`, etc.)
- either map syntax colors per app theme or at least choose one syntect palette per built-in theme

This is not a blocker, but it is an architectural inconsistency.

---

### 6. Rendering cache still does an O(n) full transcript scan every frame
**Severity:** Medium  
**File:** `src/app/visuals.rs`

The visuals cache avoids expensive re-rendering, which is good, but `update_visuals_cache()` still walks the full transcript every frame to fingerprint entries.

### Why it matters
For long sessions, the app may still pay a per-frame linear cost even when little changed.

### Recommendation
Consider incremental invalidation:

- mark only appended/changed transcript indices dirty on mutation
- recompute heights only on width/theme changes
- keep the current live-tail special case

This is more of a scaling concern than an immediate bug, but the app is already feature-rich enough that long sessions are plausible.

---

### 7. Documentation drift is visible
**Severity:** Medium  
**Files:** `README.md`, `docs/USER_GUIDE.md`

The repository has a very detailed `docs/USER_GUIDE.md`, but `README.md` still describes the project as a generated Hello World app.

That is now badly out of date.

There also appears to be at least one behavior/docs mismatch:

- the user guide says plan markers are hidden from the transcript
- interview markers are stripped in code
- plan markers are parsed on `agent_end`, but I did not find equivalent stripping logic for them

### Why it matters
- wrong first impression for contributors/users
- feature discoverability suffers
- trust drops when docs and behavior diverge

### Recommendation
At minimum:

- replace the README with a current overview
- link to `docs/USER_GUIDE.md`
- either strip plan markers too, or update the guide to reflect current behavior

---

### 8. CI coverage misses Linux test execution
**Severity:** Medium  
**File:** `.github/workflows/ci.yml`

The workflow runs:

- fmt/clippy/doc on Ubuntu
- tests on macOS and Windows

But unit/integration tests are **not** executed on Linux.

### Why it matters
The app is terminal- and OS-sensitive. Linux is a primary environment for TUIs.

### Recommendation
Add `ubuntu-latest` to the test matrix unless there is a known reason not to.

---

## Low priority / polish

### 9. README and product positioning should be simplified and surfaced earlier
**Severity:** Low  
**Files:** `README.md`, `docs/USER_GUIDE.md`

The project has enough functionality now that it deserves a concise top-level README with:

- what the app is
- screenshots or feature bullets
- how to run it
- what `pi` dependency is required
- links to the guide

Right now the best documentation exists, but it is not where most people will look first.

---

### 10. There are a few places where blocking work still lives on the UI path
**Severity:** Low/Medium depending on workload  
**Files:** `src/app/mod.rs`, `src/files.rs`

The code is already conscious about offloading some heavy work (`spawn_blocking` for history and file walking), but there are still a few sync operations in UI-adjacent paths.

This is not catastrophic today, but worth standardizing: disk reads, preview builds, and any expensive parsing/highlighting should consistently use background tasks if they are user-triggered and potentially slow.

---

## Positive architecture notes

These are design choices I would keep:

- **typed RPC model** instead of ad hoc JSON access everywhere
- **best-effort offline mode** when `pi` cannot spawn
- **terminal cleanup + panic hook + crash dump** as first-class behavior
- **feature-gated native notifications** while keeping OSC 777 always available
- **virtualized transcript rendering** with per-entry visuals cache
- **good separation of domain modules** outside the oversized `app/mod.rs`
- **strong command catalog model** for built-ins + pi-provided commands

---

## Suggested priority order

### Next 1-2 iterations
1. Fix `RpcClient::call()` pending-entry leak on send failure.
2. Add timeouts around bootstrap and periodic stats RPCs.
3. Update `README.md` to reflect the real product.
4. Add Linux to the test matrix.

### Next refactor cycle
5. Break up `src/app/mod.rs` by responsibility.
6. Move file preview to bounded/background I/O.
7. Introduce theme-aware markdown/syntax coloring.

### Later scalability work
8. Make visuals-cache invalidation incremental instead of whole-transcript per frame.
9. Continue moving modal-specific logic into dedicated modules.

---

## Final assessment

`rata-pi` is already a **serious TUI application**, not a prototype. The code shows care, test discipline, and a real product direction.

The review outcome is:

- **Quality:** good
- **Reliability:** good, with a few concrete hardening gaps
- **Maintainability:** currently strained by `src/app/mod.rs`
- **Performance:** thoughtful overall, but with a few avoidable blocking paths
- **Docs:** strong guide, weak top-level README

If you address the RPC timeout behavior and start decomposing `src/app/mod.rs`, the project will be in a much healthier place for future feature work.
