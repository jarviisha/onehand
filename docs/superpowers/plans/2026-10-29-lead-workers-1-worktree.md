# Lead workers, PR 1: the worktree and its folders — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `open_session`, called by a session's agent, opens a worker on a worktree of a window project's repository, with that repository's `.git` and the lead's `dirs` writable, never on a folder another session or process already works in.

**Architecture:** Core gains the rules that want tests without a GUI: the worktree check and the folder check on canonical paths (`worktree::worker`), the reading of every process of the user (`worktree::occupants`), `SessionOpts::dirs` sent as ACP's `additionalDirectories`, and `dirs` on `Call::OpenSession`. The app resolves an open off the UI thread, then on the UI thread (single-threaded, so check-and-take is atomic) checks the reservation, the folder's sessions and the cap, reserves the folder, and starts the session with its folders through `run_unattended`. The workers' code goes in a new `crates/app/src/control/workers.rs`.

**Tech Stack:** Rust, GPUI, tokio (core's ACP client), git CLI, `/proc` / `lsof`.

**Spec:** `docs/proposals/lead-workers/spec.md`, section 1 (*The project*, *One worker per worktree*, *What the worker may write*) and *Who gets what*; PR 1 in its *Pull requests*.

## Global Constraints

- Core stays GUI-free and runtime-free: blocking functions, run from the app on `cx.background_executor()`; never a file read, git or process listing in a handler or a render.
- Nothing in core is `pub` unless the app names it; first-party libraries carry `#![warn(unreachable_pub)]`.
- Comments describe, never cite a document (no `spec.md`, `docs/…`, section names in code).
- Files stay under about 800 lines of code; `crates/app/src/control.rs` (742) only routes to `control/workers.rs`.
- A refusal the caller can act on is the tool's answer (`Err(String)` from the arm), in plain words naming what failed.
- An outside client's `open_session` is unchanged: a worktree as `project` or a non-empty `dirs` from `Peer::Outside` is refused, naming them.
- Checks before the PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, `cargo tree -p onehand-core -i gpui-pre` erroring "did not match any packages".

## Review Focus

- A worktree path given with a trailing slash, through a symlink, or as `..`-free but non-canonical (`/a/./b`): accepted as the same folder, and its reservation collides with the same folder named another way. Pinned in Task 2 and Task 6.
- A window project that is a folder inside a checkout (a monorepo subtree): its repository still matches a worktree of that repository. Pinned in Task 2.
- A process whose `/proc/<pid>` vanishes between listing and reading: skipped, not unknown; a permission refused: unknown. Pinned in Task 3.
- Two opens on one worktree in flight together: the second refused once the first holds the reservation, even if both passed the process reading. Pinned in Task 6.
- A worker's agent restarted (`Ctrl+Shift+R`): its `additionalDirectories` go again, and it never takes the warm adapter. Pinned in Task 1 (load/fork carry them) and Task 5 (connect skips warm).

---

### Task 1: `SessionOpts::dirs` as `additionalDirectories`

**Files:**
- Modify: `crates/core/src/acp/client.rs:65-72` (the struct), `crates/core/src/acp/client/handshake.rs` (`new_session`, `load`, the `session/fork` params in `answer_init`)
- Modify: every `SessionOpts {` literal: `crates/core/src/acp/client/tests.rs:24,103,993`, `crates/core/examples/acp_smoke.rs:55`, `crates/app/src/acp.rs:207`
- Test: `crates/core/src/acp/client/tests.rs`

**Interfaces:**
- Produces: `SessionOpts { cwd, opening, mcp_servers, dirs: Vec<PathBuf> }`; `dirs` empty sends no `additionalDirectories` key at all.

- [ ] **Step 1: Write the failing tests** beside `session_new_carries_the_sessions_mcp_servers`, with a `client_opening`-like helper taking `dirs`:
  - `session_new_carries_the_folders_a_worker_may_write`: dirs `["/w/.git", "/run"]` → `new["params"]["additionalDirectories"] == json!(["/w/.git", "/run"])`.
  - `session_new_without_folders_sends_no_key`: empty dirs → `new["params"].get("additionalDirectories").is_none()`.
  - `session_load_carries_the_folders_too`: `Opening::Resume("s1")` with an agent answering `loadSession: true` → `session/load` params carry them.
- [ ] **Step 2: Run** `cargo test -p onehand-core --locked additional` — FAIL (no field).
- [ ] **Step 3: Implement**: the field, and one helper in `handshake.rs` inserting `"additionalDirectories"` (paths as display strings) into the params object when non-empty, used by `new_session`, `load` and the fork params. Every other literal passes `dirs: Vec::new()`.
- [ ] **Step 4: Run** `cargo test -p onehand-core --locked acp::` — PASS.
- [ ] **Step 5: Commit** `feat(acp): a session may name further folders its agent may write`

### Task 2: the worktree and the folders, on canonical paths

**Files:**
- Create: `crates/core/src/worktree/worker.rs` (declared `pub mod worker;` in `crates/core/src/worktree.rs`), tests in `crates/core/src/worktree/worker/tests.rs`

**Interfaces:**
- Consumes: `worktree::git(root) -> Command` (crate-private), `crate::process::output_within`, `LOCAL_LIMIT`.
- Produces:
  - `pub struct Repo { pub top: PathBuf, pub common: PathBuf }` — both canonical; `common` is the repository's common git directory.
  - `pub fn repo_of_blocking(dir: &Path) -> Result<Repo, String>` — `git rev-parse --show-toplevel` and `--git-common-dir` in `dir` (a relative common dir joined to `dir`), each canonicalized.
  - `pub fn worktree_of_blocking(path: &Path, projects: &[PathBuf]) -> Result<Repo, String>` — `path` must be absolute and a folder; its canonical form must equal its `Repo::top` (else `"<path> is inside the checkout at <top>; name its top level"`); its `common` must equal the `common` of some project (else `"<path> is a checkout of a repository no project of this window belongs to"`). A project that is not a git checkout is skipped.
  - `pub fn writable_blocking(dirs: &[PathBuf], repo: &Repo, home: Option<&Path>) -> Result<Vec<PathBuf>, String>` — each absolute and an existing folder, canonicalized, then refused when it is `/`, `home`, inside or equal to `repo.top`, holding `repo.top`, or holding the main checkout (`repo.common`'s parent when `common` ends in `.git`, else `common`). The refusal names the folder as given and as resolved. Returns the canonical folders in order, duplicates dropped.
  - `pub fn worker_dirs(repo: &Repo, writable: Vec<PathBuf>) -> Vec<PathBuf>` — `repo.common` first when it is not inside `repo.top` (a linked worktree), then `writable`.

- [ ] **Step 1: Write the failing tests**, each on a fresh repo in `std::env::temp_dir()` made with `git init`, one commit, and `git worktree add ../<name>-wt -b w`, as `worktree.rs`'s own tests do:
  - `a_linked_worktree_of_a_project_is_accepted`: project = main checkout, path = the worktree → `Ok(Repo { top: canonical wt, common: canonical main/.git })`.
  - `the_main_checkout_named_through_a_symlink_is_the_same`: path = a symlink to the worktree → `top` is the resolved worktree.
  - `a_folder_inside_a_worktree_is_refused_naming_its_top`: path = `wt/src` → `Err` containing the canonical worktree path.
  - `a_project_inside_the_main_checkout_still_matches`: project = `main/sub` → the worktree is accepted.
  - `a_repository_no_project_belongs_to_is_refused`: a second unrelated repo → `Err` containing "no project of this window".
  - `writable_refuses_the_root_home_and_the_worktree`: `/`, the given home, `wt`, `wt/src`, a folder holding `wt`, the main checkout's parent → each `Err`; `writable_refuses_through_a_symlink`: a symlink outside pointing at `wt` → `Err` naming both forms.
  - `writable_keeps_an_outside_folder_canonical`: a temp run folder given with a trailing `/.` → `Ok([canonical run])`.
  - `worker_dirs_put_the_common_git_first_for_a_linked_worktree` and `…_nothing_for_the_main_checkout`.
- [ ] **Step 2: Run** `cargo test -p onehand-core --locked worktree::worker` — FAIL.
- [ ] **Step 3: Implement** the four functions in `worker.rs`, its module header saying why every path is canonicalized first.
- [ ] **Step 4: Run** the same — PASS.
- [ ] **Step 5: Commit** `feat(worktree): which folder a worker may open on and write`

### Task 3: every process of the user working in a folder

**Files:**
- Create: `crates/core/src/worktree/occupants.rs` (`pub mod occupants;`), tests in `crates/core/src/worktree/occupants/tests.rs`

**Interfaces:**
- Produces:
  - `pub enum Occupants { None, Found(Vec<String>), Unknown(Vec<String>) }` — `Found`/`Unknown` hold `"<pid> <command>"` lines.
  - `pub fn occupants_blocking(folder: &Path, skip: &[u32]) -> Occupants` — Linux: every numeric entry of `/proc` owned by the same uid as `/proc/self`, its `cwd` read with `read_link`; elsewhere `lsof -a -d cwd -u <uid> -Fpcn` within `LOCAL_LIMIT`.
  - Private, for the tests: `fn judge(folder: &Path, table: impl IntoIterator<Item = Entry>, skip: &[u32]) -> Occupants` where `struct Entry { pid: u32, command: String, cwd: Result<PathBuf, std::io::ErrorKind> }`; `fn parse_lsof(out: &str) -> Vec<Entry>`.
  - Rule in `judge`: a skipped pid is left out; `Err(NotFound)` is a process gone, left out; any other `Err` goes to unknown; `cwd.starts_with(folder)` is found. Found wins over unknown (both are refusals; found is the one the person can act on). `lsof` failing to run or out of time is `Unknown(["lsof: <why>"])`.

- [ ] **Step 1: Write the failing tests** over hand-made tables:
  - `none_when_every_cwd_is_elsewhere`; `found_names_pid_and_command` (`Entry { pid: 41, command: "node", cwd: Ok("/w/src") }` → `Found(["41 node"])`);
  - `a_skipped_pid_is_not_found` (pid 41 in `skip`);
  - `a_process_gone_meanwhile_is_left_out` (`Err(NotFound)` → `None`);
  - `an_unreadable_cwd_is_unknown_never_none` (`Err(PermissionDenied)` → `Unknown(["7 bash"])`);
  - `found_wins_over_unknown`;
  - `a_sibling_with_a_common_prefix_is_not_inside` (`/w2` for folder `/w` → `None`);
  - `parse_lsof_reads_pid_command_and_cwd` on `"p41\ncnode\nn/w/src\np7\ncbash\nn/home\n"`.
- [ ] **Step 2: Run** `cargo test -p onehand-core --locked worktree::occupants` — FAIL.
- [ ] **Step 3: Implement**; the module header says why an unreadable process is a refusal and not a pass (an earlier run's worker nobody in this run knows the pid of).
- [ ] **Step 4: Run** — PASS. Also run once by hand on this machine: a test `occupants_finds_this_test_process` (`#[cfg(target_os = "linux")]`) with folder = the test's cwd and no skip → `Found` containing `std::process::id()`.
- [ ] **Step 5: Commit** `feat(worktree): read every process of the user working in a folder`

### Task 4: `dirs` on `open_session`

**Files:**
- Modify: `crates/core/src/control.rs` (`Call::OpenSession`, the catalogue entry, the parse arm); tests in its existing `#[cfg(test)]` module or `control/tests.rs`

**Interfaces:**
- Produces: `Call::OpenSession { project: String, agent: String, prompt: Option<String>, dirs: Vec<PathBuf> }`.

- [ ] **Step 1: Write the failing tests**: `open_session_reads_dirs` (`{"project":"p","agent":"a","dirs":["/run"]}` → `dirs == [PathBuf::from("/run")]`); `open_session_without_dirs_has_none`; `open_session_refuses_dirs_not_strings` (`"dirs": "/run"` and `"dirs": [1]` → `Err` containing "`dirs` must be a list of folder paths").
- [ ] **Step 2: Run** `cargo test -p onehand-core --locked control::` — FAIL.
- [ ] **Step 3: Implement**: the field; the parse; the schema `"dirs": {"type": "array", "items": {"type": "string"}, "description": "Absolute folders outside the project the session's agent may also write, for a session's agent only"}`; the `project` description of `open_session` becomes "A project of the window, by its name or its path; for a session's agent, also the top level of a git worktree of a repository one of the window's projects belongs to".
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(control): open_session takes the folders a worker may write`

### Task 5: a session started with its folders

**Files:**
- Modify: `crates/app/src/chat/conversation.rs` (`Conversation.dirs`, `Conversation::opening`), `crates/app/src/chat/pane/sessions.rs` (`open_unshown`, `connect_as`), `crates/app/src/chat/session.rs` (`ChatSession::spawn`), `crates/app/src/acp.rs` (`connect`, `start`), `crates/app/src/shell/remote_runs.rs` (`run_unattended`), and every caller of the changed signatures (`cargo check` lists them; each passes `Vec::new()`)

**Interfaces:**
- Consumes: `SessionOpts::dirs` (Task 1).
- Produces:
  - `Shell::run_unattended(&mut self, dir: PathBuf, spec: AgentSpec, dirs: Vec<PathBuf>, cx) -> Option<(u64, Entity<ChatSession>)>`
  - `ChatPane::open_unshown(&mut self, uid, root, spec, dirs: Vec<PathBuf>, cx)`
  - `ChatSession::spawn(uid, root, spec, opening, dirs: Vec<PathBuf>, cx)`
  - `AcpRuntime::connect(&self, spec, cwd, opening, dirs: Vec<PathBuf>)` — takes the warm adapter only when `opening == Fresh` **and** `dirs.is_empty()`.
  - `Conversation.dirs: Vec<PathBuf>`, kept for the session's life so a restart (`connect_as`) spawns with them again.

- [ ] **Step 1: Write the failing test** in `crates/app/src/acp.rs`'s tests (or beside `Warm::matches`): `a_session_with_folders_never_takes_the_warm_adapter` — the decision as a small pure function `fn takes_warm(opening: &Opening, dirs: &[PathBuf]) -> bool` (`Fresh` + empty → true; `Fresh` + one dir → false; `Resume` + empty → false).
- [ ] **Step 2: Run** `cargo test -p onehand --locked takes_warm` — FAIL.
- [ ] **Step 3: Implement** the plumbing; the field's doc comment says why it is held for the session's life (a restart respawns from it).
- [ ] **Step 4: Run** `cargo check --workspace --locked` and `cargo test -p onehand --locked takes_warm` — PASS.
- [ ] **Step 5: Commit** `feat(app): a session keeps the folders it was opened with`

### Task 6: the open, for a session's agent

**Files:**
- Create: `crates/app/src/control/workers.rs` (declared `mod workers;` in `crates/app/src/control.rs`), tests in `crates/app/src/control/workers/tests.rs`
- Modify: `crates/app/src/control.rs` — the `Call::OpenSession` arm becomes a call into `workers::open`; `Sessions` gains `reserved: Reservations`; `forget(uid)` also calls `reserved.release_worker(uid)`.

**Interfaces:**
- Consumes: Tasks 2–5.
- Produces:
  - `pub(super) struct Reservations` with `fn take(&mut self, folder: &Path) -> Result<(), String>` (refused `"<folder> already has a worker opening or open on it"`), `fn bind(&mut self, folder: &Path, uid: u64)`, `fn release(&mut self, folder: &Path)`, `fn release_worker(&mut self, uid: u64)`.
  - `pub(super) fn open(caller: Option<(u64, PathBuf)>, project: String, agent: String, prompt: Option<String>, dirs: Vec<PathBuf>, shell, projects, answer: &mut Option<Answer>, cx) -> Result<String, String>`.

The flow, for `caller: Some` (a session's agent):
1. In the handler: resolve the agent spec (as today). If `project` names a window project (`control::pick_project`), the candidate folder is that path; otherwise it must be absolute. Take the answer and spawn.
2. Background: the canonical folder and its `Repo` (`repo_of_blocking` for a window project, `worktree_of_blocking(path, projects)` otherwise); `writable_blocking(dirs, repo, home)`; `occupants_blocking(folder, skip)` with `skip` = `std::process::id()` plus every pid of `terminal.processes()` read before spawning; the canonical path of every window root, for step 3.
3. UI thread, in one update: refuse when another session is open on a root whose canonical path equals the folder (naming it); refuse at the cap (today's words); `reserved.take(folder)`; `Occupants::Found` / `Unknown` refuse (released) with "<n> processes work in <folder>: …; the person has to stop them before a worker opens there" / "could not tell whether anything works in <folder>: …"; then `run_unattended(folder, spec, worker_dirs(repo, writable), cx)`, `reserved.bind(folder, uid)` on success, `release` on `None`; the first prompt as today; the answer as today plus a `dirs: …` line when any were added.
- For `caller: None` (outside): a `project` that is not a window project, or non-empty `dirs`, is refused naming them; otherwise exactly today's arm.

- [ ] **Step 1: Write the failing tests** on `Reservations`: `a_reserved_folder_refuses_a_second_open`; `release_after_a_failed_open_frees_it`; `a_bound_worker_closing_frees_its_folder` (`bind(f, 7)`, `release_worker(7)`, `take(f)` → `Ok`); `release_worker_leaves_other_folders`.
- [ ] **Step 2: Run** `cargo test -p onehand --locked control::workers` — FAIL.
- [ ] **Step 3: Implement** `Reservations`, `open`, the arm, and `forget`'s release.
- [ ] **Step 4: Run** `cargo test -p onehand --locked` (guards included) — PASS.
- [ ] **Step 5: Commit** `feat(control): open a worker on a worktree, one per folder`

### Task 7: the contract

**Files:**
- Modify: `docs/control.md` (the `open_session` row: a worktree's top level and `dirs` for a session's agent, the refusals; *A session's agent*: one worker per folder, the process reading, the writable folders), `docs/proposals/lead-workers/spec.md` section 1 (the reservation taken on the UI thread once the background checks answer, which is what makes it atomic; marked built in PR 1), `docs/proposals/lead-workers/README.md` (piece 3: PR 1 built)

- [ ] **Step 1:** Write the three changes.
- [ ] **Step 2:** `git diff --check`; no code cites a document (`grep -rn "control.md\|spec.md" crates/` finds nothing new).
- [ ] **Step 3:** Run the Global Constraints' checks.
- [ ] **Step 4: Commit** `docs(control): open_session on a worktree, with the folders a worker writes`
