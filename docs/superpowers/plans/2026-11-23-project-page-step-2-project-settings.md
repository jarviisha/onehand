# Project page, step 2: Settings ▸ Projects — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A project's settings (check command, build hosts, worktree folder) live in a new
*Projects* section of Settings, reachable from the project's `⋯` menu as *Project settings…*;
the project page loses its *Settings* box and the Worktrees box its *Worktree folder* field.

**Architecture:** The shell already keeps every project's check command field
(`Shell::check_inputs`, by path) and does build hosts and the worktree folder through the
projects' drive. What changes is *which* project they act on: today the project on screen
(`build_root`, `project_here`), from now on a project named by the caller. A new
`Shell::settings_project: Option<PathBuf>` says which project Settings ▸ Projects shows; the
page renders from it in `crates/app/src/settings/projects.rs`, reading the shell as the other
settings pages do.

**Tech Stack:** Rust, GPUI, gpui-component, `onehand` app crate (no core change).

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, Goals: *Settings gets a
Projects section*. Step 2 of 6. The host's *Answer cards on the host* box and an offloaded
project's host switch stay on the page until the *Cards* step.

## Global Constraints

- Settings holds no primary button; every button there is secondary.
- Words: nav row *Projects*; menu entry *Project settings…*; fields *Check command*,
  *Build hosts*, *Worktree folder* keep the words they have on the page today, their about
  lines included.
- Only projects whose sessions run on this machine and are not transient are offered
  (`ProjectRoot::runs_here()`, `!transient`), since only they have a check command, build hosts
  or worktrees.
- Every list capped with its count of the rest (build hosts keep `BUILD_HOSTS_SHOWN` = 20; the
  picker lists the window's projects, capped at `LIST_SHOWN` = 50 with *N more not shown*).
- No comment cites a document; files under ~800 lines; `make fmt`, `cargo test --workspace
  --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`; answer the `ui` checklist.

## Review Focus

1. The project shown in Settings is removed from the window while Settings is open: the page
   falls back to the first offered project, or says there is none; nothing panics on a stale
   path.
2. The build host form opened from Settings saves to the project it was opened for, even when
   the project on screen differs.
3. *Project settings…* on a host or offloaded project is not offered.
4. A check command typed in Settings shows *Run check* on that project's page once it is
   shown (the same field entity, so the same value).
5. A window with no project that runs here: the section says so instead of an empty picker.

---

### Task 1: build hosts and the worktree folder act on a named project

**Files:**
- Modify: `crates/app/src/shell/build_hosts.rs` (`open_build_host_form`,
  `confirm_remove_build_host` take `root: PathBuf`; `build_root` and `show_build_hosts` go
  once nothing calls them)
- Modify: `crates/app/src/shell/worktrees.rs:50-90` (`pick_worktree_folder(root)`, and a
  `clear_worktree_folder(root)` beside it)
- Modify: `crates/app/src/shell/boot.rs` (the arms for `AddBuildHost`, `EditBuildHost`,
  `RemoveBuildHost` pass the project on screen for now)
- Test: `crates/app/src/shell/tests.rs`

**Interfaces:**
- Produces, on `Shell`:
  - `pub(crate) fn open_build_host_form(&mut self, root: PathBuf, editing: Option<usize>, window, cx)`
  - `pub(crate) fn confirm_remove_build_host(&mut self, root: PathBuf, at: usize, window, cx)`
  - `pub(crate) fn pick_worktree_folder(&mut self, root: PathBuf, cx)`
  - `pub(crate) fn clear_worktree_folder(&mut self, root: PathBuf, cx)`

- [ ] **Step 1: failing test** `the_build_host_form_is_for_the_project_it_was_opened_on`: a
  shell with two local projects, the second on screen; `open_build_host_form(first, None, …)`;
  `assert_eq!(shell.build_host_draft().unwrap().root, first)`.
- [ ] **Step 2:** `cargo test -p onehand the_build_host_form_is_for` → FAIL (arity).
- [ ] **Step 3:** change the four signatures; callers pass the root.
- [ ] **Step 4:** `cargo test -p onehand` → PASS.
- [ ] **Step 5:** commit `refactor(projects): build hosts and the worktree folder name their project`.

### Task 2: Settings ▸ Projects

**Files:**
- Modify: `crates/app/src/settings.rs` (`SettingsPage::Projects` after `Hosts` in the enum,
  `ALL`, `label` *Projects*, `icon` `IconName::FolderOpen` or the nearest folder mark the enum
  has; `settings()` match arm)
- Create: `crates/app/src/settings/projects.rs` (`pub(super) fn projects_page(handle:
  &Entity<Shell>, cx: &App) -> AnyElement`)
- Move: `build_host_line` and the rows of `crates/app/src/chat/pane/project_page/build_hosts.rs`
  into `settings/projects.rs`, their clicks calling Task 1's methods on the shell with the shown
  project
- Modify: `crates/app/src/shell/settings_page.rs` (`settings_project` reader;
  `pub(crate) fn pick_settings_project(&mut self, root: PathBuf, window, cx)` which makes the
  check fields (`make_check_inputs`, made `pub(super)`) and sets the field)
- Modify: `crates/app/src/shell.rs` / `boot.rs` (the new field, `None`)
- Test: `crates/app/src/settings/tests.rs`

**Interfaces:**
- Consumes: Task 1's four methods; `Shell::check_inputs`.
- Produces: `Shell::settings_project(&self, cx) -> Option<PathBuf>`: the field when it names an
  offered project of the window, else the first offered one, else `None`.

- [ ] **Step 1: failing tests** in `settings/tests.rs`:
  - `the_projects_section_offers_only_projects_that_run_here`: offered list excludes a host
    root and a transient root.
  - `the_shown_project_falls_back_when_it_leaves`: field names a path no longer in the window →
    `settings_project` is the first offered.
- [ ] **Step 2:** run them → FAIL (no such page/method).
- [ ] **Step 3:** implement. The page, top to bottom, in `section`s as the Hosts page draws
  them: the *Project* picker (`crate::controls::menu_below` + `onehand_ui::picker`, as
  `schedules_page/form.rs`'s `picker`), *Check command* (`text_input` on the shell's field,
  about *What Run check and a command step naming none run here*), *Build hosts* (the moved rows,
  *Add build host…*, *No build hosts*), *Worktree folder* (the place, *Change…*, *Beside the
  repository* while set). With none offered: *No project here runs on this machine*.
- [ ] **Step 4:** `cargo test -p onehand` → PASS.
- [ ] **Step 5:** commit `feat(settings): a Projects section holds each project's settings`.

### Task 3: the page sheds its settings; the menus lead to them

**Files:**
- Modify: `crates/app/src/chat/pane/project_page.rs:560-580` (the *Settings* box goes; the host
  bypass box stays), `EmptyProject.check_input`/`builds` and `set_check_input`/`set_build_hosts`
  go if nothing else reads them (*Run check* reads the project's facts — keep that)
- Delete: `crates/app/src/chat/pane/project_page/build_hosts.rs` (moved in Task 2)
- Modify: `crates/app/src/chat/pane/project_page/worktrees_box.rs:219-272` (the *Worktree
  folder* field goes) and `events.rs` (`WorktreeAct::PickFolder`/`ClearFolder`,
  `ChatPaneEvent::AddBuildHost`/`EditBuildHost`/`RemoveBuildHost` go)
- Modify: `crates/app/src/chat/pane/events.rs` (`ProjectAction::Settings`), `header.rs` (the
  project dots menu gains *Project settings…* where the project runs here),
  `crates/app/src/rail/project/menu.rs` (same, after *Copy project path*)
- Modify: `crates/app/src/shell/settings_page.rs`: `pub fn show_project_settings(&mut self,
  root: PathBuf, window, cx)` = `pick_settings_project` + `show_settings_page(Projects)` +
  `open_settings`
- Modify docs: `docs/screens.md` (the page's *Settings* box and *Worktree folder* move to the
  *Settings* section as *Projects*; the menus' new entry), `docs/hosts.md` (*A project's build
  hosts* are changed in Settings ▸ Projects), `docs/workflows.md:628` (the check row opens
  *Project settings…*), `docs/tasks.md` if it names where the check command is typed
- Test: `crates/app/src/shell/tests.rs`

- [ ] **Step 1: failing test** `project_settings_opens_settings_on_that_project`: two projects,
  the first on screen; `show_project_settings(second, …)`; Settings is shown
  (`settings_shown`), its page is `Projects`, `settings_project` is the second.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** implement the menu entries and removals; let warnings find the dead code.
- [ ] **Step 4:** full checks: `make fmt && cargo test --workspace --locked && make clippy
  CLIPPY_EXTRA="-- -D warnings"` → all pass.
- [ ] **Step 5:** commit `feat(projects): a project's settings move to Settings`.

## After the tasks

Opus review of the branch; PR `feat(settings): a project's settings live in Settings ▸ Projects`.
