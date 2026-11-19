# Project store, step 1: projects kept by onehand — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every project gets an id and a file of its own under `<data root>/projects/<id>/`, the
workspace files refer to it, existing data is migrated, splits and run cuts are recorded as the
project's worktrees, and a project can be renamed. A worktree is still shown as its own rail row
(that changes in step 3).

**Architecture:** A new core module `project` holds the model (`Project`, `Projects`), its file,
the placement rule (which project a folder belongs to, asking git), a process-wide
`ProjectsDrive` (owns every `project.toml`, its writes and put-backs) and the migration. Each
window's `WorkspaceDrive` keeps, per row, the project's id and a copy of the facts it draws
(`label`, `check`, `builds`, `offload`) that the app refreshes by telling it the project
(`WorkspaceDrive::told`) whenever `ProjectsDrive` answers `Effect::Changed`. Changes to those facts
move from `WorkspaceDrive` to `ProjectsDrive`.

**Tech Stack:** Rust, `serde` + `toml`, GPUI on the app side, git through `crate::process`.

**Spec:** `docs/superpowers/specs/2026-11-19-project-store-design.md` (read it first; this plan
implements its step 1, with the revisions it records for step 1).

**Delivery:** two PRs. Tasks 1–7 are `feat(projects): keep projects in onehand's own store`
(nothing on screen changes). Task 8 is `feat(projects): rename a project`. Branch
`feat/project-store` from `origin/main`; the spec and this plan travel in the first PR.

## Global Constraints

- Core stays GUI-free: `cargo tree -p onehand-core -i gpui-pre` must still fail with "did not
  match any packages".
- Nothing in core is `pub` unless something outside the crate names it.
- Drives never read the clock, mint ids, or do I/O: the carrier mints a `ProjectId` and hands it
  in; writes are effects; waits are `Arm`/`Elapsed`.
- Our own enums are matched whole (no `_` arm) in drives, links and their tests.
- Every file written with `config::write_atomic`; every write on `Shared::writer`, in order.
- Code describes; it never cites a document.
- A file past ~800 lines of code is split along a seam in the same PR; tests live in
  `foo/tests.rs`.
- `<id>` is 8 lowercase hex characters; the project's folder is `projects/<id>/`, its file
  `project.toml`.
- Offload data moves to `<data root>/offload/<stem_for(folder)>/`.
- The old workspace file is copied once to `onehand-workspace.toml.before-projects` before the
  migration first rewrites it.
- Checks before each PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D
  warnings"`, `make fmt`. Build dirs: `target/` is a symlink to `/data`; a temporary worktree sets
  `CARGO_TARGET_DIR=/data/<name>-target`.

## Review Focus

1. **The migration stopped halfway** (projects written, workspace files not): the next launch
   makes no second project for any folder. Pinned in Task 6
   (`a_second_run_after_a_cut_short_one_makes_no_new_project`).
2. **An older build wrote the workspace file back** without ids: the next launch finds the same
   projects by path, no duplicates. Pinned in Task 6 (`entries_an_older_build_wrote_find_their_projects`).
3. **One folder in two workspaces** is one project, and a change made in one window shows in the
   other. Pinned in Task 6 (`one_folder_in_two_workspaces_is_one_project`) and Task 3
   (`a_change_answers_changed_for_its_project`).
4. **`onehand --build` after the migration** still finds the project's build hosts, now in
   `project.toml`. Pinned in Task 5 (`build_finds_hosts_in_the_project_file`).
5. **An offloaded folder after the migration** still finds its `offload.json`. Pinned in Task 6
   (`offload_data_moves_to_its_folder_stem`) and Task 5 (`offload_dir_is_keyed_by_folder`).

---

### Task 1: The project model and its file

**Files:**
- Create: `crates/core/src/project.rs` (types, `Projects`), `crates/core/src/project/file.rs`
  (load/save), `crates/core/src/project/tests.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod project;`), `crates/core/src/config.rs` (no
  change to `projects_in`; it already answers `<root>/projects`)

**Interfaces:**
- Produces (in `onehand_core::project`):
  - `pub struct ProjectId(String)`: `#[serde(transparent)]`, `Clone, Debug, PartialEq, Eq, Hash,
    PartialOrd, Ord`, `Display`; `pub fn as_str(&self) -> &str`;
    `pub fn mint(seed: u64) -> ProjectId` (8 lowercase hex of the FNV-1a of `seed`'s bytes;
    reuse `workspace::fnv1a`, made `pub(crate)`).
  - `pub enum MadeBy { Split, Run, Worker, Added }`, serialized lowercase (`"split"` …).
  - `pub struct Worktree { pub path: PathBuf, pub branch: String, pub from: Option<String>, pub made_by: MadeBy, pub offload: Option<Offloaded> }`
  - `pub struct Project { pub id: ProjectId, pub name: String, pub host: Option<HostName>, pub path: PathBuf, pub offload: Option<Offloaded>, pub check: Option<String>, pub builds: Vec<BuildHost>, pub worktree_parent: Option<PathBuf>, pub worktrees: Vec<Worktree> }`.
    Every `Option`/`Vec` field `#[serde(default, skip_serializing_if = …)]`, as
    `config::workspace::ProjectConfig` does. `Project::new(id, host, path) -> Project` names it
    `workspace::label_for(&path)`.
  - `pub enum Folder { Home, Worktree(usize) }` — which of a project's folders a path is.
  - `impl Project { pub fn folder(&self, host: Option<&HostName>, path: &Path) -> Option<Folder>; pub fn offload_of(&self, folder: Folder) -> Option<&Offloaded>; pub fn label_of(&self, folder: Folder) -> String }`
    (`label_of(Home)` is `name`; `label_of(Worktree(i))` is `label_for(&worktrees[i].path)`, as a
    worktree row is labelled today).
  - `#[derive(Default, Clone)] pub struct Projects(BTreeMap<ProjectId, Project>)`, also
    `FromIterator<Project>`, with
    `pub fn get(&self, id: &ProjectId) -> Option<&Project>`,
    `pub fn find(&self, host: Option<&HostName>, path: &Path) -> Option<(&Project, Folder)>`,
    `pub fn iter(&self) -> impl Iterator<Item = &Project>`,
    `pub(crate) fn put(&mut self, project: Project)`.
  - In `project/file.rs`: `pub const FILE: &str = "project.toml";`
    `pub enum ProjectLoad { Found(Project), Missing, Unreadable(String) }`,
    `pub fn load_from(dir: &Path) -> ProjectLoad`, `pub fn save_to(project: &Project, dir: &Path) -> std::io::Result<()>` (through `write_atomic`),
    `pub fn load_all_blocking(projects_dir: &Path) -> (Projects, Vec<String>)` — every
    `<projects_dir>/*/project.toml` that loads; each unreadable one a problem string, never
    written over.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/project/tests.rs`:

```rust
#[test]
fn a_project_round_trips_through_its_file() {
    let dir = scratch("round-trip");
    let mut p = Project::new(ProjectId::mint(7), None, "/code/onehand".into());
    p.check = Some("make lint".into());
    p.worktrees.push(Worktree { path: "/code/onehand-feat".into(), branch: "feat".into(),
        from: Some("main".into()), made_by: MadeBy::Split, offload: None });
    save_to(&p, &dir).unwrap();
    assert!(matches!(load_from(&dir), ProjectLoad::Found(back) if back == p));
}

#[test]
fn defaults_are_left_out_of_the_file() {
    let dir = scratch("defaults");
    save_to(&Project::new(ProjectId::mint(1), None, "/a".into()), &dir).unwrap();
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
    assert!(!text.contains("check") && !text.contains("worktrees") && !text.contains("offload"));
}

#[test]
fn a_minted_id_is_eight_lowercase_hex() {
    let id = ProjectId::mint(42);
    assert_eq!(id.as_str().len(), 8);
    assert!(id.as_str().chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(ProjectId::mint(42), ProjectId::mint(43));
}

#[test]
fn find_answers_the_home_folder_and_each_worktree() {
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.worktrees.push(wt("/code/a-x"));
    let projects = Projects::from_iter([p.clone()]);
    assert!(matches!(projects.find(None, Path::new("/code/a")), Some((_, Folder::Home))));
    assert!(matches!(projects.find(None, Path::new("/code/a-x")), Some((_, Folder::Worktree(0)))));
    assert!(projects.find(Some(&HostName("vps".into())), Path::new("/code/a")).is_none());
}

#[test]
fn an_unreadable_project_file_is_a_problem_and_kept() {
    let root = scratch("unreadable");
    std::fs::create_dir_all(root.join("deadbeef")).unwrap();
    std::fs::write(root.join("deadbeef").join(FILE), "id = [").unwrap();
    let (projects, problems) = load_all_blocking(&root);
    assert_eq!(projects.iter().count(), 0);
    assert_eq!(problems.len(), 1);
    assert_eq!(std::fs::read_to_string(root.join("deadbeef").join(FILE)).unwrap(), "id = [");
}
```

(`scratch(name)` and `wt(path)` are local helpers: a fresh temp dir per test, and a `Worktree`
with `branch = "x"`, `made_by = MadeBy::Added`. `Projects` implements `FromIterator<Project>`.)

- [ ] **Step 2: Run, expect compile failure.** `cargo test -p onehand-core project::` → FAIL.
- [ ] **Step 3: Implement the interfaces above.**
- [ ] **Step 4: Run.** `cargo test -p onehand-core project::` → PASS.
- [ ] **Step 5: Commit** `feat(projects): a project and its file`.

---

### Task 2: Which project a folder is (the placement rule)

**Files:**
- Create: `crates/core/src/project/place.rs`, `crates/core/src/project/place/tests.rs`
- Modify: `crates/core/src/project.rs` (`mod place; pub use place::{Placement, place_blocking};`)

**Interfaces:**
- Consumes: `Projects::find`, `worktree::worker::repo_of_blocking` (`Repo { top, common }`),
  `worktree::current_branch_blocking`.
- Produces:
  - `pub enum Placement { Known(ProjectId, Folder), WorktreeOf { project: ProjectId, branch: String }, New }`
  - `pub fn place_blocking(projects: &Projects, host: Option<&HostName>, folder: &Path) -> Placement`
    — in order: `projects.find` → `Known`; a host folder → `New`; else ask git about `folder`:
    when it is a **linked** checkout (its `common` is not `top/.git`), its sub-path is
    `folder.strip_prefix(top)`, and some local project's folder has the same `common` and the same
    sub-path within its own checkout → `WorktreeOf` that project, `branch` from
    `current_branch_blocking(folder)` (detached: `"HEAD"`). Anything else, git failing included →
    `New`. Projects are asked about lazily: git runs on a project's folder only when `folder` is a
    linked checkout.
  - `pub(crate) fn repo_key_blocking(folder: &Path) -> Option<(PathBuf, PathBuf, bool)>`: the
    folder's `(common, sub_path, is_main_checkout)`, `None` outside git. Task 6 groups by it.

- [ ] **Step 1: Write the failing tests** (`repo_with_worktree` copied from
  `crates/core/src/worktree/worker/tests.rs`, which makes a main checkout and a linked worktree):

```rust
#[test]
fn a_known_folder_is_known() {
    let (main, _) = repo_with_worktree("known");
    let p = Project::new(ProjectId::mint(1), None, main.clone());
    let projects = Projects::from_iter([p.clone()]);
    assert_eq!(place_blocking(&projects, None, &main), Placement::Known(p.id, Folder::Home));
}

#[test]
fn a_linked_checkout_of_a_known_project_is_its_worktree() {
    let (main, linked) = repo_with_worktree("linked");
    let p = Project::new(ProjectId::mint(1), None, main);
    let projects = Projects::from_iter([p.clone()]);
    assert!(matches!(place_blocking(&projects, None, &linked),
        Placement::WorktreeOf { project, .. } if project == p.id));
}

#[test]
fn a_linked_checkout_at_another_sub_path_is_new() {
    let (main, linked) = repo_with_worktree("sub-path");
    std::fs::create_dir_all(main.join("apps/web")).unwrap();
    let p = Project::new(ProjectId::mint(1), None, main.join("apps/web"));
    let projects = Projects::from_iter([p]);
    assert_eq!(place_blocking(&projects, None, &linked), Placement::New);
}

#[test]
fn a_main_checkout_is_never_a_worktree() {
    let (main, linked) = repo_with_worktree("main-new");
    let p = Project::new(ProjectId::mint(1), None, linked);
    let projects = Projects::from_iter([p]);
    assert_eq!(place_blocking(&projects, None, &main), Placement::New);
}

#[test]
fn a_folder_outside_git_and_a_host_folder_are_new() {
    let plain = scratch("plain");
    assert_eq!(place_blocking(&Projects::default(), None, &plain), Placement::New);
    assert_eq!(place_blocking(&Projects::default(), Some(&HostName("vps".into())), Path::new("/srv/a")), Placement::New);
}
```

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core project::place` → FAIL.
- [ ] **Step 3: Implement `place_blocking` and `repo_key_blocking`.** Canonicalize `folder` before
  comparing, as `worktree::worker` does.
- [ ] **Step 4: Run.** → PASS.
- [ ] **Step 5: Commit** `feat(projects): which project a folder belongs to`.

---

### Task 3: `ProjectsDrive`

**Files:**
- Create: `crates/core/src/project/drive.rs`, `crates/core/src/project/drive/tests.rs`
- Modify: `crates/core/src/project.rs` (`pub mod drive; pub use drive::ProjectsDrive;`),
  `docs/drives.md` (a row for the new drive: what it decides, its carrier `crates/app/src/projects.rs`,
  held on `Shared`)

**Interfaces:**
- Consumes: Tasks 1–2; `saving::{Saves, Pending}`; `workspace::{Offloaded, BuildHost, HostName}`.
- Produces (`onehand_core::project::drive`):
  - `pub const SAVE_DEBOUNCE: Duration` = 600 ms (the workspace's value).
  - `pub enum Thing { Name, Check, Builds, Worktrees, Offload(Option<HostName>, PathBuf) }` and
    `pub enum Value { Name(String), Check(Option<String>), Builds(Vec<BuildHost>), Worktrees(Vec<Worktree>), Offload { folder: PathBuf, on: Option<Offloaded> } }`
    — one `Saves<Thing, Value>` per project.
  - `pub struct ProjectWrite { pub id: ProjectId, pub pending: Pending<Thing, Value>, pub project: Project, pub dir: PathBuf }`
  - `pub enum Effect { Write(Box<ProjectWrite>), Arm { token: u64, after: Duration }, Changed(ProjectId), Signal(Signal) }`
    (`Write`'s late answer is **recorded**; `Arm`'s is **dropped** by token.)
  - `pub enum Signal { SaveFailed { name: String, why: String } }`
  - `pub enum Input { Written { id: ProjectId, pending: Pending<Thing, Value>, result: Result<(), String> }, Elapsed { token: u64 } }`
  - `pub struct ProjectsDrive` — `pub fn new(projects: Projects, dir: PathBuf) -> Self` (`dir` is
    `<data root>/projects`); `Deref<Target = Projects>`.
  - Commands (each `#[must_use]`, answering `Vec<Effect>` unless said):
    - `pub fn place(&mut self, host: Option<HostName>, folder: PathBuf, placement: Placement, made: MadeBy, fresh: ProjectId) -> (ProjectId, Vec<Effect>)`:
      `Known` → its id, no effects; `WorktreeOf` → records `Worktree { path: folder, branch, from: None, made_by: made, offload: None }`
      unless the project already records it, write now, `Changed`; `New` →
      `Project::new(fresh, host, folder)` written now, `Changed`. A `New` whose folder `find`
      already knows (placed twice before the first answer landed) answers the known id instead.
    - `pub fn record_worktree(&mut self, id: &ProjectId, worktree: Worktree) -> Vec<Effect>` —
      what a split or a run cut calls; a path already recorded is left as it is.
    - `pub fn rename(&mut self, id: &ProjectId, name: &str) -> Vec<Effect>` — trimmed; blank or
      unchanged does nothing; written now.
    - `pub fn set_check(&mut self, id: &ProjectId, typed: &str) -> Vec<Effect>` — today's
      `WorkspaceDrive::set_check` rule (trim, blank is none), written after `SAVE_DEBOUNCE`.
    - `pub fn put_build_host(&mut self, id: &ProjectId, entry: BuildHost, replacing: Option<usize>) -> Result<Vec<Effect>, String>`
      and `pub fn remove_build_host(&mut self, id: &ProjectId, entry: &BuildHost) -> Vec<Effect>`
      — moved from `WorkspaceDrive` with their refusals word for word (the "runs its sessions on
      a host" refusal asks the project folder's offload and host).
    - `pub fn set_offload(&mut self, host: Option<&HostName>, folder: &Path, on: Option<Offloaded>) -> Option<Vec<Effect>>`
      — the folder's own offload (home or worktree); `None` when no project holds the folder;
      written after `SAVE_DEBOUNCE`.
    - `pub fn handle(&mut self, input: Input) -> Vec<Effect>` — `Written` failing puts each value
      no later write carries back into the project and answers `Changed(id)` then
      `Signal(SaveFailed)`; `Elapsed` with the newest token writes every project with a change
      waiting.
  - Every change answers `Changed(id)` after its `Write`/`Arm`.

- [ ] **Step 1: Write the failing tests** (`drive/tests.rs`; a `drive()` helper makes a drive over
  one project `p` at `/code/a`):

```rust
#[test]
fn placing_a_new_folder_writes_a_project_named_after_it() {
    let mut d = ProjectsDrive::new(Projects::default(), "/data/projects".into());
    let (id, fx) = d.place(None, "/code/onehand".into(), Placement::New, MadeBy::Added, ProjectId::mint(9));
    assert_eq!(d.get(&id).unwrap().name, "onehand");
    assert!(matches!(&fx[..], [Effect::Write(w), Effect::Changed(c)] if w.dir.ends_with(id.as_str()) && *c == id));
}

#[test]
fn placing_a_known_folder_changes_nothing() {
    let (mut d, p) = drive();
    let (id, fx) = d.place(None, "/code/a".into(), Placement::Known(p.id.clone(), Folder::Home), MadeBy::Added, ProjectId::mint(9));
    assert_eq!(id, p.id);
    assert!(fx.is_empty());
}

#[test]
fn a_worktree_placed_or_recorded_twice_is_recorded_once() {
    let (mut d, p) = drive();
    let wt = Worktree { path: "/code/a-x".into(), branch: "x".into(), from: None, made_by: MadeBy::Split, offload: None };
    let _ = d.record_worktree(&p.id, wt.clone());
    assert!(d.record_worktree(&p.id, wt).is_empty());
    assert_eq!(d.get(&p.id).unwrap().worktrees.len(), 1);
}

#[test]
fn renaming_trims_and_ignores_blank_and_unchanged() {
    let (mut d, p) = drive();
    assert!(d.rename(&p.id, "   ").is_empty());
    assert!(d.rename(&p.id, "a").is_empty());
    let fx = d.rename(&p.id, "  Onehand  ");
    assert_eq!(d.get(&p.id).unwrap().name, "Onehand");
    assert!(matches!(fx.last(), Some(Effect::Changed(id)) if *id == p.id));
}

#[test]
fn a_failed_write_puts_the_name_back_and_says_so_once() {
    let (mut d, p) = drive();
    let fx = d.rename(&p.id, "B");
    let Some(Effect::Write(w)) = fx.into_iter().next() else { panic!() };
    let fx = d.handle(Input::Written { id: p.id.clone(), pending: w.pending, result: Err("disk full".into()) });
    assert_eq!(d.get(&p.id).unwrap().name, "a");
    assert_eq!(fx.iter().filter(|e| matches!(e, Effect::Signal(Signal::SaveFailed { .. }))).count(), 1);
    assert!(fx.iter().any(|e| matches!(e, Effect::Changed(id) if *id == p.id)));
}

#[test]
fn a_change_answers_changed_for_its_project() {
    let (mut d, p) = drive();
    let fx = d.set_check(&p.id, " make test ");
    assert!(fx.iter().any(|e| matches!(e, Effect::Arm { .. })));
    assert!(fx.iter().any(|e| matches!(e, Effect::Changed(id) if *id == p.id)));
    assert_eq!(d.get(&p.id).unwrap().check.as_deref(), Some("make test"));
}

#[test]
fn offload_is_the_folders_own() {
    let (mut d, mut p) = drive();
    let _ = d.record_worktree(&p.id, Worktree { path: "/code/a-x".into(), branch: "x".into(), from: None, made_by: MadeBy::Split, offload: None });
    let on = Offloaded { host: HostName("vps".into()), mirror: "/m".into(), branch: None };
    let _ = d.set_offload(None, Path::new("/code/a-x"), Some(on.clone())).unwrap();
    p = d.get(&p.id).unwrap().clone();
    assert_eq!(p.offload, None);
    assert_eq!(p.worktrees[0].offload, Some(on));
    assert!(d.set_offload(None, Path::new("/elsewhere"), None).is_none());
}

#[test]
fn build_hosts_keep_their_refusals() {
    let (mut d, p) = drive();
    let entry = BuildHost::new(HostName("win".into()));
    let _ = d.put_build_host(&p.id, entry.clone(), None).unwrap();
    assert_eq!(d.put_build_host(&p.id, entry, None).unwrap_err(), "a already builds on win");
}
```

(`BuildHost::new` stands for however `crates/core/src/workspace/build_host.rs` builds one in its
own tests; use that.)

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core project::drive` → FAIL.
- [ ] **Step 3: Implement the drive.** Model the save/put-back on `WorkspaceDrive`'s
  (`change`, `save`, `save_soon`, `handle(Input::Written)`), one `Saves` per project, one debounce
  token for the whole drive.
- [ ] **Step 4: Run.** → PASS. Then `cargo test -p onehand-core` → PASS.
- [ ] **Step 5: Commit** `feat(projects): the drive that keeps every project`.

---

### Task 4: Workspace files refer to projects; rows carry their project

**Files:**
- Modify: `crates/core/src/config/workspace.rs`, `crates/core/src/workspace.rs`,
  `crates/core/src/workspace/drive.rs`, `crates/core/src/workspace/drive/projects.rs`,
  `crates/core/src/workspace/drive/conversation.rs`, `crates/core/src/task/tasks/link.rs`,
  their tests (`config/tests.rs`, `workspace/tests.rs`, `workspace/drive/tests.rs`,
  `task/tests.rs` where they build rows)

**Interfaces:**
- Consumes: Task 1 (`Project`, `Projects`, `Folder`, `ProjectId`).
- Produces:
  - `ProjectConfig` (the workspace file's entry) gains `pub id: Option<ProjectId>` (written first,
    left out when `None`). `check`, `offload`, `builds` stay readable (an entry an older build
    wrote) but are no longer written: `#[serde(default, skip_serializing)]`. `host`, `path`,
    `pinned` are written as today. `active` stays a folder key (`host_path`) in this step.
  - `ProjectRoot` gains `pub project: Option<ProjectId>` (`None` only for a transient row and for
    an entry nothing could place). Its `label`, `check`, `builds`, `offload` keep their names and
    types and become the project's facts as last told; doc them so.
  - `impl ProjectRoot { pub fn of(project: &Project, folder: Folder) -> ProjectRoot; pub(crate) fn told(&mut self, project: &Project) }`
    — `told` sets `label = project.label_of(folder)`, `check`, `builds` (project's),
    `offload = project.offload_of(folder)`, where `folder` is found again by the row's path.
  - `Workspace::from_config(cfg: WorkspaceConfig, storage_dir: PathBuf, projects: &Projects) -> Workspace`
    — an entry whose `id` names a project and whose path is one of its folders becomes
    `ProjectRoot::of`; otherwise `projects.find(host, path)` is tried; otherwise a row with
    `project: None` and the entry's own `check`/`builds`/`offload` (an older build's facts).
  - `Workspace::to_config` writes `id` from `root.project`.
  - `launch_blocking(listed, seed, projects: &Projects) -> Launch` (passes `projects` through).
  - `Workspace::seeded(project: &Project) -> Workspace` (the seed's row is `ProjectRoot::of(project, Folder::Home)`; its storage dir still `storage_for(&project.path)`).
  - `WorkspaceDrive`:
    - `pub fn told(&mut self, project: &Project) -> Vec<Effect>` — updates every row whose
      `project` is `project.id`; a row whose `offload` changed has its sessions dropped
      (`drop_sessions_in`, today's `set_offload` rule) and, going back to none, answers
      `Effect::ForgotOffload(path)`; answers `Effect::Changed` when anything changed. Writes
      nothing: these facts are not in the workspace file.
    - `open_project(dir, project: &Project)`, `open_host_project(project: &Project)`,
      `keep_project(dir, project: &Project) -> (usize, Vec<Effect>)`,
      `keep_project_quietly(dir, project: &Project)` — the new row is
      `ProjectRoot::of(project, project.folder(host, &dir)?)`; `Workspace::add_root`,
      `add_host_root`, `add_root_quietly` take the built `ProjectRoot` instead of a path.
    - Removed: `set_check`, `put_build_host`, `remove_build_host`, `set_builds`, `set_offload`,
      `Thing::Check`, `Thing::Builds`, `Value::Check`, `Value::Builds`.
  - `task::tasks::link::keep(tasks, workspace, project: Option<&Project>, start)` — a `None`
    project refuses the keep as a gone window does today.
  - `workspace::drive::conversation`: the resume path that called `keep_project(saved.root)` now
    answers a new `Effect::Place { root: PathBuf, then: Saved }` instead when the row is not in
    the workspace; the app places the folder (Task 7) and calls
    `pub fn resume_placed(&mut self, saved: Saved, project: &Project) -> Vec<Effect>`, which does
    what the old branch did after `keep_project`.

- [ ] **Step 1: Write the failing tests:**

```rust
// config/tests.rs
#[test]
fn an_entry_writes_its_id_and_no_project_facts() {
    let mut cfg = WorkspaceConfig::default();
    cfg.projects.push(ProjectConfig { id: Some(ProjectId::mint(3)), path: "/a".into(),
        check: Some("make".into()), ..Default::default() });
    let text = toml::to_string_pretty(&cfg).unwrap();
    assert!(text.contains(&format!("id = \"{}\"", ProjectId::mint(3))));
    assert!(text.contains("path = \"/a\""));
    assert!(!text.contains("check"));
}

#[test]
fn an_older_entry_still_reads_its_facts() {
    let cfg: WorkspaceConfig = toml::from_str("name = \"w\"\n[[projects]]\npath = \"/a\"\ncheck = \"make\"\n").unwrap();
    assert_eq!(cfg.projects[0].id, None);
    assert_eq!(cfg.projects[0].check.as_deref(), Some("make"));
}

// workspace/tests.rs
#[test]
fn rows_take_their_facts_from_their_project() {
    let mut p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    p.name = "Alpha".into();
    p.check = Some("make".into());
    p.worktrees.push(wt("/code/a-x"));
    let projects = Projects::from_iter([p.clone()]);
    let cfg = config_with(&[(Some(p.id.clone()), "/code/a"), (Some(p.id.clone()), "/code/a-x")]);
    let ws = Workspace::from_config(cfg, "/ws".into(), &projects);
    assert_eq!(ws.roots[0].label, "Alpha");
    assert_eq!(ws.roots[1].label, "a-x");
    assert_eq!(ws.roots[1].check.as_deref(), Some("make"));
    assert_eq!(ws.roots[1].project.as_ref(), Some(&p.id));
}

#[test]
fn an_entry_without_an_id_finds_its_project_by_path() {
    let p = Project::new(ProjectId::mint(1), None, "/code/a".into());
    let ws = Workspace::from_config(config_with(&[(None, "/code/a")]), "/ws".into(), &Projects::from_iter([p.clone()]));
    assert_eq!(ws.roots[0].project, Some(p.id));
}

#[test]
fn transient_rows_are_still_left_out_and_ids_are_written() {
    // to_config of a workspace with one placed row and one transient row
    // holds one entry, with the placed row's id.
}

// workspace/drive/tests.rs
#[test]
fn told_a_new_name_relabels_every_row_of_the_project() { /* label of home row becomes the new name; worktree row keeps its folder name; Effect::Changed answered; no Effect::Write */ }

#[test]
fn told_an_offload_drops_the_sessions_of_that_folder_only() { /* sessions in /code/a-x close (Effect::Close), sessions in /code/a stay */ }
```

(Write the two commented tests out in full in the same style; `config_with` and `wt` are local
helpers.)

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core workspace config` → FAIL.
- [ ] **Step 3: Implement.** Move each removed `WorkspaceDrive` test of check/builds/offload to
  Task 3's suite if it is not already covered there; delete the rest.
- [ ] **Step 4: Run.** `cargo test -p onehand-core` → PASS (the app does not compile yet; that is
  Task 7).
- [ ] **Step 5: Commit** `refactor(workspace): rows carry their project`.

---

### Task 5: What still finds a project by its folder outside the window

**Files:**
- Modify: `crates/core/src/workspace.rs` (`project_dir` → `offload_dir`),
  `crates/core/src/host/offload/drive.rs`, `crates/core/src/host/offload/drive/blocking.rs`,
  `crates/core/src/host/build/cli.rs`, their tests

**Interfaces:**
- Produces:
  - `pub(crate) fn offload_dir(path: &Path) -> PathBuf` = `config_dir()/offload/stem_for(path)`;
    every `project_dir` caller uses it; `config::offload_in(root) -> PathBuf` beside
    `projects_in`.
  - `host::build::cli` finds the project through the store:
    `load_all_blocking(&projects_in(root))`, the project whose folder or a recorded worktree is
    `repo.top` or `main`; its name and `builds` as before; "is not a project of any workspace"
    becomes "is not a project onehand keeps".

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn offload_dir_is_keyed_by_folder() {
    let dir = offload_dir(Path::new("/code/onehand"));
    assert!(dir.ends_with(format!("offload/{}", stem_for(Path::new("/code/onehand")))));
}

#[test]
fn build_finds_hosts_in_the_project_file() {
    // a data root with projects/<id>/project.toml whose path is a temp repo and builds = [win],
    // asked from inside the repo: Asked's entry is the win entry.
}
```

(Write the second in full on the pattern of the existing tests in
`crates/core/src/host/build/cli` — they already build a data root with a workspace file; build a
project file instead.)

- [ ] **Step 2: Run, expect failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** `cargo test -p onehand-core host::` → PASS.
- [ ] **Step 5: Commit** `refactor(projects): offload data and builds found without a workspace`.

---

### Task 6: The migration

**Files:**
- Create: `crates/core/src/project/migrate.rs`, `crates/core/src/project/migrate/tests.rs`
- Modify: `crates/core/src/workspace/migrate.rs` (`migrate_in` calls the project migration last;
  `move_offloads` targets `offload/<name>`), `crates/core/src/workspace/migrate/tests.rs`

**Interfaces:**
- Consumes: Tasks 1, 2 (`repo_key_blocking`), 4 (`ProjectConfig.id`), 5 (`offload_in`).
- Produces: `pub(crate) fn migrate_projects_in(config: &Path, seed: u64) -> Vec<String>` — the
  problems met. `seed` mints ids (`ProjectId::mint(seed + n)`, skipping any whose folder exists).
  `workspace::migrate_storage_blocking(seed: u64)` takes it from its caller (boot passes the
  clock's nanoseconds xor the process id), since nothing in core reads the clock.

Rules, in this order, every step passing over what is already in place:

1. Load the store (`load_all_blocking`) and every workspace file (`recents::list_blocking`,
   `WorkspaceConfig::load_from`; an unreadable one is a problem, untouched).
2. For each entry whose `id` is `None` or names no project in the store: if `projects.find` knows
   its folder, that is its project. Otherwise key it: a host folder by `(host, path)`; a local
   folder through `repo_key_blocking` by `(common, sub_path)`; a folder git cannot answer for by
   its path. Entries of one key make one project:
   - its folder: the entry that is the main checkout; else `common`'s parent joined with
     `sub_path` when that is a folder on disk; else the first entry met;
   - every other folder of the key: a `Worktree` with `made_by = Added`, branch from
     `current_branch_blocking` (`"HEAD"` when detached), its entry's `offload` as its own;
   - `check` and `builds`: the folder's entry's, else the first entry having them; the folder's
     entry's `offload` as the project's.
   A key that matches a project already in the store (its folder has that key) adds to it instead
   of making one.
3. Write each new or changed project (`save_to`), then rewrite each workspace file whose entries
   changed, giving each entry its id; copy the file to `onehand-workspace.toml.before-projects`
   first, only when that copy does not exist.
4. Rename every `projects/<name>/` that has no `project.toml` and holds any of `offload.json`,
   `coming-back`, `backups` to `offload/<name>/` (each entry of it moved with `move_into`; an
   existing target is passed over and said; the emptied folder removed).

- [ ] **Step 1: Write the failing tests** (`migrate/tests.rs`; `root()` makes a data root,
  `workspace(root, id, entries)` writes a workspace file, `repo_with_worktree` as in Task 2):

```rust
#[test]
fn worktree_entries_of_one_repository_become_one_project() {
    let (root, (main, linked)) = (root("one-repo"), repo_with_worktree("one-repo"));
    workspace(&root, "w1", &[&main, &linked]);
    assert!(migrate_projects_in(&root, 1).is_empty());
    let (projects, _) = load_all_blocking(&root.join("projects"));
    let all: Vec<_> = projects.iter().collect();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].path, main);
    assert_eq!(all[0].worktrees[0].path, linked);
    assert_eq!(all[0].worktrees[0].made_by, MadeBy::Added);
    let cfg = load_ws(&root, "w1");
    assert!(cfg.projects.iter().all(|e| e.id.as_ref() == Some(&all[0].id)));
}

#[test]
fn one_folder_in_two_workspaces_is_one_project() { /* w1 and w2 both hold /tmp/plain: one project, both entries its id */ }

#[test]
fn a_second_run_after_a_cut_short_one_makes_no_new_project() {
    // run once; then rewrite w1 as it was (entries without ids), run again:
    // still one project, the same id, and the .before-projects copy is the first one.
}

#[test]
fn entries_an_older_build_wrote_find_their_projects() {
    // run once; then rewrite w1 with ids dropped and check = "make" on the entry: run again:
    // one project, and the entry has the project's id.
}

#[test]
fn a_main_checkout_not_in_any_workspace_is_still_the_project_folder() {
    // only the linked worktree is in w1: the project's path is the main checkout,
    // and the linked one is its worktree.
}

#[test]
fn a_folder_that_does_not_resolve_is_a_project_of_its_own() { /* /nonexistent/x: one project at that path */ }

#[test]
fn an_unreadable_workspace_file_is_left_untouched() { /* bytes before == bytes after; one problem */ }

#[test]
fn offload_data_moves_to_its_folder_stem() {
    // projects/onehand-1234abcd/offload.json -> offload/onehand-1234abcd/offload.json,
    // and projects/ then holds only project folders.
}
```

(Write each commented test out in full in the style of the first.)

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core project::migrate` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** `cargo test -p onehand-core migrate` → PASS.
- [ ] **Step 5: Commit** `feat(projects): bring every workspace's projects into the store`.

---

### Task 7: The app carries `ProjectsDrive`

**Files:**
- Create: `crates/app/src/projects.rs` (the carrier; private module in `lib.rs`)
- Modify: `crates/app/src/state.rs` (`Shared.projects: ProjectsDrive`),
  `crates/app/src/shell/boot.rs`, `crates/app/src/shell/boot/open.rs`,
  `crates/app/src/shell/storage.rs`, `crates/app/src/shell/roots.rs`,
  `crates/app/src/shell/drafts.rs`, `crates/app/src/shell/workflows.rs`,
  `crates/app/src/task/place.rs`, `crates/app/src/shell/carry.rs`,
  `crates/app/src/shell/check_commands.rs`, `crates/app/src/shell/build_hosts.rs`,
  `crates/app/src/shell/offload.rs`, `crates/app/src/offload.rs`, and every other caller the
  compiler names

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces (`crate::projects`):
  - `pub(crate) fn carry(fx: Vec<project::drive::Effect>, cx: &mut App)` — `Write` on
    `Shared::writer` (`save_to`), its answer back as `Input::Written`; `Arm` as a timer answering
    `Input::Elapsed`; `Changed(id)` → for each window in `crate::windows::live(cx)`, deferred,
    `shell.window.workspace.told(&project)` carried with `carry_workspace`; `Signal::SaveFailed`
    → a toast in each window holding the project, `Couldn't save {name}: {why}`.
  - `pub(crate) fn mint(cx: &App) -> ProjectId` — `ProjectId::mint` from the clock's nanoseconds
    and a process counter, again until `projects.get` and `projects/<id>` are both free.
  - `pub(crate) fn place(host: Option<HostName>, folder: PathBuf, made: MadeBy, cx: &mut App, then: impl FnOnce(&Project, &mut App) + 'static)`
    — `projects.find` answers at once; otherwise `place_blocking` on the background executor over
    a clone of the projects, then `ProjectsDrive::place` and `carry`, then `then`.
  - `pub(crate) fn record(of: &Path, worktree: Worktree, cx: &mut App) -> Option<Project>` — the
    project holding `of` (home or worktree) records `worktree`; answers the project as it now is.

Changes at the call sites:
- **Boot:** after `migrate_storage_blocking`, `load_all_blocking(projects_in(config_dir()))`
  (problems logged), `ProjectsDrive::new` into `Shared`. The seed folder is placed blocking
  (`place_blocking` then `ProjectsDrive::place` with `mint`), `Workspace::seeded(&project)`, and
  the place's effects carried once `Shared` exists. `launch_blocking` gets `&shared.projects`.
- **Opening a workspace** (`storage.rs`, `cli.rs` path already done): `from_config(…, &projects)`.
- **Folder picker and second launch** (`Shell::open_root`): `projects::place(None, dir, Added, …)`
  then `open_project(dir, &project)`. **Host folder** (`open_host_root`): the same with the host.
- **Split** (`drafts.rs`): after `add_blocking`/`subtree_or`, `projects::record(&draft.root,
  Worktree { path: dir, branch, from: <the branch the draft was cut from, if the draft knows it>,
  made_by: Split, offload: None })`, then `open_project(dir, &project)`.
- **Run cuts** (`workflows.rs` launcher, `task/place.rs` `Effect::Cut`): on `Ok(cut)`,
  `projects::record(<the launch's project folder>, Worktree { path: cut.dir, branch: cut.branch,
  from: None, made_by: Run, offload: None })` before `keep_project` / feeding `Input::Cut`.
  `link::keep` is handed `projects.find(None, dir)`'s project.
- **Checkout starts** (`workflows.rs:396`, `keep_project(dir)`): the project from
  `projects.find(None, &dir)`; none → the same refusal a gone project gets (`PROJECT_GONE`).
- **Resume** (`Effect::Place` from Task 4): `projects::place(None, root, Added, …)` then
  `resume_placed`.
- **Check, build hosts, offload:** `check_commands.rs` → `projects.set_check(&id, …)`;
  `build_hosts.rs` → `put_build_host`/`remove_build_host` by the row's project id; `shell/offload.rs`
  → `projects.set_offload(None, &path, …)`; each answer `carry`'d. `carry.rs`'s `put_back` arms
  for `Value::Check`/`Value::Builds` go (the project's `Changed` redraws them); the check field's
  text is re-read from the row on `Changed`.
- **Offload paths:** `crate::offload::backup_dir` and anything else naming `project_dir` use
  `offload_dir`.

- [ ] **Step 1: Make it compile.** `cargo check --workspace` until clean; each fix at a caller is
  one of the bullets above.
- [ ] **Step 2: Write the failing app test** in `crates/app/src/shell/tests.rs` (or the module the
  existing shell tests use), on the `Shared::for_test` pattern:

```rust
#[gpui::test]
fn renaming_a_project_relabels_it_in_every_window(cx: &mut gpui::TestAppContext) {
    // two windows holding one project; ProjectsDrive::rename carried through projects::carry:
    // both windows' rows read the new label once the deferred updates ran.
}
```

- [ ] **Step 3: Run, expect failure, then fix until it passes.** `cargo test -p onehand renaming_a_project` → PASS.
- [ ] **Step 4: Run everything.** `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt`, and the GUI-free check (`cargo tree -p onehand-core -i gpui-pre` fails as it should).
- [ ] **Step 5: Commit** `feat(projects): the app keeps projects through their drive`.
- [ ] **Step 6: Review and PR 1.** An opus subagent reviews `origin/main..HEAD`; fix what it
  confirms; open PR `feat(projects): keep projects in onehand's own store` (house style), wait for
  CI, ask the owner before merging.

---

### Task 8: Rename a project (PR 2)

**Files:**
- Modify: `crates/app/src/rail/project.rs` (the project row's menu), the project page
  (`crates/app/src/chat/pane/project_page.rs`), `crates/app/src/dialogs.rs` or the module the
  workspace's own rename dialog lives in (reuse it), `CONTEXT.md`, `docs/screens.md`,
  `docs/known-gaps.md`

**Interfaces:**
- Consumes: `ProjectsDrive::rename`, `crate::projects::carry`.

- [ ] **Step 1: Read the `ui` skill and follow its checklist** for a row-menu item and a rename
  dialog; reuse the dialog the workspace's *Rename…* opens.
- [ ] **Step 2: Add *Rename project…*** to a project folder's row menu and to its page's menu (not
  to a worktree row: it has no name of its own yet). The dialog starts on the current name; *Save*
  calls `rename` and carries it; blank keeps *Save* spent.
- [ ] **Step 3: Write the app test** `renaming_from_the_row_menu_saves_the_project` (the dialog's
  submit reaches `ProjectsDrive::rename`; the row's label reads the new name).
- [ ] **Step 4: Documents.** `CONTEXT.md`: *Project* — "A repository or a folder onehand keeps,
  under a name of its own, with the worktrees cut from it and the sessions started on it";
  *Project folder* and *Worktree* added, as the spec's *Goals* word them. `docs/screens.md`: the
  row menu's and project page menu's *Rename project…*. `docs/known-gaps.md`: deleting a project's
  kept data; following a moved project folder.
- [ ] **Step 5: Run checks** as in Task 7 Step 4; **commit** `feat(projects): rename a project`;
  review, PR `feat(projects): rename a project`, CI, ask before merging.
