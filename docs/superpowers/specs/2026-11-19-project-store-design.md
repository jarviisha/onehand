# Projects kept by onehand, owning their worktrees

Status: approved by the owner on 2026-11-19; steps 1 and 2 built. Decisions marked *owner* were made by the owner in
conversation; those marked *builder's call* are the designer's and may be overturned by the plan.

## Intent

Today a project *is* its folder. Its identity is the pair (host, path), its name is the folder's
name (`label_for`), and every fact about it is kept in a per-workspace record or a table keyed by
that path. Nothing links a worktree back to the project it was split from: the split dialog adds
the new checkout as an independent project, a lead's worker on a worktree gets a `transient`
project that is never written down, and a workflow run's worktree is linked back only through its
task's `Setup` record.

The owner wants a project to be something onehand keeps: it has an id and a name of its own,
lives in onehand's data root, has one folder the project was added from, and owns every worktree
cut from that folder's repository. The link between a worktree and its project is kept, not
re-derived each time; renaming a project never touches a folder.

## Goals

- A project has an id and a name, kept by onehand in `<data root>/projects/<id>/`, independent of
  any workspace. A project in two workspaces is one project: one name, one list of worktrees, one
  set of issues. *(owner)*
- A project is a repository, or a folder inside one, or a folder outside git *(owner: "A")*:
  - its **project folder**: the folder it was added from (unchanged rule: may be a folder inside a
    repository, as today);
  - its **worktrees**: further checkouts of that folder's repository, each recorded in the
    project with its branch and where it came from. A project whose folder is outside git has no
    worktrees, and a split is refused as today.
  - No other kind of sub-folder. A monorepo that wants several projects is several projects on one
    repository, each worktree belonging to the project that made it. *(owner: "(ii)")*
- The project can be renamed; the name is the project's own, defaulting to the folder's name.
- Issues belong to the project, not to the workspace. *(owner)*
- Worktrees are created beside the repository by default, as today; each project remembers the
  folder its worktrees go in, set on the project's page. *(owner: "A")*
- In the rail a worktree is a mark on a session, not a level: a project's row lists all its
  sessions, and a session running in a worktree carries the worktree's branch on its line.
  Managing worktrees (list, branch, new, remove) is on the project's page. *(owner: "1")*
- A conversation records both the project and the folder it ran in, so listing groups by project
  and `session/load` is still sent the right `cwd`. A conversation whose worktree has been removed
  stays listed under its project and can be read, not resumed.

## Non-goals

- Sessions started outside onehand (`claude` or `codex` in a terminal) are not listed or adopted.
  *(owner)*
- Following a project folder that was moved or renamed on disk. The id makes it possible later;
  nothing here re-points a project folder.
- Deleting a project's kept data. Removing a project from a workspace never deletes
  `projects/<id>/`; adding its folder again finds the same project, name and issues back. An
  explicit *Delete project* is recorded in `docs/known-gaps.md`. *(builder's call)*
- Any change to the host service, its data, mirror names or build folders on a host: those are
  named after folders, and stay so.
- Workspace ids: a workspace made from a folder keeps the folder-derived id it has today.

## Anti-goals

- Starting a session gets slower. *New session* still starts at once in the project folder; a
  worktree is chosen only when the person asks for one.
- Anything already kept is lost or silently changed by the move: transcripts, issues, offload data,
  tasks, schedules. The migration copies or renames, never deletes, and a file it cannot read is
  never written over (the rules `workspace/migrate.rs` already follows).
- A second source of truth for what is on disk. Which worktrees exist is git's; the project records
  what onehand knows about them.

## Constraints

- Core stays GUI-free; rules and what happens next live in core drives, carried out by the app
  (`docs/drives.md`). The project store is a drive's model, not front-end state.
- Every window runs in one process (the instance lock refuses a second), so project files are
  written by one process: through an `OrderedWriter`, whole, with `write_atomic`.
- The folder-keyed maps in the app (editor, terminal, file tree, git status, task queue places,
  worker reservations) stay keyed by folder: they are about folders, not projects.
- The control socket's `project` argument keeps accepting a name or a path
  (`control::pick_project`); the name it matches is now the project's own name.
- `docs/screens.md`, `CONTEXT.md` and the feature documents are contracts; each step updates the
  ones it changes (see *Documents*).

## Approaches considered

**Where a project's state lives.**
1. *A project store independent of workspaces*: `projects/<id>/project.toml`, the workspace file
   holding only references. **Picked** *(owner)*: a project in two workspaces is one project, and
   `projects/` already holds what belongs to a project and to no workspace (offload data).
2. *Inside the workspace file*: an `id` and `name` added to `[[projects]]`. Smaller, but one repo in
   two workspaces becomes two projects with two names and two worktree lists, and
   `projects/<stem>` stays keyed by path.

**What a project groups.**
- *A: a repository and its worktrees* **picked** *(owner)*: git confirms the link.
- *B: any folders the person gathers*: flexible, but the link is only what was declared.

**Where worktrees go.**
- *A: beside the repository by default, the parent folder remembered per project* **picked**
  *(owner)*.
- *B: inside onehand's store*: keeps code folders tidy, but puts multi-gigabyte checkouts
  (`target/`, `node_modules/`) in the config directory, which is backed up and synced as dotfiles,
  and hides them from tools outside onehand.

**How the rail shows worktrees.**
- *1: a branch mark on the session's line* **picked** *(owner)*: the rail stays two levels.
- *2: a worktree level between project and session*: three levels, four with workers, against the
  one level of nesting `docs/screens.md` allows.

**Sub-folders.**
- *(i) recorded sub-folders a session can start in*: rejected.
- *(ii) nothing new; a project folder may be inside a repository, as today* **picked** *(owner)*.

## Design

### On disk

```text
<data root>/
  projects/<id>/
    project.toml      the project
    issues.json       its issues (moved from workspaces/<ws>/issues/<stem>.json)
  offload/<folder stem>/   one per offloaded folder (project folder or worktree): offload.json,
                           coming-back/, backups/, moved from projects/<stem>/
  workspaces/<ws>/onehand-workspace.toml
    [[projects]] id = "…", pinned = true    references, in rail order
```

- `<id>`: random, 8 lowercase hex characters, minted once, never derived from a path or name, and
  the folder is named by the id alone (a name in it would go stale on the first rename).
  *(builder's call)*
- `project.toml` holds: `id`, `name`, the project folder (`host`, `path`, and its `offload`),
  `check`, `builds`, `worktree_parent` (absent: beside the repository), and `[[worktrees]]` each
  with `path`, `branch`, `from` (the branch or commit it was cut from, when known), `made_by`
  (`split`, `run`, `worker`, or `added` for one git listed that the person
  adopted) and its own `offload`. Defaults are left out, as the workspace file does today.
- **Offload is per folder, not per project** *(owner)*: the project folder and each worktree is
  offloaded on its own, as each worktree project can be today. Offloading a folder moves the
  sessions in that folder, in every window holding the project; sessions in the project's other
  folders are untouched. Its data stays keyed by folder, as offload is: `<folder stem>` is
  `stem_for(folder)`, the name the host's mirror is already given, so nothing on a host changes,
  and the offload code keeps finding its data from the folder alone. *(builder's call, revised
  while planning step 1: under `projects/<id>/` every offload errand would have to be handed the
  project's id)*
- The workspace file keeps only what is the workspace's: its name, its layout, the active project
  (by id) and, per project, the reference and `pinned`. Order, pin and active stay per workspace.
- At most one project per project folder (host, path), and a worktree folder belongs to at most
  one project. The rule is about the folder sessions run in, not the checkout: two projects on one
  repository (a monorepo split in two) each record their own sub-folder of a shared checkout.
  *(builder's call)*
- Adding a folder (*Add project…*, `onehand <folder>`, a resume, a control call) goes through the
  folder index first: a known project's folder or recorded worktree adds that project. Failing
  that, git is asked: a linked checkout of a known project's repository, at that project's
  sub-path, is recorded as that project's worktree (`made_by = added`) and adds the project.
  Anything else is a new project. *(builder's call)*
- **Compatibility with an older build.** Each reference in the workspace file also keeps the
  project folder's `host` and `path`, written for an older build only: it then still opens the
  workspace, showing each project folder as it used to. The new build reads the project from
  `project.toml` and treats an entry without `id` (an older build wrote the file back) as a folder
  to add, through the rule above. Issues and offload data moved by the migration are not seen by an
  older build. *(builder's call)*

### In core

- A `ProjectId` type and a `Project` model (what `project.toml` holds), with a `ProjectsDrive`
  owning every project this process has loaded and answering each change with effects (write the
  project file, signal a failed save and put the value back, as `Saves` does today). It is shared
  by every window (held in `Shared`), so a rename or a new worktree in one window shows in every
  window holding the project. *(builder's call: the plan settles the exact seam with
  `WorkspaceDrive`, keeping "no drive changes another drive's model")*
- `ProjectRoot` in the workspace tree refers to its project by id; its facts that move to the
  project (`label`→`name`, `check`, `builds`, `host`, `path`, and each folder's `offload`) are
  read from the project. `runs_here` becomes a question about a folder.
  `transient` goes: a worker's folder is a worktree of its project.
- `Session` gains the folder it runs in (the project folder or one of its worktrees). Sessions
  still are not written into any file; the folder comes back through the conversation.
- A folder index answers *which project, and which of its folders, is this path* for every caller
  that has a path today: resume, `open_worker`, `start_workflow`, a schedule's start, the remote
  bridge's `/open`, `onehand <folder>` on the command line, `onehand --build`.

### Worktrees

- **Membership is recorded, and git is asked what exists.** A worktree is the project's when the
  project records it. The project page also reads `git worktree list` of the project folder's
  repository and offers each listed checkout no project records as *Add to this project*; a
  recorded worktree git no longer lists is shown missing, with *Forget*. *(builder's call)*
- Every way onehand cuts a worktree records it in the project in the same change: the split dialog,
  a workflow run (`cut_blocking` callers), and a lead's worker opened on a worktree (the folder is
  recorded with `made_by = worker` when the project does not hold it yet). The worktree's folder
  is placed by `worktree_dir_in(project.worktree_parent)` or `worktree_dir` when none is set.
- Removing a worktree through onehand removes the record with it. A task's worktree keeps today's
  flow (`worktree/removal.rs`: forge, merged pull request). Any other worktree (split, worker,
  added) gets a judgement of its own in core: removable when git reports no uncommitted changes in
  the checkout and the occupants check finds nothing using it, and refused while another project
  records a folder inside the same checkout. Its branch is deleted only when git's safe delete
  (`git branch -d`) takes it, i.e. it is merged; otherwise the branch is kept and the confirmation
  says so. Nothing a person committed is lost. A task's `Setup` keeps its own record as today.
  *(builder's call)*
- On a repository two projects share, git's list offers a checkout to a project only when the
  checkout holds that project's sub-path and no project records that folder.
- A new worktree is cut only while the project folder runs here (today's `runs_here` rule, asked
  of the project folder). A worktree already recorded runs here or is offloaded on its own,
  whatever the project folder does. A host project has no worktrees, as today.

### Sessions, the rail and the Workbench

- A project's row lists all its sessions, in its own order; a session whose folder is a worktree
  shows a branch mark and the branch on its line. The worker nesting rules are unchanged; the rule
  that hides a worktree project holding only workers goes, since there is no such project.
- *New session* starts in the project folder, at once. Beside a session on screen it starts in that
  session's folder (as it already does beside a worker). Starting in another worktree, or in a new
  one, is offered from the project page's worktree rows and from the *New session* menu.
- The Workbench (file tree, editor), the terminal dock and git badges follow the folder of the
  session on screen, so the person sees the checkout the agent works in. The folder-keyed state
  each of them keeps is unchanged. *(owner)*
- *Rename project…* on the project's row menu and on its page; the name is checked for being empty
  only. Two projects may share a name; where a name must pick one project (the control socket) the
  existing refusal naming both applies.
- The project page gains a *Worktrees* section: each recorded worktree (branch, folder, its
  sessions, *New session*, *Remove worktree…*), git's unrecorded ones, missing ones, *New
  worktree…*, and the worktree folder setting. Its exact layout follows the `ui` skill and is
  written into `docs/screens.md` in the step that builds it.

### Conversations, tasks and schedules

- `meta.json` gains `project` (the id); `root` keeps meaning the folder the session ran in and is
  what `session/load` is given as `cwd`. Listing a project's conversations reads `project`; a
  conversation whose folder is a removed worktree is listed, read-only, with resume refused in the
  words the restore failure already uses. *(the read-only/refused wording is the builder's call)*
  Until step 3 a worktree is still a row of its own, so a row lists what ran in its folder, and
  the project's own row also what ran in a folder the project no longer records; nothing is
  listed on two rows. Resuming one whose folder is gone is refused as it is today (*The folder
  that conversation ran in is gone*). *(revised while planning step 2)*
- Tasks and schedules keep the folders they hold (`Setup.repo/dir`, `Schedule.project`): those say
  where work runs. Grouping them by project goes through the folder index. *(builder's call)*
- Issues: one file per project, `projects/<id>/issues.json`. `IssueKey.file` points at it; the
  control tools' `project` argument resolves to that file.

### Migration (once, at launch, safe to run again)

Following `workspace/migrate.rs`'s rules: staged, idempotent, nothing deleted, an unreadable file
never written over, bulky data renamed rather than copied, the old workspace file kept beside the
new one as `onehand-workspace.toml.before-projects`.

1. **Projects.** Every `[[projects]]` entry of every workspace file is grouped by its project:
   - a local folder inside git is keyed by its repository's common directory and its sub-path
     within the checkout; entries with the same key are one project. The project folder is the
     entry that is the main checkout's (or the main checkout's same sub-path, when that exists on
     disk and none of the entries is it); the rest become its worktrees with `made_by = added`.
   - a folder outside git, a folder that does not resolve, and a host folder are each a project of
     their own, keyed by (host, path).
   - an offloaded folder merges like any other, keeping its offload as its folder's own.
   - `check` and `builds` come from the project folder's entry, else the first entry having them.
     *(builder's call)*
2. **Workspaces.** Each entry is given its project's id. Until step 3 a worktree is still its own
   row, so each entry stays a reference to one folder and `active` stays a folder; in step 3 the
   entries of one project in one workspace become one reference, pinned if any was, and `active`
   becomes the id.
3. **Offload data.** `projects/<stem>/` is renamed to `offload/<stem>/`, leaving `projects/`
   holding projects only.
4. **Issues.** Every `workspaces/<ws>/issues/<stem>.json` of a project's folders is merged into its
   `issues.json`: an issue imported from a forge appears once (matched by its forge link, the newer
   `updated` kept); local issues keep their numbers where free and are renumbered after the
   highest otherwise. No task names an issues file any more (issue runs were removed before this
   work), so none is repointed. Each legacy file taken in is renamed `.json.merged`, and the
   project's file records it, so a run cut short never takes it twice. *(revised while planning
   step 2)*
   The owner's data has both cases: worktree issue files synced with the same GitHub repository as
   their project's, and numbers starting at 1 in each.
5. **Conversations.** Each `meta.json` whose `root` resolves to a project gets its `project` id,
   rewritten whole through `write_atomic`. A `root` is resolved, in order: through the folder index;
   through git's common directory and sub-path when the folder is still on disk (a lead's worker
   worktree that was never written down); through the tasks whose `Setup.dir` it is, to their
   `Setup.repo` (a run's worktree since removed). Only the id is stamped: no worktree record is
   made for history, so the project's worktree list holds checkouts, not old folders. One that
   resolves to nothing is left as it is, listed nowhere, as such a conversation is today.
   *(builder's call)*

### Control socket and remote bridge

- `list_projects` prints the project's name and its folder as today, then its worktrees' branches.
- A `project` argument matches the project's name, its folder, or one of its worktrees' folders;
  a worktree's folder also names the folder the session runs in (`open_worker` and `open_session`
  start there). `open_worker`'s answer names the project and the folder.
- The remote bridge's `/open` resolves a conversation's folder through the index.

## Steps

Each step is one PR and leaves the app whole. The plan for each is written when the step before it
has landed.

1. **The project store.** `ProjectId`, `project.toml`, `ProjectsDrive`, workspace files holding
   references, the migration's parts 1–3, renaming a project, and the add rule above. From this
   step every split and run cut records its worktree in its project, so nothing made before step 3
   is left a separate project. What a worktree looks like on screen does not change yet: it is still
   shown and opened as it is today until step 3. *(the plan may show a worktree as its own row in
   this step if that keeps the step whole)*
2. **Issues and conversations follow the project.** Migration parts 4–5, `meta.json`'s `project`,
   listing by project, issues per project.
3. **Worktrees owned by the project.** `Session` carries its folder, the rail's branch mark, the
   Workbench and terminal following the session's folder, every cut recording its worktree, workers
   without transient projects, the control socket's folder resolution.
4. **The project page's Worktrees section.** Listing, adopting what git lists, missing worktrees,
   new and remove, the worktree folder setting.

## Documents

- `CONTEXT.md`: *Project* redefined (kept by onehand; a folder and its worktrees), *Project folder*
  and *Worktree* added, *Worker*'s note on worktree projects updated.
- `docs/screens.md`: the rail's branch mark and the hidden-worktree-project rule; the project page's
  Worktrees section and rename.
- `docs/control.md`, `docs/workflows.md`, `docs/tasks.md`, `docs/schedules.md`: what a `project`
  argument, a run's worktree and a schedule's project now mean.
- `docs/known-gaps.md`: deleting a project's kept data; following a moved folder.

## Left to the builder

- The exact split of responsibility between `ProjectsDrive` and `WorkspaceDrive`, and how a window
  hears that a project changed.
- The id's minting and collision check.
- The wording of every new on-screen string, within `docs/ui-patterns.md`'s rules.
- Whether step 1 can show merged worktrees in their old place or must already show them as
  sessions with a mark.
