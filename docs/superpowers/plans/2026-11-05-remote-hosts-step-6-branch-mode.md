# Remote hosts step 6: branch mode

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native execution)
> to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every
> task that changes what is drawn follows the `ui` skill and answers its checklist.

**Goal:** An offloaded project can run in branch mode instead of mirror mode. There is no key and
no lock: the laptop's folder stays free. The host's sessions work on a branch of their own in a
mirror of its own, and the host commits what each turn leaves. The laptop fetches that branch
into a remote-tracking ref and says how many commits it has to merge. *Update host from laptop*
merges the laptop's current commit into the host's branch.

**Architecture:** branch mode reuses step 5's mirrors, offload form, Offloads global and project
page box. What it adds:

- Core gets one small file of laptop-side git (push a commit, fetch the branch, count what it is
  ahead).
- The host gets one small file of mirror-side git (init, start the branch, merge, commit at a
  turn's end).
- The wire gets three additions, each optional so that an older peer still reads.

The code moves only by git over the person's own ssh. Branch mode never runs rsync, except for
*Send to <host>*, which already uses it.

**Tech stack:** Rust, GPUI + gpui-component, git CLI, ssh.

**Spec:** `docs/superpowers/specs/2026-11-03-remote-hosts-design.md`. Read *Branch mode*,
*Offloaded projects*, *Mirror and the key* and *Permission bypass*. As built so far:
`docs/hosts.md`, *Offloaded projects*.

## Decided with the owner on 2026-11-05

1. **The mode is chosen in the offload form.** The form gets a two-way choice; mirror mode is the
   default. The mode stays fixed until *Run here again*. To change it, run the project here again,
   then offload it again with the other choice. In branch mode *Run here again* is always offered
   (there is no key), except while *Update host from laptop* runs. Each mode has a mirror of its
   own on the host, so switching never touches the other mode's code.
2. **The host's branch lands on the laptop as a remote-tracking ref,** `refs/remotes/onehand/<host>`.
   `git branch -r` shows it as `onehand/<host>`, and `git merge onehand/<host>` merges it. onehand
   never creates or moves a local branch. It adds no remote to `.git/config`, because it fetches
   by the mirror's own URL.
3. **New commits are shown in two places.** The project page's *On <host>* box gives the count of
   commits `onehand/<host>` has that `HEAD` does not; the count drops to none once merged. The
   rail's project row gets a calm mark (not a needs-attention mark) while that count is above
   zero, with a tooltip giving the count. There is no toast and no desktop notice.

## Builder's calls (said here so review can disagree)

- **The branch's name** is `onehand/<host>`. In `<host>`, every character that is not an ASCII
  letter, digit, `.`, `_` or `-` becomes `-`. Runs of `-` collapse into one, `..` becomes `.`, and
  `-` and `.` are trimmed from the ends, so `user@box.lan` gives `onehand/user-box.lan` and
  `fe80::1` gives `onehand/fe80-1` (`branch_for`). An empty result becomes `onehand/host`. The
  laptop picks the name and sends it, so the host needs no name of its own.
- **The branch mirror's name** is `stem_for(folder)` + `.branch`, beside the mirror-mode mirror.
- **Git over ssh, never the attach,** both ways. Each way is a plain `git push` / `git fetch` with
  `GIT_SSH_COMMAND="ssh <SSH_OPTS>"` (rsync's options), `GIT_TERMINAL_PROMPT=0`, to
  `ssh://<host><mirror>` (`git_url`). An IPv6 host that is not bracketed gets brackets, keeping
  any `user@`. The host `local` uses the mirror's path. The host needs git on its
  non-interactive PATH; a clone needs that already.
- **The first copy pushes the laptop's `HEAD` commit, not the folder.** The laptop pushes
  `HEAD:refs/onehand/laptop` (forced), then the host's `MirrorUpdate` starts the branch from that
  commit and checks it out. The mirror holds only that commit's history: other branches, tags,
  stashes and uncommitted changes stay on the laptop, as the spec says of uncommitted changes.
  A folder with no commit is refused: *<project> has no commit to start the host's branch from*.
  Mirror mode's refusals (a folder that is not a repository's top, a nested repository) apply too.
- **Offloading again onto a branch mirror that already has the branch** keeps the host's branch as
  it is and merges nothing. Updating it is *Update host from laptop*'s job.
- **A branch mirror has no key.** The host makes it with the key on the host's side, so
  `prompt_to` lets prompts through unchanged. `MirrorKey` on a branch mirror is refused: *<path>
  has no key; it works on a branch*. The laptop never locks it, never sends it up before a prompt,
  and never copies it back.
- **Update host from laptop** sits in the *On <host>* box. It runs in this order:
  1. Attach.
  2. Ask the host for the mirror's state. While `working` it refuses: *Wait for the sessions on
     <host> to finish*.
  3. Push `HEAD` to `refs/onehand/laptop`.
  4. Send `MirrorUpdate`. On the host: refused while working; commit what is left; then
     `git merge --no-edit <commit>`. A merge that conflicts runs `git merge --abort` and answers
     *Merging this machine's commit into <branch> on <host> stopped on a conflict in: <files>; the
     host's branch is as it was* (at most 10 files named, then *and N more*).
  5. Fetch, then count again.

  A failure stands in the box's failed line, as step 5's do. A success says nothing: the count
  changes.
- **While an update runs, a prompt to that mirror is refused by the host:** *The host's branch is
  being updated from the laptop; send the prompt again in a moment*. Without this, a prompt that
  lands between the working check and the merge would set an agent working while git merges under
  it.
- **At a turn's end the host commits what was left** (`git add -A`, then `git commit`) in the
  session's branch mirror. The host's `.gitignore` keeps `bin/`, `obj/` and the like out of it.
  - The commit's author is `onehand <onehand@onehand.invalid>`, set with `-c` (`user.name`,
    `user.email`), so that a host with no git identity commits all the same. The message is
    *Work left at a turn's end*.
  - Nothing to commit is not a failure.
  - A mirror whose HEAD is not on the branch is not committed to. The agent switched branches, or
    is in the middle of a rebase. Both are logged in `host.log` and are a known gap.
  - Every git write in branch mirrors (the turn-end commit, the update) takes one process-wide
    mutex, so that two turn ends, or a turn end and an update, never race on `index.lock`.
  - The commit runs on `spawn_blocking`, never under the hub's lock.
- **When the laptop fetches.** `MirrorState` carries `head`, the branch's commit on the host.
  `refresh` asks for it and fetches only when it differs from `refs/remotes/onehand/<host>` here.
  That check runs at these points:
  - when a turn of a session on the project ends here;
  - in step 5's 30 s poll (`OFFLOAD_POLL`) while the host is attached, which also covers an attach
    landing: the next poll catches up;
  - when the project page shows.

  It never attaches by itself, as step 5's check does not. The count is
  `git rev-list --count HEAD..refs/remotes/onehand/<host>`, read on the background executor.
- **The laptop keeps no state file for branch mode.** The ref is the state: what was fetched is
  the ref, and the count is read from it. `offload.json` is neither read nor written for a branch
  root, so an unreadable one cannot lock it.
- **Kept in the workspace file:** `offloaded` as today (`<host>:<mirror>`), plus
  `offloaded_branches`, keyed the same way and valued with the branch name, present only for
  branch mode. A file written before step 6 reads as mirror mode.
- **Run here again** in branch mode drops the root's host sessions here, as in mirror mode. It
  leaves the mirror, the host's branch and the laptop's ref as they are.
- **Send to <host> and the bypass switch** work on a branch mirror unchanged.

## PR boundaries

- **6a, `feat/remote-hosts-branch-host`:** the wire, the host's branch mirrors and core's
  laptop-side git (Tasks 1–3). No app change apart from what compiles against the wire.
- **6b, `feat/remote-hosts-branch-app`:** the workspace field, the form's choice, the Offloads
  entry, the box, the rail mark, *Update host from laptop*, and the docs (Tasks 4–7).

Each PR: `git fetch`, branch from `origin/main`, build task by task, then
`cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"` and `make fmt`.
One opus subagent reviews `origin/main..HEAD`, and what it confirms is fixed. Then open the PR,
wait for CI, and ask the owner before merging.

## Global constraints

- Core stays GUI-free and runtime-free: every git call is a blocking function, run off the UI.
- Nothing in core is `pub` unless the app or the host names it.
- Code describes; it never cites a document. Every `.md` file is in English.
- No hard-coded colour or size; icons from `IconName`; buttons through `controls::action`.
- No file passes about 800 lines of code. `crates/app/src/offload.rs` is at about 660 lines of
  code, so branch mode's app logic goes in `crates/app/src/offload/branch.rs`.
- Wire additions are `#[serde(default)]`, so an older laptop and an older host each still read
  what the other sends.

## Review focus

1. **A prompt racing an update.** An update and a prompt that arrive together must never leave an
   agent working while git merges under it (Task 2's `updating` refusal).
2. **A turn-end commit racing another, or an update.** Two sessions in one mirror end their turns
   together; the mutex must hold, and neither commit may fail on `index.lock` (Task 2's test
   commits twice from two threads).
3. **A host with no git identity.** The turn-end commit and the merge commit still land (Task 2's
   test runs with `HOME` pointed at an empty folder and `GIT_CONFIG_NOSYSTEM=1`).
4. **A merge that conflicts leaves the branch exactly as it was:** no `MERGE_HEAD`, a clean tree,
   the same commit (Task 2's test).
5. **The person's branches are never touched.** A fetch writes only
   `refs/remotes/onehand/<host>`, whatever is checked out, and even after the host's branch was
   rewritten (forced refspec `+`) (Task 3's test).

---

## PR 6a

### Task 1: the wire

**Files:**
- Modify: `crates/core/src/host/wire.rs` (`Call::Mirror`, a new `Call::MirrorUpdate`,
  `MirrorState`)
- Modify: `crates/core/src/host/client/` (the `HostConn::mirror` signature, a new
  `HostConn::mirror_update`)
- Modify: `crates/app/src/offload.rs` (pass `None` for the new argument)

**Interfaces (produces):**
- `Call::Mirror { name: String, #[serde(default)] branch: Option<String> }`: a branch makes it
  a branch mirror on creation. On an existing mirror the branch is ignored.
- `Call::MirrorUpdate { path: PathBuf, commit: String }`, answered `Reply::Mirror(MirrorState)`.
- `MirrorState { …, #[serde(default)] head: Option<String> }`: the branch's commit, `None` for a
  mirror-mode mirror or a branch not started yet.
- `HostConn::mirror(&self, name: &str, branch: Option<&str>) -> Result<MirrorState, HostError>`
- `HostConn::mirror_update(&self, path: &Path, commit: &str) -> Result<MirrorState, HostError>`

- [ ] **Step 1:** Write the test `an_older_mirror_call_still_reads`. A `Call::Mirror` serialized
  without `branch` deserializes with `branch: None`, and a `MirrorState` without `head` reads
  with `head: None`.
- [ ] **Step 2:** Add the fields and the call. Update the exhaustive matches in `serve.rs` and
  `files.rs`: `MirrorUpdate` touches `path`, and is not a file call. Fix the callers.
- [ ] **Step 3:** Run `cargo test -p onehand-core wire`, then `cargo check --workspace`. Both
  pass.
- [ ] **Step 4:** Commit: `feat(hosts): the wire for branch mirrors`.

### Task 2: branch mirrors on the host

**Files:**
- Create: `crates/host/src/hub/branch.rs`: blocking git in a branch mirror
- Modify: `crates/host/src/hub/mirrors.rs`: `Mirror.branch`, creation, `mirror_key`'s refusal,
  `prompt_to`'s `updating` refusal, `head` in `mirror_state`
- Modify: `crates/host/src/hub.rs`: `updating: HashSet<PathBuf>`, and the turn-end commit in
  `record`
- Modify: `crates/host/src/serve.rs`: `Call::MirrorUpdate`
- Test: `crates/host/src/hub/tests.rs`, plus `crates/host/src/hub/branch/tests.rs` for the git
  functions

**Interfaces (produces), in `branch.rs`:**
- `pub(super) fn init_blocking(mirror: &Path) -> Result<(), String>` runs `git init`.
- `pub(super) fn head_blocking(mirror: &Path, branch: &str) -> Option<String>`
- `pub(super) fn update_blocking(mirror: &Path, branch: &str, commit: &str) -> Result<(), String>`.
  With no branch yet, it runs `git checkout -b <branch> <commit>`. Otherwise it runs
  `commit_left_blocking`, then the merge, with `--abort` on a conflict and the conflict message
  from *Builder's calls*.
- `pub(super) fn commit_left_blocking(mirror: &Path, branch: &str) -> Result<bool, String>`
  answers whether it committed. It does nothing when HEAD is not `refs/heads/<branch>`.
- `static GIT_WRITES: Mutex<()>`, taken by both writers.
- `Mirror { owner, key, generation, #[serde(default)] branch: Option<String> }`

Write `MirrorUpdate` as follows:

1. Under the hub's lock: check the owner; refuse unless the mirror is a branch mirror; refuse
   while working (*Sessions are still working on the host*); insert the path into `updating`.
2. On `spawn_blocking`: run `update_blocking`.
3. Remove the path from `updating`, then answer `mirror_state`.

`record` handles a turn's end as follows: on `Entry::Event(AcpEvent::TurnEnded { .. })` for a
session whose cwd lies in a branch mirror, spawn `commit_left_blocking` on `spawn_blocking`, log
a failure, and never block the hub.

Every test below runs git with `HOME` pointed at an empty folder and `GIT_CONFIG_NOSYSTEM=1`.

- [ ] **Step 1: write the failing tests.**
  - `a_branch_mirror_starts_on_the_laptops_commit_and_has_no_key`: push a commit into an inited
    mirror; `update` → HEAD is on `onehand/box` at that commit, and `head` is answered.
    `mirror_key(.., Laptop, ..)` errs with *has no key*. A prompt to a session in it is not
    refused.
  - `an_update_merges_and_a_conflict_leaves_the_branch_as_it_was`: a clean merge moves the
    branch. A conflicting one errs naming the file, with the branch's commit unchanged, no
    `.git/MERGE_HEAD`, and `git status --porcelain` empty.
  - `an_update_commits_what_was_left_first`: an uncommitted file in the mirror ends up in a
    commit by `onehand` before the merge.
  - `a_turn_end_commits_on_the_branch_only`: `commit_left_blocking` commits a change, then
    answers `false` for nothing to commit, and `false` with HEAD on another branch.
  - `two_commits_at_once_both_land`: two threads each write a file and run
    `commit_left_blocking`; both are committed, and neither errs.
  - `a_prompt_while_updating_is_refused`: with the path in `updating`, `prompt_to` errs with the
    *being updated* line.
  - `a_mirror_kept_before_branches_reads_as_mirror_mode`: a `mirrors.json` without `branch`
    loads.
- [ ] **Step 2:** Run `cargo test -p onehand-host branch`. The new tests fail.
- [ ] **Step 3:** Implement as the interfaces say.
- [ ] **Step 4:** Run `cargo test -p onehand-host`. Everything passes.
- [ ] **Step 5:** Commit: `feat(hosts): branch mirrors on the host`.

### Task 3: the laptop's git for branch mode

**Files:**
- Create: `crates/core/src/host/offload/branch.rs` (declared from `offload.rs`)
- Test: `crates/core/src/host/offload/branch/tests.rs`

**Interfaces (produces):**
- `pub fn branch_for(host: &HostName) -> String`
- `pub fn tracking(branch: &str) -> String` → `refs/remotes/<branch>`
- `fn git_url(addr: &HostAddr, mirror: &Path) -> String`
- `pub fn push_head_blocking(addr: &HostAddr, folder: &Path, mirror: &Path) -> Result<String, String>`
  checks `listed_blocking`'s refusals and answers the commit pushed.
- `pub fn fetch_blocking(addr: &HostAddr, folder: &Path, mirror: &Path, branch: &str) -> Result<(), String>`
  uses the refspec `+refs/heads/<branch>:refs/remotes/<branch>` with `--no-tags`.
- `pub fn local_head_blocking(folder: &Path, branch: &str) -> Option<String>` reads the tracking
  ref's commit.
- `pub fn ahead_blocking(folder: &Path, branch: &str) -> Result<usize, String>`; with no tracking
  ref yet it is `0`.

- [ ] **Step 1: write the failing tests,** with the host `local` and scratch repositories, as in
  `offload/tests.rs`.
  - `a_branch_is_named_for_its_host`: `box` → `onehand/box`; `user@box.lan` →
    `onehand/user-box.lan`; `fe80::1` → `onehand/fe80-1`; `..` → `onehand/host`.
  - `a_url_names_the_host_as_ssh_does`: `box` → `ssh://box/m`; `user@[fe80::1]` keeps its
    brackets; `fe80::1` → `ssh://[fe80::1]/m`; `local` → `/m`.
  - `a_head_with_no_commit_is_refused`.
  - `a_push_carries_the_commit_and_nothing_uncommitted`: after the push,
    `refs/onehand/laptop` in the mirror equals the laptop's HEAD, and a dirty file here is absent
    there.
  - `a_fetch_writes_only_the_tracking_ref`: the laptop's `refs/heads/*` are unchanged, even
    after the host's branch was rewritten and fetched again, and with a branch named like it
    checked out here. `ahead_blocking` counts the host's new commits, and is `0` after
    `git merge`.
- [ ] **Step 2:** Run `cargo test -p onehand-core offload::branch`. The tests fail.
- [ ] **Step 3:** Implement. `GIT_SSH_COMMAND` is built from `SSH_OPTS`, as `rsync_command`
  builds its `-e`.
- [ ] **Step 4:** Run `cargo test -p onehand-core`. Everything passes.
- [ ] **Step 5:** Update `docs/hosts.md` (the host side: *Branch mirrors on the host*) and the
  spec's status line. Commit: `feat(hosts): push and fetch a host's branch`.

## PR 6b

### Task 4: the mode, kept

**Files:**
- Modify: `crates/core/src/workspace.rs`. `ProjectRoot.offload` becomes
  `Option<Offloaded>`, where `pub struct Offloaded { pub host: HostName, pub mirror: PathBuf, pub branch: Option<String> }`.
- Modify: `crates/core/src/config.rs`: `offloaded_branches: BTreeMap<PathBuf, String>`
- Modify: every `.offload` site (`chat/pane.rs`, `rail/project.rs`, `shell/roots.rs`,
  `shell/offload.rs`, core's own)
- Test: `crates/core/src/workspace/tests.rs`

- [ ] **Step 1:** Extend the offload round-trip test (`workspace/tests.rs:596`) to cover a branch
  root and a mirror root saved and read back, plus a file with no `offloaded_branches`, which
  reads as mirror mode.
- [ ] **Step 2:** Run `cargo test -p onehand-core workspace`. The new assertions fail to compile.
- [ ] **Step 3:** Implement, then run `cargo check --workspace`.
- [ ] **Step 4:** Run `cargo test -p onehand-core workspace`. It passes.
- [ ] **Step 5:** Commit: `feat(hosts): keep an offloaded project's sync mode`.

### Task 5: branch mode in the Offloads entry

**Files:**
- Create: `crates/app/src/offload/branch.rs`
- Modify: `crates/app/src/offload.rs`. `Offload` gets
  `branch: Option<BranchState>`. `locked()` is `false` and `claim_send` is `false` when it is
  set; `here()` holds unless it is updating. `track` skips `offload.json` for a branch root. The
  poll and `check_soon` call `branch::refresh` for a branch root.

**Interfaces (produces):**
- `pub(crate) struct BranchState { pub(crate) name: String, pub(crate) ahead: Option<usize>, pub(crate) updating: bool }`
- `pub(crate) async fn offload_branch_to(cx, path, host) -> Result<(PathBuf, String), String>`
  answers the mirror and the branch. In order: attach, mirror (`<stem>.branch`, the branch), and,
  unless `head` is set, push and then `mirror_update`; finally fetch.
- `pub(crate) async fn refresh(cx, path)`: never attaches. Live host → `mirror` → fetch if `head`
  differs from the local one → `ahead`.
- `pub(crate) async fn update_host(cx, path)`: the order in *Builder's calls*. A failure goes to
  `Offload.failed`.

- [ ] **Step 1:** Extend `the_lock_follows_who_holds_the_code`: an entry with `branch` set is
  never locked, is `here()` unless updating, and is never claimed for a send.
- [ ] **Step 2:** Run `cargo test -p onehand the_lock`. It fails.
- [ ] **Step 3:** Implement. Every git call runs on the background executor, and every host call
  through `hosts::on_runtime`.
- [ ] **Step 4:** Run `cargo test -p onehand offload`. It passes.
- [ ] **Step 5:** Commit: `feat(hosts): an offloaded project on a branch`.

### Task 6: the form, the box and the rail (UI: run the `ui` skill and its checklist)

**Files:**
- Modify: `crates/app/src/shell/offload.rs`. `OffloadDraft.on_branch: bool`, picked while
  nothing sends. `commit_offload` takes the branch path. `offload_landed` keeps
  `Offloaded { branch }`. A new `update_host_from_laptop(path)`.
- Modify: `crates/app/src/dialogs/offload.rs`. A `choice` with *Into this folder* (default) and
  *On a branch*, and under it one muted line for the picked mode:
  - mirror: *The host's code comes back into this folder when its sessions finish; edits here wait
    meanwhile.*
  - branch: *The host works on onehand/<host>, from this folder's last commit; what is not
    committed stays here. You merge the branch when you choose.* (`<host>` as `branch_for`
    names it, or *the host* while the field is empty.)
- Modify: `crates/app/src/chat/pane/project_page/offload_box.rs`. For a branch root, *On <host>*
  reads:
  - *Sessions on <host> work on onehand/<host>; this folder stays yours*;
  - then *onehand/<host> has N commits this branch does not* (*1 commit*), or *onehand/<host>
    has nothing this branch does not*, or nothing while `ahead` is unread;
  - the failed line;
  - *Update host from laptop* (secondary, short), loading while updating and refused with *The
    branch is being updated*.

  The page showing calls `refresh`.
- Modify: `crates/app/src/chat/pane.rs` (or where `ChatPaneEvent` is): `UpdateHost(PathBuf)`,
  handled by the shell.
- Modify: `crates/app/src/rail/project.rs`. A calm mark while `ahead > 0`, with the tooltip
  *onehand/<host> on <host> has N commits to merge*, in the muted ink and never the warning ink.
  The icon is picked by the `ui` skill from `IconName` (a git-branch shape if the bundle has one).

- [ ] **Step 1:** Write the test `the_branch_box_counts_what_is_left_to_merge`: a pure
  `branch_line(name, ahead) -> Option<String>` gives 0, 1 and N in the words above.
- [ ] **Step 2:** Run the test. It fails.
- [ ] **Step 3:** Implement the form, the box, the event, the rail mark and the shell handler.
- [ ] **Step 4:** Answer the `ui` skill's checklist in the commit body. Run `cargo test -p onehand`.
- [ ] **Step 5:** Commit: `feat(hosts): offload on a branch, and update the host from here`.

### Task 7: docs

**Files:** `docs/hosts.md` (*Branch mode* under *Offloaded projects*, *Not built* emptied),
`docs/screens.md` (the form's choice, the box, the rail mark), `CONTEXT.md` (**Sync mode**,
**Branch mode**, and the host's branch, with what to avoid), `docs/known-gaps.md`, and the spec's
status (step 6 built).

The known gaps to add:
- an agent that leaves the branch gets no turn-end commit;
- the mirror holds only `HEAD`'s history, without the laptop's other branches, tags or `origin`;
- the laptop's ref is never pruned;
- switching mode goes through *Run here again*.

- [ ] **Step 1:** Write the docs, each rule in its one owning document.
- [ ] **Step 2:** Run `cargo test -p onehand guards`, and check every path the docs cite exists.
- [ ] **Step 3:** Commit: `docs(hosts): branch mode`.
