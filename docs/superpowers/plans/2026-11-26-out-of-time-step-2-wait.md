# Out of time, step 2: the run waits, Extend or Stop — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A run whose time runs out waits for the person instead of ending: same run, same
session; *Extend* gives it the workflow's timeout again and carries on, *Stop* ends it as a
timeout ends one today.

**Architecture:** A new engine wait, `Await::OutOfTime`, entered by `Run::ran_out` and left by
`Run::extended` (a carry-on prompt, as a gate miss sends) or `Run::stopped(TimedOut)`. The run's
drive turns the timer's cut into the wait (cancel the turn, pause the clock, say it), takes
*Extend* as a third `Answer` beside approve and revise, holding it until the cancelled turn has
ended, and grows the `Budget`. `extended_secs` is kept in the run file. The task reads the wait as
`Working::Waiting` with a new `Stand::OutOfTime`; the app draws *Extend* and *Stop* where it draws
*Continue* and *Revise…* for an approval.

**Tech Stack:** Rust; onehand-core (engine, drive, task model); onehand app (GPUI).

**Spec:** `docs/superpowers/specs/2026-11-26-out-of-time-design.md` — sections *Out of time*,
*Words*, and the clock's `extended_secs`. The board (team place, extend priority) is step 3.

## Global Constraints

- Core GUI-free; nothing `pub` unless named outside core; exhaustive matches on our enums (no `_`
  arms on `Action`, `Await`, `Stand`, `Answer`, `Held` — the guards check it).
- No comment or string names a document.
- Words, exactly: note *Out of time at <step label> after <spent> of <timeout>. Extend to go on.*;
  stand line *Out of time at <step label>*; detail *It worked <spent> of its <timeout> timeout.*;
  buttons *Extend* (primary) and *Stop*; history line *extended by <timeout>*.
- The extend prompt, exactly: *You ran out of time at <step label>. You have <timeout> more.
  Carry on with the step from where you were.*
- `<spent>` and `<timeout>` are said as `facts::said_duration`-style `45m` / `1h30m`-free text:
  reuse the existing `worked()` helper in `task/work.rs` for spent, `Template::timeout_said()` for
  the timeout.
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

1. Extend pressed while the cancelled turn has not ended yet: held, sent once it ends; never a
   prompt queued behind a busy turn (which would read as a take-over) — Task 2 test.
2. Stop pressed while out of time ends `Stopped(TimedOut)`, not `ByPerson`, so Retry and the board
   treat it as a timeout — Task 2 test.
3. The cancelled turn's end is never judged as the step's answer (no gate, no miss, no output kept)
   — Task 2 test.
4. A run extended, then cut off by a restart, resumes with its extension: limit = timeout +
   extended_secs − spent — Task 1 test.
5. A person typing in the session while out of time is a take-over, as today — Task 2 test.

---

### Task 1: The engine's wait and the kept extension

**Files:**
- Modify: `crates/core/src/workflow/run.rs` (`Await::OutOfTime`; `Action::AwaitExtend`; `Run::extended_secs`; `ran_out`, `out_of_time_at`, `extended`, `awaiting_extend`)
- Modify: every exhaustive `match` on `Action` (`task/drive/steps.rs` `act` and `carry_out_without_session`, `steps.rs:241` list, tests' `name()` helpers)
- Test: `crates/core/src/workflow/tests.rs`

**Interfaces:**
- Produces:
  - `#[serde(default, skip_serializing_if = "is_zero")] pub(crate) extended_secs: u64` on `Run` (a private `fn is_zero(n: &u64) -> bool`).
  - `pub(crate) fn ran_out(&mut self, now: u64) -> Action` — only while `awaiting` is `Turn` (else `Idle`): `awaiting = OutOfTime`, a history line `out of time` (`log_here`), returns `Action::AwaitExtend`. The visit stays open.
  - `pub fn awaiting_extend(&self) -> bool`.
  - `pub fn out_of_time_at(&self) -> Option<ApprovalAt>` — the open visit while `OutOfTime`, as `approval_at` is built (reuses `ApprovalAt`: a press drawn from the run's open visit).
  - `pub(crate) fn extended(&mut self, at: &ApprovalAt, by: Duration, now: u64) -> Action` — `Idle` unless `out_of_time_at() == Some(at)`; else `extended_secs += by`, history `extended by <timeout_said>`, `awaiting = Turn`, returns `Action::Prompt(<the extend prompt>)` with the step's label and `timeout_said()`.
  - `Action::AwaitExtend` — "wait for a person to extend the run or stop it".

- [ ] **Step 1: Failing tests** (workflow/tests.rs; drive a run to a turn with the existing helpers there)
  - `a_run_out_of_time_waits_with_its_visit_open`: at a turn, `ran_out` → `AwaitExtend`, `awaiting_extend()`, `!over()`, last visit `ended_at == None`, `out_of_time_at().is_some()`; `ran_out` again → `Idle`.
  - `extending_carries_on_the_step_and_keeps_the_extension`: `extended(&at, 45m)` → `Action::Prompt(text)` with `text == "You ran out of time at Implement. You have 45m more. Carry on with the step from where you were."` (use the step the helper reaches), `awaiting_turn()`, `extended_secs == 2700`, a history line containing `extended by 45m`; a second `extended` with the same `at` → `Idle`.
  - `an_extension_is_kept_through_a_save_and_a_read` (serde round trip; a run with 0 writes no `extended_secs` key).
  - `ran_out_does_nothing_outside_a_turn`: at an approval → `Idle`.
- [ ] **Step 2: Run** `cargo test -p onehand-core --lib workflow::tests` — compile errors / FAIL.
- [ ] **Step 3: Implement.** The new `Action::AwaitExtend` arm in `act` does nothing more than the common tail (save, refresh) in this task; Task 2 adds its note. In `carry_out_without_session` it returns like `AwaitApproval`.
- [ ] **Step 4: Run** `cargo test -p onehand-core --lib` — PASS.
- [ ] **Step 5: Commit** `feat(workflow): a run that runs out of time waits, and an extension is kept`

---

### Task 2: The drive waits instead of cutting, and takes Extend

**Files:**
- Modify: `crates/core/src/task/drive/budget.rs` (`extend(&mut self, by: Duration)`)
- Modify: `crates/core/src/task/drive.rs` (`start`: limit includes `extended_secs`; `Elapsed(Timeout)` with nothing left → `self.ran_out(chat, now, fx)` instead of `cut(TimedOut)`; `stop`: while `awaiting_extend` it ends `Stop::TimedOut`; `waits_on_person` includes `awaiting_extend`; `extend(at, chat, now) -> Vec<Effect>` beside `approve`; `answer` handles `Answer::Extended`)
- Modify: `crates/core/src/task/drive/pin.rs` (`Answer::Extended(ApprovalAt)`)
- Modify: `crates/core/src/task/drive/steps.rs` (`act`'s `AwaitExtend` arm: the note)
- Modify: `crates/core/src/task/drive/link.rs` (`extend(&mut self, at, now) -> Out` beside `approve`)
- Test: `crates/core/src/task/drive/tests.rs`

**Interfaces:**
- Consumes: Task 1's `ran_out`, `extended`, `out_of_time_at`, `awaiting_extend`, `extended_secs`, `Action::AwaitExtend`.
- Produces:
  - `RunDrive::ran_out(&mut self, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>)` (private): held behind a pin like `cut` (`Held::Cut` pattern — add `Held::RanOut`); asks `Asked::Cancel` when `chat.busy`; `after_work = false`; `advance(.., |run| run.ran_out(now.secs))`.
  - `pub(crate) fn extend(&mut self, at: ApprovalAt, chat: Option<&Chat>, now: Now) -> Vec<Effect>` via `command_with`, then `answer(Answer::Extended(at))`.
  - In `answer`: `Answer::Extended(at)` — **while `chat.busy` (the cancelled turn still ending) it is kept in a new field `after_drain: Option<Answer>` and done on the next `Input::TurnEnded`/`Notified` that finds the chat idle**; otherwise `by = template.timeout_limit()` (a run with `none` never gets here; `unwrap_or_default` is fine), `run.extended(&at, by, now.secs)`, and when that is not `Idle`: `budget.extend(by)`, arm `Timer::Timeout` for `budget.left`, then `act`.
  - `TasksLink`/`link.rs::extend(at, now) -> Out` — as `approve`.
  - Note text in `act` for `AwaitExtend`: `Out of time at {label} after {worked(spent)} of {timeout_said}. Extend to go on.`

- [ ] **Step 1: Failing tests** (use `prompted()` from step 1's tests)
  - `running_out_waits_instead_of_ending`: advance past the limit, `Elapsed(Timeout)` → `finished(&fx) == None`, `rig.drive.run().awaiting_extend()`, a `Cancel` asked (the turn was busy), the note starting `Out of time at`, `waits_on_person(true)`.
  - `the_cancelled_turn_is_not_judged`: after running out, `turn_ends("half done")` → no `ReadFacts`, no miss, `outputs` unchanged.
  - `extend_waits_for_the_cancelled_turn_then_carries_on`: run out with the turn still busy; `extend(at)` → no prompt submitted yet; `turn_ends("")` → the extend prompt submitted, `extended_secs == limit`, an `Arm(Timeout)`.
  - `an_extended_run_runs_out_again_only_after_its_extension`.
  - `stop_while_out_of_time_ends_as_a_timeout`: `rig.stop()` → `Outcome::Stopped(Stop::TimedOut)`.
  - `typing_while_out_of_time_is_a_take_over` (as the existing take-over test does, while waiting).
  - `a_resumed_extended_run_keeps_its_extension`: a run with `spent_secs = limit + 10`, `extended_secs = limit` started → `Arm(Timeout)` after `limit - 10`.
- [ ] **Step 2: Run** `cargo test -p onehand-core --lib drive` — FAIL.
- [ ] **Step 3: Implement**; update the existing tests that expected `Stopped(TimedOut)` straight from the timer (`the_clock_ends_the_run_once_nothing_is_left`, `a_review_runs_the_budget`, `a_run_resumed_over_its_limit_times_out_at_once`, and the ones at tests.rs ~775, ~1841) to expect the wait, then `stop()` → `Stopped(TimedOut)` where they assert the outcome.
- [ ] **Step 4: Run** `cargo test -p onehand-core --lib` — PASS.
- [ ] **Step 5: Commit** `feat(task): a run out of time waits for Extend or Stop in its own session`

---

### Task 3: The task reads the wait

**Files:**
- Modify: `crates/core/src/task/work.rs` (`Stand::OutOfTime { step: String, said: String }`; computed under `Working::Waiting` when the run `awaiting_extend()`; its row line and detail line)
- Modify: every exhaustive match on `Stand` in core and app
- Test: `crates/core/src/task/work/tests.rs`

**Interfaces:**
- Produces: `Stand::OutOfTime { step, said }` — `step` the step's label; `said` = `It worked {worked(spent)} of its {timeout_said} timeout.`; the row's line *Out of time at {step}*; grouped as `Group::Waiting` (already, through `Working::Waiting`).

- [ ] **Step 1: Failing test** `a_run_out_of_time_stands_out_of_time` (a task whose run `ran_out`, working `Waiting` → `Stand::OutOfTime { step: "Implement", .. }`, its line).
- [ ] **Step 2–4:** run, implement, run `cargo test -p onehand-core --lib` — PASS.
- [ ] **Step 5: Commit** `feat(task): a task out of time says where and how long it worked`

---

### Task 3 addendum: the next action

In `task/work.rs`, `next_action` for `Stand::OutOfTime`: `said` = *Out of time at {step}*,
primary `Act::Extend` (a new `Act` variant), others `[Act::OpenSession]`. Add `Act::Extend` to
every exhaustive match on `Act`.

---

### Task 4: Extend where Continue is drawn, Stop as it is

**Files:**
- Modify: `crates/app/src/task/carrier.rs` (beside `approve` at ~268: `pub(crate) fn extend(task: &str, at: ApprovalAt, cx: &mut App)` → `give(task, cx, move |run, now| run.extend(at, now))`)
- Modify: `crates/app/src/task.rs:224-245` (`Shown` gets `pub(crate) out_of_time: Option<ApprovalAt>` from `driven.run().out_of_time_at()`; a `pub(crate) fn out_of_time_of(id: &str, cx: &App) -> Option<ApprovalAt>` beside `review_of`)
- Modify: `crates/app/src/chat/pane/step_strip.rs:66-102` (while `shown.out_of_time` is `Some`: *Extend*, `.primary().short()`, before the always-there *Stop*; deferred press calling `task::extend` with the task id and the `at` read at render, as *Continue* defers `approve`)
- Modify: `crates/app/src/tasks_page/detail.rs` (beside `review_card` at ~494: `out_of_time_card` — a page card titled *Out of time*, the stand's `said` line (*It worked … of its … timeout.*), and one `answer_row` with *Extend* (primary) beside *Carries on in the same session*; called where `review_card` is, setting `held` the same way so the trail drops its own primary)
- Modify: `crates/app/src/tasks_page.rs:483 task_acts` (a `TaskAct::Extend` first for `Group::Waiting` when the row's run is out of time: `Row` gets `extend: Option<ApprovalAt>` filled in `task/listing.rs:86-97 row_of` from the driven run), and `TaskAct::Extend`'s label *Extend* and press → `task::extend`; the board's row menu reads `task_acts`, so it gets *Extend* too
- Docs: `docs/workflows.md` (the report table's row *The timeout runs out*: the turn is cancelled and the run waits, `Await::OutOfTime`; *Extend* adds the timeout and carries on in the same session, *Stop* ends it timed out; `extended_secs`), `docs/tasks.md` (states: *Out of time* waits on a person; its actions), `docs/screens.md` (the step strip's actions while out of time)
- Test: `crates/app/src/task/tests/` (a forge-free flow test, as the existing approval flow tests do: a run driven past its timeout shows `Shown::out_of_time`, `task::extend` sends the extend prompt), `crates/app/src/tasks_page` tests if `task_acts` has them

- [ ] **Step 1: Failing tests**
  - `a_run_out_of_time_offers_extend_on_the_strip_and_extend_carries_on` (app task tests, on the mock flow rig the approval tests use).
  - `task_acts_put_extend_first_for_a_run_out_of_time` (a `Row` with `group: Waiting`, `extend: Some(..)`, a session, stoppable → `[Extend, OpenSession, Stop]`).
- [ ] **Step 2: Run** `cargo test -p onehand --lib out_of_time task_acts` — FAIL.
- [ ] **Step 3: Implement**; answer the `ui` skill checklist (peer: *Continue* on the strip and the detail's review card — same builder, variant, size).
- [ ] **Step 4: Run** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.
- [ ] **Step 5: Commit** `feat(task): Extend a run out of time from its strip, its detail and its row`
