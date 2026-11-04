# Remote hosts step 5: offloaded projects in mirror mode, with the key, plus Send to host

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking. Every task that changes what is drawn follows the `ui` skill and answers
> its checklist.

**Goal:** A local project can be offloaded to a host: its new sessions run on the host against a
mirror of its folder, while its tree, editor, terminal and git status stay the laptop's. The key
says which side may change the code; the code goes up before a prompt and comes back when the
host's sessions on it are done, and never overwrites a change made outside onehand without asking.
Files the copy leaves out reach the host only through *Send to <host>*.

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md`, *Offloaded projects*, *Mirror
and the key*, *One laptop at a time*, *Permission bypass*.

## Decided with the owner on 2026-11-04

1. **Offloading converts a local project in place.** *Offload to a host…* in the rail's project
   menu and the project page's menu opens a small form with the host field and its picker (reused
   from *Add a project on a host*). The root stays the same root; its new sessions open on the
   host. *Run here again* puts it back, offered only while the laptop holds the key; the mirror
   stays on the host so a later offload sends only what changed.
2. **The key on screen:** the rail's project row carries a mark only while the host holds the key
   (tooltip *<host> has the code until its sessions finish*); the project page has a box *On
   <host>* saying who holds the key and when the code last moved; the project's editor and
   terminals show a thin line saying why they are locked. The laptop holding the key shows no mark.
3. **Sync back stopped by outside changes:** the page shows a box *Bringing the code back
   stopped*, listing the changed files and refs (capped), with *Take the host's code* (asks first;
   the laptop's versions of those files are copied to a backup folder first, whose path it says)
   and *Check again*. The rail marks the project as needing attention. The key stays with the host
   meanwhile.
4. **Send to <host>** sits in a right-click menu on the Files tree of an offloaded project, works
   whoever holds the key, says its result in a toast, and remembers nothing.
5. **The bypass switch** applies to offloaded projects exactly as to host projects: the same box,
   words and default, kept by the host per mirror folder. No new host code for it.

## Builder's calls (said here so review can disagree)

- **The host is the key's authority.** The host keeps, per mirror, its owner and who holds the key
  (`mirrors.json` in its data directory, the `bypass.json` pattern). The laptop caches it. This
  closes the one race the laptop cannot: a queued prompt flushed at a turn end landing on the host
  while the laptop is bringing the code back. Each mirror carries a generation the host bumps on
  every prompt to a session on it; the key returns to the laptop only if nothing on the mirror is
  working and the generation is the one the laptop saw before it copied the code down. A prompt
  to a session on a mirror whose key the laptop holds is refused by the host, and the transcript
  says *The laptop holds this project's code; send the prompt again*.
- **"Working" on the host** is busy, or any background job or subagent still running. The hub
  folds `Background`/`SubagentSpawned`/`SubagentEnded` beside `busy_after` and reports it per
  session (`SessionInfo.working`).
- **This laptop's id is persisted:** `laptop()` (`crates/core/src/host/client/attach.rs`) reads
  `<config_dir>/laptop-id`, written once, instead of a per-run id. The same id names the mirror's
  owner; the host's `held()` keeps working (an earlier run of this laptop counts as its own, which
  is what that rule wants). Another laptop's `Open`/`Watch` on a mirror it does not own is refused,
  *<path> is offloaded from another laptop*.
- **The mirror's place:** `<host data>/mirrors/<laptop id>/<stem>`, `stem` being
  `workspace::stem_for(local path)`. The host makes it and answers its absolute path.
- **Kept on the laptop:** the workspace file gets `offloaded`, keyed by the local root, valued
  `<host>:<mirror>` (`root_key` format). `<config_dir>/workspaces/<stem>/offload.json` keeps the
  manifest of what was last sent or brought back, the cached key, and a stopped sync's list.
- **The copy is `rsync`, run as a process by core's blocking functions,** with
  `-e "ssh <the attach's ssh options>"`, or a plain path for the host `local`. The list is
  `git ls-files --cached --others --exclude-standard -z` plus `.git`, fed with `--from0
  --files-from=-`, and files gone since the manifest are listed too, with
  `--delete-missing-args`. Up, `.git` goes with `--delete`; down, `.git` comes back **without**
  deleting, so a commit made outside onehand keeps its objects. `rsync` is checked on both ends
  first, and a missing one is named: *rsync is not installed on <host>* / *on this machine*.
- **The manifest** is each listed file's size and mtime (rsync keeps mtimes), plus `HEAD` (its
  symbolic name and id) and `for-each-ref refs/heads refs/tags`. `// ponytail: size+mtime; hash
  the files if touched-but-unchanged files stop syncs too often.`
- **When the code comes back:** while the host holds the key and the host is attached, the laptop
  asks the host's mirror state at each turn end of a session on it, on each attach, and every
  30 s (`OFFLOAD_POLL`), since a session no window watches sends no turn end here. Order: state
  says nothing works → compare the folder with the manifest (stop and list on a difference) →
  copy down → hand the key back with the generation seen → write the new manifest → unlock. A
  refused hand-back leaves the folder holding the newer code and the project locked; the next
  check copies again.
- **What becomes of the root's local sessions on offload:** offloading is refused while one is
  mid-turn (*Wait for <n> sessions here to finish*), and closes them; their conversations stay in
  the project's past conversations. New sessions open on the host.
- **The lock** is enforced where things write: the editor's buffers are `readonly` and `Save` is
  refused; the project's local terminal tabs drop input (a flag on the writer in
  `crates/terminal-ui`, so typing, paste and the IME are all covered); the root's own git actions
  that write are off. *Send to <host>* stays on.
- **Send to <host>** takes the row pressed: a file, or a folder with everything under it, ignored
  files included. Selecting several rows is not built (the tree has no multi-selection); a folder
  covers the common case. Listed in known-gaps.
- **What an offloaded project does not do** is what a host project does not do, for the same
  reasons (workflows, tasks, schedules, unattended runs, worktrees, the check command, onehand's
  tools, the Telegram bridge), except that the Workbench keeps every mode, being the laptop's.

## PR boundaries

Three PRs, each its own branch from `origin/main` after the previous merges, each with its docs.

### PR 5a: `feat/remote-hosts-offload` — `feat(hosts): mirrors on a host, and the code's way there and back`

No UI. Core and host only; tested against the host `local` in temp directories.

- [ ] **Task 1: persisted laptop id.** `laptop()` reads or writes `<config_dir>/laptop-id`
  (`config::write_atomic`; a write that fails falls back to the per-run id and logs). Test: two
  reads agree; a missing file is created.
- [ ] **Task 2: the host's mirrors.** `crates/host/src/hub.rs`: `mirrors.json` loaded in
  `Hub::load` (`Err` kept as with bypass: an unreadable file refuses every mirror call and gate,
  saying why in `host.log`); `Session.working` folded from background and subagent events;
  `SessionInfo.working` on the wire. New calls in `crates/core/src/host/wire.rs`, answered on the
  connection loop as `Bypass` is:
  - `Mirror { name }` → `Reply::Mirror { path, key, generation, working }`: makes the folder on
    first use, records this laptop as owner, refuses another owner's.
  - `MirrorKey { path, to, generation }` → `Done`, or refused when `to` is the laptop and the
    mirror has anything working or another generation.
  - `MirrorFiles { path }` → the mirror's list (`git ls-files … -z` plus `.git`, run off the loop
    as `files.rs` calls are).
  Gates: a `Prompt` request to a session on a laptop-held mirror is refused and journaled as a
  failure line in that session; every prompt on a mirror bumps its generation; `Open`/`Watch` on
  another laptop's mirror is refused. Every new variant goes into every match (guards rule).
  Tests in `crates/host/src/hub/tests.rs`: owner refusal; key refused while working, while the
  generation moved, granted when idle; prompt refused while the laptop holds the key; state
  survives a reload; unreadable `mirrors.json` refuses.
- [ ] **Task 3: the client side.** `HostConn::mirror`, `mirror_key`, `mirror_files`
  (`crates/core/src/host/client/files.rs`); `run` from `install.rs` made `pub(super)`-reachable
  for `command -v rsync` with the probe-marker trick.
- [ ] **Task 4: `crates/core/src/host/offload.rs`** (split into `offload/` siblings if past 800
  lines): `Manifest::read_blocking(folder)`, `Manifest::changes(&self, folder) -> Changes { files,
  refs }`, `send_up_blocking(addr, folder, mirror, &Manifest)`, `bring_back_blocking(addr, folder,
  mirror, list, &Manifest)`, `send_path_blocking(addr, folder, mirror, rel)` (*Send to host*),
  `backup_blocking(folder, &Changes, into) -> PathBuf`, `rsync_present_blocking`. Persisted state
  `OffloadState { mirror, key, manifest, stopped: Option<Changes> }` in `offload.json`. Tests with
  real `git` and `rsync` on temp folders through the host `local`: first copy sends tracked,
  untracked-not-ignored and `.git`, never an ignored file; a deletion goes up; a file changed
  outside after sending is listed and blocks; a moved HEAD or a new branch is listed, object
  churn alone is not; coming back keeps a laptop-only commit's objects; backup copies exactly the
  listed files.
- [ ] **Task 5: docs.** `docs/hosts.md` gains *Offloaded projects* (the host side and the copy, as
  built); the spec's *Order of work* says 5a is built.

### PR 5b: `feat/remote-hosts-offload-app` — `feat(hosts): offload a project and run its sessions on a host`

- [ ] **Task 1: the workspace.** `ProjectRoot.offload: Option<(HostName, PathBuf)>` (host,
  mirror); `WorkspaceConfig.offloaded`, read and written through `root_key` as `checks` is;
  `ProjectRoot::session_host() -> Option<(&HostName, &Path)>` giving a host root's own pair or an
  offloaded root's host and mirror. Core tests: round trip; a host root never carries `offload`.
- [ ] **Task 2: sessions on the mirror.** Every place that picks the host for a session reads
  `session_host()` instead of `on_host()`, and the host's cwd comes apart from the local key
  (`OnHost`/`BeginOnHost` get the cwd; `host_events`, `host_restart`, `watch_host_session`,
  `clear_active`, `host_rows`, the bypass box). The tree, editor, terminal and git sweep keep
  reading `on_host()`/`local_path()`, so they stay the laptop's. `ProjectFacts` treats an
  offloaded root as a host one for the local-only features. Test the pure picks in core.
- [ ] **Task 3: offload and back.** *Offload to a host…* (rail project menu, project page menu,
  local roots only) opens `crates/app/src/dialogs/offload.rs`: *Host* with the picker from
  `dialogs/host_project.rs`; *Offload* checks `rsync` on both ends, attaches, asks `Mirror`, sends
  the code up, writes the manifest, closes the root's local sessions (refused while one is
  mid-turn), sets `offload`. Progress says *Sending <project> to <host>…*; a failure stands inline.
  *Run here again* (same menus, offloaded roots, laptop holding the key) detaches the root's host
  sessions and clears `offload`.
- [ ] **Task 4: the key on send.** In `ChatPane::submit_from`, a session of an offloaded root whose
  laptop holds the key: the prompt waits in the composer (*Sending the code to <host>…*, send
  refused meanwhile), the code goes up, `MirrorKey` to the host, the manifest is written, the
  project locks, then the prompt is submitted. A failure says so in a toast and leaves the prompt
  in the composer and the key with the laptop. With the key on the host the prompt goes at once.
- [ ] **Task 5: the code comes back.** Per window, the offloaded roots whose host holds the key
  are checked as set out above (turn ends via a root-carrying turn-end event, attach, the 30 s
  poll while attached). A stop writes `stopped` and leaves the key with the host. Toast nothing on
  success; the lock lifts.
- [ ] **Task 6: the lock and the marks.** Editor buffers `readonly` and `Save` refused for a
  locked root's paths, with a status line *<host> has this project's code until its sessions
  finish; edits are off meanwhile*; the terminal tabs of the root drop input, with the same line
  in the dock; the rail row's mark (Lucide `lock`, picked in `icons!`) with the tooltip; a stopped
  sync is the row's needs-attention signal. The project page's *On <host>* box: who holds the key,
  *Last moved <when>*, and the bypass box as on a host project.
- [ ] **Task 7: docs.** `docs/hosts.md` *Offloaded projects* completed; `docs/screens.md` (the
  page's boxes, the rail mark, the locked lines); `CONTEXT.md` (offloaded project, mirror, the
  key); `docs/known-gaps.md`.

### PR 5c: `feat/remote-hosts-offload-conflict` — `feat(hosts): settle a stopped sync, and send files to a host`

- [ ] **Task 1: the stopped box.** On the project page, *Bringing the code back stopped*: *These
  changed on this machine while <host> held the code:* then the files and refs, at most 50
  (`STOPPED_CAP`, *<n> more not shown*). *Check again* reruns the check. *Take the host's code*
  asks *Take <host>'s code over these changes?*, naming that the laptop's versions go to
  `<backup path>` first; yes backs up, copies down, hands the key back, and toasts where the
  backup is. A refs change's backup is a `refs.txt` of the old `HEAD` and refs (the objects stay,
  since `.git` comes back without deleting).
- [ ] **Task 2: Send to <host>.** A right-click menu on Files rows (`crates/files/src/view.rs`,
  `menu_row` from `onehand_ui`), shown only on an offloaded root: *Send to <host>*. It goes up
  through a new `Request::SendToHost(&Path)` to the app, which runs `send_path_blocking` on the
  background executor and toasts *Sent <name> to <host>* or the failure.
- [ ] **Task 3: docs and the spec's status** (step 5 built); `docs/known-gaps.md` (no
  multi-selection for *Send to host*, size+mtime manifest, `.git` objects not pruned on the way
  back).

## Global constraints

- Core stays GUI-free and runtime-free (`cargo tree -p onehand-core -i gpui-pre` errors); rsync and
  git run as blocking processes, called from the app's background executor or the ACP runtime.
- Nothing new in core is `pub` unless the app or host names it. Our enums matched exhaustively.
- No comment or string names a document. Words on screen in English; no hard-coded colour or size.
- Nothing listens on the network; rsync rides the person's own ssh.
- Checks before each PR: `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D
  warnings"`, `make fmt`, then an opus review of `origin/main..HEAD`.

## Review focus

1. No path overwrites a laptop change without the person's yes: the compare runs before every copy
   down, and a refused hand-back never lets the next prompt send stale code up (5a Task 2/4 tests).
2. The prompt/hand-back race: a prompt landing between the copy down and the hand-back keeps the
   key on the host (generation test).
3. An ignored file never goes up except through *Send to host* (5a Task 4 test).
4. The lock covers every writer on the laptop side that onehand owns: editor save, terminal input
   (typing, paste, IME), git actions.
5. Another laptop cannot open sessions on a mirror it does not own.

## Verification

- Unit tests as listed; the offload tests run real `git` and `rsync` against the host `local`.
- `make showcase` by the owner, or `/drive` when asked: offload a scratch project to `local`, send
  a prompt with the Mock UI agent, see the lock and the rail mark, the code come back, then edit a
  file outside onehand mid-turn and see the stopped box.
