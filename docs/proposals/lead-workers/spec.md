# Spec: onehand as a host for a lead's workers

Step 3 of [README.md](README.md)'s order, from the decisions in [onehand.md](onehand.md). It says
what the first version does, built in the six pull requests at the end. Each pull request moves
what it builds into [control.md](../../control.md); the last one removes this folder's onehand
part.

## Terms

- **Lead**: a session whose agent is switched on (*Agent controls onehand*) and calls the tools.
- **Worker**: a session a lead opened with `open_session`. The relation lives in memory, in the
  app's `Sessions` (`crates/app/src/control.rs`), keyed by the worker's uid, and goes when either
  session closes.
- **Run of the app**: one process. A **run id** is minted once per process, as a token is
  (`control::new_token`), and is never written anywhere.
- **Handle**: `<run id>-<uid>`. It is what every tool below takes as `session`.

## Who gets what

`open_session` keeps its current inputs for every caller. Its new inputs (`request`, `turn`,
`model`, `dirs`) and a worktree as its project are **for a session's agent only**: an outside
client naming any of them is refused, naming them. `session_state`, `send_prompt` and
`cancel_turn` are listed to a session's agent only, and only to one switched on; an outside
client never sees them in `tools/list`. A session standing with a set of tools (a schedule's) gets
them only if the set names them.

## 1. `open_session`

Built in PR 1, except the waiting, the answer's handle, `request`, `turn` and `model`, which are
PR 2's; what is built is in control.md's *A worker*.

### Input

| Field | Type | Meaning |
|---|---|---|
| `project` | string | a project of the window, by name or path, as today; **or** the absolute path of a worktree (below) |
| `agent` | string | as today |
| `prompt` | string, optional | the first prompt |
| `request` | string, required from a session's agent | the caller's id for this open |
| `turn` | integer ≥ 1, optional | the turn number `prompt` is; required with `prompt` from a session's agent |
| `model` | string, optional | the model the worker runs, by the agent's own value id |
| `dirs` | array of strings, optional | further folders the worker may write |

### The project

- A name or path of a window project resolves as today (`control::pick_project`).
- Otherwise `project` must be the absolute path of a **worktree's top level, of a repository a
  window project belongs to**. Every path is canonicalized first (symlinks resolved), and so is
  what git answers:
  - `git rev-parse --show-toplevel` in it, canonicalized, is the path itself: a folder inside a
    checkout is refused, naming the top level it belongs to;
  - `git rev-parse --path-format=absolute --git-common-dir`, canonicalized, equals the same for
    some window project, read the same way from that project's folder, whether the project is the
    main checkout, a linked worktree or a folder inside one.
  Read on the background executor, never in the call's handler. Anything else is refused, naming
  why (not a folder, not a worktree's top level, a repository no project of this window belongs
  to).
- A worktree opens as a transient root through `Shell::run_unattended`, selecting nothing and
  taking no focus. A worktree that is already a root of the window is used as it is.

### One worker per worktree

- **A reservation**, for every open by a session's agent, on a worktree or on a window project:
  the root's canonical path, taken on the UI thread in the same update that checks it, once the
  background reading has answered (the UI thread runs nothing beside it, so two opens cannot both
  pass), held in `Sessions` until the open fails, or until the worker it opened closes. A second open on a reserved folder, by any lead, is refused at once, naming the session
  or the open holding it. **Any other session open on that folder refuses it too**, the person's
  or the lead's own: a worker never shares its folder with another session. An outside client's
  open is unchanged.
- **What the reservation cannot see** is a worker left by an earlier run: after a crash its
  agent may still run, with nothing in this run knowing its pid. So before opening, onehand looks
  through **every process of the same user** for one working in the worktree, by the directory
  the system says it is in: `/proc/<pid>/cwd` for each pid in `/proc` on Linux, and
  `lsof -a -d cwd -u <user> -Fpn` elsewhere, bounded in time. Left out: onehand's own process and
  the shells of its own terminals, by the pids it holds. `working_in_blocking` does not do this
  today: it only checks the pids it is given, and treats a directory it cannot read as no match.
  The new reading lives beside it in core, with tests over a fake process table.
- **Three answers, and only one opens**: none found, and every process of the user was read:
  the open goes on. Some found: refused, naming each (its pid and command) and saying the person
  has to stop them. **Some could not be read** (a process gone between listing and reading
  aside, and a permission refused aside too: the system refuses its own user the folder of a
  process that user may not trace, `systemd --user` and `ssh-agent` on every desktop, and an
  agent is never one; `lsof` missing or out of time counts): refused as unknown, naming what
  could not be read, never taken as none. The refusal is the lead's answer, and nothing is
  opened.

### What the worker may write

`session/new` gets ACP's `additionalDirectories` (`Conn::new_session`,
`crates/core/src/acp/client/handshake.rs`, through a new `SessionOpts::dirs`):

- the repository's common `.git`, canonicalized, when the session opens on a worktree;
- each of `dirs`, which must be absolute and existing, and is **canonicalized before any check**,
  so a symlink or an alias cannot pass for another folder: then none of `/`, the home folder, the
  worktree, a folder inside it, or a folder holding it or the repository. The first that fails
  refuses the call, naming it as given and as resolved, and why. The canonical paths are what
  `session/new` gets and what the answer names.

A session with any of these never takes the warm adapter (`AcpRuntime::warm`,
`crates/app/src/acp.rs`), which opened its session without them. `session/load` and
`session/fork` send them too, so a restart of the worker's agent keeps them.

### The mode and the model

Set **before the first prompt**, with the planner workflow steps use
(`workflow::settings::next`, `crates/core/src/workflow/settings.rs`): mode first, then `model` as
the config option `model`.

- The mode is `config::run_mode`'s, with `[unattended] mode` only when the worker's agent is the
  one unattended runs use, else the worker's preset's, else none. The lead names no mode.
- A mode or a model the agent does not offer **refuses the open**. The answer names what the agent
  does offer.

### Waiting, and the answer

The call answers once the worker is up, not before:

1. the cap and the arguments are checked; the worktree is reserved and a place under the cap is
   taken; the worktree's checks and the process reading run;
2. the session starts; the call waits for `Link::Connected`;
3. the mode and the model are picked, each answered;
4. `prompt` is submitted. A blank `prompt` is refused at step 1, so after step 3 the only blocker
   left is a link lost, which is a failure.

**One deadline for the whole open**, `OPEN_DEADLINE`, 120 seconds from step 1, named beside the
tools. `HANDSHAKE_TIMEOUT` (`crates/core/src/acp/client.rs`) bounds only the handshake and stops
counting once the connection is ready, so nothing else bounds a `session/set_mode` or
`session/set_config_option` the agent never answers. Past the deadline the open fails at the step
it was in, as any failure does. An answer arriving after that finds its session ended and is
dropped with it; nothing it says reaches the lead or the session.

**The lead is checked again** before each of steps 2 to 4, and before the answer: still open,
still switched on, still standing with these tools. A lead that closed or was switched off while
its worker was opening **abandons the open**: it fails as below, the first prompt is never sent,
no relation is made, and nothing is answered, since nobody waits for the answer. A relation is
only ever written once the prompt has gone, so `control::forget` running in between cannot have
one made again after it.

It answers, one fact per line:

```
session: <handle>
name: <the rail's name>
project: <root path>
mode: <mode id, or the agent's own>
model: <value, or the agent's own>
dirs: <each folder added, or none>
turn: <n> accepted
```

**A failure at any step leaves nothing behind**, and touches nothing else:

- **The session first, by its uid**: only the session this open started is closed, which ends its
  agent's process group. Never `Shell::end_unattended`, which drops the whole root with every
  session on it (`Shell::forget_root`).
- **Then the root, only if this open added it** as transient and no session is left on it. A root
  that was a window project, or one a session of anyone else's stands on, stays.
- The reservation, the place under the cap and the `request` id are freed, and the prompt is
  never sent. The answer says which step failed and why, in the agent's words where it gave
  any.

### The request id

Kept per lead for the run of the app, with the arguments it came with.

- The same `request` with the same arguments: the open in progress answers `<handle> is still
  opening; call again with the same request`; one that finished answers its first answer again;
  one that failed is forgotten, so the call opens afresh. Nothing is opened or sent twice, and no
  second place is taken.
- The same `request` with other arguments is refused, naming the request.

## 2. `session_state(session)`

For a worker of the caller. A handle from another run is refused as such; a handle of this run
whose session no longer exists answers `link: closed`, since a uid is never reused within a run.
Another lead's worker, or a session no lead opened, is refused.

Built from core's separate facts, never from `SessionSignal`. The rule that reads them goes into
core beside the chat model, as a function over the `Chat` plus the relation's two numbers, with
tests without a GUI. It answers, one fact per line:

| Line | Values | From |
|---|---|---|
| `link` | `connecting`, `connected`, `lost`, `closed` | `Chat::link` |
| `turn` | `none` (never prompted), `running`, `ended`, `failed: <the failure's words>`, `cancelled` | `Chat::busy`, `Chat::cancelled`, `Chat::failure` |
| `waiting` | `none`, or `<n> permission`, `<n> question`: only a person answers these | the unresolved permission and question cards |
| `activity` | `running: <what>`, `none reported`, `unknown` (below) | `Chat::running_since(None)`'s four sources |
| `held by` | `lead`, `person` | below |
| `turns` | `sent <n>, ended <m>` | the relation |
| `mode`, `model`, `dirs` | as the open answered | the session |

`ended <m>` is the last turn the lead sent that is over: `m = n` once the turn after the lead's
last send has ended and the person has not prompted since. **A turn over is not a stage done**;
the lead reads the worker's `status.md` for that.

**`activity` tells apart what was reported from what is known**, from every source core keeps
(`Chat::running_since`, `crates/core/src/chat/model/background.rs`):

- `running: <what>`, naming each that holds: the turn, `<n> in background: <names>`, `<n>
  subagents`, `working by itself` (the agent writing with no turn open, `own_cycle`).
- `none reported`: connected, and none of the four holds. **This is not "stopped"**: it is enough
  to send the next turn (the skill's case 1), never to open a replacement on the worktree.
- `unknown`: the link is lost, or still connecting. A lost link clears the turn and the activity
  core held (`AcpEvent::Disconnected` in `crates/core/src/chat/model/apply.rs`: `busy` goes
  false, `forget_activity`), so empty values then prove nothing about the agent's processes.

**Stopped is only ever confirmed by closing**: `link: closed` means onehand ended the session and
its agent's process group. Whether anything outside that group still works in the worktree is
what the next `open_session` there reads (section 1); a lead that needs a replacement after
`unknown` asks the person to close the worker, then opens again.

## 3. `send_prompt(session, text, turn)`

For a worker of the caller, held by the lead.

- **Refused, not queued**, by `Chat::submit_blocker`'s rule, in its words: a turn running,
  connecting or lost, nothing to send.
- **Once per turn number.** The relation keeps the last turn accepted and its text. The same turn
  and text answers `turn <n> already sent` and sends nothing. The same turn with other text is
  refused, naming the turn. A turn below the last accepted is refused, naming both.
- The answer `turn <n> accepted` means the prompt went to the agent as the next turn, nothing more.

## 4. `cancel_turn(session)`

For a worker of the caller, held by the lead, with a turn running. As the composer's Stop does
(`Chat::cancel_turn`). Refused while `waiting` is not `none`, so the lead never cancels what a
person is about to answer, and while the person holds the session. It answers no card. The answer
`cancelling turn <n>` means the cancel was sent; `session_state` says when the turn is over.

## 5. Who holds a worker

The relation keeps `sent`, the worker's `Chat::prompts_sent` after the lead's last send (or the
open's). The **person holds the worker** once a prompt of theirs has gone to the agent:
`Chat::prompts_sent > sent`. That is a prompt sent from the composer, the rail's resend, the remote
bridge, or a queued prompt at the moment it is sent. **Queueing alone takes nothing**, so
`Chat::prompted_beyond`, which counts the queue too, is not the rule here; the comparison goes into
core beside it.

A queued prompt still never races the lead: the queue is sent in the same update that ends the
turn (`AcpEvent::TurnEnded` in `crates/core/src/chat/model/apply.rs`), so no state ever reads
the turn ended with a person's prompt still waiting. A `cancel_turn` while one waits ends the turn,
and the person's prompt then goes, as with the composer's Stop.

### *Give back to the lead*

- In the `⋯` menu of the worker's block in the workers' region (section 7). Offered only while
  the person holds the worker and the lead is still open and switched on. With no such lead the
  session stays the person's.
- It sets `sent` to the worker's `prompts_sent` now. The lead learns of it through
  `session_state`. Nothing is sent to it, and nothing is said in the transcript.
- A visible change: it follows the `ui` skill, for its words and its placement.

## 6. The lead and the cap

- **The lead closes or is lost**: `control::forget` drops its relations. Its workers stay open,
  owned by nobody, and go back into the rail; they are the person's to close. An open still in
  progress is abandoned (section 1). Switching the lead off does the same for opens in progress;
  its workers already open keep their relation, unusable until it is switched on again.
- **The cap** of four counts workers still open, the ones opening included, unchanged otherwise.
  A worker that failed to open gives its place back.

## 7. The person's view

What onehand.md section 8 decided, as built. Contract work in the same pull request: the rail,
the composer and the docks in `docs/screens.md`, one line for the new region in DESIGN.md's
*Layout*, and the `ui` skill's checklist answered.

- **The rail** leaves out a session that is a worker of an open lead. The lead's row takes the
  most urgent state of itself and its workers, by `SessionSignal`'s own order, and the *need
  attention* count counts workers. When the lead goes, its workers are listed again under their
  project.
- **The deck**, not a strip: a card tucked behind the lead's composer, narrower than it, its
  top edge showing above the composer's. Folded, that edge reads: the number of workers, one
  count per state that has any, most urgent first, each with its mark and ink, and at the end the
  most urgent worker's name and how long it has stood so. Pressed, it slides up over the
  transcript to a row per worker (its mark, name, state word and age, turn, background count;
  under them its latest activity, or the question it waits on, in the warning ink), and slides
  back when pressed again or on Esc. A row's action: *Stop* while it runs. Pressing a row opens
  the region at that worker. The slide is the motion DESIGN.md allows a tucked card,
  built once in `crates/ui/src/tucked.rs`, the one file `guards.rs`'s `nothing_animates` lets
  animate.
- **The region** is a panel of its own in the right dock, beside the Workbench and never with
  it. Its header button sits before the Workbench's, filled while the region is open, present
  only while the lead on screen has workers. Its strip, as tall as the header: the lead's name,
  maximize, hide. With the lead on screen changing, it shows the new lead's workers, or closes
  when that one has none.
- **A block** per worker, stacked, at most four, so no further cap. Its head: state mark, name,
  state word, *held by person* while the person holds it, *Stop* while a turn runs, `⋯` (*Give back
  to the lead*, *Close*). A folded block: the transcript's tail at a bounded height, kept at its
  end, and the oldest pinned card. The open block (one at most, the first one opened by default):
  the whole transcript, the pinned card, and a compact composer with Send, Stop and Queue as the
  main one has, no `+` menu.
- **Every state**: connecting (the head's spinner, no body yet), failed and lost (the failure's
  words in the body, *Send the last prompt again* in `⋯` as on the rail), closed by the person
  (the block goes).
- A worker's `ChatSession` is mounted in the region only; the agent pane never shows it while
  its lead is open.
- **Focus**, every way it moves; the region and the deck track their own focus, as a dock panel
  does, so their keys reach only them:
  - opening or folding the deck with the pointer leaves the caret where it was, in the lead's
    composer as a rule; a keymap command, *Show the workers*, opens it with the keyboard and puts
    focus on its first row; `↑`/`↓` move along the rows, `Enter` opens the row's worker in the
    region, `Esc` folds the deck and gives the caret back to the lead's composer;
  - a worker's block opened from the deck or the strip takes the caret into its compact composer;
  - **a block leaving the region while it holds focus** (the person closes the worker, it is
    closed from elsewhere, its lead goes and it returns to the rail) hands the caret to the
    lead's composer **before** the block leaves the frame, as `ChatPane::reclaim_focus` does for
    a closing panel, or the whole keymap goes with it;
  - **the lead on screen changing** while focus is in the region gives the caret to the composer
    of the session now shown, first, then the region shows the new lead's workers or closes;
  - **the Workbench taking the region's place**, or the region the Workbench's, moves focus out
    of the one leaving before it goes, to the lead's composer; the one arriving takes no focus.
- **Height**: the open block keeps a minimum, `WORKER_OPEN_MIN`, about ten rows of transcript and
  its composer, in rems so zoom scales it. When the folded blocks, the pinned cards and the open
  block's minimum do not fit, the folded blocks drop their tails first and keep their heads and
  pinned cards; when even that does not fit (a low dock, a large zoom, several cards), the stack
  of blocks scrolls as a whole, the open block keeping its minimum and its own scroll inside.

## 8. What is not done

- Nothing is written to disk: no handle, relation, request id or transient root survives a
  restart. A handle from an earlier run is refused as such.
- No tool answers a card, a permission or an approval.
- No `close_session`, no model per turn, no mode named by the lead, no outside client owning a
  worker, no cutting of the worktree.
- The commands a worker runs and its network access stay in the agent's own settings. Claude's
  adapter reads them from the user's, the project's and the local settings; Codex's from its own
  config.

## Required cases

| Case | Covered by |
|---|---|
| A restart mid-stage | nothing survives; a handle from the old run is refused; the lead opens anew (the skill's case 5) |
| A lost answer to a send | `send_prompt` once per turn number |
| A lost answer to an open | the `request` id |
| An old worker that may still run | a worktree reserved in this run is refused; any process of the user working in it, or one that could not be read, refuses the open; `activity: none reported` never counts as stopped |
| The person steps in | `held by: person`; `send_prompt` and `cancel_turn` refused until *Give back* in the worker's block |
| A finished worker holding a place | the cap's refusal names the four; closing is the person's |
| A turn that fails at once | `turn: failed: <the failure's words>` |
| A stale approval, a fix at approval | the skill's files; nothing here |

## Pull requests

Each one is buildable and testable alone, carries its tests, updates control.md for what it
builds, and keeps files under the size the repository allows (`crates/app/src/control.rs` is near
it: the workers' tools go in `control/workers.rs`).

1. **The worktree and its folders**: `project` as a worktree's top level, the canonical checks
   with git, the reservation, the reading of every process of the user, `dirs` canonicalized,
   `SessionOpts::dirs` into `additionalDirectories`, the warm adapter skipped. Tests: the worktree
   rule from a main checkout, a linked worktree and a folder inside one, a relative common dir;
   the folder rule through a symlink; the process reading's three answers over a fake process
   table; a second open on a reserved worktree; `session/new` params with and without folders.
2. **The handle and the open**: the run id, the handle, waiting for the worker, the mode and the
   model before the first prompt, `OPEN_DEADLINE`, the lead checked again at each step,
   `request`, `turn`, cleanup by session, the outside client refused the new inputs. Tests:
   request dedup and refusals; a model not offered leaves nothing behind (with the mock agents);
   an agent that never answers a pick fails at the deadline and a late answer is dropped; a failed
   open beside a successful one on the same project leaves the other running; a lead closed or
   switched off mid-open leaves nothing and sends nothing.
3. **`session_state`**: the core rule over the chat's facts, the tool. Tests: each line from a
   chat in that state; `activity` naming a subagent and the agent working by itself; `unknown`
   after a disconnect, never `none reported`.
4. **`send_prompt` and `cancel_turn`**, with who holds a worker. Tests: once per turn, other text
   refused, refused while held, cancel refused while a card waits.
5. **The person's view** (section 7) with ***Give back to the lead***, the contract documents
   with it. Tests: which sessions the rail lists with a lead open and after it closes; the lead
   row's state; the region following the lead on screen; focus handed back before a focused
   block leaves, on a lead change and when the Workbench takes the region's place.
6. onehand.md's part leaves this folder, its rest moved into control.md and the screens'
   documents.
