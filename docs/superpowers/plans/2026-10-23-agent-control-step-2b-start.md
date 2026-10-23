# R3 step 2b: `start_workflow` and `start_issue`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An MCP client with outside control on starts a workflow on a brief, or on an issue of a
project's own, as the launcher makes a task, off screen, and hears the task id and whether it
started or queued, or why not.

**Architecture:** Core `control` gains the two calls and a brief for a supervised issue task.
`task::submit` gains a told form that answers where the task landed (`Placed`). A task can be
marked to be driven off screen the first time it is driven, which `Shell::drive_task` honours by
the path an issue's run already takes. The launcher's worktree cut and preflight are lifted into
functions the tool path shares. The app's `control::perform` takes the answer's sender, so the two
start calls answer when the place is known.

**Spec:** `docs/superpowers/specs/2026-10-23-agent-control-design.md` (Decision 4's table, rows
`start_workflow`, `start_issue`). Step 2a plan: `2026-10-23-agent-control-step-2a-control.md`.

## Global Constraints

- A task goes in through `task::submit` (its told form), as the launcher makes one: the
  workflow's place decides checkout or worktree; the agent is the first configured one.
- `start_issue` is a supervised task whose brief is the issue: not the unattended pick. Nothing is
  claimed or said on the forge, unattended need not be set up, and the brief does not say nobody
  is watching.
- A task a tool starts steals neither focus nor the caret: it is driven off screen, its session
  in the rail, the task on the Tasks page. Only the first drive; a person's Resume or Retry later
  shows as it does today.
- What blocks the launcher's *Run* blocks the tool, in the preflight's own words.
- A person still sees the toasts they see today (queued, over the cap).

## Review Focus

1. A workflow id that is not in the library, or a file that cannot be read: refused, saying which.
   → Task 3 (`perform` answers `Err` before anything is kept).
2. An issue number the project's own issues do not hold, or a project keeping none: refused in
   the pick's words. → Task 3, `pickable_one_blocking`'s error passed through.
3. A worktree workflow on a project not in git: the cut's error is the answer, nothing kept.
   → Task 2 `cut_worktree_blocking` returns it; Task 3 answers it.
4. The window closing between the call and the place: `Err("the window closed")`, not a hang.
   → Task 1 `request_told` sends on every branch; a dropped sender answers `Err`.
5. `"issue": "12"` or `0` or `-1`: a tool error naming `issue`. → Task 1 test.

---

### Task 1: Core

**Files:** `crates/core/src/control.rs`, `control/tests.rs`, `crates/core/src/unattended.rs` (+ its tests).

- `Call::StartWorkflow { project: String, workflow: String, title: String, brief: Option<String> }`
  (`brief` optional, as the launcher's body is) and `Call::StartIssue { project: String, issue: u64, workflow: Option<String> }`;
  `issue` must be a positive integer. Catalogue order: `list_projects`, `list_agents`,
  `open_session`, `list_workflows`, `start_workflow`, `start_issue`, `list_tasks`, `stop_task`,
  `open_file`.
- `pub fn brief_supervised(tracker: &Tracker, issue: &Issue) -> Brief` beside `brief_for`: the
  issue's title and body, instructions `"This is <tracker.names(issue)>."` and nothing about
  nobody watching.
- Tests: the two calls parse; `issue` as a string, `0` and `-1` refused naming `issue`; the
  catalogue lists nine; `brief_supervised_names_the_issue_and_says_nobody_is_away`.

### Task 2: App: told submit, off-screen drive, shared cut and preflight

**Files:** `crates/app/src/task/place.rs`, `crates/app/src/task.rs`, `crates/app/src/shell/workflows.rs`.

- `pub(crate) enum Placed { Started, Queued(String), Refused(String) }`;
  `pub(crate) fn submit_told(task: Task, window: AnyWindowHandle, cx: &mut App) -> oneshot::Receiver<Placed>`;
  `request` becomes `request_told(id, window, tell: Option<oneshot::Sender<Placed>>, cx)`, sending
  on every branch (over the cap → `Refused(why)`; no such task or window gone → `Refused`; free →
  `Started`; waiting → `Queued(said)`); `request` and `submit` keep their signatures.
- `Tasks.off_screen: HashSet<String>`; `pub(crate) fn drive_off_screen(id: &str, cx)`,
  `pub(crate) fn take_off_screen(id: &str, cx) -> bool`.
- `drive_task`: `match (task.issue(), take_off_screen)`: an issue's task, or one marked, comes up
  through `run_unattended`; `unattended::opening` only for an issue's.
- `fn cut_worktree_blocking(root: &Path, title: &str) -> Result<(PathBuf, String, Option<String>), String>`
  lifted from `commit_workflow`'s closure; `commit_workflow` calls it.
- `pub(crate) fn run_preflight(&self, root: &Path, workflow: Result<Template, String>, ground: Option<&Ground>, cx) -> Vec<Finding>`
  lifted from `launcher_preflight`, which calls it.
- `start_workflow(.., off_screen: bool, ..) -> (String, oneshot::Receiver<Placed>)`.
- Check: `cargo test -p onehand` (existing task tests keep passing).

### Task 3: App: perform the two calls

**Files:** `crates/app/src/control.rs`, `crates/app/src/instance.rs`.

- `perform(call, answer: oneshot::Sender<Result<String, String>>, cx)`: the seven from 2a answer
  at once; the two starts `cx.spawn` and answer when placed.
- `StartWorkflow`: `pick_project`; the library entry whose id is `workflow`, else
  `Err("no workflow with id X; list_workflows gives them")`, unreadable → `Err`; brief from
  `title` and `brief`; then `start(..)`.
- `StartIssue`: `pick_project`; workflow given, or unattended's default when one is set, else
  `Err("name a workflow; list_workflows gives them")`; off the UI thread,
  `pickable_one_blocking(connectors, root, issues_file, issue)` → first row → `brief_supervised`;
  then `start(..)`.
- `async fn start(window, root, template, brief, cx) -> Result<String, String>`: `run_preflight`
  with no ground, a blocking finding → `Err(its text)`; worktree place → `cut_worktree_blocking`
  off the UI thread, the worktree added as a project; `start_workflow(.., off_screen: true)`;
  `Started` → `"Task <id> started: <title>"`, `Queued(said)` → `"Task <id> queued: <said>"`,
  `Refused(why)` → `Err(why)`, a dropped sender → `Err("the task did not start")`.
- Check: `cargo test -p onehand instance` (the socket test now expects nine tools).

### Task 4: Documents, checks, review, PR

- `docs/control.md`: the two rows, off-screen drive, supervised issue. `CHANGELOG.md`.
- `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo test --workspace --locked`;
  Opus review; PR; CI; ask before merging.
