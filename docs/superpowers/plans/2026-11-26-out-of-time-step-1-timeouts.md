# Out of time, step 1: timeouts and the clock — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A workflow's `timeout` and a command step's own `timeout` accept `none`, and a run's
clock counts only the time an agent works on a step.

**Architecture:** One parser for a limit (`parse_limit`: a duration or `none`) in
`workflow/template.rs`; `Budget` learns an unlimited limit; `RunDrive` pauses the budget for every
action but a prompt and resumes it when a prompt is handed to the session; the command step's
limit travels on `Effect::RunCommand` to `run_command_blocking` and `process::output_until`.

**Tech Stack:** Rust, onehand-core (GUI-free), GPUI app crate for the workflow form.

**Spec:** `docs/superpowers/specs/2026-11-26-out-of-time-design.md` (sections *Configuration* and
*The clock*; `extended_secs` is step 2's, not this one's).

## Global Constraints

- Core stays GUI-free and runtime-free; nothing in core is `pub` unless named outside the crate.
- No comment or string names a document. Every `.md` in English.
- `none` is the only word for no limit, compared after `trim()`, case-sensitive like the units.
- A command step with no `timeout` keeps 15 minutes. New workflows keep `45m`.
- Where a timeout is shown to a person, `none` reads **no limit**.
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`.

## Review Focus

1. A run with `timeout = "none"` arms no timer at all and never ends `TimedOut`, even after days
   (`Elapsed(Timeout)` arriving anyway from an old timer is harmless) — Task 2 test.
2. A run resumed with `spent_secs` already over a finite limit still times out at once, as today —
   Task 2 test.
3. A command step with `timeout = "none"` runs until it exits or is called off; a Stop still kills it
   — Task 3 test on `output_until` with `None` and a cancel.
4. An unreadable command `timeout` (`"soon"`, `"0m"`) is refused by validation, naming the step — Task 1 test.
5. Time spent pinning a mark, running a command, pushing or opening a pull request is not counted:
   a run that only ran a long command has `spent_secs` unchanged — Task 2 test.

---

### Task 1: The limit grammar and the command step's `timeout`

**Files:**
- Modify: `crates/core/src/workflow/template.rs` (parser, `Template::timeout_limit`, `StepKind::Command`, `StepSpec::command_limit`, `describe` line for Command untouched)
- Modify: `crates/core/src/workflow/validate.rs:42-47` and the `StepKind::Command` arm (~224)
- Modify: every `StepKind::Command { command, on_fail }` pattern that binds fields (`template.rs:256`, `validate.rs:224`, `app/src/workflows_page/draft.rs:153,310,503`, `task.rs:272`, tests) to add `timeout` or `..`
- Test: `crates/core/src/workflow/template.rs` (`every_tests`), `crates/core/src/workflow/tests.rs`

**Interfaces:**
- Produces:
  - `pub fn parse_limit(text: &str) -> Option<Option<Duration>>` — `Some(None)` for `none`,
    `Some(Some(d))` for what `parse_every` reads, `None` when unreadable. Exported from `workflow.rs`
    beside `parse_every`.
  - `Template::timeout_limit(&self) -> Option<Duration>` — `None` = no limit; unreadable falls back to `45m`.
  - `Template::timeout_said(&self) -> String` — `"no limit"` for `none`, else the text as written.
  - `StepKind::Command { command: Option<String>, on_fail: String, #[serde(default, skip_serializing_if = "Option::is_none")] timeout: Option<String> }`
  - `StepSpec::command_limit(&self) -> Option<Duration>` — for a command step: `timeout` absent → `Some(15m)`, `none` → `None`, unreadable → `Some(15m)`; for any other kind → `None`.
  - `pub(crate) const COMMAND_LIMIT: Duration` moves from `facts.rs` to `template.rs` (15 min).

- [ ] **Step 1: Failing tests**

```rust
// every_tests
#[test]
fn a_limit_is_a_duration_or_none() {
    assert_eq!(parse_limit("none"), Some(None));
    assert_eq!(parse_limit(" none "), Some(None));
    assert_eq!(parse_limit("2h"), Some(Some(Duration::from_secs(7200))));
    assert_eq!(parse_limit("None"), None);
    assert_eq!(parse_limit("0m"), None);
    assert_eq!(parse_limit(""), None);
}

// workflow/tests.rs
#[test]
fn a_workflow_without_a_limit_has_none() {
    let mut t = builtin::all().remove(0);
    t.timeout = "none".into();
    assert!(validate::validate(&t).is_empty());
    assert_eq!(t.timeout_limit(), None);
    assert_eq!(t.timeout_said(), "no limit");
}

#[test]
fn a_command_step_has_fifteen_minutes_unless_it_says() {
    // a command step parsed from TOML with no timeout, with "30m", with "none"
    // command_limit(): Some(900s), Some(1800s), None
}

#[test]
fn an_unreadable_command_timeout_is_refused_naming_its_step() {
    // timeout = "soon" on step `verify` → a problem on that step reading
    // "its timeout `soon` is not a duration such as 15m or 2h, or none"
}

#[test]
fn a_command_timeout_survives_a_save_and_a_read() {
    // store::parse(store text of a template whose verify step has timeout = "none")
    // round-trips; a step with no timeout writes no `timeout` key
}
```

- [ ] **Step 2: Run** `cargo test -p onehand-core limit` — expect compile errors / FAIL.
- [ ] **Step 3: Implement** the interfaces above. Validation of the workflow timeout uses
  `parse_limit`, message: ``the timeout `{}` is not a duration such as 45m, 2h or 90s, or none``.
  `facts.rs` imports `COMMAND_LIMIT` from `template.rs` until Task 3 removes its use there.
- [ ] **Step 4: Run** `cargo test -p onehand-core` — PASS (fix every pattern the new field breaks).
- [ ] **Step 5: Commit** `feat(workflow): a timeout may be none, and a command step may name its own`

---

### Task 2: The budget counts only the agent's work, and may be unlimited

**Files:**
- Modify: `crates/core/src/task/drive/budget.rs` (whole struct)
- Modify: `crates/core/src/task/drive.rs:136-175` (`start`), `:250` (`spent_secs`), `:429-440` (`Elapsed(Timeout)`)
- Modify: `crates/core/src/task/drive/steps.rs` (`act`: pause; `send`: resume)
- Modify: callers of `timeout_limit()` in tests (`.unwrap()`)
- Test: `budget.rs` tests, `crates/core/src/task/drive/tests.rs`

**Interfaces:**
- Consumes: `Template::timeout_limit() -> Option<Duration>` (Task 1).
- Produces:
  - `Budget::start(limit: Option<Duration>, now: Instant, running: bool) -> Budget`
  - `Budget::left(&self, now: Instant) -> Option<Duration>` — `None` when unlimited.
  - `Budget::spent(&self, now: Instant) -> Duration`
  - `RunDrive::spent_secs` = `spent_before + budget.spent(now)`.
  - The rule: **`act` pauses the budget for every action except `Action::Prompt`; `send` resumes it
    when the prompt is actually handed over** (after `settle`, where `sent_at` is set). The existing
    resumes (card answered in `Notified`, approval answered, room given, status checks seen) stay;
    each is followed by an `act` that pauses again unless it prompts.
  - `RunDrive::start` creates the budget **not running** (the first prompt starts it), arms
    `Timer::Timeout` only when the limit is `Some`, for `limit - spent_before` (zero when over).

- [ ] **Step 1: Failing tests** (drive tests; reuse `Rig`, `checking()`, `at_approval()`)

```rust
#[test]
fn an_unlimited_run_arms_no_timer_and_never_times_out() {
    let mut t = checkout(); t.timeout = "none".into();
    let (mut rig, fx) = Rig::start(t, None, connected());
    assert!(!names(&fx).contains(&"Arm(Timeout)"));
    rig.now += Duration::from_secs(60 * 60 * 24 * 3);
    let fx = rig.on(Input::Elapsed(Timer::Timeout));
    assert_eq!(finished(&fx), None);
}

#[test]
fn the_clock_does_not_run_before_the_first_prompt() {
    let (mut rig, _) = Rig::start(checkout(), None, connected());
    rig.now += Duration::from_secs(90);
    rig.on(Input::Pinned(Ok("c0".into())));
    assert_eq!(rig.drive.run().spent_secs, 0);
}

#[test]
fn a_command_does_not_spend_the_clock() {
    let (mut rig, fx) = checking();   // a run whose one step is a command, in a session
    assert!(names(&fx).contains(&"RunCommand"));
    rig.now += Duration::from_secs(600);
    rig.on(Input::CommandExited(ran(true)));
    assert_eq!(rig.drive.run().spent_secs, 0);
}

#[test]
fn a_run_resumed_over_its_limit_times_out_at_once() {
    // run with spent_secs = limit + 1 → start arms Arm(Timeout) after 0; once a prompt is sent,
    // Elapsed(Timeout) ends it Stopped(TimedOut)
}
```

  Update the two tests whose premise changes, keeping their intent:
  `the_clock_ends_the_run_once_nothing_is_left` advances time **after** the first prompt is sent;
  `spent_time_is_folded_into_what_is_saved` counts 90 s **after** the prompt, read at the next save.
  `budget.rs` tests: add one for `start(None, ..)` → `left` is `None`, `spent` still counts.

- [ ] **Step 2: Run** `cargo test -p onehand-core drive` — FAIL.
- [ ] **Step 3: Implement.** `Elapsed(Timeout)`: `match self.budget.left(now.at) { Some(z) if z.is_zero() => cut, Some(left) => re-arm, None => {} }`.
- [ ] **Step 4: Run** `cargo test -p onehand-core` — PASS.
- [ ] **Step 5: Commit** `feat(task): a run's clock counts only its agent's work, and may have no limit`

---

### Task 3: The command step's limit reaches the command

**Files:**
- Modify: `crates/core/src/process.rs:70-110` (`output_until(cmd, limit: Option<Duration>, cancel)`; `output` passes `Some(limit)`; its two tests)
- Modify: `crates/core/src/workflow/facts.rs` (`run_command_blocking(dir, command, limit: Option<Duration>, cancel)`; the timed-out tail reads `timed out after {m}m` as now)
- Modify: `crates/core/src/task/drive/signals.rs:188` (`RunCommand { dir, command, limit: Option<Duration> }`), `steps.rs` (both `RunCommand` sites fill `limit` from `self.run.current().and_then(StepSpec::command_limit)` — a check task's one step is a command step, so it gets 15m as now)
- Modify: `crates/app/src/task/carrier.rs:430,600-615` (pass `limit` through)
- Test: `process.rs` tests

**Interfaces:**
- Consumes: `StepSpec::command_limit` (Task 1).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn no_limit_runs_until_called_off() {
    // output_until(sh -c "sleep 5", None, cancel) with cancel set from another thread after 100ms
    // → Err(Failure::Cancelled) in well under 5s
}
```

- [ ] **Step 2: Run** `cargo test -p onehand-core process` — FAIL (signature).
- [ ] **Step 3: Implement**; the deadline is `limit.map(|l| Instant::now() + l)`.
- [ ] **Step 4: Run** `cargo test --workspace --locked` — PASS.
- [ ] **Step 5: Commit** `feat(workflow): a command step runs as long as its own timeout says`

---

### Task 4: Where people set and read it

**Files:**
- Modify: `crates/app/src/workflows_page/form.rs:119` — about: `Working time, such as 45m or 2h, or none`
- Modify: `crates/app/src/workflows_page/draft.rs` — the step draft gets `pub timeout: Entity<InputState>` (its own input; not the status checks' `wait`), loaded from a command step's `timeout` (empty when absent), saved as `Some(trimmed)` when not empty
- Modify: `crates/app/src/workflows_page/step.rs:550-557` — under *Command*, a field *Timeout*, about `Such as 15m or 2h, or none; 15m when empty`, disabled when shipped
- Modify: `crates/core/src/preflight.rs:401`, `crates/app/src/dialogs.rs:717`, `crates/core/src/workflow/run/retry.rs:69`, `crates/core/src/task/work.rs:206` — show `timeout_said()`
- Modify docs: `docs/workflows.md` (the example file's `timeout` comment, a command step's `timeout` key, *The clock* paragraph at ~417: counts only while the agent works on a step; paused for commands, forge calls, a mark, a prompt waiting for the link, as for a person), `docs/tasks.md` (the Retry dialog's timeout line may read *no limit*)
- Test: `crates/app/src/workflows_page/draft/tests.rs`

- [ ] **Step 1: Failing test** — a draft loaded from a template whose command step has `timeout = "none"` saves it back unchanged; one with an empty field saves `timeout: None`.
- [ ] **Step 2: Run** `cargo test -p onehand draft` — FAIL.
- [ ] **Step 3: Implement** the field and the display changes; follow the `ui` skill's checklist for the new field.
- [ ] **Step 4: Run** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt` — all clean.
- [ ] **Step 5: Commit** `feat(workflow): set a step's and a workflow's timeout, none included, in the editor`
