# A project's board: a team of agents on a kanban

## Intent

The person wants to build a **team of agents that develops a product**, working the way a
software team works off a kanban board. The person is the **product owner**: they talk an idea
through with an agent, the agent writes it into the backlog, and the person moves what they
accept into *To do*. From there the team takes over: each column is a role (developer, reviewer,
tester, …) played by an agent of its own, work is pulled as capacity frees, and work that does
not pass goes back a column, as in a real team.

This is an upgrade of workflows, not a second engine: a board's columns are the steps of a
workflow, its items are tasks, and the board is a new view over them. Prior art is Orca's
experimental orchestration, where a coordinator agent dispatches workers; that keeps the rules
in a prompt, and this design keeps them in the engine.

## Goals

- One board per project.
- **Backlog** and **To do** at the left, fixed; **Done** at the right, fixed. Between them, the
  steps of the board's workflow, which the person adds, removes and reorders by editing that
  workflow.
- A step names the agent that plays it, so different steps run different agents (a Codex
  developer, a Claude reviewer).
- An agent step can **review**: approve the work, or send it back to an earlier step with a note.
- A step can carry a **limit**: at most N tasks of the project at it at once (WIP).
- The person discusses with an agent in an ordinary session; that agent writes the item into the
  Backlog through onehand's tools.
- The person moves an item from Backlog to To do; nothing else does. To do is ordered, and its
  order is the order the team pulls in.
- The end of the line is configurable: a pull request left for the person, a step a person
  approves, or both, all built from the step kinds a workflow has (`approval`, forge steps).

## Non-goals

- Boards spanning several projects or repositories.
- A board of its own beside workflows (a second model of columns and cards).
- A coordinator agent that runs the board. The engine pulls, limits and sends back.
- Dragging a task between step columns by hand. A person acts on a running task as they do
  today: approve, send back, Stop, Retry, Resume, take over.
- Merging on its own. A workflow has no merge step today and this does not add one; an
  auto-merging end is a later forge step.
- Several agents on one step of one task at once (splitting a task). A task is at one step at a
  time; "many agents on a task" means one per step, one after another.
- Boards for projects on a host: onehand's tools do not reach them yet.

## Anti-goals

- **Existing workflows change behaviour.** Every new key is optional, and a workflow without them
  runs exactly as it does now: one session for every step, only a command or status checks going
  back, no limit. The three shipped workflows, the launcher, *Run check*, schedules and
  `start_issue` are unchanged.
- **Two lists that disagree.** A board task is a task: it is on the Tasks page too, with the same
  state, and the board never keeps a copy of it.
- **The team touching the Backlog.** Only the person moves an item to To do. An agent can write
  into the Backlog, never past it.
- **The board's rules living in an agent's prompt.** Pulling, limits and sending back are the
  engine's, decided in core.

## Constraints (from the repository)

- Rules and what happens next live in core, in a drive (`RunDrive`, the tasks' drive); the app's
  carrier only carries effects out. Core stays GUI-free and runtime-free.
- `CONTEXT.md` owns the words. **Card** is taken (a parked permission or question), so a board's
  items are never called cards: they are **tasks**, and those still in the Backlog are
  **issues** (below). **Step** is a step, never a stage or phase. New terms (board, step limit,
  review step) are added to `CONTEXT.md` with the work.
- A workflow file with a key this build does not read is refused, and a newer `schema_version` is
  unreadable and never written over. A file is written with the lowest version that holds what
  it uses: 2 only when it uses a new key, so a workflow without them stays readable by an older
  build (my call; the editor writes `SCHEMA_VERSION` on every save today,
  `app/src/workflows_page/draft.rs`). Older builds already refuse an unknown key.
- **A downgrade is not fully covered by the version.** A run's snapshot in a task file is read
  with plain serde and no version check (`task/files.rs`): an older build drops `agent` and
  `limit` from a snapshot, would Resume or Retry on one agent and saves without them, and cannot
  read a snapshot with a `review` step, so it skips that task with a log line. The docs say so;
  nothing more is built for it (my call).
- Rendering is bounded: each column has a named cap and says when it bites.
- UI work follows the `ui` skill, `DESIGN.md`, `docs/screens.md` and `docs/ui-patterns.md`.
- Every change that adds or moves behaviour follows `docs/drives.md`.

## Approaches

- **A. Upgrade the workflow engine; the board is a view of it.** Chosen. One engine, one
  model: the Tasks page, approvals, Retry and Resume, the per-step diffs, schedules and the tools
  all carry over, and the rules stay in core. The cost is changing the engine's core assumption
  that one session carries every step, so it is several PRs.
- **B. A board engine beside workflows.** Rejected: columns and steps, cards and tasks would be two
  models of the same thing, and two lists that disagree are what the Tasks page is built to avoid.
- **C. A lead agent that runs the board.** Rejected: limits, roles and sending back would live in
  a prompt the agent may forget or break, it spends tokens on managing, and the team stops when
  the lead does.

The person picked A, one board per project, and built in the order of *Steps of the work*.

## The design

### The board

- **A project has at most one board**, bound to one workflow of the library by its workflow id,
  kept with the project in the project store. The board is off until a workflow is bound. The
  bound workflow must take `place = "worktree"`, since a team works several tasks at once and the
  checkout is held by one task at a time; binding one that works in the checkout is refused,
  saying why.
- **Columns, left to right:**
  - **Backlog**: the project's open issues that no task was made from, newest first (the order is
    the builder's call).
  - **To do**: the board's tasks not yet at their first step, in the person's order.
  - **One column per step** of the bound workflow, in its order, each showing the tasks whose
    current run is at that step (waiting on an approval, on a card or on the step's limit
    included, each said on the task).
  - **Done**: the board's tasks whose outcome is done, the newest first, capped. A task stopped
    by a person, taken over or dismissed leaves the board, and its issue, still open, is back in
    the Backlog (my call).
  - A task that ended and needs attention stays in the column of the step it ended at, marked as
    the Tasks page marks it, with the same actions.
- **The board's tasks** are the project's tasks made from the board. Tasks started any other way
  (launcher, schedule, `start_issue`, *Run check*) are not on the board, even when they run the
  bound workflow.
- **Changing the bound workflow** (editing its steps, or binding another) applies to tasks that
  start after; a run already going keeps its snapshot, as every run does. A task whose step no
  longer exists in the current workflow is shown in a column for that step, kept until the task
  leaves it (the exact rendering is the builder's call).

### The Backlog is the project's issues

My call, for the builder to keep unless planning shows it fails: the Backlog is the project's
own issues, not a new kind of item.

- The discussing agent already has `create_issue`, which writes a title and body into the
  project's issue file; an issue's body can carry **Acceptance**. No new tool is needed to write
  into the Backlog.
- An issue tracked in step with GitHub is in the Backlog too, so the person's forge issues and
  their onehand-written ones are one list.
- `start_issue` already turns an issue into a task's brief, on *Work an issue*; moving an issue
  to To do does the same on the board's workflow, through `task::submit`.
- Editing an item in the Backlog is editing the issue, in the Issues mode.
- **The task keeps a link to its issue**, new persisted data: today a task keeps none (`Brief` is
  title, body and instructions, `workflow/run/setup.rs`; `Source::Issue` folds into `Workflow`,
  `task.rs`). The link is the issue's number, or its forge reference when the file is kept in step
  with a forge (my call).
- **The Backlog is the open issues no live or done board task links to.** Only board tasks hide
  their issue; a task `start_issue` made does not (my call). It offers only issues a brief can be
  made from, with the same filter `supervised_brief_blocking` applies (`issues.rs`), so it never
  shows an issue that cannot be worked.
- **A board task that ends done closes its issue** in onehand's own tracker; on an issue kept in
  step with a forge it is left to the pull request's merge (my call). This keeps a done task's
  issue out of the Backlog after the history cap removes the task.
- Moving an issue to To do goes through the same brief-making path `start_issue` uses
  (`control.rs`), not `task::submit` directly.

### To do and pulling

- **To do is a list of issue numbers** on the project's board settings (`project.toml`), first
  pulled first. The person can reorder it and move an issue back to the Backlog.
- **The task is made when the issue is pulled**, through the off-screen start (brief from the
  issue, the board's workflow, place per the workflow), so To do items are not tasks and are not
  on the Tasks page. A refused pull puts the issue back in the Backlog with a toast.
- **To do is a queue the engine pulls from, in the person's order:** the first issue in To do is
  pulled once the board's team has room (and, from step 4 on, the first step has room under its
  limit).
- **Pulled tasks start off screen**, through the path schedules and tools use
  (`start_off_screen`), not in a window: the person moved the issue long before, and the project
  may be open in several windows. Pulling happens while onehand is open (my call).
- The worktree is cut when the task is pulled, not when it enters To do, so idle items leave no
  worktree. This is a real cost: `Setup.dir` and `branch` are fixed when a task is made today
  (`workflow/run/setup.rs`), and both start paths cut the worktree before the task is kept
  (`task/launch.rs`, `task/tasks/off_screen.rs`), so a task needs a setup whose place is filled
  in at start (my call; the builder may cut at To do instead if that proves much cheaper, removing
  the worktree when the task goes back to the Backlog).

### A step's agent

- An agent step may name `agent`, an agent from the agent list (a built-in agent or a declared
  one), with its agent settings as today. A step that names none uses the run's agent.
- **A step that names an agent has a session of its own in the run**, keyed by the step, not by
  the agent (my call): started on its first visit and kept for the rest of the run, so a developer
  sent back by a reviewer carries on with what it knew, and a reviewer of the same agent never
  judges work in the context that wrote it. Steps that name no agent share the run's session, as
  today. Every session works in the task's place and is named for the task with the step's
  label, so the rail tells them apart.
- **This is the deepest change.** One session per run is wired through, not just assumed:
  `RunDrive` is a run on one session (`task/drive.rs`), `link::Joined` holds one session
  (`task/drive/link.rs`), the carrier keeps one uid per run (`app/src/task/carrier.rs`),
  `Tasks::task_on` finds a task by that uid (`app/src/task.rs`), `Setup.agent` is one agent, and
  the app opens the session before the run is driven (`Begin::Session`, `task/tasks/begin.rs`).
  A drive asking for a session mid-run is a new effect.
- Preflight checks every agent the workflow names, not only the run's.
- Resume, Retry, take over, Stop and the cap on sessions apply to every session of the run; how
  each maps (for example, which session a take over makes ordinary) is the builder's call, kept in
  the run's drive.

### Review steps

- A new step kind, **review**, is an approval played by an agent: it names the step whose work it
  judges (`of`), the step it sends back to (`on_reject`, defaulting to `of`), an agent and a
  prompt. Its agent answers **approve**, or **send back** with a note.
- Sending back is a new visit to the `on_reject` step with the note in its prompt, as a person's
  *Revise* on an approval step does. Each send back is a miss, so a reviewer and a developer that
  never agree end the run exhausted under the workflow's allowance.
- How the agent gives its verdict must not be read from free prose. My call: a tool of onehand's
  the review step's session is given for that step only (approve, or send back with a note), in
  the set-of-tools way a schedule's sessions already use; a turn that ends without calling it is a
  miss. The builder may pick another form that is as unambiguous. The tool is listed for the
  review step's session from `session/new` and refused outside a visit to its step, rather than
  added and removed per step: an agent may ignore `notifications/tools/list_changed`
  (`docs/control.md`), and one that lists tools once would never see it (my call).
- Gates stay as they are: what onehand reads from git and the transcript. A verdict is a judgment
  asked of the agent, not a claim about the work, so it does not weaken them.

### Step limits

- Any step may carry `limit = N`: at most N tasks of the project at that step at once, counted over
  every task of the project running this workflow (by workflow id). A run that reaches a full step
  waits before it, said on the task (*Waiting for room at Review*), and starts first in first out
  as room frees. Waiting for room is not working time and does not count against the timeout.
- A step without a limit has none, as today.

### The board screen

- A page in the agent pane, as the Tasks page is, opened from the project (its page and its rail
  row); its exact place, look and words are the builder's, through the `ui` skill and the UI
  contract.
- The person moves an issue from Backlog to To do, reorders To do and moves a not-yet-started task
  back. Pointer and keyboard both reach these; drag is a later step, menus or buttons first.
- A task on the board opens its task detail, as on the Tasks page. Its actions are the Tasks
  page's.
- Each column is capped (a named cap) and says how many it left out.
- The board works with any workflow bound, so step 1 is a usable board with today's single-agent
  engine.

### Words to add to CONTEXT.md

**Board**, **Backlog** (a board's column of the project's open issues), **To do** (a board's
ordered tasks before their first step), **Step limit**, **Review step**. The builder words them and
names what to avoid (*card* for a board's item, *lane*, *stage*, *ticket*).

## Steps of the work

Each lands on its own and leaves a usable app.

1. **The board and its Backlog.** Binding a workflow to a project, the board's team size, the
   board page with Backlog, To do, the step columns and Done, moving issues to To do, ordering To
   do, pulling (and pulling again after a restart). One agent per run, as today.
2. **A step's agent.** `agent` on a step, a session per agent in a run, preflight over all of them.
3. **Review steps.** The `review` kind, its verdict tool, sending back with a note.
4. **Step limits.** `limit` on a step and waiting for room.
5. **Finishing.** Drag on the board, editing the bound workflow's steps from the board.

Steps 2–4 are useful without a board too, in any workflow. Each step's plan is written before it
is built, and each updates `docs/workflows.md`, `docs/tasks.md`, `docs/control.md` and
`CONTEXT.md` where its behaviour lands.

## Decided with the person after the review

- **A board has a team size**: how many of its tasks may run at once, set by the person (for
  example 4), from step 1. A task in To do starts when a running one finishes, ends or leaves,
  in the person's order. Step limits (step 4) narrow it per step; they never raise it.
- **A restart keeps To do and pulls again.** Tasks in To do never started, so they stay where
  they are, in order, and the board pulls from them again once onehand is open. A task that had
  started follows today's rule: it comes back interrupted under *Needs attention* and waits for
  the person's Resume, since an agent cut off mid-turn is the person's to look at first. A run
  waiting for room at a step had started, so it comes back interrupted too. To do's order and
  its tasks are persisted; the place queue is in memory only (`task/queue.rs`), so this is new.

## Left to the builder

- Whether To do is *Queued* with an order and a new reason, or a state of its own.
- The board's place in the window, its look, and its words, through the `ui` skill.
- The Backlog's order, and how a task whose step left the workflow is drawn.
- How Resume, Retry, take over and Stop cover a run with several sessions.
- The exact form of the review verdict, if not the tool.
- The column caps' values.
- What happens to the issue when its task is done (closing it, or leaving it to the pull
  request's merge on a forge).
