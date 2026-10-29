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
- Otherwise `project` must be an absolute path of a **worktree of a repository a window project
  belongs to**: `git rev-parse --git-common-dir` in it equals that of some window project. Read on
  the background executor, never in the call's handler. Anything else is refused, naming why
  (not a folder, not a git worktree, a repository no project of this window belongs to).
- A worktree opens as a transient root through `Shell::run_unattended`, selecting nothing and
  taking no focus. A worktree that is already a root of the window is used as it is.
- **A worktree with other processes working in it is refused**, read with
  `worktree::removal::working_in_blocking`, leaving out onehand's own process and the sessions
  and terminals it holds there. The refusal is the lead's answer: it names each process (its pid
  and command) and says the person has to stop them, and nothing is opened. This is required
  case 4 on onehand's side: after a crash an old worker may still run, and no second one opens on
  its worktree until the person stops it.

### What the worker may write

`session/new` gets ACP's `additionalDirectories` (`Conn::new_session`,
`crates/core/src/acp/client/handshake.rs`, through a new `SessionOpts::dirs`):

- the repository's common `.git`, when the session opens on a worktree;
- each of `dirs`, which must be absolute and existing, and none of: `/`, the home folder, the
  worktree, a folder inside it, or a folder holding it or the repository. The first that fails
  refuses the call, naming it and why.

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

1. the cap and the arguments are checked; a place under the cap is taken;
2. the session starts; the call waits for `Link::Connected`, bounded by the handshake's own
   timeout (`HANDSHAKE_TIMEOUT`, `crates/core/src/acp/client.rs`);
3. the mode and the model are picked, each answered;
4. `prompt` is submitted. A blank `prompt` is refused at step 1, so after step 3 the only blocker
   left is a link lost, which is a failure.

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

**A failure at any step leaves nothing behind**: the session is ended, a root the open added is
dropped with `Shell::end_unattended` (which uses `Shell::forget_root`), the place under the cap
and the `request` id are freed, and the prompt is never sent. The answer says which step failed
and why, in the agent's words where it gave any.

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
| `background` | `none`, or `<n> running: <names>` | `Chat::background` |
| `held by` | `lead`, `person` | below |
| `turns` | `sent <n>, ended <m>` | the relation |
| `mode`, `model`, `dirs` | as the open answered | the session |

`ended <m>` is the last turn the lead sent that is over: `m = n` once the turn after the lead's
last send has ended and the person has not prompted since. **A turn over is not a stage done**;
the lead reads the worker's `status.md` for that. A worker is **confirmed stopped for this
session** when `turn` is not `running`, `waiting` and `background` are `none`.

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
  owned by nobody, and go back into the rail; they are the person's to close.
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
| An old worker that may still run | a worktree with other processes in it is refused |
| The person steps in | `held by: person`; `send_prompt` and `cancel_turn` refused until *Give back* in the worker's block |
| A finished worker holding a place | the cap's refusal names the four; closing is the person's |
| A turn that fails at once | `turn: failed: <the failure's words>` |
| A stale approval, a fix at approval | the skill's files; nothing here |

## Pull requests

Each one is buildable and testable alone, carries its tests, updates control.md for what it
builds, and keeps files under the size the repository allows (`crates/app/src/control.rs` is near
it: the workers' tools go in `control/workers.rs`).

1. **The worktree and its folders**: `project` as a worktree, the check with git, the process
   check, `dirs`, `SessionOpts::dirs` into `additionalDirectories`, the warm adapter skipped.
   Tests: the worktree rule and the folder rule in core; `session/new` params with and without
   folders.
2. **The handle and the open**: the run id, the handle, waiting for the worker, the mode and the
   model before the first prompt, `request`, `turn`, cleanup on failure, the outside client
   refused the new inputs. Tests: request dedup and refusals; a model not offered leaves nothing
   behind (with the mock agents).
3. **`session_state`**: the core rule over the chat's facts, the tool. Tests: each line from a
   chat in that state.
4. **`send_prompt` and `cancel_turn`**, with who holds a worker. Tests: once per turn, other text
   refused, refused while held, cancel refused while a card waits.
5. **The person's view** (section 7) with ***Give back to the lead***, the contract documents
   with it. Tests: which sessions the rail lists with a lead open and after it closes; the lead
   row's state; the region following the lead on screen.
6. onehand.md's part leaves this folder, its rest moved into control.md and the screens'
   documents.
