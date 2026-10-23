# Runs off windows, step 1: the driver decides in core

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every decision `crates/app/src/task/driver*.rs` makes today is made by a GUI-free state
machine in core, `onehand_core::task::drive::Drive`. It is tested rule by rule. The app's driver
becomes an adapter that feeds it inputs and performs its effects. No behaviour change.

**Architecture:**
- `Drive` holds the run and the driver's own state: today's `Driven` minus every GPUI handle.
- `Drive::on(input, chat, now) -> Vec<Effect>`. `chat: Option<&onehand_core::chat::Chat>` is
  the run's session as it stands, or `None` once it is gone. The machine reads only that, and
  every query the driver makes today (`link`, `prompted_beyond`, `prose_since`, `offer`,
  `in_force`, `held`, `picks_waiting`, `pick_waiting`, `awaiting_permission`, `busy`,
  `cancelled`, `failed`/`failure`, `modes`) is already a `Chat` method or field.
- Everything that mutates the session, touches the disk or waits on time is an `Effect` that
  the app carries out, and the result comes back as an `Input`.
- Held calls stop being boxed closures and become queued `Held` values, replayed by the
  machine.

**Tech Stack:** Rust; core (`onehand-core`, no GUI, no runtime), app (GPUI).

**Spec:** `docs/superpowers/specs/2026-10-22-runs-off-windows-design.md`, decisions 1 and 2,
step 1.

## Global Constraints

- **No behaviour change.** Every comment in today's `driver.rs`, `driver/settings.rs` and
  `driver/forge.rs` that states a rule is preserved in the module that now owns that rule, and
  gets a test (the list is under each task).
- **The machine is synchronous.** It has no timers, threads, `async` or I/O. "Wait N" is
  `Effect::ArmClock`, and "do this after the current event" is `Effect::Defer(Input)`.
- **Core stays GUI-free:** `cargo tree -p onehand-core -i gpui` must error. `#![warn(unreachable_pub)]`
  applies: `Drive`, `Input`, `Effect` and `Held` are `pub` (the app names them); helpers are
  private.
- **No file past about 800 lines of code.** `task/drive.rs` splits into `drive/settings.rs` (the
  picks) and `drive/tests.rs` from the start. The app's `driver.rs` must come out smaller than
  it is.
- **Every `Effect` and `Input` match in the app is exhaustive** (the app's guards).
- Comments describe; they never cite a document.

## Interfaces (shared by every task)

```rust
// crates/core/src/task/drive.rs
pub struct Drive { /* task, run, pinned, held, sent, answer_from, pending, up, settled,
                      picking, mode_out, noted, budget, spent_before, card,
                      command_running: bool, stopping: Option<Stop>, over: bool */ }

pub enum Input {
    // from the session
    Appended, TurnEnded, AwaitingUser, Disconnected, PickRefused(String),
    Notified,          // any notify: a card may have been answered
    SessionGone,
    PicksAnswered,     // the deferred follow-up to Appended while picking
    // from a person
    Approved { at: String }, Revised { at: String, note: String }, Stop,
    // results of effects
    Pinned(Result<String, String>),
    Measured(Result<Mark, String>),
    TurnRead(Result<Facts, String>),
    NotTaken,          // the session refused the prompt
    CommandExited(CommandResult),
    ForgeDone(Result<Option<PrOpened>, String>),
    ChecksSeen(Seen),
    ClockFired,
}

pub enum Effect {
    Pin { dir: PathBuf, refs: Vec<String> },
    Measure { dir: PathBuf },
    ReadFacts { dir: PathBuf, from: <step_from type> },
    Submit(String),
    SetMode(String),
    Pick(Pick),
    CancelTurn,
    Note(String),
    RunCommand { dir: PathBuf, command: String },
    CallOffCommand,               // set the running command's / forge call's cancel flag
    Forge { commit: Option<String> },
    WatchChecks { pushed: Option<String>, wait: Duration },
    ArmClock(Duration),
    Defer(Input),
    Save,                         // write `drive.run()` (with spent_secs) into the task
    Waiting, Answered, CameUp,    // the unattended report's hooks for this task
    Pause { why: String, fix: String },
    Refresh,
    Finish { outcome: Outcome },  // the app does today's `finish`: note, keep, store, ended, drop
}

impl Drive {
    pub fn start(task: String, run: Run, first: Action, pinned: usize, now: Instant,
                 chat: Option<&Chat>) -> (Drive, Vec<Effect>);
    pub fn on(&mut self, input: Input, chat: Option<&Chat>, now: Instant) -> Vec<Effect>;
    pub fn task(&self) -> &str;
    pub fn run(&self) -> &Run;              // spent_secs already folded in for Save/Finish
    pub fn pinned(&self) -> usize;
    pub fn waits_on_person(&self) -> bool;
    pub fn stopping(&self) -> bool;
    pub fn card(&self) -> bool;
    pub fn spent_secs(&self, now: Instant) -> u64;
}
```

The variants above are the plan's best reading of today's driver. Implementing may show that a
variant is missing or that two should merge. When that happens, change it in the code and record
it in the ledger; the tasks that use it read the ledger.

## Review Focus

1. **Order of effects within one input**, as today's code runs them:
   - the step's announcement note comes before the action's own note;
   - `Save` comes after the action;
   - `Refresh` comes last.
   The adapter performs effects in the order returned (Task 4, by inspection plus the app
   tests).
2. **`Defer`** is used exactly where today's code calls `cx.defer` inside the driver:
   - `settings::answered` after `Appended`;
   - `end(Closed)` on release;
   - `unattended::answered` after a card is answered.
   Nowhere else (Task 3).
3. **A finished machine ignores every input** (`over`), as today's `live.remove` makes later
   events find nothing (Task 1 test).
4. **`spent_secs`** counts the budget's used time plus `spent_before`, and `Save`/`Finish` see
   it folded into the run (Task 1 test).
5. **The session gone** (`chat == None`) with a non-ending action: nothing is carried out, as
   today's `carry_out` (Task 1 test).

---

### Task 1: The machine's frame (holding, acting, ending, the clock)

**Files:**
- Create: `crates/core/src/task/drive.rs` (`pub mod drive;` in `crates/core/src/task.rs`) and
  `crates/core/src/task/drive/tests.rs`.

It moves from `driver.rs`: `Driven`'s fields, `held`, `answer`, `cut`, `end`, `advance`, `act`
(the pin before an action), `carry_out` (with `announce_step`, `ends_run`, `short`), `measure`'s
two outcomes, `after_turn`'s outcome (`turn_ended` / `failed`), `run_command`'s outcome,
`clock`, `finish`'s decision and the timeout's `cut(TimedOut)`.

- [ ] **Step 1: Write the failing tests.** Each builds a `Run` from a shipped template (the
  workflow tests' fixtures; find them with `grep -n "fn template\|fn run_of" crates/core/src/workflow`)
  and a `Chat::default()`:
  - `a_pin_holds_what_is_asked_meanwhile_and_an_ending_wins`: start → `Pin`. `Approved` and
    `Stop` arrive before `Pinned(Ok)`. After `Pinned`, the effects end the run (`Finish` with
    `Stopped(ByPerson)`) and no `Submit` is among them.
  - `a_turn_ending_while_a_pin_is_held_is_not_judged`: `TurnEnded` while held → no `ReadFacts`.
  - `the_first_stop_reason_is_the_one_kept`: `RunCommand` running; `Stop` then `ClockFired`
    with the budget gone → one `CallOffCommand`; `CommandExited` → `Finish(Stopped(ByPerson))`.
  - `a_stop_while_a_command_runs_waits_for_it`: `Stop` while `RunCommand` → `CallOffCommand`,
    no `Finish` until `CommandExited`.
  - `the_budget_does_not_run_while_a_person_is_waited_on`: `AwaitApproval` pauses, and the clock
    fired meanwhile re-arms rather than ending.
  - `the_clock_ends_the_run_once_nothing_is_left`: `ClockFired` with the budget gone → `CancelTurn`
    then `Finish(Stopped(TimedOut))`.
  - `a_finished_drive_ignores_everything`: after `Finish`, any input → no effects.
  - `a_gone_session_only_lets_the_run_end`: `chat = None` with `Action::Prompt` → no effects;
    with `Finish` → `Finish`.
  - `the_step_is_announced_once_per_step`: two actions in the same step → one `Note("Step 1 of …")`.
  - `spent_time_is_folded_into_what_is_saved`.
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core task::drive`.
  Expected: a compile error.
- [ ] **Step 3: Implement** the frame. `send`, `came_up` and the picks can return
  `Effect::Submit` directly for now; Task 2 replaces that.
- [ ] **Step 4: Verify:** `cargo test -q -p onehand-core task::drive` passes, and so does the
  whole core suite.
- [ ] **Step 5: Commit:** `feat(task): a run's driving decided in core: holding, acting, ending and the clock`.

### Task 2: Sending, coming up, and the visit's settings

**Files:**
- Modify: `crates/core/src/task/drive.rs`.
- Create: `crates/core/src/task/drive/settings.rs`, moved from `crates/app/src/task/driver/settings.rs`
  (`Picking`, `open_visit`, `wanted`, `settle`, `answered`, `refused`, `plan`, `said`, and the
  setting failure that pauses unattended runs for an issue's run). Whether the task is an
  issue's comes in as a `bool` on `Drive::start`.

It moves from `driver.rs`: `send` (pending until connected, take-over before sending,
`came_up`, `settle`, `answer_from`, `sent`, and `NotTaken` → failed), `came_up` (the mode
refused, `SetMode`, the `Mode {mode} set` note, `start_settings`) and `start_settings`.

- [ ] **Step 1: Write the failing tests** (a `Chat` driven by `chat.apply(AcpEvent::…)` to
  connect it, offer modes and config options, and answer picks):
  - `a_prompt_waits_for_the_agent_to_come_up`: not connected → no `Submit`; `Appended` once
    connected → `Submit`.
  - `a_prompt_of_a_persons_own_is_a_take_over`: `prompted_beyond(sent)` →
    `Finish(Stopped(TakenOver))`.
  - `a_mode_the_agent_does_not_offer_fails_the_run_on_its_mode`: → `Pause` +
    `Finish(Failed(..))` with the mode's words.
  - `the_runs_mode_is_set_before_its_first_prompt`: `SetMode` then `Note("Mode … set")` before
    any `Pick`/`Submit`.
  - `picks_go_one_at_a_time_and_the_prompt_waits`: two options wanted → `Pick`, then after
    `PicksAnswered` the next `Pick`, then `Submit` and the `Settings: …` note.
  - `a_refused_pick_fails_the_run_at_the_step`; `a_refusal_of_the_runs_mode_is_not_a_steps`;
    `a_persons_own_pick_refused_is_not_the_runs`.
  - `a_prompt_not_taken_fails_the_run`.
- [ ] **Step 2: Run them to see them fail:** `cargo test -q -p onehand-core task::drive`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify,** then **Step 5: Commit:** `feat(task): a run's prompts and settings decided in core`.

### Task 3: The session's events, cards, commands and the forge

**Files:**
- Modify: `crates/core/src/task/drive.rs`. Add the `on_event` mapping (`Appended` → take-over
  or `Defer(PicksAnswered)`; `TurnEnded` → `Stop` when cancelled, failed when failed,
  `ReadFacts` when judged; `AwaitingUser` → card, pause, `Waiting`; `Disconnected` →
  `End(LinkLost)`; `PickRefused`), the card answered on `Notified` (→ `Answered`, as a `Defer`),
  `SessionGone` → `Defer` of `End(Closed)`, `Approved`/`Revised`, `CommandExited`,
  `ForgeDone`, `ChecksSeen` (resume the budget), and the actions `Push`, `OpenPullRequest` and
  `AwaitStatusChecks`.

- [ ] **Step 1: Write the failing tests:**
  - `a_cancelled_turn_is_a_persons_stop`;
  - `a_failed_turn_fails_the_run_in_its_words`;
  - `a_card_pauses_the_budget_and_says_it_waits`;
  - `an_answered_card_resumes_and_says_so_later`;
  - `a_lost_link_ends_the_run`;
  - `a_closed_session_ends_the_run_after_the_event`;
  - `a_push_notes_the_commit_and_calls_the_forge`;
  - `status_checks_pause_the_budget_and_free_the_slot`;
  - `checks_seen_resume_the_budget`;
  - `an_approval_is_judged_against_the_visit_it_was_drawn_from` (an approval for a stale `at` →
    `Idle`, no budget resume).
- [ ] **Step 2: Run them to see them fail;** **Step 3: Implement;** **Step 4: Verify;**
  **Step 5: Commit:** `feat(task): a run's events, cards and forge steps decided in core`.

### Task 4: The app's driver becomes the adapter

**Files:**
- Modify: `crates/app/src/task/driver.rs`:
  - `Driven` keeps `drive: Drive`, the session handle, the window, the `Subscription`s and
    `Task`s, the command's cancel flag and the status-check task.
  - `start`, `approve`, `revise`, `stop` and the session subscriptions become "build an
    `Input`, call `drive.on(input, Some(&session.read(cx).chat), Instant::now())`, then
    `perform(effects)`".
  - `perform` matches every `Effect` exhaustively. The I/O bodies stay here: today's `act`
    spawn, `measure` spawn, `after_turn` spawn, `run_command` spawn, `forge::on_forge`,
    `forge::watch_status_checks`, `clock` and `finish`'s side.
- Modify: `crates/app/src/task/driver/forge.rs` (the I/O only, feeding `ForgeDone` /
  `ChecksSeen`).
- Delete: `crates/app/src/task/driver/settings.rs` (moved to core in Task 2).
- Modify: `crates/app/src/task.rs` where it reads `Driven` fields (`d.run`, `d.task`,
  `d.stopping`, `d.waits_on_person()`, `d.command`): through `drive` accessors.
- Modify: `docs/workflows.md`, where it describes the driver: the decisions are core's
  (`task::drive`), and the app carries them out.

- [ ] **Step 1: Switch over.** Keep each moved comment with the code that now owns its rule.
- [ ] **Step 2: Verify:**
  - `cargo test -q --workspace --locked` (the app's guards included);
  - `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`;
  - `cargo tree -p onehand-core -i gpui` (must error);
  - `wc -l crates/app/src/task/driver.rs` is smaller than 793;
  - `grep -n "fn plan\|fn settle\|Picking" crates/app/src/task` finds nothing.
- [ ] **Step 3: Commit:** `refactor(task): the app's driver carries out what core decides`. Open the PR
  against `main`.
- [ ] **Step 4: Hand the owner a manual check:**
  - the Mock UI agent (CLAUDE.md) on a workflow run from the launcher: steps, an approval
    (Continue and Revise), Stop mid-turn, and a check command;
  - an unattended run if one is set up.
