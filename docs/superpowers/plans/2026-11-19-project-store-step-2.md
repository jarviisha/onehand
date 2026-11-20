# Project store, step 2: issues and conversations follow the project — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A project keeps one issues file, `projects/<id>/issues.json`, shared by every folder of
it and every workspace holding it; a conversation records the project it belongs to, and a
project's page lists the conversations of a worktree since removed.

**Architecture:** Core gains `project::file::issues_file(&ProjectId)`, an `Issues` merge, and two
more migration passes in `project::migrate` (legacy issue files merged in, `meta.json` stamped
with its project). The Issues views stop computing a file from the workspace's storage folder:
the Workbench tells them, per project folder, the file to read (`Request::IssueFiles`). A
conversation's `meta.json` gains `project`, and listing goes through a `chat::Scope` the app
builds from the projects drive, so a row lists what ran in its folder and a project's own row also
lists what ran in a folder the project no longer records.

**Tech Stack:** Rust, serde/serde_json, GPUI.

**Spec:** `docs/superpowers/specs/2026-11-19-project-store-design.md` (step 2; *Conversations,
tasks and schedules*, *Migration* parts 4–5).

**Delivery:** two PRs, branch per PR from `origin/main`.
PR A, tasks 1–4: `feat(issues): one issues file per project`.
PR B, tasks 5–8: `feat(chat): conversations follow their project`. Each ends with an opus review,
CI, and the owner's word before merging.

## Global Constraints

- Core stays GUI-free (`cargo tree -p onehand-core -i gpui-pre` fails to match), no async runtime,
  nothing `pub` that nothing outside names.
- Drives never read the clock or do I/O; migration and listing are blocking functions run off the
  UI thread (migration at boot, before any window).
- Our own enums matched whole; `Request` gains a variant, so every mode's `handle` names it.
- Every file written through `config::write_atomic` (or `issues::save_blocking`, which uses it).
- Migration: nothing deleted; a file that cannot be read is never written over and is said; a run
  cut short anywhere is finished by the next launch without duplicating anything.
- The issues file of a project: `<data root>/projects/<id>/issues.json`.
- A legacy issues file, once merged, is renamed to `<stem>.json.merged` beside where it was.
- Only a project whose folder is on this machine keeps issues (as today: a host project has none).
- Host sessions keep no conversation here (as today); nothing in this plan touches them.
- Checks before each PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D
  warnings"`, `make fmt`. `target/` is on `/data`.

## Review Focus

1. **The issues merge cut short** after the project's file is written and before its sources are
   renamed: the next launch must not merge them again (local issues would come back twice, under
   new numbers). Pinned in Task 2 (`a_second_merge_takes_nothing_twice`).
2. **The same forge issue in two legacy files** (a project and its worktree both synced with
   GitHub) appears once, the newer copy kept. Pinned in Task 1
   (`a_forge_issue_kept_in_two_files_is_kept_once_the_newer`).
3. **A project's worktree row and its own row** show the same issues, and an issue written from one
   shows in the other; the workspace page lists it once. Pinned in Task 4
   (`a_projects_folders_share_one_issues_file`).
4. **A conversation of a worktree since removed** is listed on the project's own row, and a
   conversation of a worktree still a row lists on that row only, never twice on the workspace
   page. Pinned in Task 5 (`a_scope_takes_its_folder_and_its_projects_strays`) and Task 7
   (`a_conversation_is_listed_once_across_a_projects_rows`).
5. **A `meta.json` written by an older build** (no `project`) still lists on its folder's row, and
   an older build still reads a new one. Pinned in Task 5
   (`a_meta_without_a_project_reads_and_one_with_it_reads_in_an_older_shape`).

---

## PR A: issues

### Task 1: One issues file per project, and merging two

**Files:**
- Modify: `crates/core/src/issues.rs`, `crates/core/src/project/file.rs`, `crates/core/src/issues/tests.rs`

**Interfaces:**
- Produces:
  - `onehand_core::project::file::issues_file(id: &ProjectId) -> PathBuf` =
    `store_dir().join(id.as_str()).join("issues.json")`.
  - `Issues` gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub(crate) merged_from: Vec<String>`:
    the legacy files already taken in, by path, so a merge is never taken twice.
  - `pub(crate) fn merge(into: &mut Issues, from: Issues, source: &Path) -> bool` in `issues.rs`:
    `false`, changing nothing, when `source` is in `into.merged_from`; else takes every issue of
    `from` in and records `source`:
    - an issue with a `link` that `into` holds an issue with the same `link.connector` and
      `link.key` for: kept once, the copy with the newer `updated`, at `into`'s number;
    - any other: at its own number when `into` has no issue of that number, else at `into.next`;
      `into.next` stays past every number held;
    - `synced_with` and `last_synced`: `into`'s when it has them, else `from`'s.
  - Removed: `issues::file_for` and `issues::issues_dir`'s use for new files (`issues_dir` stays
    `pub(crate)` for `remove_workspace_blocking` and the migration, which read the legacy place).
    Every caller that used `file_for` is changed in Tasks 3–4.
  - `IssueKey`'s doc: named by its file and number; one file per project, shared by every folder
    of it and every workspace holding it.
  - `save_blocking`'s guard and doc: the file's folder's parent (`projects/`, or a workspace folder
    for a legacy file) must still be there — unchanged in code, its doc no longer says *workspace*.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/issues/tests.rs`:

```rust
#[test]
fn a_forge_issue_kept_in_two_files_is_kept_once_the_newer() {
    let mut into = issues_with(vec![linked(1, "gh", "42", 100, "old title")]);
    let from = issues_with(vec![linked(7, "gh", "42", 200, "new title")]);
    assert!(merge(&mut into, from, Path::new("/ws/issues/a-x.json")));
    assert_eq!(into.issues.len(), 1);
    assert_eq!((into.issues[0].number, into.issues[0].title.as_str()), (1, "new title"));
}

#[test]
fn a_local_issue_keeps_its_number_where_free_and_takes_the_next_where_not() {
    let mut into = issues_with(vec![local(1, "a"), local(2, "b")]);
    let from = issues_with(vec![local(2, "c"), local(5, "d")]);
    merge(&mut into, from, Path::new("/ws/issues/a-x.json"));
    let numbers: Vec<_> = into.issues.iter().map(|i| (i.number, i.title.clone())).collect();
    assert_eq!(numbers, vec![(1, "a".into()), (2, "b".into()), (3, "c".into()), (5, "d".into())]);
    assert_eq!(into.next, 6);
}

#[test]
fn a_source_merged_once_is_not_merged_again() {
    let mut into = Issues::default();
    let source = Path::new("/ws/issues/a-x.json");
    assert!(merge(&mut into, issues_with(vec![local(1, "a")]), source));
    assert!(!merge(&mut into, issues_with(vec![local(1, "a")]), source));
    assert_eq!(into.issues.len(), 1);
}

#[test]
fn a_projects_issues_file_is_in_its_folder() {
    let id = ProjectId::mint(5);
    assert_eq!(issues_file(&id), store_dir().join(id.as_str()).join("issues.json"));
}
```

(`issues_with`, `local(number, title)` and `linked(number, connector, key, updated, title)` are
local helpers building `Issues` and `LocalIssue`s the way the file's other tests do.)

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core --lib issues::tests` → FAIL (no `merge`).
- [ ] **Step 3: Implement.** Keep `from`'s issue order; append in it.
- [ ] **Step 4: Run.** → PASS. `cargo check -p onehand-core` may fail on removed `file_for` in
  core callers (`task/work`, `issues/link.rs` tests): point each at a file built by the test
  itself; production callers outside core are Task 3–4's.
- [ ] **Step 5: Commit** `feat(issues): one issues file per project, and merging two`.

### Task 2: The migration brings each project's issues in

**Files:**
- Modify: `crates/core/src/project/migrate.rs`, `crates/core/src/project/migrate/tests.rs`

**Interfaces:**
- Consumes: Task 1's `merge`, `issues_file`; `issues::load_blocking`, `issues::save_blocking` (made
  `pub(crate)`); `workspace::stem_for`; `config::workspaces_in`.
- Produces: `fn merge_issues(config: &Path, projects: &Projects, problems: &mut Vec<String>)`,
  called last in `migrate_projects_in`. For each project whose folder is on this machine, and
  each workspace folder under `workspaces/`, each of the project's folders (its own, then its
  worktrees in their order) whose legacy file `<ws>/issues/<stem_for(folder)>.json` exists:
  load it (unreadable: said, left as it is), `merge` it into the project's issues (read from
  `projects/<id>/issues.json`, empty when absent); once every source of the project is taken,
  `save_blocking` the project's file; only once that landed, rename each source taken to
  `<stem>.json.merged` (a rename that fails is said; `merged_from` keeps it from being taken
  twice).

- [ ] **Step 1: Write the failing tests** (`migrate/tests.rs`, building legacy files with
  `issues::save_blocking` under `<root>/workspaces/<ws>/issues/`):

```rust
#[test]
fn a_projects_issues_from_every_folder_and_workspace_land_in_its_file() {
    // repo_with_worktree; w1 holds main and the worktree, each with a legacy file of one
    // local issue #1; w2 holds main with a legacy file of one issue #1.
    // After migrate_projects_in: projects/<id>/issues.json holds three issues numbered 1, 2, 3,
    // each legacy file is renamed to .json.merged, and nothing else under workspaces/ changed.
}

#[test]
fn a_second_merge_takes_nothing_twice() {
    // As above, then put each .json.merged back to .json (a run cut short before its renames)
    // and run again: still three issues.
}

#[test]
fn an_unreadable_legacy_file_is_left_and_said() {
    // A legacy file holding "{": a problem names it, it keeps its name and bytes, and the
    // project's file holds the other sources' issues.
}

#[test]
fn a_host_project_keeps_no_issues_file() {
    // A host entry: no projects/<id>/issues.json is written.
}
```

(Write each out in full in the style of the file's other tests.)

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core --lib project::migrate` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** → PASS.
- [ ] **Step 5: Commit** `feat(projects): bring each project's issues into its file`.

### Task 3: The Issues views are told each folder's file

**Files:**
- Modify: `crates/core/src/workbench.rs` (`Request`), `crates/issues/src/lib.rs`,
  `crates/issues/src/view.rs`, `crates/issues/src/view/page.rs`, `crates/issues/src/view/store.rs`
  (doc only), every mode's `handle` (`crates/claude-plugins`, `files`, `markdown`, `neovim`,
  `editor`), `crates/app/src/workbench/panel.rs` and its tests, the issues crate's tests

**Interfaces:**
- Produces:
  - `Request::IssueFiles(&'a HashMap<PathBuf, PathBuf>)` in place of `Request::SetStorage`: each
    project folder of the window on this machine, with the issues file its project keeps.
    Broadcast whole whenever it changes; a folder left out keeps no issues here.
  - `IssuesView`: `storage: Option<PathBuf>` becomes `files: HashMap<PathBuf, PathBuf>`;
    `set_storage` becomes `pub(crate) fn set_files(&mut self, files: HashMap<PathBuf, PathBuf>, cx)`,
    which drops the state of every folder whose file changed or left and keeps the rest.
    `state_for`, `file()`, the page's `key()`, `selected_key`, `page_list` read `files`.
  - The hint drawn for a folder with no file: *Issues are kept for the projects on this machine*
    (in place of the storage-folder hint, in both places it is drawn).
  - `testing::OpenView::new(root: &Path, file: &Path, cx)`.

- [ ] **Step 1: Change the tests first**: every issues-crate test that built a view with a
  storage folder builds it with a `files` map of one folder; add

```rust
#[gpui::test]
fn two_folders_of_one_file_show_one_list(cx: &mut TestAppContext) {
    // A view told {/a -> f, /a-x -> f}: an issue written through /a shows when /a-x is the
    // folder on screen, read from the one IssuesFile entity.
}

#[gpui::test]
fn a_folder_told_no_file_keeps_nothing(cx: &mut TestAppContext) {
    // Told {}: the folder on screen draws the hint and no list.
}
```

- [ ] **Step 2: Run, expect compile failure.** `cargo test -p onehand-issues --locked` → FAIL.
- [ ] **Step 3: Implement**, the new variant named in every mode's `handle` (the modes that do not
  keep issues answer it `false`, as they did `SetStorage`).
- [ ] **Step 4: Run.** `cargo test -p onehand-issues --locked` → PASS; `cargo check --workspace`
  fails only in the app, which is Task 4.
- [ ] **Step 5: Commit** `refactor(issues): the views are told each folder's issues file`.

### Task 4: The app tells each folder's file, and reads issues by project

**Files:**
- Modify: `crates/app/src/shell/carry.rs`, `crates/app/src/shell/boot.rs`,
  `crates/app/src/shell/storage.rs`, `crates/app/src/shell/issues_page.rs`,
  `crates/app/src/shell/off_screen.rs`, `crates/app/src/shell/pages.rs`,
  `crates/app/src/workspace_page.rs`, `crates/app/src/control.rs` and their tests,
  `docs/drives.md` (the issues file row), `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1's `issues_file`, Task 3's `Request::IssueFiles`.
- Produces:
  - `Shell::issue_files(&self) -> HashMap<PathBuf, PathBuf>`: every row with a project and a local
    path, its path to `issues_file(&project)`.
  - `Shell::tell_issue_files(&mut self, cx)`: broadcast to the Workbench and the Issues page when
    the map differs from the last told (kept in a new field `told_issue_files`). Called where
    `SetStorage` was sent (panel built, page built), on every `Effect::Changed`, and with an empty
    map where `SetStorage(None)` was (a workspace deleted).
  - `Shell::issues_file(root)` (off_screen.rs) answers `issue_files().get(root)`; `PageProject.issues`
    the same.
  - The workspace page reads each file once: `reload` keys its reads by file, a project's issues
    listed under the first of its rows.
- CHANGELOG, under *Breaking*: a project keeps one issues file, in `projects/<id>/` in the config
  directory; at the first start of this build the issues each workspace kept for it and for its
  worktrees are brought in, a forge's issue once, an issue of onehand's own renumbered when its
  number was taken; each file taken in is left beside where it was, renamed `.json.merged`.

- [ ] **Step 1: Write the failing app tests:**

```rust
#[gpui::test]
fn a_projects_folders_share_one_issues_file(cx: &mut TestAppContext) {
    // Shared.projects holds P with worktree W recorded; the window opens P's folder and W's.
    // create_issue through the control tools on W's folder: the issue is in
    // projects/<id>/issues.json, both folders' Issues views list it, and the workspace page
    // lists it once.
}

#[gpui::test]
fn a_row_of_no_project_keeps_no_issues(cx: &mut TestAppContext) {
    // The rig's /tmp row (no project): issue_files() has no entry for it, and create_issue on
    // it is refused in the words a project with no issues file gets today.
}
```

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand --locked share_one_issues` → FAIL.
- [ ] **Step 3: Implement.** Existing tests that relied on `file_for(storage, root)` (control,
  workspace page) seed a project for their folder and read its file.
- [ ] **Step 4: Run everything** (Global Constraints' checks) → PASS.
- [ ] **Step 5: Commit** `feat(issues): one issues file per project`; review (opus,
  `origin/main..HEAD`), fix what it confirms, open PR A, CI, ask the owner.

---

## PR B: conversations

### Task 5: A conversation records its project, and a scope says which a row lists

**Files:**
- Modify: `crates/core/src/chat/store.rs`, `crates/core/src/chat/store/write.rs`,
  `crates/core/src/chat/model.rs`, `crates/core/src/chat/model/persist.rs`,
  `crates/core/src/remote/chats.rs`, their tests

**Interfaces:**
- Produces:
  - `Meta` gains `#[serde(default, skip_serializing_if = "Option::is_none")] project: Option<ProjectId>`
    (`FORMAT` stays 2: the field is optional both ways). `MetaWrite` gains `project`; a write keeps
    the `project` already on disk when it carries none.
  - `Chat::new(uid, root, agent, store, project: Option<ProjectId>)`; `Chat.project` feeds
    `meta_write`.
  - `ConvMeta` gains `pub root: PathBuf` and `pub project: Option<ProjectId>`.
  - `pub struct Scope { pub folder: PathBuf, pub project: Option<ProjectId>, pub home: bool, pub recorded: Vec<PathBuf> }`
    with `pub fn holds(&self, root: &Path, project: Option<&ProjectId>) -> bool`: `root == folder`;
    or, when `home`, `project == self.project` (both `Some`) and `root` is neither `folder` nor in
    `recorded` (the project's other folders, which list their own).
  - `list_conversations(store, scope: &Scope, agent: Option<&str>) -> Vec<ConvMeta>` and
    `list_across(store, scopes: &[Scope]) -> Vec<(PathBuf, ConvMeta)>` (the first scope holding a
    conversation takes it, keyed by `scope.folder`) in place of the path-taking ones.
  - `remote::chats::scan_archives(scopes: Vec<(Scope, String)>)`.

- [ ] **Step 1: Write the failing tests** (`chat/store/tests.rs`):

```rust
#[test]
fn a_scope_takes_its_folder_and_its_projects_strays() {
    let p = ProjectId::mint(1);
    let home = Scope { folder: "/a".into(), project: Some(p.clone()), home: true, recorded: vec!["/a-x".into()] };
    assert!(home.holds(Path::new("/a"), None), "its own folder, project or not");
    assert!(home.holds(Path::new("/a-gone"), Some(&p)), "a folder its project no longer records");
    assert!(!home.holds(Path::new("/a-x"), Some(&p)), "a recorded worktree lists its own");
    assert!(!home.holds(Path::new("/b"), Some(&ProjectId::mint(2))));
    let worktree = Scope { folder: "/a-x".into(), project: Some(p.clone()), home: false, recorded: vec![] };
    assert!(!worktree.holds(Path::new("/a-gone"), Some(&p)), "only the project's own row takes strays");
}

#[test]
fn a_meta_without_a_project_reads_and_one_with_it_reads_in_an_older_shape() {
    // A meta.json written without `project` lists through a scope of its folder; one written
    // with it parses as a serde_json::Value holding every field an older Meta had.
}

#[test]
fn a_write_keeps_the_project_on_disk_when_it_carries_none() { /* save with Some(p), then a chat with None: still p */ }

#[test]
fn listing_across_lists_each_conversation_once() {
    // Two scopes, /a (home of p, recorded [/a-x]) and /a-x: a conversation of /a-x is keyed
    // /a-x only; one of /a-gone stamped p is keyed /a.
}
```

- [ ] **Step 2: Run, expect failure.** `cargo test -p onehand-core --lib chat::store` → FAIL.
- [ ] **Step 3: Implement.** Existing callers of `Chat::new` in core tests pass `None`.
- [ ] **Step 4: Run.** `cargo test -p onehand-core --lib` → PASS.
- [ ] **Step 5: Commit** `feat(chat): a conversation records its project`.

### Task 6: The migration stamps each conversation with its project

**Files:**
- Modify: `crates/core/src/project/migrate.rs`, `crates/core/src/project/migrate/tests.rs`

**Interfaces:**
- Consumes: `chat::conversations_dir` (as `<config>/conversations`), Task 2's `Projects`,
  `place::repo_key_blocking`.
- Produces: `fn stamp_conversations(config: &Path, projects: &Projects, problems: &mut Vec<String>)`,
  called after `merge_issues`. For each `conversations/<id>/meta.json`, read as a
  `serde_json::Value` (every field kept as it was): one with `project` is passed over; else its
  `root` is resolved, in order, through `projects.find(None, root)`; through
  `repo_key_blocking(root)` matched against each local project's key, when the folder is on disk;
  through the tasks whose `setup.dir` is `root` (`<config>/tasks/*.json`, read as values), to the
  project holding their `setup.repo`. Resolved: `project` set and the file rewritten with
  `write_atomic`. Not resolved: left as it is. Unreadable: said, left as it is.
  `// ponytail:` note on the per-launch cost (every meta read once per launch, git asked only for
  the unstamped), and the upgrade (a marker of the last run) if it ever shows at start.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn a_conversation_of_a_projects_folder_is_stamped() { /* meta root = project folder: project set, other fields byte-for-byte as before */ }

#[test]
fn a_conversation_of_an_unrecorded_worktree_is_stamped_through_git() { /* repo_with_worktree, project on main, meta root = the linked one */ }

#[test]
fn a_conversation_of_a_removed_run_worktree_is_stamped_through_its_task() { /* root gone from disk; a task with setup.dir = root and setup.repo = the project folder */ }

#[test]
fn a_conversation_nothing_resolves_is_left_as_it_is() { /* bytes unchanged */ }
```

- [ ] **Step 2: Run, expect failure.** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** `cargo test -p onehand-core --lib project::migrate` → PASS.
- [ ] **Step 5: Commit** `feat(projects): stamp each conversation with its project`.

### Task 7: The app lists conversations by scope

**Files:**
- Modify: `crates/app/src/projects.rs` (`scope_of`), `crates/app/src/chat/session.rs` and
  `chat/session/host.rs` (`new_chat`), `crates/app/src/chat/pane/project_page.rs`,
  `crates/app/src/chat/pane/history.rs`, `crates/app/src/workspace_page.rs`,
  `crates/app/src/remote.rs`, their tests

**Interfaces:**
- Consumes: Task 5's `Scope`, `list_conversations`, `list_across`, `scan_archives`.
- Produces:
  - `crate::projects::scope_of(folder: &Path, cx: &App) -> Scope`: from `projects.find(None, folder)`;
    `home` when it is the project's own folder, `recorded` the project's other folders (its own
    folder and its worktrees, `folder` left out); a folder of no project gets a scope of itself
    alone.
  - `new_chat(uid, root, spec, on_host, project: Option<ProjectId>)`; `ChatSession::spawn` passes
    `projects::of(&root, cx).map(|p| p.id)`.
  - The project page's history and the header's archives (`scan_archives`,
    `scan_project_history`), the resume picker (`history.rs`, its agent kept), the workspace page
    and the remote bridge's scan build their scopes on the UI thread and list off it.

- [ ] **Step 1: Write the failing app tests:**

```rust
#[gpui::test]
fn a_conversation_of_a_removed_worktree_shows_on_its_projects_page(cx: &mut TestAppContext) {
    // A conversation store holding one meta stamped with P whose root is a folder P no longer
    // records: the project page of P's own row lists it; a worktree row's page does not.
}

#[gpui::test]
fn a_conversation_is_listed_once_across_a_projects_rows(cx: &mut TestAppContext) {
    // P's own row and its worktree row in one window; a conversation of the worktree: the
    // workspace page lists it once, under the worktree's row.
}

#[gpui::test]
fn a_new_conversation_records_its_project(cx: &mut TestAppContext) {
    // A session started on P's folder writes a meta.json whose project is P's id.
}
```

- [ ] **Step 2: Run, expect failure.** → FAIL.
- [ ] **Step 3: Implement.** Point the conversation store at the test's own folder the way the
  existing pane tests do.
- [ ] **Step 4: Run everything** → PASS.
- [ ] **Step 5: Commit** `feat(chat): conversations listed by their project`.

### Task 8: Documents

**Files:**
- Modify: `docs/screens.md` (the project page's past conversations: its folder's, and, on the
  project's own row, those of a folder the project no longer holds), `docs/known-gaps.md` (an
  issue renumbered by the merge keeps whatever named it by its old number), `CHANGELOG.md`
  (*Breaking*: conversations record their project; at the first start of this build each is
  given its project where one is found; an older build still reads them), the spec's status line
  (step 2 built).

- [ ] **Step 1: Write them**, then `git diff --check`.
- [ ] **Step 2: Commit** `docs(projects): issues and conversations by project`; review (opus,
  PR B's range), fix what it confirms, open PR B, CI, ask the owner.
