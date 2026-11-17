# App-managed workspaces

## Intent

onehand decides where a workspace is kept; the person no longer picks, binds or unbinds a
storage folder. Today a workspace lives in whatever folder it was bound to (the owner's are in
`~/.local/share/onehand/data/ws01`, `ws02`), `state.toml` remembers those folders, and
*Open workspace…* guesses whether a picked folder holds the file or is the project a workspace
was made from. The owner wants the app to manage it: one known place, no unbound workspace, no
guessing, and room to grow.

At the same time, a project's settings stop being spread over five parallel tables in the
workspace file, and a project's own data (offload) stops sharing the workspace folder's path
function.

## Goals

- Every workspace is kept under `<config_dir>/workspaces/<id>/`; there is no other place and no
  unbound workspace.
- The list of workspaces is every workspace there, most recently opened first.
- One record per project in the workspace file.
- A project's own data (not one workspace's) is kept under `<config_dir>/projects/<stem>/`.
- Existing data keeps working after the upgrade, without the person doing anything.

## Non-goals

- Keeping a workspace in a folder of the person's choosing (sync folders, another disk).
  Backing up `<config_dir>` covers it.
- An older build reading the new files. A downgrade after migrating finds no workspace and
  seeds one; the old folders are still on disk untouched.
- Sharing one project's settings between workspaces. `projects/` holds only what was already
  per project (offload); pins, checks and unattended stay per workspace.
- Renaming a workspace's folder when the workspace is renamed. The id is fixed at creation.

## Anti-goals

- Losing a workspace: migration copies, never moves or deletes, the old folders.
- Treating an unreadable file as an empty folder (`WorkspaceLoad::Unreadable` stays distinct
  from `Missing` everywhere it is today).

## Approaches

- **A. One record per project only.** Fixes the five tables; leaves bind/unbind, the guessing
  and the overloaded `storage_for`.
- **B. A, plus `projects/` for project-owned data.** Removes the false "workspace" folders that
  hold only `offload.json`; still leaves storage chosen by the person.
- **C. B, plus the app owns where workspaces live.** Removes bind/unbind, the unbound state,
  *Open workspace…*'s two-folder guess, the recents cap that forgets workspaces whose files
  remain, and the names copied into `state.toml`.

**Picked: C**, by the owner, because the app managing workspaces is simpler to use and to build
on. Cost accepted: no custom storage folder, a one-time migration, and a *Delete workspace*
action becomes necessary.

## Layout on disk

```
<config_dir>/                    # ~/.config/onehand, or $ONEHAND_CONFIG_DIR
  state.toml                     # which workspaces were opened, most recent first, by id
  workspaces/<id>/
    onehand-workspace.toml       # the file name is kept (builder's call to change; see below)
    issues/<stem>.json           # unchanged: per workspace × project
  projects/<stem>/
    offload.json
    coming-back/                 # can hold a whole checkout, .git included
    backups/<when>/              # offload::backup_dir
```

- `<id>`: for a workspace made from a folder, `workspace::stem_for(folder)` — exactly today's
  `storage_for` name, so existing `workspaces/<stem>/` folders are already in place and
  *New workspace…* on a folder that already has one still finds and opens it. For a migrated
  external folder, `stem_for(old folder path)`. Never derived from the name.
- `<stem>`: `stem_for(project path)`, unchanged, so a host's mirror (named by the same stem)
  still matches.
- `storage_for` splits into two functions with one meaning each: the workspace folder for an id,
  and the project folder for a path. Its three project callers (`offload/drive/blocking.rs`,
  `offload::backup_dir`, the offload drive) take the second. `stem_for` stays the one naming
  rule.

## The workspace file

```toml
name = "Foxaiwork"
active = "/home/me/foxaiwork/MSTT"     # by key, as pins already are; replaces active_root

[[projects]]
path = "/home/me/foxaiwork/pos-btmc"
pinned = true

[[projects]]
path = "/srv/app"
host = "build01"                         # absent for this machine

[[projects]]
path = "/home/me/foxaiwork/MSTT"
unattended = true
check = "cargo test"
offload = { host = "build01", mirror = "/srv/mirror", branch = "feat/x" }  # branch optional

[layout]
# PanelLayout, unchanged
```

- Order of `[[projects]]` is the rail's order, as `roots` is today.
- Every field but `path` has a default and is omitted when it has its default.
- `active` names a project by its key (host + path). A key matching no project reads as the
  first, as an out-of-range index does today.
- **Reading the old shape.** A file with `roots` and the five tables (`pinned`, `unattended`,
  `checks`, `offloaded`, `offloaded_branches`) and `active_root` still loads; the `<host>:<path>`
  strings in it are parsed as today. It is written in the new shape at the next save. A file
  holding both shapes reads the new one.
- Transient projects are still left out of the file.

## state.toml

- Holds the ids, most recently opened first, and nothing else that matters: `names`,
  `workspace_dir` and `recent_workspaces` are read once, for migration, and no longer written.
- No cap on what is kept. The list a menu draws keeps a named cap and says when it bites, as
  every list does.
- A workspace folder not named in `state.toml` is still a workspace: the list is every
  `workspaces/*/onehand-workspace.toml`, ordered by `state.toml` first, then the rest by name.
- An id named in `state.toml` with no folder is dropped from it.

## Listing workspaces

- Read once at boot, before the first window (boot already runs blocking migrations there),
  into the process-wide list the rail's menu reads; kept in step as workspaces are created,
  renamed, opened and deleted, through the recents drive (which becomes the workspace list's
  drive). A name shown in the menu comes from this list, not from `state.toml`.
- A file that will not parse is listed by its folder name and refuses to open with the reason,
  rather than being dropped or overwritten. `WorkspaceLoad::Unreadable` gains the reason as a
  string; it stays a separate case from `Missing`.
- Every leftover comes back: a folder hidden today (dropped from recents, past the old cap of 8)
  is listed again. That is intended; *Delete workspace…* is how one goes. (Designer's call.)

## Behaviour

- **Boot.** Open the first listed workspace that loads. If none exists, make one from the CLI
  argument (else the current directory) as *New workspace…* does, and write it before the window
  opens. A write that fails shows the window anyway, says so in a toast, and keeps retrying at
  each later save; there is no state in which the workspace is knowingly not saved.
- **New workspace…** picks a folder, as today, and opens the workspace `stem_for(folder)` if it
  exists, else makes and writes it there. Its name is the folder's label, as today. A write that
  fails still refuses to open the window, as today: unlike boot, there is a window already on
  screen and nothing is lost by not opening a second. (Designer's call.)
- **Open workspace… goes away** (rail menu and Settings). The rail menu lists every workspace.
- **Switching** from the rail menu is unchanged: a workspace already on screen is brought forward.
- **Delete workspace…** on Settings ▸ Workspace, for this window's workspace: a confirming
  dialog naming the workspace and saying its issues go with it, then the window closes and the
  folder `workspaces/<id>/` is removed, and the id leaves the list. Refused while a run or an
  unattended run is going in it. Deleting the last workspace ends with the app's last window as
  closing it does; the next launch seeds a new one.
  - *A run going in it* means a run, unattended run or unfinished task whose issue file is under
    `workspaces/<id>/`: that is exactly what the delete destroys. A run on one of its projects
    reporting into another workspace's issues does not block it. (Designer's call.)
  - **Nothing may write the folder back.** The workspace drive gets a deleted state: from the
    moment the delete is confirmed it answers no write, the close-time flush included, and the
    issues mode is told its storage is gone before the window closes. The folder is removed only
    after the window has closed and the writer's queue has drained, so no late write recreates
    `onehand-workspace.toml` and with it the workspace.
  - Finished tasks pointing into the deleted folder keep their path and read as having no issue
    file, as a task whose file went missing does today.
- **Settings ▸ Workspace** loses *Storage folder*'s *Choose folder…* and *Unbind*. The folder's
  path stays as a read-only line in mono, so it can be found for a backup.
- Windows are keyed by id where they are keyed by storage folder today; every
  `storage_dir: Option<PathBuf>` that was `None` only for an unbound workspace becomes a plain
  value (`Request::SetStorage` included).

## Migration

One blocking pass at boot, behind the instance lock, beside the existing
`migrate_old_dir_blocking` calls. Idempotent: every step skips what is already in place, so it
can run at every boot and needs no marker.

1. For each folder in the old `recent_workspaces` and `workspace_dir` (read from the raw file,
   not through today's capped `AppState::load`; the file itself never held more than 8, which is
   accepted) that is not already under `<config_dir>/workspaces/`: if it holds an
   `onehand-workspace.toml` that loads, copy it and its `issues/` into a temporary folder beside
   the target, then rename it to `workspaces/<stem_for(folder)>/`, unless that target already
   exists. A copy that fails halfway leaves only the temporary folder, which the next boot
   removes and starts again, so "target exists" always means "copied whole". The old folder is
   left as it is. Its old `names` entry becomes the copied file's `name` if the file has none.
1b. Tasks keep the absolute path of their issue file (`TrackerRef::Local { file }`,
   `Synced { file, .. }` in `unattended/tracker.rs`, inside each `<config_dir>/tasks/<id>.json`).
   For every workspace step 1 copied, rewrite in each task file a `file` under the old folder's
   `issues/` to the same name under the new one, so open tasks and unattended runs report into
   the copy the app shows. Written atomically, one task file at a time, before step 3.
2. For each `workspaces/*/` holding `offload.json`, `coming-back/` or `backups/`: rename each into
   `projects/<same stem>/` unless the target exists. Always a rename, never a copy: `coming-back/`
   can hold a whole checkout. Both sit under `<config_dir>`, so the rename does not cross a
   filesystem. A folder left with nothing in it is removed.
3. Write `state.toml` in the new shape, the ids in the old recents' order.
4. Each problem is logged (`eprintln!`, as the other migrations do) and the pass goes on; a
   failed copy leaves that workspace un-migrated and the old `state.toml` fields are kept until
   every step it needed has landed.

The file-format change (one record per project) is not part of this pass: it happens at load
and save, so a file copied in later by hand also reads.

## Code it touches

- Core: `config.rs` (`WorkspaceConfig` and its old-shape read; `config.rs` is at 799 lines, so
  `WorkspaceConfig`, `PanelLayout` and `WorkspaceLoad` move to a sibling `config/workspace.rs`
  first), `config/state.rs`, `config/recents.rs` (`Forgot::Unbound` and `Signal::ForgetFailed`
  go; delete comes in), `workspace.rs` (`from_config`/`to_config`, `storage_for` split),
  `workspace/drive.rs` (`bind`/`unbind` go; the snapshot always has a folder),
  `host/offload/**` (the project folder), `issues.rs` (non-optional storage; paths change only
  for migrated external workspaces, handled by step 1b), `unattended/tracker.rs` (step 1b),
  `workbench.rs` (`SetStorage`), plus the migration.
- App: `shell/storage.rs`, `shell/boot.rs`, `windows.rs`, `settings/pages.rs`,
  `rail/workspace.rs`, the Workbench modes matching `SetStorage`.
- Docs: `docs/screens.md` (the workspace menu's entries, Settings ▸ Workspace),
  `docs/hosts.md` (how a host project is kept in the workspace file; where `offload.json` lives),
  `docs/known-gaps.md` (the unbind entry goes), `CHANGELOG.md`.
- Rules that hold: what happens next lives in drives (the workspace list's drive owns create,
  delete and ordering; the app carries the file work); no hard-coded paths outside `config_dir()`;
  the confirm dialog follows the UI patterns for removals.

## Left to the builder

- Whether to rename `onehand-workspace.toml` to `workspace.toml` while every file is being
  rewritten anyway. Keeping it is the smaller diff and was this design's default.
- The exact name and shape of the workspace list's drive, and whether it absorbs
  `RecentsDrive` or sits beside it.
- The menu's cap value.
- How the PR is cut (one PR, per the owner's habit of one PR per feature with its docs).
