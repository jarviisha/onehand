# Out of time, step 3: the board — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A run out of time gives its team place and its step's room back once its agent is at
rest, so the board pulls the next issue; an *Extend* on a board task with a full team waits for a
place, first in line ahead of To do.

**Architecture:** The run's drive learns two states beside the wait: *rested* (out of time, the
cancelled turn and the work it left over) and *queued for a place* (Extend pressed on a board
task, not yet given one). Core's `TasksLink::holds_team` says which board tasks count toward the
team; `board::pull` reserves room for extends waiting and `board::extends_given` hands places to
them oldest first. The app asks for a place on Extend, gives it on every pull and on every place
freed, and pulls when a run rests. A limited step's room is let go on rest and asked again on
Extend, whose prompt then goes once the room is given.

**Tech Stack:** Rust; onehand-core (drive, task link, board); onehand app (board carrier, task carrier).

**Spec:** `docs/superpowers/specs/2026-11-26-out-of-time-design.md`, *The board*. This is the last
step: its PR deletes the spec and the three plans.

## Global Constraints

- Rules in core, the app only carries them: who counts toward the team, who is given a place and
  in what order are core's.
- Words: a task waiting for a place says *Waiting for a team place*; grouped as queued, not as
  needing attention.
- Exhaustive matches on our enums; nothing in core `pub` unless named outside; no comment cites a document.
- Checks: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

1. A run out of time whose cancelled turn has not ended still counts toward the team (no over-run
   while it still works) — Task 1 test.
2. Background work the cancelled turn left (`step_at_work`) keeps it counted until it is done — Task 1 test.
3. Two extends waiting with one place freed: the older one goes, the other keeps waiting, and no
   To do issue starts while any extend waits — Task 2 test.
4. Stop on a task waiting for a place ends it timed out and frees nothing twice — Task 1 test.
5. Extend at a full limited step waits for room, then sends the carry-on prompt (not the step's
   own prompt), in the same visit — Task 1 test.

---

### Task 1: The drive rests, queues for a place, and asks room again

**Files:**
- Modify: `crates/core/src/workflow/run.rs` (an `extend_text: Option<String>` beside `awaiting`, `#[serde(skip)]`; `extended`: at a limited step stores the prompt there and returns `wait_for_room()`)
- Modify: `crates/core/src/workflow/run/room.rs` (`room_given`: with `extend_text` taken, `awaiting = Turn` and `Action::Prompt(text)`, no new visit)
- Modify: `crates/core/src/task/drive.rs` (fields `rested: bool`, `queued_extend: Option<(ApprovalAt, u64)>`; `queue_extend`, `queued_extend_at`, `gives_place`; `waits_on_person` false while queued; the drain arm sets `rested` when `!step_at_work(chat)`; `Notified` sets it when the left work ends; `ran_out` sets it at once when the chat was idle and nothing is at work; on becoming rested: `Effect::Rested`; `stop` while queued clears it; a given extend clears it)
- Modify: `crates/core/src/task/drive/room.rs` (`left_room` also lets go when `rested`)
- Modify: `crates/core/src/task/drive/signals.rs` (`Effect::Rested`)
- Modify: `crates/core/src/task/tasks/link.rs` (`working`: `Queued` while `queued_extend_at` is some; `holds_team(id) -> bool`)
- Test: `crates/core/src/task/drive/tests.rs`, `crates/core/src/workflow/tests.rs`

**Interfaces:**
- Produces:
  - `RunDrive::queue_extend(&mut self, at: ApprovalAt, now: Now) -> Vec<Effect>` — refused (empty) unless `out_of_time_at() == Some(at)`; keeps `(at, now.secs)`, `Effect::Refresh`, and a note *Waiting for a team place.*
  - `pub fn queued_extend_at(&self) -> Option<u64>` (when it asked) and `pub(crate) fn take_queued_extend(&mut self) -> Option<ApprovalAt>`.
  - `pub fn gives_place(&self) -> bool` — `rested && awaiting_extend` or a queued extend.
  - `TasksLink::holds_team(&self, id: &str) -> bool` — `working(id).is_some()` and not `gives_place`.
  - `Effect::Rested` (no fields) — the run gave its place back.
- Consumes: step 2's `ran_out`, `extended`, `Answer::Extended`, `after_drain`.

- [ ] **Step 1: Failing tests**
  - drive: `a_run_out_of_time_rests_once_its_cancelled_turn_ends` (out_of_time → `!gives_place()`; `turn_ends("")` → `gives_place()`, `Rested` in fx).
  - drive: `work_the_cancelled_turn_left_keeps_it_from_resting` (a step at work when the turn ends → not rested; `Notified` once the work ends → rested).
  - drive: `resting_at_a_limited_step_lets_its_room_go` (`LeaveRoom` in fx on rest).
  - drive: `a_queued_extend_waits_for_a_place_then_carries_on` (rested; `queue_extend(at)` → `queued_extend_at().is_some()`, `!waits_on_person`, a note *Waiting for a team place.*; `take_queued_extend()` then `extend(at)` → prompt submitted, `queued_extend_at()` none).
  - drive: `stop_while_waiting_for_a_place_ends_timed_out`.
  - drive: `a_stale_queue_extend_is_refused` (wrong `at` → empty, nothing queued).
  - workflow: `an_extend_at_a_full_step_waits_for_room_then_carries_on` (a template whose step has `limit`; `extended` → `AwaitRoom`; `room_given` → `Prompt(<extend text>)`, visit count unchanged).
  - link: `a_rested_or_queued_board_task_holds_no_team_place` (in `task/tasks/tests.rs` or the link tests: working Waiting/Queued yet `holds_team` false).
- [ ] **Step 2: Run** `cargo test -p onehand-core --lib` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p onehand-core --lib` — PASS.
- [ ] **Step 5: Commit** `feat(task): a run out of time gives its place back at rest, and an extend can wait for one`

---

### Task 2: The board counts places and gives extends theirs first

**Files:**
- Modify: `crates/core/src/board.rs` (`running` takes `holds: &dyn Fn(&str) -> bool` instead of `working`; `pull` takes `extends_waiting: usize` and starts no To do issue past `team - running - extends_waiting`; new `extends_given(board: &Board, running: usize, waiting: &[String]) -> Vec<String>`, oldest first, at most the team's room; new `extends_waiting(tasks, project, asked: &dyn Fn(&str) -> Option<u64>) -> Vec<String>` ordered by when each asked)
- Modify: `crates/core/src/project/drive/board.rs` (`ProjectsDrive::pull` passes `extends_waiting` through)
- Test: `crates/core/src/board/tests.rs` (or the board tests' file)

**Interfaces:**
- Produces: `board::extends_given`, `board::extends_waiting`, the new `running` and `pull` parameters.

- [ ] **Step 1: Failing tests**
  - `extends_waiting_take_places_before_to_do` (team 2, running 1, two waiting → one given, the older; `pull` with `extends_waiting = 1` starts nothing).
  - `no_extend_is_given_past_the_team` (team full → none).
  - `a_board_task_that_gives_its_place_is_not_running` (`running` with a `holds` that says no for one task).
- [ ] **Step 2–4:** run (FAIL), implement, run `cargo test -p onehand-core --lib` (PASS).
- [ ] **Step 5: Commit** `feat(board): extends waiting take places before To do, and a resting task holds none`

---

### Task 3: The app asks, gives and pulls

**Files:**
- Modify: `crates/app/src/task.rs` (`extend` / `extend_task`: a task from a board whose project still has a board queues (`queue_extend`) then calls `crate::board::give_places(project, cx)`; any other task extends at once; `board_running` passes `holds_team`; `board_extends_waiting(project, cx) -> Vec<String>`)
- Modify: `crates/app/src/task/carrier.rs` (`queue_extend(approval, cx)`; `Effect::Rested` → for a board task, `crate::board::pull_now(project, cx)`)
- Modify: `crates/app/src/board.rs` (`pub(crate) fn give_places(project, cx)`: reads `running` and the waiting extends, asks `board::extends_given`, and for each takes the queued `ApprovalAt` (`take_queued_extend`) and extends it; called first in `pull_read`, whose `pull` then gets `extends_waiting` of what is left)
- Modify: `crates/app/src/task/listing.rs` (a task waiting for a place says *Waiting for a team place* as its `at`)
- Docs: `docs/board.md` (*Pulling*: a run out of time gives its place back once its agent is at rest; Extend waits for a place first in line, said *Waiting for a team place*; no To do issue starts while one waits), `docs/tasks.md` (the queued group holds a task waiting for a team place), and **delete** `docs/superpowers/specs/2026-11-26-out-of-time-design.md` and `docs/superpowers/plans/2026-11-26-out-of-time-step-*.md`
- Test: `crates/app/src/task/tests.rs` (a board-free flow is covered by step 2's test; add `an_extend_with_no_board_goes_at_once` only if `extend_task`'s branch is not reached by existing tests), `crates/app/src/task/listing.rs` tests if present

- [ ] **Step 1: Failing test** — the listing says *Waiting for a team place* for a task whose drive has a queued extend (rig: a driven run out of time, rested, `queue_extend`).
- [ ] **Step 2–3:** run (FAIL), implement.
- [ ] **Step 4: Run** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.
- [ ] **Step 5: Commit** `feat(board): an extend waits for a team place first in line, and a resting task frees one`
