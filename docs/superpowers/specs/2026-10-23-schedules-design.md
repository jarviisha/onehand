# R4: schedules, work onehand starts by the clock

- Status: draft for the owner's approval.
- Source: the architecture audit of 2026-10-22 (local notes), R4 in its plan; R3's non-goal
  "starting runs by time". The conversation of 2026-10-23 settled what a schedule is, what it may
  start, where it lives and what its agent may do.
- Contracts that still govern: [CONTEXT.md](../../../CONTEXT.md), [DESIGN.md](../../../DESIGN.md),
  [docs/screens.md](../../screens.md), [docs/ui-patterns.md](../../ui-patterns.md),
  [docs/workflows.md](../../workflows.md), [docs/tasks.md](../../tasks.md),
  [docs/unattended.md](../../unattended.md) (rule 1 above all), [docs/control.md](../../control.md).
  Every visible change follows the `ui` skill.

## Intent

The owner wants onehand to start work on its own at set times, the first being: open a session on
a project with a prompt that reviews the whole codebase (through a skill, named in the prompt) and
records each problem it finds as an issue of the project. Over days that fills the Issues tab with
work an unattended run, or a person, can take.

The owner's machine is off when they are away and at night. So a schedule runs **while onehand is
open**, and one that came due while it was not runs **once** when it next is. No cloud, no
background process with no window: those are not wanted.

## What a schedule is (the owner's calls)

- **A schedule** names a project, what it starts, an agent, a mode, the agent's settings (its
  model, and whatever else it advertises, such as effort), the tools its agent may use, and when,
  as a cron expression.
- **What it starts** is one of two:
  - a session with a prompt written in the schedule; a skill is used by naming it in the prompt;
  - a workflow on a brief written in the schedule (title, and optional body).
- **Every start is a task**: on the Tasks page, with its history and its Stop, through the queue,
  one task per place, as every task is.
- **The mode** is the agent's own session mode the session starts in, as `unattended.mode` sets
  one for unattended runs, so a schedule can run through without asking for permission. onehand
  still answers nothing for a person: a question the agent asks waits for them (unattended rule 1).
- **The model and the other settings** are chosen as an agent step's are in a workflow
  (`options`, docs/workflows.md): from what the agent offers, as learned, with *Check the agent*
  when nothing is known yet, judged by the preflight, and set when the session comes up. None
  chosen leaves the agent's own.
- **The tools** are a set chosen per schedule from onehand's tools (docs/control.md), none by
  default. The schedule's sessions list only those, and a call to any other is refused, whatever
  the prompt asks.
- **When**: the form offers quick choices (every N hours, every day at, on weekdays at, every week
  on a day at) that write a cron expression, and *Expression…* to type one. The expression is what
  is kept and what the clock reads; one typed that a quick choice can say lights that choice.
- **Where**: a *Schedules* page on the rail beside Tasks, Issues and Workflows, every schedule of
  the window's projects with the projects filter the Tasks page has; and a *Schedules* box on each
  project's page listing that project's, leading to the page.
- **Told when it ends**: a desktop notification, and the task on the Tasks page.
- **Set per schedule, in its form** (each with the default shown):
  - **Labels for the issues it creates**, none by default. Naming unattended's trigger label
    (`auto`) here hands every issue the schedule finds straight to unattended runs; leaving it
    out leaves them for a person to label.
  - **When it was missed** (onehand closed at its time): *run once when onehand opens* (default)
    or *skip to the next time*. Missed many times, it still runs once.
  - **When the last run is still going** at its time: *skip this time* (default) or *queue
    behind it*, which starts it once the last one has stopped, through the queue.
- **Issues stay local first.** Two new tools: `list_issues` (the project's open issues) and
  `create_issue` (one issue in the project's own issue file, as the Issues tab keeps them). The
  agent is to read the list before writing. Pushing to a forge is the existing sync's business;
  Jira and other trackers are R5.

## Goals

1. **A schedule is kept and read in core.** A core `schedule` module holds the schedule's shape,
   the cron expression's parse and match (five fields: minute, hour, day of month, month, day of
   week; `*`, numbers, ranges, lists and steps), the quick choices as expressions both ways, and
   the rule "what is due now": given the expression, the last time it fired and now, whether to
   fire, once, however many times it came due meanwhile. Pure and tested.
2. **The clock is the app's**, one check a minute while onehand runs, plus one at start (which is
   the catch-up). A schedule fires in the window that holds its project; one whose project no
   window holds stays due until one does.
3. **A start goes through what exists.** A workflow start is a task through `task::submit_told`,
   driven off screen, as R3's `start_workflow` makes one. A session start is a task too: a task
   whose workflow is one agent step carrying the prompt (no gate, no approval, no command), made
   at the start, so it is listed, stopped and kept like any other.
4. **Tools by schedule.** R3's standing gains a set: a session may use all of the tools (the
   composer's switch, as now), none, or the schedule's set. Core's `control::handle` lists and
   calls by that set; nothing else decides it.
5. **The two issue tools** join the catalogue, decided in core like the rest: `list_issues
   {project?}` answers the open issues, capped and saying how many it left out; `create_issue
   {project?, title, body}` writes one and answers its number, and refuses a title an open issue
   already has, naming that issue. `project` defaults to the calling session's. The tool takes no
   labels: an issue written from a scheduled task's session gets that schedule's labels, and one
   written from any other session none, so no agent labels its own issue for an unattended run.

## Non-goals

- Running while onehand is closed or the machine is off. No daemon, no system cron, no cloud.
- More than one catch-up per schedule: a schedule missed ten times runs once, or not at all when
  it is set to skip.
- Answering a card, a permission or an approval on a person's behalf, ever.
- Trackers other than onehand's own issue file and the existing forge sync (R5).
- Calendar features: time zones other than the machine's, holidays, "last Friday of the month".
- Schedules for the project's check command alone (a workflow can hold one).

## Anti-goals (a failure even if it works)

- A scheduled run that answers, or lets its agent answer, a card, a permission or an approval.
- A schedule's agent using a tool not in its set, through any path.
- A run firing twice for one due time, or a burst of runs after a long absence.
- A scheduled run stealing focus or the caret, or doing anything with nothing on screen saying so
  (it is on the Tasks page, its session in the rail).
- The Issues tab filling with the same problem every day because the run never looked.

## Constraints

- Core stays GUI-free and runtime-free; the clock is a GPUI timer in the app, never a thread
  sleeping in core.
- One rule in one place: due-ness and the cron in core `schedule`; the tool set in core `control`.
- A schedule's run is a task; nothing about it bypasses the queue, the preflight or the off-screen
  drive R3 built (`Shell::start_workflow_told`, `task::submit_told`, `Placed`).
- `crates/app/src/control.rs` is near 700 lines; the issue tools' performing goes in a file beside
  it if it would pass 800.
- Every list on the new page is capped and says what it left out; Settings-style rules for the
  form (`docs/ui-patterns.md`, *Forms*).

## Decisions

### 1. Where schedules are kept *(builder's call)*

`<config dir>/schedules.toml`, app-wide: each schedule names its project by path, so one file
serves every window, and a project open in no window simply waits. Each entry carries an id, the
fields above, whether it is on, and the last time it fired (written when it fires, so a restart
neither repeats nor loses one).

### 2. Missed and overlapping times (the owner's call: configurable)

As set on the schedule (above). A skip is said on the schedule's row (*skipped: the last run was
still going*, *skipped: missed while onehand was closed*) and its last-fired time moves on. *Queue
behind it* keeps at most one start waiting per schedule. The queue's own rule still holds for
different schedules on one place.

### 2a. A schedule made, or switched back on *(builder's call)*

It fires from its next due time on: nothing due before it was made or switched on is caught up.
The schedule keeps the time it was switched on beside the time it last fired, and the due rule
reads the later of the two.

### 3. A schedule that cannot start *(builder's call)*

A preflight block, an agent no longer configured, a mode the agent does not offer: the schedule
records the reason on its row, notifies once, and tries again at its next due time. It is never
switched off by itself.

### 4. What a workflow start takes from the schedule *(builder's call)*

The schedule's agent is every agent step's that names none, as the launcher's first agent is
today; the schedule's mode and settings are every step's that sets none, and a step's own mode,
model or option wins, setting by setting. The preflight judges that. A prompt start's one step
carries the schedule's mode and settings as its own.

### 5. What a scheduled task keeps *(builder's call)*

The agent, mode, settings, tool set and issue labels are copied onto the task when it starts, as a run keeps
its workflow's snapshot. A Retry or a Resume uses the copy, so editing, switching off or deleting
the schedule never widens what an earlier task may do.

### 6. The schedule's sessions and the tools

Every session of a scheduled task (the one session of a prompt start; each agent step's session
of a workflow start) stands with the schedule's set. The composer's switch in those sessions
shows the set's state and can switch the session to all tools or off, as any session's can. A
session the schedule's tools open, or a run they start, starts off, as in R3.

## Steps (one PR each, in order)

| Step | What | Behaviour change |
|---|---|---|
| 1 | Core `schedule` (shape, cron, quick choices, due rule) and `schedules.toml`, with tests | none |
| 2 | Tool sets in core `control`; `list_issues` and `create_issue` | a session can be given some tools; agents can read and write issues |
| 3 | The clock, firing, catch-up, overlap and notifications; scheduled tasks | schedules in the file run |
| 4 | The Schedules page, the project page's box, the form | schedules are made and edited in the app |

Each step updates its owning documents: a new `docs/schedules.md`, `docs/control.md` (tool sets,
the two tools), `docs/screens.md` (the page and the box), CONTEXT.md (**Schedule**), CHANGELOG.md.

## Testing

- **Core:** the cron parser's fields and refusals; matching across month ends and day-of-week;
  quick choices to expressions and back; the due rule (not yet, due, missed many times → once or
  skipped as set, last fired after now → not, nothing before it was switched on); the overlap
  choice; a tool set's `tools/list` and refused call; `create_issue` refusing
  a duplicate open title.
- **App:** firing writes the last-fired time once; a schedule whose project is not open stays due;
  the overlap skip.
- **By hand with `/drive`:** a schedule a minute ahead fires, its task appears, its session's agent
  sees only its tools; a schedule edited from the page.

## Left to the builder

- The cron implementation: hand-written in core (no new dependency) unless that grows past what a
  five-field matcher needs.
- The exact words of the page, the form and the notification, in CONTEXT.md's words.
- How the one-step workflow of a prompt start is built and named on the Tasks page.
- The tool-set picker's form: checkboxes per tool are enough. Every tool may be chosen; none is
  by default *(builder's call)*.
