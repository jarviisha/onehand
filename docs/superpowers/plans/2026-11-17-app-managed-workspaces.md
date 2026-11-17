# App-managed workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** onehand keeps every workspace under `<config_dir>/workspaces/<id>/` itself (no bind/unbind, no unbound workspace), one record per project in the workspace file, project-owned offload data under `<config_dir>/projects/<stem>/`, and a one-time copy-not-move migration of existing data.

**Architecture:** Core owns the file shapes (`config/workspace.rs`), the workspace list (`RecentsDrive`, now listing every workspace folder) and the migration (`workspace/migrate.rs`, blocking, run at boot). The workspace drive loses `bind`/`unbind` and gains a deleted state. The app drops the storage-folder UI and *Open workspace…*, and adds *Delete workspace…*.

**Tech Stack:** Rust, serde + toml + serde_json, GPUI + gpui-component.

**Spec:** `docs/superpowers/specs/2026-11-17-app-managed-workspaces-design.md`

## Global Constraints

- Branch `feat/app-managed-workspaces` off `main`; one PR, docs included; squash merge. Commit titles `type(scope): summary`.
- Read `AGENTS.md` first. Every visible change (Tasks 3, 4, 6) follows the `ui` skill (`.claude/skills/ui/`) and answers its checklist.
- Rules and what happens next live in core drives; the app only carries effects. Match core enums exhaustively, no wildcard arms.
- No `pub` in core unless named outside the crate. No comment or string names a document.
- File names kept: `onehand-workspace.toml`, `state.toml`. `<id>` = `workspace::stem_for(folder)`; `<stem>` = `stem_for(project path)`. Never derived from the name.
- `WorkspaceLoad::Unreadable` is never treated as `Missing`; an unreadable file is never overwritten.
- Migration copies workspace folders, never moves or deletes them; offload data is renamed, never copied.
- Checks before each commit: `make fmt`, `cargo test -p onehand-core` (or `cargo test` for app tasks), and at the end `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo tree -p onehand-core -i gpui-pre` (must error "did not match any packages").
- Split any file before it passes ~800 lines of code (tests not counted).

## Review Focus

1. **Seed folder holds an unreadable workspace.** At boot with no listed workspace that loads, if `storage_for(seed)` holds an unreadable file, the window opens without ever writing over it, and says so (Task 4 test `seed_over_an_unreadable_file_never_writes_it`).
2. **A file in both shapes, or `active` naming nothing.** New shape wins; `active` matching no project reads as the first (Task 2 tests).
3. **Migration run twice, or interrupted.** A leftover temp folder is removed and the copy redone; a second run changes nothing (Task 5 tests `a_half_copy_is_redone`, `a_second_run_changes_nothing`).
4. **A late write after Delete.** After `WorkspaceDrive::delete`, `flush`, `save` and `save_soon` answer no write (Task 6 test `a_deleted_workspace_writes_nothing`), and the folder removal is queued on `Shared.writer` after the window closes.
5. **The workspace list drawn with many entries.** The rail menu caps at `MENU_CAP` and says how many are not shown (Task 4 step).

---

### Task 1: Move the workspace file's types out of `config.rs`

`config.rs` is at 799 lines; it cannot take more.

**Files:**
- Create: `crates/core/src/config/workspace.rs`
- Modify: `crates/core/src/config.rs` (lines ~437-800: `WorkspaceConfig`, `PanelLayout`, its impls, `WorkspaceLoad`, `impl WorkspaceConfig`)
- Test: whatever in `crates/core/src/config/tests.rs` / `persist_tests.rs` names them keeps compiling through the re-export.

**Interfaces:**
- Produces: `crate::config::{WorkspaceConfig, PanelLayout, WorkspaceLoad}` unchanged paths (`mod workspace; pub use workspace::{...}` in `config.rs`).

- [ ] **Step 1:** Create the branch: `git checkout -b feat/app-managed-workspaces`.
- [ ] **Step 2:** Move the items verbatim; `write_atomic` stays in `config.rs` and is used via `super::`.
- [ ] **Step 3:** Run `cargo test -p onehand-core` — expected PASS, same count as on `main`.
- [ ] **Step 4:** Commit `refactor(config): keep the workspace file's types in a module of their own`. Commit the spec and this plan in the same commit.

---

### Task 2: One record per project in the workspace file

**Files:**
- Modify: `crates/core/src/config/workspace.rs`
- Modify: `crates/core/src/workspace.rs` (`from_config` ~311-388, `to_config` ~395-440, `parse_root_key`, `root_key_of`)
- Test: `crates/core/src/config/workspace/tests.rs` (create), `crates/core/src/workspace/tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct WorkspaceConfig {            // serde(default)
      pub name: String,
      pub active: Option<String>,         // host_path(host, path) of the active project
      pub projects: Vec<ProjectConfig>,
      pub layout: PanelLayout,
  }
  pub struct ProjectConfig {               // serde(default) on every field but path; skip_serializing_if default
      pub path: PathBuf,
      pub host: Option<HostName>,
      pub pinned: bool,
      pub unattended: bool,
      pub check: Option<String>,
      pub offload: Option<OffloadConfig>,
  }
  pub struct OffloadConfig { pub host: HostName, pub mirror: PathBuf, pub branch: Option<String> }
  pub enum WorkspaceLoad { Found(WorkspaceConfig), Missing, Unreadable(String) }
  ```
- `WorkspaceConfig` deserializes through a private `Raw` struct holding both shapes (`projects`, `active`, and the old `roots`, `active_root`, `pinned`, `unattended`, `checks`, `offloaded`, `offloaded_branches`); `From<Raw>` uses the new shape when `projects` is non-empty, else builds `projects` from the old fields with today's rules (keys parsed by `parse_root_key`, which moves to `config/workspace.rs` as `pub(crate)`; an `offloaded` entry naming a host's folder is passed over; `active` = the `active_root`'th root's key). Serialize writes only the new shape.
- `Workspace::from_config` reads `projects` (normalize local paths, dedup by host+path as today); `to_config` writes them, transient roots left out, `active` = key of the active root if kept, else the first kept.

- [ ] **Step 1: Write failing tests** in `config/workspace/tests.rs`:
  - `the_old_shape_reads_as_one_record_per_project`: TOML with `roots = ["/a", "build01:/srv/b"]`, `active_root = 1`, `pinned = ["/a"]`, `checks = { "/a" = "cargo test" }`, `offloaded = { "/a" = "build01:/m" }`, `offloaded_branches = { "/a" = "feat/x" }` → `projects[0] == ProjectConfig { path: "/a", host: None, pinned: true, check: Some("cargo test"), offload: Some(OffloadConfig { host: "build01", mirror: "/m", branch: Some("feat/x") }), unattended: false }`, `projects[1].host == Some("build01")`, `active == Some("build01:/srv/b")`.
  - `the_new_shape_wins_over_the_old`: a file with both `projects = [{path="/n"}]` and `roots = ["/o"]` → one project, `/n`.
  - `a_save_writes_only_the_new_shape`: serialize → text contains `[[projects]]`, no `roots`, no `pinned = false`.
  - `an_unreadable_file_says_why`: `load_from` on a dir holding `name = ` → `Unreadable(why)` with non-empty `why`.
- [ ] **Step 2:** In `workspace/tests.rs`: `active_names_a_project_by_key` (round-trip keeps the active root across reordering) and `an_active_naming_nothing_reads_as_the_first`.
- [ ] **Step 3:** Run `cargo test -p onehand-core workspace` — expected FAIL (types missing).
- [ ] **Step 4:** Implement as in Interfaces; update every existing test and caller of the old fields and of `Unreadable` (core and app: `match` arms gain `(_)`).
- [ ] **Step 5:** `cargo test` — expected PASS.
- [ ] **Step 6:** Commit `refactor(workspace): keep one record per project in the workspace file`.

---

### Task 3: Every workspace has its folder; offload data goes to `projects/`

**Files:**
- Modify: `crates/core/src/config.rs` (add `workspaces_dir`), `crates/core/src/workspace.rs` (`storage_dir`, `seeded`, add `project_dir`), `crates/core/src/workspace/drive.rs` (remove `bind`/`unbind`, `snapshot` ~402, ~354, ~428), `crates/core/src/host/offload/drive.rs:733`, `crates/core/src/host/offload/drive/blocking.rs` (216, 361, 387, 439)
- Modify (app): `windows.rs` (`storage_dir: PathBuf`; remove `bind_storage`), `shell/boot.rs` (37, 114, 566-575, 586, 640, 648), `shell/storage.rs` (remove `storage_dir()` Option, `unbind_storage`, `bind_storage`, `pick_storage_dir`; `new_workspace` sets no storage itself), `shell/pages.rs:158`, `shell/remote_runs.rs:159`, `rail/search.rs:67`, `settings/pages.rs` (~90-150)
- Test: `crates/core/src/workspace/tests.rs`, `crates/core/src/workspace/drive/tests.rs`, `crates/app/src/shell/storage/tests.rs`, `crates/app/src/shell/tests.rs`, `crates/app/src/shell/carry/tests.rs`

**Interfaces:**
- Produces:
  - `pub fn config::workspaces_dir() -> PathBuf` = `config_dir().join("workspaces")`; `workspace::storage_for(root)` = `workspaces_dir().join(stem_for(root))` (meaning: the workspace folder made from `root`).
  - `pub fn workspace::project_dir(path: &Path) -> PathBuf` = `config_dir().join("projects").join(stem_for(path))`. All offload callers of `storage_for` switch to it (`OffloadState::load/save` dir, `coming-back`, `backup_dir`).
  - `Workspace.storage_dir: PathBuf`; `Workspace::seeded(root)` sets it to `storage_for(&root)`.
  - `WorkspaceDrive::snapshot` always has a dir; `Effect::Remember` is sent whenever the name changes (no bound check).
  - `Request::SetStorage(Option<&Path>)` keeps its type: `None` now means "the workspace was deleted" (Task 6), `Some` otherwise.

- [ ] **Step 1: Write failing tests:** `seeded_is_kept_beside_the_other_workspaces` (`Workspace::seeded("/x/proj").storage_dir == storage_for("/x/proj")` and ends in `workspaces/proj-<8 hex>`); `a_project_dir_is_named_as_its_issues_are` (`project_dir(p).file_name() == stem_for(p)`, parent named `projects`); in `host/offload/tests.rs`, `backup_dir_is_under_the_project_dir`.
- [ ] **Step 2:** Run `cargo test -p onehand-core` — expected FAIL.
- [ ] **Step 3:** Implement the core side; delete the unbound tests in `workspace/drive/tests.rs` and adjust the rest.
- [ ] **Step 4:** App side. Settings ▸ Workspace: the *Storage folder* field keeps its label and shows the folder in mono, read-only, with no buttons; `about` line: "Where onehand keeps this workspace". Delete `a_bind_is_remembered_and_an_unbind_forgotten_in_the_file` and `only_an_unbinds_failed_forget_is_said` in `shell/storage/tests.rs`.
- [ ] **Step 5:** `cargo test` — expected PASS.
- [ ] **Step 6:** Commit `refactor(workspace): keep every workspace in onehand's own folder, and offload data per project`.

---

### Task 4: The workspace list is every workspace folder

**Files:**
- Modify: `crates/core/src/config/state.rs`, `crates/core/src/config/recents.rs`
- Modify (app): `state.rs` (76, 89), `shell/boot.rs` (`boot` ~719-735, `open_or_focus`, `open_window` ~648), `shell/carry.rs:78-81`, `shell/storage.rs` (`recents`, `open_recent`, remove `open_workspace` and `workspace_in`'s second-folder use), `rail/workspace.rs` (menu), `settings/pages.rs` (remove *Open workspace…*)
- Test: `crates/core/src/config/state/tests.rs`, `crates/core/src/config/recents/tests.rs`, `crates/app/src/shell/storage/tests.rs`

**Interfaces:**
- Produces:
  ```rust
  // state.rs
  pub struct AppState {                       // serde(default)
      pub(crate) workspaces: Vec<String>,     // ids, most recently opened first; written
      #[serde(skip_serializing)] pub(crate) workspace_dir: Option<PathBuf>,      // read for Task 5 only
      #[serde(skip_serializing)] pub(crate) recent_workspaces: Vec<PathBuf>,     // read for Task 5 only
      #[serde(skip_serializing)] pub(crate) names: BTreeMap<PathBuf, String>,    // read for Task 5 only
  }
  // recents.rs
  pub struct Listed { pub dir: PathBuf, pub name: String, pub unreadable: Option<String> }
  impl RecentsDrive {
      pub fn load() -> Self;                                   // AppState::load + list_blocking(&workspaces_dir())
      pub fn new(state: AppState, path: PathBuf, listed: Vec<Listed>) -> Self;
      pub fn listed(&self) -> &[Listed];                       // state order first, then the rest by name
      pub fn opened(&mut self, dir: PathBuf, name: &str) -> Vec<Effect>;   // to the front; adds if absent
      pub fn named(&mut self, dir: &Path, name: &str) -> Vec<Effect>;      // updates the listed name in memory; no write (names are not in state.toml), so empty
      pub fn dropped(&mut self, dir: &Path) -> Vec<Effect>;                // folder gone or deleted
      pub fn handle(&mut self, input: Input) -> Vec<Effect>;
  }
  pub(crate) fn list_blocking(root: &Path) -> Vec<Listed>;  // every root/*/onehand-workspace.toml; Unreadable → name = folder name, unreadable = Some(why)
  pub enum Signal { WriteFailed(String) }                   // ForgetFailed and Forgot go
  ```
  `AppState::touch`/`forget` work on ids; `MAX_RECENTS` and the cap go. An id with no folder is dropped by `RecentsDrive::load`.
- App: `Shell::recents(&self, cx) -> Vec<Listed>`; the rail menu lists them, current one checked, unreadable ones disabled with their reason as the row's detail; `const MENU_CAP: usize = 20;` past it a muted last row "N more not shown". No parent-path detail (every folder now has the same parent). Menu entries after the separator: *New workspace…* only.
- Boot: open the first `listed()` entry that loads (`WorkspaceConfig::load_from`); none → `seed_workspace()` and, unless its folder is `Unreadable`, write it (`config.save_to`) before `open_or_focus`; a failed write opens anyway and shows a `toast::failed("Saving the workspace", why)` once the window exists.

- [ ] **Step 1: Write failing tests** (core):
  - `state_writes_only_ids`: `AppState` with `workspaces = ["a-1"]` and legacy fields set → serialized text has `workspaces`, no `recent_workspaces`, `names`, `workspace_dir`.
  - `every_workspace_folder_is_listed_in_order`: temp root with folders `a`, `b`, `c` each holding a workspace file named `A`,`B`,`C`, plus `d` holding none; state `["c"]` → `listed()` dirs `c, a, b`, no `d`.
  - `an_unreadable_workspace_is_listed_with_why`.
  - `opened_moves_to_the_front_and_writes`.
- [ ] **Step 2:** App test in `shell/storage/tests.rs`: `seed_over_an_unreadable_file_never_writes_it` (scratch `ONEHAND_CONFIG_DIR` as the existing `recents_at` helper does; seed folder's file holds `name = `; after boot's path the file's bytes are unchanged).
- [ ] **Step 3:** Run — expected FAIL.
- [ ] **Step 4:** Implement core, then app.
- [ ] **Step 5:** `cargo test` — expected PASS.
- [ ] **Step 6:** Commit `feat(workspace): list every workspace onehand keeps, and drop Open workspace`.

---

### Task 5: Migrate what earlier builds kept

**Files:**
- Create: `crates/core/src/workspace/migrate.rs`, `crates/core/src/workspace/migrate/tests.rs`
- Modify: `crates/core/src/workspace.rs` (`mod migrate; pub use migrate::migrate_storage_blocking;`), `crates/app/src/shell/boot.rs` (the `migrate_old_dir_blocking` chain ~663)

**Interfaces:**
- Produces: `pub fn migrate_storage_blocking() -> Vec<String>` (problems, for `eprintln!`) = `migrate_in(&config_dir())`; `pub(crate) fn migrate_in(config: &Path) -> Vec<String>`. Reads `state.toml` raw (no cap), runs the spec's steps 1, 1b, 2, 3 in that order.
- Step 1 copy: into `workspaces/.<id>.migrating`, then `rename` to `workspaces/<id>`; any `workspaces/.*.migrating` left from before is removed first. `<id>` = `stem_for(old folder)`. A copied file with an empty `name` gets the old `names` entry. `list_blocking` (Task 4) skips folders starting with `.`.
- Step 1b: for each `config/tasks/*.json`, parse as `serde_json::Value`, replace every string that starts with `<old>/issues/` with `<new>/issues/` + the rest; write back with `write_atomic` only when something changed.
- Step 2: for each `workspaces/*/`, rename `offload.json`, `coming-back`, `backups` into `projects/<same name>/` unless the target exists; remove the workspace folder if left empty.
- Step 3: write `state.toml` with `workspaces` = ids of every old recent that is now under `workspaces/`, old order; the legacy fields are dropped only if every step 1 copy for them landed (else written back as read).

- [ ] **Step 1: Write failing tests** (all on a temp config root):
  - `an_external_workspace_is_copied_and_left_in_place`: old folder `ext/ws02` with a workspace file and `issues/p-1.json`; state names it → `workspaces/<stem_for(ext/ws02)>/onehand-workspace.toml` and `issues/p-1.json` exist, `ext/ws02` byte-identical.
  - `the_old_name_fills_an_empty_one`.
  - `task_files_follow_the_copy`: a task JSON holding `"file": "<ext/ws02>/issues/p-1.json"` → now holds the new path; a task holding an unrelated path is byte-identical.
  - `offload_data_moves_to_the_project_dir`: `workspaces/x-1/offload.json` and `coming-back/f` → under `projects/x-1/`, `workspaces/x-1` gone.
  - `a_half_copy_is_redone`: a pre-existing `workspaces/.<id>.migrating` with partial content → after the run, target complete, no `.migrating` left.
  - `a_second_run_changes_nothing`: run twice; tree and `state.toml` identical after the second.
  - `state_keeps_the_old_order_as_ids`.
- [ ] **Step 2:** Run `cargo test -p onehand-core migrate` — expected FAIL.
- [ ] **Step 3:** Implement; call it in `boot` before `AppConfig::load_resolved`.
- [ ] **Step 4:** `cargo test` — expected PASS.
- [ ] **Step 5:** Commit `feat(workspace): bring workspaces kept by earlier builds into onehand's folder`.

---

### Task 6: Delete workspace

**Files:**
- Modify: `crates/core/src/workspace/drive.rs`, `crates/core/src/task/tasks.rs`
- Modify (app): `settings/pages.rs` (a section after *Unattended runs*), `shell/storage.rs` (`confirm_delete_workspace`, `delete_workspace`), the issues mode's `SetStorage(None)` handling (`crates/issues/src/lib.rs:194`: drop its file drive, write nothing)
- Test: `crates/core/src/workspace/drive/tests.rs`, `crates/core/src/task/tasks/tests.rs`, `crates/app/src/shell/storage/tests.rs`

**Interfaces:**
- Produces:
  - `WorkspaceDrive::delete(&mut self)`: sets `deleted`; afterwards `save`, `save_soon`, `flush`, `snapshot` answer nothing; a landed earlier write is not answered with `Remember`.
  - `Tasks::holds(&self, dir: &Path) -> bool`: some task not `dismissed()` with `outcome().is_none()` whose `issue()` tracker is `Local { file }` or `Synced { file, .. }` with `file.starts_with(dir)`.
  - App: `Shell::confirm_delete_workspace(&mut self, window, cx)` with `Confirm { id: "delete-workspace", title: "Delete <name>?", description: "Its projects stay on disk. Its issues kept in onehand go with it.", act: "Delete", .. }`; the button is `controls::action("delete-workspace").tall().secondary().label("Delete workspace…")`, `.refuses(Some("A task is still working on this workspace's issues"))` when `holds`. On yes: `workspace.delete()`, issues `SetStorage(None)`, `window.remove_window()`; then `Shared.writer.write(move || remove_dir_all(dir))` and `recents.dropped(&dir)`.
  - Section heading *Delete workspace*, line "The window closes and onehand forgets this workspace".

- [ ] **Step 1: Write failing tests:** `a_deleted_workspace_writes_nothing` (drive: change name, `delete()`, then `save_soon`, `flush` → no `Effect::Write`/`WriteOnClose`); `holds_only_unfinished_tasks_on_the_dir` (one dismissed, one finished, one running with `Local { file }` under dir → true only with the running one; a file elsewhere → false).
- [ ] **Step 2:** App test `deleting_removes_the_folder_after_the_window_closes` (scratch config root; after confirm and settle, `storage_dir` gone and not in `recents().listed()`).
- [ ] **Step 3:** Run — expected FAIL.
- [ ] **Step 4:** Implement.
- [ ] **Step 5:** `cargo test` — expected PASS.
- [ ] **Step 6:** Commit `feat(workspace): let a workspace be deleted from its settings`.

---

### Task 7: Docs, and the checks CI runs

**Files:**
- Modify: `docs/screens.md:16` (menu: the workspaces, *New workspace…*), the Settings ▸ Workspace description if it names *Choose folder…*/*Unbind*/*Open workspace…*; `docs/hosts.md:220` (a host project is a `[[projects]]` record with `host`), `:511` (`offload.json` in `<config_dir>/projects/<stem>/`); `docs/known-gaps.md:382` (remove the unbind entry); `CHANGELOG.md` (Unreleased: what changes for the person, the migration, that old folders stay as they were).
- Delete: `docs/superpowers/specs/2026-11-17-app-managed-workspaces-design.md`, this plan (their behaviour is now in the owning documents and code headers).
- Update module headers that describe bind/unbind or recents: `config/state.rs`, `config/recents.rs`, `workspace/drive.rs`, `shell/storage.rs`.

- [ ] **Step 1:** `grep -rn "Unbind\|unbind\|Open workspace\|recent_workspaces\|bound workspace" docs crates --include='*.md' --include='*.rs'` — expected only historic CHANGELOG lines.
- [ ] **Step 2:** `make fmt && make clippy CLIPPY_EXTRA="-- -D warnings" && cargo test` — expected PASS, no warnings.
- [ ] **Step 3:** `cargo tree -p onehand-core -i gpui-pre` — expected error "did not match any packages".
- [ ] **Step 4:** Commit `docs(workspace): say where onehand keeps workspaces`.
