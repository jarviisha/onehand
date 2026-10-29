# onehand as a host for workers

What onehand's tools would need so the [lead skill](skill.md) can keep its workers in onehand,
any ACP agent among them, instead of in command-line processes. The tools today are in
[control.md](../../control.md). This is the material for a spec, not the spec. The decisions
below were taken on 2026-10-29 for the first version; each says what it leaves for later.

## Where it stands

- `open_session` (`crates/app/src/control.rs`, the `Call::OpenSession` arm) resolves its project
  with `control::pick_project`, which takes only a project already in the window, by path or by
  name. A worktree the lead has just cut is refused.
- It starts the session with `Shell::run_unattended` (`crates/app/src/shell/remote_runs.rs`), which
  can already add a folder that is not a project as a **transient** root, without selecting it or
  taking focus. A transient root is never written to the workspace file
  (`Workspace` in `crates/core/src/workspace.rs`), so it does not come back after a restart.
- It answers `Opened a session with <agent> on <project>. It is in the rail, not shown`: no
  handle, and no tool reaches the session again.
- Which session opened which lives in `Sessions.opened`, in memory only; the switch that lets a
  session's agent call the tools is not saved either. The cap of four counts sessions still open,
  working or not.
- The conversation itself is kept: its transcript is written at every turn end and read back.
- An agent runs in a process group of its own (`crates/core/src/acp/client/transport.rs`), ended
  with everything it started when onehand lets go of it. A clean quit therefore stops every
  worker; a crash or a kill of onehand may leave a worker's processes running.
- Session uids count from 1 in every run of the app (`Shared::next_uid`), so the same number names
  different sessions in two runs.
- Whether a prompt can be sent is already one rule, `Chat::submit_blocker` in core (busy, not
  connected, nothing to send, an unreadable attachment); what a session is doing is
  `SessionSignal` (disconnected, failed, needs input, running, done).
- An unattended run's mode is `config::run_mode` (`crates/core/src/config.rs`): `[unattended]
  mode` when set, else what the agent's preset is known to need (`acceptEdits` for Claude, none
  for Codex, none on purpose for Copilot, whose only mode that does not ask allows every tool).
  It is set before the first prompt, and a mode the agent does not offer fails the run.
- `session/new` carries only `cwd` and `mcpServers` (`Conn::new_session`,
  `crates/core/src/acp/client/handshake.rs`). ACP's own `additionalDirectories` field is read by
  both adapters onehand ships: Claude's (0.81.2) adds the directories to its SDK session, where
  `acceptEdits` covers them; Codex's (2.2.2) adds them to its sandbox's writable roots. Neither
  carries a list of allowed commands or network access: those stay in each agent's own settings,
  which Claude's adapter reads (user, project, local).
- The warm adapter (`AcpRuntime::warm`, `crates/app/src/acp.rs`) is taken by a session with the
  same agent and `cwd`; it has already sent `session/new` with nothing beyond them.

## The first version in one list

A worktree added as a transient root, with its repository's `.git` and the lead's folders
writable; the mode unattended runs use; `open_session` taking a request id, a turn number, a
model and those folders, and answering with a handle that names the run of the app;
`session_state`; `send_prompt`; `cancel_turn`; *Give back to the lead* on a worker's session.
**Nothing new is written to disk**: after a restart the lead opens new workers and the job's state
comes from the skill's files.

## 1. Opening a worker on a fresh worktree

`git worktree add`, then `open_session` on it, misses a step: the worktree is not a project. It is
added **without changing the selection or the focus**, and the creation of the worktree stays out of
`open_session`; the lead still cuts it with git.

**Decided:** `open_session` takes, besides a project of the window, a **worktree of a repository
one of the window's projects belongs to**, checked with git, and opens it as a transient root
through `run_unattended`. Any other folder is refused, naming why; a tool never opens an agent on
an arbitrary path.

**Decided (2026-10-29): what the worker may write besides its worktree.** A worker commits into
its repository's `.git`, which lies outside a worktree, and writes its status into the lead's run
folder, outside every worktree. Both go in `session/new`'s `additionalDirectories`:

- **The repository's `.git`**, found with `git rev-parse --git-common-dir`, added by onehand
  whenever the session opens on a worktree. Opening on a worktree already means committing in
  it, so this widens nothing. A session on a project of the window gets nothing added.
- **`dirs`, named by the lead** on `open_session`: absolute, existing folders, none of them `/`,
  the home folder, the worktree, or a folder holding the worktree or the repository. A folder
  that fails is refused, naming it and why, and nothing opens. The answer and `session_state`
  name them, so the person can see what a worker was given.
- This widens where the agent may write, not what it may do: the mode stays the person's
  (section 7), and commands and network stay in the agent's own settings.
- A session with anything in `additionalDirectories` never takes the warm adapter, which opened
  its session without them.

## 2. What survives a restart

Three things are kept apart:

| What | Lifetime |
|---|---|
| The **handle**: the id a tool names a session by, within one run of the app | the process |
| The **conversation**: the transcript and the agent's own session | on disk, as today |
| The **relation**: this lead opened this worker and may send to it | the process |

**Decided: no recovery in the first version.** After a restart the workers' transient roots and
the relation are gone. That is safe only when the old workers are gone too: after a clean quit they
are, since their process groups end with onehand; after a crash they may not be, and the lead treats
such a worker as one it cannot confirm stopped, waiting on the person (the skill's case 4). The spec
checks both: what a clean quit ends, and what a crash leaves. The person switches the lead on again (the switch is not saved, which
stays); the lead opens a new worker on the same worktree, and the worker picks the job up from the
skill's job folder, its artifacts and the worktree. The old transcript stays on disk and is not
attached to anything.

The cost is the worker's conversational context, which it rebuilds from the artifacts. **Later**,
if that proves too costly in practice: keep the relation on disk, keyed by the persisted
conversation, with the worker's root kept rather than transient, and have the lead's switch say,
as it is switched on, that it gives that session back its hold on the named workers. The person
switching the lead on is then the renewal of the grant.

## 3. The handle

`open_session` answers with the handle and the name the rail shows. The lead records the handle in
its `session.md`, knowing it holds only until the app stops.

- **A handle names the run of the app too**, not only the uid, since uids start again from 1 each
  run. A handle from an earlier run is refused as such; it never reaches whichever session took
  its number.
- **`open_session` takes a `request` id from the caller.** A second call with the same id, within
  the same run, answers with the session the first one opened and opens nothing, sends no prompt
  again and takes no second place under the cap; the same id with other arguments is refused. A
  lead that lost the answer to an open retries with the same id instead of opening a second worker
  on the same worktree.
- **The first prompt is the job's turn 1**, or whatever turn number the caller gives with it:
  `open_session` takes the turn number with the prompt, and the first prompt is deduplicated with
  the request, as above.

## 4. State

`session_state(session)` for a session the caller opened (the same ownership as sending).

It is **not** built on `SessionSignal`. That enum is the rail's call for attention: it folds several
facts into the most urgent one, and its *done* depends on whether a person has looked. The state is
read instead from the separate facts core keeps: the connection, the current turn and how the last
one ended, a card or permission waiting, background work, and who holds the session.
`SessionSignal` goes on serving the rail. The state tells apart at least:

- connecting; connected and never prompted;
- a turn running; waiting on a card or a permission, which only a person answers, so the lead
  tells the person rather than waits;
- the last turn ended, and **how**: ended, failed with its error, cancelled;
- background work still running after the turn ended;
- the person holds the session (section 6);
- closed, or disconnected.

**A turn that ended is not a stage that succeeded**, nor a worktree that stopped changing. The
stage's outcome is the worker's `status.md`; to tie the two, every send carries the lead's turn
number (below), and the state names the last turn number it saw end.

## 5. Sending

`send_prompt(session, text, turn)`:

- **Refused, not queued,** by `Chat::submit_blocker`'s rule, the same one the composer obeys: while
  a turn runs, while connecting, when nothing is sent. The lead waits for the turn to end.
- **The answer means accepted**: the prompt was handed to the agent as the next turn, not that the
  turn has done anything. The state then shows it running.
- **Sent once per turn number.** A second send with the turn number already accepted and the
  same text answers `already sent` and sends nothing, so a lead that lost the answer can safely
  send again. The same turn number with **another text** is refused, naming the turn: it is a
  mistake, not a retry.
- **Only to a session the caller opened**, and not while the person holds it (section 6).

**Decided:** `send_prompt`, `session_state` and `cancel_turn` are offered to a session's agent
only, not to an outside client, which has no session to own a worker by. **Later**, if wanted: an
outside client owns what it opened for as long as its connection lasts.

## 6. The lifecycle: the person, a stuck worker, the end

- **The person types into a worker.** From that prompt on the session is the person's: the state
  says so and `send_prompt` and `cancel_turn` are refused. **Decided:** the person gives it back
  with ***Give back to the lead*** on the worker's block (section 8), offered only on a session a lead opened and the
  person has since prompted, and **only while that lead's session is still open and switched on**;
  with no lead to give it to, the session simply stays the person's. Until then the lead tells the
  person it is waiting.
  - What counts as the person's prompt: one **sent** from the composer, a queued prompt at the
    moment it is sent (queueing alone takes nothing), and a prompt that arrives through the remote
    bridge, which is a person too.
- **The lead closes or is lost.** Its workers stay, owned by nobody; nothing is stopped. They are
  the person's to close.
- **A worker hangs.** **Decided:** `cancel_turn(session)` for a session the caller opened, which
  cancels the running turn as the composer's Stop does. It is **refused while the session waits on
  a card or a permission**, so the lead never cancels what a person is about to answer, and while
  the person holds the session. It answers no card.
- **The cap.** Four sessions open per caller, counted whether working or not, unchanged. A finished
  worker left open holds its place; the first version leaves closing it to the person, and the
  refusal already names the four. **Later:** a `close_session` for a caller's own idle worker.

## 7. The model and the mode

**Decided: one model per worker**, named on `open_session`.

- It is checked against what the agent itself offers, read from the agent once it is up, and set
  **before the first prompt**. A workflow step's `options.model` is a value a person chose for one
  agent, not a capability every agent has.
- An agent that offers no models, or not the one named: the call says so, naming the models it
  does offer, and leaves nothing behind. The session started to ask is ended, its transient root
  dropped with `Shell::forget_root`, its place under the cap given back, its `request` id freed, and
  the prompt never sent. It never runs silently on another model. Left out, the agent's own default
  holds, as today.
- **Later:** a model on `send_prompt` overriding it for one turn, for agents that can change model
  inside a session.
**Decided (2026-10-29): the mode is the one unattended runs use**, by the same rule,
`config::run_mode`, and the lead names none. `[unattended] mode` holds only when the worker's
agent is the one runs use, since a mode is the adapter's own id; otherwise the worker's preset's
mode, or none. It is set before the first prompt, as the model is; a mode the agent does not
offer refuses the open the way a missing model does, leaving nothing behind. The person widens it
in Settings or `onehand.toml`, never the lead.

What the mode does not cover is the agent's own settings, a precondition of the run and not
onehand's to write: the commands a worker must run (the check, `git add/commit/status/diff/log`,
`sha256sum`, `mv`) in Claude's `permissions.allow`, and network access (a test on a loopback
socket) in Codex's sandbox settings. The skill's access check, turn 1, finds what is missing;
the worker then waits on a permission card, which `session_state` reports.

## Not wanted

- **A tool that approves, revises or answers a card.** Approval is the skill's record; only a
  person answers a card, a permission or an approval.
- **Cutting a worktree in `open_session`.** The lead does it with git.
- **A higher cap.**
- **A *Worker under a lead* workflow** before sections 1 to 6 are built.

## 8. Where the person sees the workers

**Decided (2026-10-29).** A lead's workers leave the rail and are seen from the lead:

- **Not in the rail** while their lead is open. The lead's row carries its workers' most urgent
  state beside its own, as a folded project carries its sessions', and *N need attention* counts
  a worker that waits on the person. A worker whose lead closes or is lost goes back into the
  rail as an ordinary session, since it would otherwise be nowhere to close. A session an outside
  client opened stays in the rail, as today.
- **A deck tucked behind the lead's composer**: a card whose top edge shows above the
  composer's, reading how many workers there are, how they stand and the most urgent one.
  Pressed, it slides up to a row per worker; pressing a row opens the workers' region at that
  worker. Shown only while the lead on screen has workers.
- **The workers' region**, in the Workbench's place and not one of its modes: the two never show
  at once, and opening one puts the other away. A header button beside the Workbench's opens it,
  present only while the lead on screen has workers. It shows the workers of the lead on screen.
  - The workers **stacked top to bottom**, one block each: a head (state mark, name, state word,
    who holds it, *Stop* while a turn runs, a `⋯` menu), and under it the transcript's latest
    items at a bounded height, with the oldest pinned card or question.
  - **One block open at a time**, pressed on its head: it takes the height the others leave, shows
    the whole transcript, scrolling, its pinned card, and a compact composer. The others fold to
    their bounded tail.
  - **The person acts there in full**: answering a card or a question, *Stop*, and typing, which
    makes the worker the person's (section 6). *Give back to the lead* is in the block's `⋯`.
  - A worker's transcript is drawn in the region and nowhere else while its lead is open, since an
    entity renders where it is mounted, once.

Later, not in the first version: a worker shown in the conversation itself, the region's layout
saved.
