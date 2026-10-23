# Runs off windows, step 2: runs found by task, checks on the same drive

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three changes, none of them visible to a person:
- `Tasks.live` is keyed by task id;
- a check task runs through the same `Drive` as every other run, so `task::drive_check` and
  `Tasks.checks` go;
- `crates/app/src/task.rs` is cut under the 800-line rule.

**Architecture:**
- **A drive without a session.** `Drive::start(…, chat: None)` makes one, and it remembers that it
  has none (`sessionless`). Such a drive arms no clock, because a check has never had a timeout.
  It carries out the actions that need no session (`RunCommand`, `Measure`, `Finish`), where a
  drive whose session went still carries out only `Finish`.
- **The app's `Driven`** is keyed by task id. It holds `uid: Option<u64>` (none for a check), the
  window, and an optional session handle.
- **A finished check** says *Check passed in …* or *Check failed in …* in the window it was asked
  from, as today. A check that was stopped says nothing.

**Spec:** `docs/superpowers/specs/2026-10-22-runs-off-windows-design.md`, step 2.

## Global Constraints

- No behaviour change:
  - the check toasts and their words;
  - a stopped check is quiet;
  - quitting calls off a running check's command and waits for it;
  - *Run check* shows busy while one runs (`listing::checking`);
  - a check's row is stoppable and shows *stopping* while its command is being called off.
- Every lookup by session uid that remains (`shown(uid)`, the strip's Stop `StopWorkflow(uid)`)
  finds the run by scanning `live` for that uid. The map holds a handful of runs.
- No file past about 800 lines of code; `task.rs` comes out under it.

## Review Focus

1. **A Stop pressed while a check's first mark is still being pinned** ends the check stopped,
   and its command never runs (Task 1 test `a_check_stopped_before_its_command_runs_none`).
2. **A check arms no clock** (Task 1 test `a_check_has_no_timeout`).
3. **A session drive whose session went** still does nothing but finish (the existing
   `a_gone_session_only_lets_the_run_end`).

---

### Task 1: A drive without a session (core)

**Files:** `crates/core/src/task/drive.rs`, `crates/core/src/task/drive/tests.rs`.

- [ ] **Step 1: Write the failing tests:**
  - `a_check_runs_its_command_and_finishes_without_a_session`: `Task::check` with `chat = None`
    → `Pin`; then `Pinned` → `RunCommand`; then `CommandExited(passed)` → `Finish(Done)`.
  - `a_check_has_no_timeout`: no `ArmClock` among the start effects.
  - `a_check_stopped_before_its_command_runs_none`: `Stop` before `Pinned` → after `Pinned`,
    `Finish(Stopped(ByPerson))` and no `RunCommand`.
- [ ] **Step 2: Run them to see them fail;** **Step 3: Implement** `sessionless`; **Step 4: verify**
  `cargo test -q -p onehand-core task::drive`; **Step 5: Commit.**

### Task 2: The app keys runs by task and drives checks

**Files:** `crates/app/src/task.rs`, `crates/app/src/task/driver.rs`, `crates/app/src/task/listing.rs`,
`crates/app/src/shell/workflows.rs:433`.

- [ ] **Step 1:** Make these changes:
  - `live: HashMap<String, Driven>`;
  - `Driven { uid: Option<u64>, session: Option<WeakEntity<ChatSession>>, … }`;
  - `feed(task, input)`;
  - `driver::start_check(task, run, window)` replaces `drive_check`;
  - `stop(uid)` finds the task by uid;
  - `stop_task` feeds `Stop` to any live run, a check included;
  - drop `Tasks.checks`, so `checking`, `stoppable`, `stopping`, `working` and the quit handler
    read `live`;
  - `finish` toasts a check's outcome in `Driven.window`.
- [ ] **Step 2: Verify:** `cargo test -q --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`.
- [ ] **Step 3: Commit.**

### Task 3: Cut `task.rs`

- [ ] **Step 1:** Move the place-keeping (`request`, `ended`, `freed`, `fail`, `let_go`,
  `release`, `stop_queued`, `DRAIN_LIMIT`) to `crates/app/src/task/place.rs`, re-exported as
  before. Done when `task.rs` is under 800 lines of code and every caller compiles unchanged.
- [ ] **Step 2:** Run the full checks, then drive it with `/drive`:
  - *Run check* passes, with its toast;
  - a workflow run goes to *Workflow done*;
  - Stop mid-turn stops the run.
- [ ] **Step 3: Commit,** then open the PR.
