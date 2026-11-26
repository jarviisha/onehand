# Out of time: a run that runs out waits, and its time is the person's to set

## Why

Board task #409 (issue 50, PR #425) ran out of its 45 minutes inside Implement. The run ended
`Stopped(TimedOut)`; the only way on was *Retry*, a new run in a new session, so the 44 minutes of
context the agent had built (what it had tried, what it had proved) were gone. The agent was never
told why its turn was cancelled and wrote "I stopped as you asked". The retry then spent 2531 of its
2700 seconds: Implement 22 min, Verify (onehand's own command) 14 min, Review 38 min, all off one
clock, so the step at the end is the one cut, through no fault of its own. Verify also came within
80 s of the command steps' fixed 15-minute limit.

The owner wants: running out of time never throws work or context away; the time a run may take is
theirs to set, unlimited included; the agent knows when it was stopped for time.

## Goals

- A run whose time runs out **waits for the person** instead of ending: same run, same session,
  same context. The person *Extends* it (it goes on where it was) or *Stops* it.
- While it waits, a board task **gives its team place and its step room back**, so the board pulls
  the next issue. An *Extend* waits for a team place again, **ahead of every issue in To do**.
- **The clock counts only the agent's working time.** A command step does not spend it.
- **Timeouts are configurable, unlimited included**: the workflow's `timeout` accepts `none`, and a
  command step may name its own `timeout` (`none` included), 15 minutes when it names none.
- **The agent is told.** The transcript says the run ran out of time, never that a person stopped
  it, and the prompt an *Extend* sends says so too.

## Non-goals

- A soft warning to the agent before time runs out (a "wrap up" prompt). Separate idea, not this.
- A timeout per agent step. The run keeps one budget; *Extend* is the way past it.
- Carrying a session's context into a *Retry*. Retry stays a new run in a new session.
- A board-level or project-level timeout override. The timeout stays the workflow's; a board on a
  built-in workflow changes it through *Duplicate to edit*, which already rebinds the board.

## Anti-goals

- An out-of-time run counted as ended anywhere: it is not on *Done*, does not close its session or
  its issue, and does not let the board treat its issue as free.
- *Resume*'s meaning changing. It stays "a run that was cut off (agent gone, session gone, app
  restarted)".
- An *Extend* that runs past the team size the person set.

## Approaches

1. **A new wait in the run (`Await::OutOfTime`), chosen.** The run is not over; its drive lives,
   its turn is cancelled, its session is kept. *Extend* adds to the `Budget` and carries on. The
   board stops counting it toward the team while it waits. True to what happens (waiting on a
   person, not stopped), history stays clean, and the Tasks page already has a "waiting on a
   person" group.
2. **Make `Stop::TimedOut` resumable, Resume reusing the live session.** Less new state, but it
   bends Resume's meaning, shows a waiting run as *Stopped*, needs budget arithmetic for a run with
   0 left, and still needs a team-place path for a board task's Resume to honour the team size.
   Not much cheaper once done right.

The owner picked 1, and for a board task picked (a): an *Extend* with the team full waits for a
place, first in line, rather than running over the team.

## How it works today (what this touches)

- `workflow::Template::timeout` (`"45m"`), parsed by `parse_every`, refused by `validate.rs` when it
  does not read. The workflow form has a *Timeout* field (`workflows_page/form.rs`).
- `task::drive::Budget` counts working time; it pauses while a card, an approval, room or status
  checks wait. `RunDrive::start` arms `Timer::Timeout` for `limit - spent_secs`; on
  `Elapsed(Timeout)` with nothing left it calls `cut(Stop::TimedOut)` (`task/drive.rs:433`).
- `workflow::facts::COMMAND_LIMIT` (15 min) bounds every command step.
- `Run::awaiting: Await` (not persisted; worked out again from the step on resume).
- `TasksDrive::working` says `Queued`/`Running` from the place queue; `TasksLink::working`
  (`task/tasks/link.rs`) turns `Running` into `Waiting` through `RunDrive::waits_on_person`.
  `board::running` counts a board task as on the team while `working` is `Some`, and
  `board::taken` keeps its issue out of the Backlog.
- Step room: `RunDrive::await_room` / `room_given` / `left_room`, `TasksDrive::ask_room`.

## The design

### Configuration

- `timeout` on a workflow reads `none` as no limit, besides `45m`/`2h`/`90s`. New workflows still
  get `45m`. The form's field takes `none`; its hint says so.
- A command step may carry `timeout`, the same grammar. Absent: 15 minutes, as today. `none`: no
  limit; the command ends when it exits or the run is stopped.
- A run keeps its timeouts in its snapshot as now, so editing a workflow does not change a run
  under way. The Retry dialog's *timeout* line shows `none` as *no limit*.
- An unlimited run never reaches *Out of time*; only *Stop* ends it.

### The clock

- **The clock runs only while the agent works on a step**: from a prompt the run sends until the
  step's turn, and the agent work it left running (`after_work`), are done. It pauses for a command
  step (its own limit bounds it), forge calls, a mark being pinned and a prompt waiting for the
  link, as it already pauses for a person, room and status checks. *(Builder's call: so time can
  only run out while the agent works, and there is always a turn to cancel.)*
- `spent_secs` therefore means agent working time; the task detail's figures read the same field.
- **An extension is kept in the run file** (`extended_secs`, added beside the old fields with a
  default of 0), and `RunDrive::start` arms `timeout + extended_secs - spent_secs`. A run extended
  and then cut off by a restart keeps its extension on *Resume*. A run cut off while out of time
  comes back on *Resume* in a new session and is out of time again at once: honest, since nobody
  had given it more. *(Builder's call.)*

### Out of time

- When the budget runs out, the run's drive cancels the turn (as now) and the run moves to
  `Await::OutOfTime` at its current step and visit. The run is **not** over: no `Outcome`, no end
  of visit, its session kept open and in the rail.
- The budget can run out only while the agent works (see *The clock*). A review step (its own
  session) runs out like any agent step, in its own lane.
- **The cancelled turn drains first.** The run enters the wait at once (the strip and the board
  show *Out of time*), but its cancelled turn's end and facts are not judged as the step's answer.
  *Extend* or *Stop* pressed before the turn has ended is held, as a Stop is held today, and done
  once it has. *(Builder's call.)*
- **The team place is given back once the agent is at rest**: when the cancelled turn has ended and
  no agent work it left (`after_work`: a subagent, a background command) is still running in the
  session. Until then the task still counts toward the team, so a wait never runs over the team in
  effect. *(Builder's call.)*
- The transcript gets a note: *Out of time at <step> after <spent> of <timeout>. Extend to go on.*
  It replaces the cancelled turn's ambiguity: nothing in the transcript says a person stopped it.
- The task is `Working::Waiting` with a new stand, `Stand::OutOfTime { step, spent, limit }`,
  grouped with what waits on a person, flagged *Needs attention* on the board's row.
- **Actions**: *Extend* (primary) and *Stop*, on the run's strip under the composer, in the task
  detail and in the board row's menu, worded like the approval's *Continue*.
  - *Extend* adds the workflow's own timeout to the budget (`45m` more for a 45-minute workflow),
    then sends the session a prompt: *You ran out of time at <step>. You have <n> more minutes.
    Carry on with the step from where you were.* The step then goes on as if it had not stopped:
    same visit, same gates.
  - *Stop* ends the run `Stopped(TimedOut)`, exactly as a timeout ends one today, so *Retry* and
    everything downstream behave as now.
- A person typing in the session while it waits is a take-over, as today for any run.
- **A restart while out of time**: `Await` is not persisted, so the run comes back cut off and is
  resumed as any cut-off run is (new session, *Resume*). Accepted: the context is lost only if the
  app goes down while nobody has answered.

### The board

- An out-of-time task **does not count toward the team** (`board::running` skips it) and **leaves
  the room** of a limited step (`left_room` fires on entering the wait). It still holds its issue
  (`board::taken`), its worktree and its place, and stays in its step's column.
- A board pull happens when a task goes out of time (room freed), as for any room freed.
- *Extend* on a board task asks for a team place. With room, it goes on at once. Without, it waits,
  said as *Waiting for a team place*, and is **first in line**: the next pull gives places to
  waiting extends before any To do issue. While it waits it is `Working::Queued` (grouped as
  queued, not as needing attention) and does not count toward the team; once given a place it is
  `Running` and counts. *(Builder's call.)* Its step's room, when the step is limited, is asked for
  again as any run asks for room, after the team place.
- Lowering the team below what runs changes nothing for a task already going on, as now.
- A task outside a board has no team: *Extend* goes on at once (room at a limited step still
  applies).

## Words

- Note: *Out of time at Implement after 45m of 45m. Extend to go on.*
- Stand line: *Out of time at <step>*; detail: *It worked 45m of its 45m timeout.*
- Buttons: *Extend*, *Stop*. Waiting: *Waiting for a team place*.
- Retry dialog / workflow form: `none` reads *no limit*.

## Documents to update with the build

`docs/workflows.md` (timeout grammar, command step `timeout`, the clock, the out-of-time wait and
its report table), `docs/tasks.md` (states, stands, actions), `docs/board.md` (team count, extend
priority), `docs/screens.md` (strip actions), `CONTEXT.md` if *out of time* or *extend* needs a
glossary line.

## Steps (one PR each)

1. **Timeouts and the clock**: `none` for the workflow's timeout, a command step's `timeout`,
   the budget pausing during command steps. Small, useful on its own.
2. **Out of time**: `Await::OutOfTime`, the drive's cut becoming a wait, the note, *Extend*/*Stop*
   on strip, detail and board row, the extend prompt, the stand and grouping.
3. **The board**: the team count skipping out-of-time tasks, room left on the wait, extends waiting
   for a team place first in line.

## Left to the builder

- Whether *Extend* is an `Input` to `RunDrive` beside approval's, or goes through `TasksDrive`
  first for the team place (it must, for a board task; choose the shape).
- Where extend-waiters are kept (the tasks' drive or the board's), so long as the pull reads them
  first and they survive nothing (a restart turns them into cut-off runs anyway).
- The history line an extend writes (*extended by 45m* is enough), beside `extended_secs`.
