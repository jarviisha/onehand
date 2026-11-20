# Project store, step 4: the project page's Worktrees section — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A project's page lists its worktrees and manages them: each recorded worktree (branch,
folder, its sessions, *New session*, *Remove worktree…*), the checkouts git lists that no project
records (*Add to this project*), the recorded ones git no longer lists (*Forget*), *New
worktree…*, and the folder new worktrees go in.

**Architecture:** Core gains a read of `git worktree list` and a pure classifier turning a
project, git's list and the other projects into the section's rows; `ProjectsDrive` gains
forgetting a worktree and setting `worktree_parent`; `worktree::removal` gains the judgement for a
worktree that is not a task's (clean, unused, not shared, branch deleted only by `git branch -d`).
The app reads git's list off the UI loop when the page shows or the project changes, draws the box
in a file of its own under `chat/pane/project_page/`, and carries each action through the drives.

**Tech Stack:** Rust, GPUI, gpui-component.

**Spec:** `docs/superpowers/specs/2026-11-19-project-store-design.md` (step 4; *Worktrees*,
*Sessions, the rail and the Workbench* last two bullets, *Goals*: worktree folder per project).

**Delivery:** one PR from `origin/main`, `feat(projects): a project's page manages its worktrees`,
ending with an opus review, CI and the owner's word. The same PR deletes
`docs/superpowers/specs/2026-11-19-project-store-design.md` and the four
`docs/superpowers/plans/2026-11-*-project-store-step-*.md`, their behaviour by then in
`docs/screens.md`, `CONTEXT.md`, `docs/control.md` and `docs/known-gaps.md`.

## Global Constraints

- Core GUI-free; rules and what happens next in core drives, the app carrying them out; our own
  enums matched whole; nothing `pub` that nothing outside names.
- **Which worktrees exist is git's**; the project records what onehand knows. A recorded
  worktree git no longer lists is *missing*; one git lists that no project records is offered.
- On a repository two projects share, git's list offers a checkout to a project only when the
  checkout holds that project's sub-path and no project records that folder.
- A new worktree is cut only while the project folder runs here; a host project has no Worktrees
  section.
- Removal: removable when git reports no uncommitted changes in the checkout and the occupants
  check finds nothing using it; refused while another project records a folder inside the same
  checkout; the branch is deleted only when `git branch -d` takes it, otherwise kept and the
  confirmation says so. A task's worktree keeps today's flow (`worktree/removal.rs`). Removing
  through onehand removes the record with it.
- New worktrees go in `worktree_dir_in(project.worktree_parent)`, or `worktree_dir` when none.
- Every list capped with a named cap, saying how many it left out; every button through
  `crate::controls::action`, menu rows through `menu_item`; every removal confirmed through
  `Confirm`; words per `docs/ui-patterns.md` *Words*. The `ui` skill's checklist before the PR.
- `crates/app/src/chat/pane/project_page.rs` is past 800 lines: the box is a new sibling file, and
  nothing else is added to `project_page.rs` beyond mounting it.

## Review Focus

1. **A worktree with a session open in it** refuses *Remove worktree…*, naming the sessions, and
   *Forget* on a missing one leaves its sessions running (they keep their folder). Pinned in
   Task 2 (`a_worktree_in_use_is_not_removed`) and Task 1 (`forgetting_a_missing_worktree_keeps_its_sessions`).
2. **An unmerged branch** is kept when its worktree goes, and the confirmation says it is kept.
   Pinned in Task 2 (`an_unmerged_branch_stays_when_its_worktree_goes`).
3. **A shared repository** (two projects on sub-folders of one checkout) offers each project only
   checkouts holding its own sub-path and refuses removing a checkout the other project records a
   folder in. Pinned in Task 1 (`a_shared_repository_offers_each_project_its_own`) and Task 2
   (`a_checkout_another_project_records_is_not_removed`).
4. **git failing** (not a repository any more, a timeout) draws the recorded worktrees as they are
   recorded, with a muted line saying git's list could not be read, never an empty box. Pinned in
   Task 3 (`a_git_list_that_fails_keeps_the_recorded_rows`).
5. **A worktree folder setting pointing nowhere** (the folder deleted since) is refused when a
   worktree is cut, in the form, naming the folder. Pinned in Task 3 (`a_missing_worktree_folder_is_refused_in_the_form`).

---

### Task 1: What git lists, and the section's rows (core)

**Files:** `crates/core/src/worktree.rs` (list), new `crates/core/src/project/worktrees.rs`,
`crates/core/src/project/drive.rs`, `crates/core/src/project.rs` (mod), their tests.

**Interfaces:**
- `pub struct Listed { pub path: PathBuf, pub branch: Option<String> }` (detached: none);
  `pub fn list_blocking(folder: &Path) -> Result<Vec<Listed>, String>` in `worktree.rs`, parsing
  `git worktree list --porcelain` run in `folder`, the main checkout first, paths canonical.
- `pub enum WorktreeRow { Recorded { at: usize, on_disk: bool }, Offered(Listed) }` in
  `project/worktrees.rs`; `pub fn rows(project: &Project, listed: Option<&[Listed]>, others: &Projects) -> Vec<WorktreeRow>`:
  every recorded worktree in its order (`on_disk` = git lists its checkout; with `listed: None`
  every recorded one counts as on disk), then each listed checkout that is not the main checkout,
  holds the project's sub-path (`sub = project.path` within its checkout's top), and whose folder
  (`top.join(sub)`) no project of `others` or `project` records, offered as that folder.
- `ProjectsDrive::forget_worktree(&mut self, id: &ProjectId, path: &Path) -> Vec<Effect>` and
  `ProjectsDrive::set_worktree_parent(&mut self, id: &ProjectId, parent: Option<PathBuf>) -> Vec<Effect>`,
  written and told as `record_worktree` is; adopting is `record_worktree` with `MadeBy::Added` and
  the listed branch (`"HEAD"` when detached).

- [ ] **Step 1: Failing tests** (`worktree` tests use `project::testing::repo_with_worktree`):
  `git_lists_the_main_checkout_then_the_linked_one`; `a_recorded_worktree_git_no_longer_lists_is_missing`;
  `a_listed_checkout_no_project_records_is_offered`; `a_shared_repository_offers_each_project_its_own`;
  `forgetting_a_missing_worktree_keeps_its_sessions` (drive: `forget_worktree` then the workspace
  drive's `told`: the row's `worktrees` lose it, a session whose folder it was is still located).
- [ ] **Step 2: Run, expect failure** (`cargo test -p onehand-core --lib worktree project`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p onehand-core --lib` → PASS.
- [ ] **Step 5: Commit** `feat(projects): a project's worktrees as git lists them`.

### Task 2: Removing a worktree that is not a task's (core)

**Files:** `crates/core/src/worktree/removal.rs` (or a sibling `removal/own.rs` if the file passes
800 lines), their tests.

**Interfaces:**
- `pub struct OwnFacts { pub folder: PathBuf, pub branch: Option<String>, pub uncommitted: Result<Vec<String>, String>, pub users: Vec<String>, pub shared_with: Vec<String> }`
  (`shared_with`: names of other projects recording a folder inside this checkout).
- `pub fn judge_own(facts: &OwnFacts) -> Judged`: `Refused` listing each reason (uncommitted, as
  `judge` names them, capped at `UNCOMMITTED_SHOWN`; each user; each sharing project; git could
  not say); else `Remove { branch, why }` with `why` empty: the branch is tried, not promised.
- `pub fn own_facts_blocking(folder: &Path, users: Vec<String>, shared_with: Vec<String>) -> OwnFacts`.
- `pub fn remove_own_blocking(repo: &Path, folder: &Path, branch: Option<&str>) -> Result<Removed, String>`:
  `git worktree remove`, then `git branch -d`; a refused `-d` keeps the branch and says why in
  `Removed::branch_kept` (no `-D` ever).
- `pub fn branch_merged_blocking(repo: &Path, branch: &str) -> bool`, read before the confirmation
  so it can say *Its branch <b> is kept: it is not merged*.

- [ ] **Step 1: Failing tests:** `a_clean_unused_worktree_is_removed`; `a_worktree_in_use_is_not_removed`;
  `a_checkout_another_project_records_is_not_removed`; `an_unmerged_branch_stays_when_its_worktree_goes`
  (a scratch repo: commit on the worktree's branch, `remove_own_blocking`, the folder gone, the
  branch still listed, `branch_kept` some); `a_merged_branch_goes_with_its_worktree`.
- [ ] **Step 2–4:** fail, implement, `cargo test -p onehand-core --lib` → PASS.
- [ ] **Step 5: Commit** `feat(worktree): a worktree that is not a task's is removed when nothing is lost`.

### Task 3: The Worktrees box (app)

**Files:** new `crates/app/src/chat/pane/project_page/worktrees_box.rs`; `project_page.rs` (mount
only); the shell's page facts (`sync_project_facts`) and a git-list read beside them
(`crates/app/src/shell/` — a new `worktree_list.rs` holding the read and its token, as `git_sweep.rs`
does); `crates/app/src/shell/drafts.rs` (draft parent from the project, the missing-folder
refusal); `crates/app/src/shell/cleanup.rs` (the own removal's modal, reusing its users and
`Confirm`); `crates/app/src/projects.rs` (forget, adopt, set parent carriers);
`crates/app/src/chat/pane/project_page.rs`'s `new_session` caret (*In <branch>*); tests.

**Interfaces:**
- Consumes Task 1's `rows`, `list_blocking`, `forget_worktree`, `set_worktree_parent`, and Task
  2's `judge_own`, `own_facts_blocking`, `remove_own_blocking`, `branch_merged_blocking`.
- The box, titled *Worktrees* with its count of recorded ones, under *Past conversations*, only
  for a project here (none on a host):
  - a recorded row: the branch mark and branch, the folder (as the page writes paths), *N
    sessions* when any, then *New session* (secondary, short) and a `⋯` with *Remove worktree…*
    (danger ink); a missing one in the muted ink with *Missing* and *Forget* in place of both.
  - an offered row: its branch and folder in the muted ink, *Add to this project* (secondary).
  - capped at `WORKTREES_SHOWN = 20`, then *N more*; *No worktrees* with none; *Reading git's
    worktrees…* while the first read runs; *git's worktrees could not be read: <why>* in the muted
    ink when it fails, the recorded rows still drawn.
  - its foot: *New worktree…* (secondary, refused with the split's own words while the project
    folder does not run here), then *Worktree folder*: the folder or *Beside the repository*, with
    *Change…* (folder picker) and *Beside the repository* (quiet, only while one is set).
- The page's *New session* caret lists *In <branch>* per recorded worktree on disk, after the
  agents.
- *Remove worktree…* opens a `Confirm` titled *Remove worktree <branch>?* judged on open and on
  confirm (as the task removal's modal is); refused reasons listed; on success the record goes
  (`forget_worktree`) and a toast says *Removed <branch>* or *Removed <folder>; its branch <b> is
  kept: it is not merged*.
- The worktree form's folder starts at the project's `worktree_parent`; a parent that no longer
  exists refuses *Create* with *<folder> is not there any more*.

- [ ] **Step 1: Failing app tests:** `the_worktrees_box_lists_recorded_missing_and_offered`
  (shell rig, a scratch repo: one recorded, one recorded then deleted with `git worktree remove`,
  one added outside onehand); `a_git_list_that_fails_keeps_the_recorded_rows`;
  `adding_an_offered_checkout_records_it_and_its_row_holds_it`;
  `removing_a_worktree_forgets_it_and_its_folder_goes`; `a_missing_worktree_folder_is_refused_in_the_form`;
  `the_worktree_folder_setting_places_the_next_worktree`.
- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement**, under the `ui` skill (the box's peer is *Build hosts* in the
  *Settings* box for rows and the *Past conversations* box for the cap and lines).
- [ ] **Step 4: Run everything** → PASS; answer the `ui` checklist.
- [ ] **Step 5: Commit** `feat(projects): a project's page manages its worktrees`.

### Task 4: Documents, and the step's own papers go

**Files:** `docs/screens.md` (*Pages without a session*: the Worktrees box, the caret's *In
<branch>*), `CONTEXT.md` (*Worktree*: missing, offered), `docs/known-gaps.md` (a worker's project
made for a stray folder is not marked as such; check the two step-1 gaps still read true),
`CHANGELOG.md` (*The window*), and delete `docs/superpowers/specs/2026-11-19-project-store-design.md`
with the four step plans; `docs/superpowers/` then holds nothing of this work.

- [ ] **Step 1:** Write the documents; `git rm` the spec and plans.
- [ ] **Step 2:** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`,
  `make fmt`, `git diff --check`.
- [ ] **Step 3: Commit** `docs(projects): the project store's behaviour lives in its documents`;
  review (opus, `origin/main..HEAD`), PR, CI, ask the owner.
