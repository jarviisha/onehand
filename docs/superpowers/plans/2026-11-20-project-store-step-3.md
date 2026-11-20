# Project store, step 3: worktrees owned by the project — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A project is one row in the rail: every session of it, in its own folder or in one of its
worktrees, sits under that row, a session in a worktree carries the worktree's branch on its line,
and the Workbench, the terminal and the git badges follow the folder of the session on screen.

**Architecture:** `Session` gains the folder it runs in. Everything the window keys by "the folder
on screen" (Workbench, terminal and its dock home, git, the chat's cwd, offload locks) reads the
active session's folder instead of the row's path. A row (`ProjectRoot`) then stands for a whole
project: it keeps a told copy of the project's folders (its own and its worktrees, each with branch
and offload), a worktree folder added to the window lands in its project's row, and the workspace
file holds one entry per project, its active row by id. A lead's worker opened on a worktree
records it in the project (`MadeBy::Worker`) and runs in the project's row.

**Tech Stack:** Rust, GPUI.

**Spec:** `docs/superpowers/specs/2026-11-19-project-store-design.md` (step 3; *Sessions, the rail
and the Workbench*, *Worktrees*, *Control socket and remote bridge*; owner's decisions: the rail's
branch mark, the Workbench following the session's folder, offload per folder).

**Delivery:** three PRs, each leaving the app whole, a branch per PR from `origin/main`, each
ending with an opus review, CI and the owner's word before merging.
- PR 3A, tasks 1–2: `refactor(workspace): a session runs in a folder of its own` (nothing on
  screen changes: every session's folder is its row's).
- PR 3B, tasks 3–5: `feat(projects): a project's worktrees are folders of its row`.
- PR 3C, task 6: `feat(control): a worker runs in a worktree of its project`.

## Global Constraints

- Core GUI-free; rules and what happens next in core drives, the app carrying them out; our own
  enums matched whole; nothing `pub` that nothing outside names.
- A row's `transient` keeps one meaning only: a *project* row added to the window for a worker or a
  run alone, which leaves with its last session (`docs/screens.md`'s rule). No row is a worktree.
- The rail stays two levels (project, session), workers nested one level under their lead.
- What a session's agent runs in, and what it was resumed with, is its folder; `session/load` is
  given that folder as its cwd (unchanged from step 2).
- Words: the branch mark is `crate::icons::Icon::GitBranch` (as the composer's branch strip draws
  it) and the branch name, in the meta ink; *New session* menu entries read *In <branch>*.
- File size rule (about 800 lines of code): `crates/app/src/shell/roots.rs` and `rail.rs` are near
  it; split along a seam in the same PR if a task pushes one past it.
- Checks before each PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D
  warnings"`, `make fmt`, the `ui` skill's checklist for PR 3B.

## Review Focus

1. **Switching between two sessions of one project in two folders** moves the Workbench, the
   terminal, the git badges and the editor's save target to the session's folder, and back. The
   dock rule "a switch inside one project moves nothing" becomes "inside one folder". Pinned in
   Task 2 (`switching_to_a_session_in_another_folder_moves_the_panels`).
2. **A workspace file written by steps 1–2** (one entry per folder, worktrees included) opens as
   one row per project, its pin and its active row kept. Pinned in Task 3
   (`a_projects_entries_fold_into_one_row`).
3. **An offloaded worktree** runs its sessions on its host while the project folder's run here,
   and the locks follow each folder. Pinned in Task 3 (`a_sessions_host_is_its_folders`).
4. **Removing a project row** with sessions in several folders closes every one of them and lets
   go of each folder's panels. Pinned in Task 3 (`removing_a_row_forgets_every_folder_it_held`).
5. **A worker opened on an unrecorded worktree** is recorded in its project and runs in its
   project's row, nested under its lead; closing it leaves the row unless the row was added for it.
   Pinned in Task 6 (`a_worker_on_a_worktree_records_it_and_runs_in_its_project`).

---

## PR 3A: a session runs in a folder of its own

### Task 1: `Session.folder`, and the drive keyed by it

**Files:** `crates/core/src/agent.rs`, `crates/core/src/workspace.rs`,
`crates/core/src/workspace/drive/sessions.rs`, `crates/core/src/workspace/drive/projects.rs`,
`crates/core/src/workspace/drive/conversation.rs`, their tests.

**Interfaces:**
- `Session { spec, uid, folder: PathBuf }`; `Session::new(spec, uid, folder)`.
- `ProjectRoot::session_folder(&self, uid: u64) -> Option<&Path>`; `ProjectRoot::folder_on_screen(&self) -> &Path`
  (the active session's folder, else the row's `path`).
- `Workspace::add_session(spec, uid)` keeps adding on the row's own folder; new
  `Workspace::add_session_in(spec, uid, folder: PathBuf)`; the drive's `add_session_on`,
  `add_session_in`, `add_run_session(_on)`, `add_session_quietly` take the folder the session runs
  in, finding the row whose folder it is (the row's `path` in this PR; Task 3 widens that to the
  row's folders).
- `drop_sessions_in(dir)` drops the sessions whose `folder` is `dir` (not every session of the row
  whose path is `dir`); `told` uses it per folder.
- `WorkspaceDrive::locate` unchanged; `WorkspaceDrive::folder_of(uid) -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests** (`workspace/drive/tests.rs`):
  - `a_session_runs_in_the_folder_it_was_added_in`: `add_session_in` on `/a` then `folder_of(uid) == /a`;
    `folder_on_screen()` is the active session's folder, and the row's path with no session.
  - `dropping_the_sessions_of_a_folder_leaves_the_rows_others`: a row with sessions in `/a` and
    (set up through the tree) `/a-x`: `drop_sessions_in("/a-x")` closes the second only.
- [ ] **Step 2: Run, expect failure** (`cargo test -p onehand-core --lib workspace`).
- [ ] **Step 3: Implement**; every existing `Session::new` caller passes the row's path.
- [ ] **Step 4: Run** `cargo test -p onehand-core --lib` → PASS.
- [ ] **Step 5: Commit** `refactor(workspace): a session runs in a folder of its own`.

### Task 2: The window follows the folder of the session on screen

**Files:** `crates/app/src/shell/roots.rs` (`arrive_at_active_root`, `session_project`,
`on_screen`), `crates/app/src/shell/dock_rules.rs` (`TerminalHomes`), `crates/app/src/shell/docks.rs`,
`crates/app/src/shell/off_screen.rs`, `crates/app/src/shell/sessions.rs`,
`crates/app/src/chat/pane/sessions.rs` (`show`), `crates/app/src/shell/git_sweep.rs`, core
`workspace/drive/git.rs` (`sweep`), their tests.

**Interfaces:**
- `arrive_at_active_root` reads `folder = root.folder_on_screen()` wherever it read `root.path`:
  the Workbench's `set_root`, the terminal's `set_root`, `follow_terminal_dock`, the chat pane's
  `show(uid, folder, ..)`, `clear_active`, the warm agent. The project page (no session) stays on
  the row's path.
- `TerminalHomes::arrive` compares folders: a switch to a session in the same folder moves nothing;
  one in another folder of the project moves the dock as a switch of project does.
- `Shell::session_project(uid)` answers the session's folder.
- Off-screen and resumed sessions spawn in the session's folder (`open_off_screen`,
  `connect_unshown`, `add_unshown_resuming`, `fork_session`, `spawn_session`).
- `WorkspaceDrive::sweep` reads each distinct folder of a row that a session runs in, besides the
  row's own; `git_of(row)` stays the row's own folder's.

- [ ] **Step 1: Write the failing app test** (`crates/app/src/shell/tests.rs` or the dock rule's):
  `switching_to_a_session_in_another_folder_moves_the_panels`: a row `/a` with one session in `/a`
  and one in `/a-x` (built through the drive); selecting each puts the Workbench's root and the
  terminal's root on that session's folder; selecting the first again moves them back.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement.** The dock rule's and the panel's tests that pin "same root, nothing
  moves" become "same folder, nothing moves".
- [ ] **Step 4: Run everything** → PASS.
- [ ] **Step 5: Commit** `refactor(shell): the window follows the folder of the session on screen`;
  review (opus, `origin/main..HEAD`), PR 3A, CI, ask the owner.

---

## PR 3B: a project's worktrees are folders of its row

### Task 3: One row per project

**Files:** `crates/core/src/workspace.rs`, `crates/core/src/config/workspace.rs`,
`crates/core/src/workspace/drive.rs`, `crates/core/src/workspace/drive/projects.rs`,
`crates/core/src/workspace/drive/sessions.rs`, their tests.

**Interfaces:**
- `pub struct RowFolder { pub path: PathBuf, pub branch: Option<String>, pub offload: Option<Offloaded> }`;
  `ProjectRoot.folders: Vec<RowFolder>`, told from the project (its own folder first, then its
  worktrees in their order); a row of no project holds its own folder alone.
- `ProjectRoot::holds(&self, folder: &Path) -> bool`; `ProjectRoot::branch_of(&self, folder) -> Option<&str>`
  (a worktree's; none for the row's own folder); `ProjectRoot::host_of(&self, folder) -> Option<(&HostName, &Path)>`
  (where a session in `folder` runs: the row's host, or the folder's offload mirror).
- `Workspace::from_config(cfg, dir, projects)`: entries naming one project fold into one row at the
  project's own folder, pinned when any was; an entry of a worktree folder of a known project
  folds in the same way. `active` reads an id (`id:<id>`) or, written by an earlier build, a folder
  key (the row holding that folder).
- `Workspace::to_config`: one entry per row; `active` written `id:<id>` for a row of a project, the
  folder key otherwise.
- `open_project(dir, project)`, `keep_project(dir, project)`, `keep_project_quietly(dir, project)`:
  a folder of `project` (its own or a worktree) answers the project's row, made at the project's
  own folder when the window has none; the answer also names the folder for a session to start in.
  `placed(host, path, project)`: an unplaced row whose folder is a worktree of `project` folds into
  the project's row, its sessions keeping their folder.
- `told(project)`: `folders` told again; a session whose folder's offload changed is dropped
  (Task 1's `drop_sessions_in`); a session whose folder the project no longer records keeps running.
- `forget(idx)`: every folder of the row a session ran in, and the row's own, is said forgotten
  (`ForgotHere` per folder), so each folder's editor, tree and shells go.
- `Thing::Pin`/`Value::Pin` and `root_order` keyed by the row's own folder as now.

- [ ] **Step 1: Write the failing tests:**
  - `a_projects_entries_fold_into_one_row`: a config with entries `/a` (id p, pinned false) and
    `/a-x` (id p, pinned true), `active` the `/a-x` key: one row at `/a`, pinned, active.
  - `a_worktree_folder_opens_on_its_projects_row`: `open_project("/a-x", &p)` with no row: one row
    at `/a`, and `add_session_in(.., "/a-x")` sits in it.
  - `a_sessions_host_is_its_folders`: p's worktree `/a-x` offloaded to `vps`: `host_of("/a-x")` is
    vps and its mirror, `host_of("/a")` none.
  - `removing_a_row_forgets_every_folder_it_held`: a row with sessions in `/a` and `/a-x`:
    `forget` answers `ForgotHere` for both, and a `Close` per session.
  - `the_file_holds_one_entry_per_project_and_its_active_by_id`.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p onehand-core --lib` → PASS.
- [ ] **Step 5: Commit** `feat(workspace): one row per project`.

### Task 4: The window: the branch mark, *New session* in a worktree, a split

**Files:** `crates/app/src/rail.rs`, `crates/app/src/rail/model.rs`, `crates/app/src/rail/session.rs`,
`crates/app/src/rail/project/menu.rs`, `crates/app/src/chat/pane/header.rs` (the project menu), the
*New session* caret menu, `crates/app/src/shell/sessions.rs` (`new_session_at`),
`crates/app/src/shell/drafts.rs` (split), `crates/app/src/shell/offload.rs` (`sync_offload`),
`crates/app/src/shell/off_screen.rs` (`issue_files`, `remote_open`, `remote_roots`),
`crates/app/src/projects.rs` (`scope_of`), `docs/screens.md`, their tests.

**Interfaces:**
- Rail `Item` gains `branch: Option<SharedString>`: the session's folder's branch when it is not
  the row's own folder; drawn in the session row's meta line after the state, the branch icon then
  the name, truncating before the agent's name does.
- *New session* (the project row's `+`, the page's button and its caret menu): starts in the row's
  own folder; beside a session on screen, in that session's folder; the caret menu adds *In
  <branch>* per worktree of the project (a folder on disk), each starting there.
- A split (`drafts.rs`): records the worktree as today, then starts a new session in it on the
  project's row and shows it.
- `scope_of(folder)`: a project's row lists every conversation of its project (`recorded` empty);
  `resume_of` already resumes one that ran in another folder by its id.
- `issue_files`: every folder of every row, each with its project's file; `sync_offload`: the
  locks and offload marks per folder of every row (`RowFolder.offload`).
- `remote_open(root)`: the row holding `root` as a folder, the resumed session in `root`.
- `docs/screens.md`: the branch mark, *In <branch>*, a split starting a session; the rule about a
  worktree project holding only workers is replaced by Task 6's.

- [ ] **Step 1: Write the failing app tests:**
  - `a_session_in_a_worktree_is_marked_with_its_branch` (rail model): a row with sessions in `/a`
    and `/a-x` (branch `feat`): the second item's `branch` is `feat`, the first's none.
  - `new_session_in_a_worktree_starts_in_its_folder`.
  - `a_split_starts_a_session_in_the_new_worktree` (a scratch repo, as the drafts tests make one).
  - `a_projects_row_lists_every_conversation_of_it` (`scope_of` on the row's folder lists one that
    ran in a recorded worktree).
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement**, under the `ui` skill (the meta line's peer is the session row's own
  state word; the menu entries are `menu_item`s).
- [ ] **Step 4: Run everything** → PASS; answer the `ui` checklist.
- [ ] **Step 5: Commit** `feat(rail): a session in a worktree carries its branch`.

### Task 5: The control socket and the bridge name a project's folders

**Files:** `crates/core/src/control.rs` (`pick_project`), `crates/app/src/control.rs`
(`perform_now`, `ListProjects`, `OpenSession`, `OpenWorker`), `crates/core/src/control/calls.rs`,
`docs/control.md`, their tests.

**Interfaces:**
- The window's projects for the tools: one per row, each with its folders. `pick_project` matches
  the project's name, its folder, or one of its worktrees' folders, and answers the row and the
  folder named (the row's own when named by name).
- `open_session` and `open_worker` start in the folder named.
- `list_projects`: `name — path — check: yes/no`, then ` — worktrees: <branch>, …` when it has any.
- The caller's project (`session_project`) is its session's folder, which `issues_file` resolves to
  its project's file (step 2).

- [ ] **Step 1: Write the failing tests:** `a_worktree_folder_names_its_project` (core
  `pick_project`), `open_session_on_a_worktree_folder_starts_there` (app).
- [ ] **Step 2–4:** fail, implement, pass; the control tests that wrote an issue through a
  worktree row's name write it through the project's.
- [ ] **Step 5: Commit** `feat(control): a project is named by any of its folders`; review (opus,
  `origin/main..HEAD`), PR 3B with `CHANGELOG.md` (*The window*: a project is one row, a session in
  a worktree carries its branch, the Workbench and terminal follow the session's folder), CI, ask.

---

## PR 3C: a worker runs in a worktree of its project

### Task 6: A worker's folder is its project's worktree

**Files:** `crates/app/src/control/workers.rs`, `crates/app/src/shell/off_screen.rs`,
`crates/app/src/projects.rs`, core `workspace/drive/sessions.rs` (`add_run_session`),
`crates/app/src/rail.rs` (the hiding rule), `docs/screens.md`, `CONTEXT.md` (*Worker*), their tests.

**Interfaces:**
- When a worker's start lands on a folder no project records and `resolve` found it a checkout of
  the repository of a project of the window, the app records it in that project
  (`record_worktree`, `MadeBy::Worker`, its branch from git) before `open_off_screen`.
- `add_run_session(dir, ..)`: the row holding `dir` as a folder takes the session in `dir`; with
  none in the window, the project holding `dir` comes in as a `transient` row (a project row added
  for the work, which leaves with its last session); a folder no project holds is placed first as
  any folder is (`Effect::Unplaced`), never a row of a folder of its own.
- The rail's rule: a `transient` row holding nothing but workers whose lead is listed is left out
  (as today, now of a project row rather than a worktree's).

- [ ] **Step 1: Write the failing tests:**
  - `a_worker_on_a_worktree_records_it_and_runs_in_its_project` (app, the workers tests' harness):
    a lead on project `p`; `open_worker` naming an unrecorded linked worktree of `p`'s repository:
    `p` records it with `made_by = worker`, the worker's session sits in `p`'s row with that
    folder, nested under its lead; closing it leaves `p`'s row.
  - `a_worker_on_a_project_not_in_the_window_brings_it_in_and_takes_it_out`.
- [ ] **Step 2–4:** fail, implement, pass.
- [ ] **Step 5: Commit** `feat(control): a worker runs in a worktree of its project`; review, PR
  3C with `CHANGELOG.md`, CI, ask the owner.
