# Unattended runs

**One small issue, one session, one pull request, nobody watching.**

An audit leaves behind more issues than anyone wants to work by hand, and the
obvious reflex — paste them all into one session — is the one thing that cannot
work: a session that holds twenty unrelated fixes runs out of context window
before it runs out of issues. So the unit of work is one issue, the unit of
context is one session, and the session is thrown away when the issue is done.
A run is worth having only because it is *disposable*.

This describes what is built. Where the build settled something the design left
open, the section says so.

## The shape

```
tick (every N minutes, one process, one run at a time)
  └─ gh issue list --label <trigger> --author @me   → the first issue, or nothing
     └─ claim it: remove the trigger label + comment "onehand started a run"
        └─ git fetch; git worktree add -b <branch> ../<repo>-auto-issue-123 origin/<default>
           └─ add it as a project root, mint a session on it *without showing it*, set the mode
              └─ one prompt: the issue, the rules, open the PR yourself
                 └─ watch: turn ended · parked an ask · adapter lost · timed out · taken over
                    └─ verdict: gh pr list --head <branch>  → PR or no PR, on every ending
                       └─ close the session, drop the root, comment the outcome
                          (except when taken over: the session and root stay)
```

Every arrow is a state a run can die in, and each one ends the same way: a
comment on the issue saying what happened, and the trigger label already gone so
nothing picks it up again.

## What it reuses

The feature is mostly wiring. Everything below already exists and is already
driven without a human at the keyboard by the remote bridge, which is the
precedent this follows in shape as well as in code.

| What a run needs | What answers it |
|---|---|
| Mint a session on a root and spawn its adapter | `Workspace::add_session` + the pane's connect — **not** `Shell::start_session`, see *A run does not take the window* |
| Push a prompt in with no composer | `Shell::remote_prompt` → `ChatPane::remote_prompt` (already answers sent / queued / refused-with-a-reason) |
| Know a turn settled | `ChatEvent::TurnEnded`, emitted by the session pump |
| Know the agent parked a question | `ChatEvent::AwaitingUser(UserAsk)` |
| Know the adapter died | `ChatEvent::Disconnected`, and `Chat::link` |
| Watch a session from outside the window tree | `App::subscribe` on the `Entity<ChatSession>` |
| Cancel a turn | `Chat::cancel_turn` |
| Stop asking for permission | `Chat::set_mode` → `AcpRequest::SetMode` |
| An isolated checkout | `worktree::add_blocking` (plus a start point, see *Base branch*), `worktree_dir_in`, `validate_branch`, `slug` |
| Find a window that holds a root | the walk over `Shared::windows` every remote path does; the window handle is kept for the teardown, the one step that needs a `Window` |
| End the agent and write the transcript | `ChatPane::close` (the transcript is already written at every turn end) |
| Talk to GitHub | the `gh` CLI, shelled out blocking like `gitstat` and `worktree` already do |
| Config that is off until asked for | `[remote.telegram]`, copied wholesale as a shape |

Two consequences worth stating. **No new dependency**: `gh` carries the auth, the
API version and the JSON, so there is no HTTP client, no token in the config and
no OAuth dance. And **no scheduler engine**: one repeating timer on the
background executor, one interval in the config, no cron expressions.

## Where the code is

| File | What |
|---|---|
| `crates/core/src/unattended.rs` | `Issue`, the `gh` calls, `branch_for`, `prompt_for`, `claim_comment`, `Ending` and `report`, `parse_every`, `target_dir` |
| `crates/app/src/unattended.rs` | the tick, the live `Run`, the subscription, the timeout, the wind-down, the teardown |
| `crates/core/src/config.rs` | `UnattendedConfig` |
| `crates/core/src/workspace.rs` | `ProjectRoot::transient`, left out by `to_config` |
| `crates/core/src/worktree.rs` | `branch_off_blocking` (a new branch from a named start) and `fetch_blocking` |
| `crates/app/src/shell.rs` | `run_unattended`, `end_unattended`, `adopt_unattended`, and `forget_root`, the half of `remove_root` that asks nothing and moves nothing |
| `crates/app/src/chat/pane.rs` | `open_unshown` and `reading` |

The split is the one the crate boundary already forces: everything that can be
decided without a window — which issue, what the prompt says, what the outcome
sentence is, how long `"30m"` is — is core's and is tested there; the app holds
the parts that need a `Window`, an entity or a timer.

## The name

`[unattended]`, `core::unattended`, `app::unattended`, and one piece of work is a
**run**. Not "queue": `Chat::queue` already means the single prompt waiting
behind a turn, and one word for two things inside one app is how two answers end
up swapped. Not "scheduler" either — what it schedules is one tick; the
interesting noun is the run.

The rest of the vocabulary, one meaning each:

- **tick** — one look for work, every `every`; does nothing while a run is live.
- **trigger label** — the label (`label`) whose presence on an issue asks for a
  run. Its removal is the claim.
- **claim** — removing the trigger label and commenting that a run started. The
  only state a run keeps.
- **outcome** — how a run ended, one of a closed set, always commented.
- **take over** — a person acting inside a run's session. The run stops watching
  and the session becomes an ordinary one.

## Config

```toml
[unattended]
enabled = false          # off unless asked for
label = "auto"           # the trigger label; empty (the default) picks nothing
every = "30m"            # how often to look
timeout = "45m"          # a run that neither finishes nor asks is cancelled
mode = "acceptEdits"     # the ACP session mode a run starts in
agent = "Claude Code"    # which agent spec; the default agent when unset
```

**Fail-closed twice, for the reason `allowed_chats` is.** `enabled = false` is
the default because a feature that starts an agent writing to a repository on
the strength of a file nobody edited is not a default anybody chose. And an
**empty `label` picks nothing at all** rather than picking every issue: the
failure of forgetting to fill it in has to be "nothing runs", not "everything
runs".

**Only issues you opened.** The issue body goes into the prompt word for word,
and the agent it goes to may run `git` and `gh` with your credentials. Anybody
with triage rights can put a label on an issue, and on a public repository
anybody at all can write the issue it goes on — so the label alone would let a
stranger's text drive an agent holding your token. The tick therefore lists
`--author @me` only. An `authors` key is the way to widen it when an audit bot
files the issues; not added until one does.

**No `repo` key.** The repositories are the project roots already open in the
app's windows, and `gh` reads the repository out of the directory it runs in.
Which repositories a run may touch is therefore a list the user already curates
by hand, in the rail, and there is no second list to keep in step with it. What
it costs is that every open root with the trigger label in its issues is fair
game, which is exactly what the label is for.

**`mode` is the adapter's id, not ours.** Modes come back from `session/new` and
are the adapter's to name, so the config names one rather than the app mapping a
word of its own onto a moving set. An id the agent does not offer is complained
about once and the run does not start — a run that silently fell back to the
mode that asks questions would park on the first write.

**`acceptEdits` covers file edits and nothing else.** Under it Claude Code still
asks before every command — `cargo test`, `git commit`, `gh pr create` — so with
rule 1 below a run would end at the first check it tried to run, which is every
run. What a run may execute is therefore the **repository's own permission
allowlist** (`.claude/settings.json`: the build, the tests, `git`, `gh`), which
the adapter reads on `session/new`. That keeps the list next to the code it
builds, reviewed like the code, and keeps rule 1 meaningful: a command outside
the list is exactly the thing that should stop a run nobody is watching.
`bypassPermissions` would also work and is not the recommendation — under it
rule 1 never fires, and "unattended" becomes "unsupervised with full rights".
A root with no allowlist is not refused up front; its first run ends on the
first command with the command quoted on the issue, which says what to add.
**The project list is not the whole grant**: the adapter also reads the user's
own `~/.claude/settings.json`, so what a run may do is the union of the two.
That is stated rather than worked around — a second, app-supplied list would be
one more place a permission can hide — and it is why the project list should be
a deliberate one, not a list of whatever was once approved by hand.

## The seven rules that do not get simplified

1. **A parked ask ends the run. It is never answered.** If the agent asks for
   permission or asks a question, the run cancels the turn, closes the session
   and comments the question on the issue. Auto-granting is the one shortcut that
   turns "unattended" into "an agent with no supervision and full rights", and
   it is also the honest signal that the issue was not small: an issue that needs
   a decision needs a person. The mode set at the start is what keeps this rare;
   this is what happens when it is not enough. **The one exception is a person
   already there**: if the ask parks while the user is reading that very
   conversation — `Attention::Reading`, the rule that already decides whether a
   parked ask goes to the desktop — the run is taken over (rule 7) instead of
   cancelled, and the ask stays up for them. "Never answered" means never
   answered *by the run*; a person looking at the card is the person it asks.
2. **Never the user's checkout.** Every run is a fresh `git worktree` beside the
   repository, on a branch of its own. An agent writing to the tree somebody is
   working in, while they are working in it, is not a risk worth the ten lines
   this saves — and `worktree::add_blocking` already exists. It is branched off
   the remote's default branch and never off the user's HEAD; see *Base branch*.
3. **Claim by removing the trigger label, before the prompt goes in.** That is
   the whole of the state: no local queue file, no sqlite, nothing to reconcile
   after a crash. A run interrupted by a quit or a panic leaves an issue with no
   trigger label and the claim comment, which is written to read correctly when
   orphaned: *"onehand started an unattended run on this issue. If no outcome
   follows, the run was interrupted — re-add `<label>` to retry."* The comment's
   own timestamp says when, so the sentence carries no time. It never says a run *is*
   happening, since that is the one sentence a crash makes false. It also makes a loop impossible by construction — the label
   that would cause a second pick is gone before any work begins.
4. **One wall-clock timeout per run, and it cancels.** An agent that neither
   finishes nor asks is the expensive failure, and it is the one nobody is
   watching for. The timeout is per run rather than per turn because a turn that
   ends is progress and a run is what is being bounded.
5. **The verdict is a PR, not the agent's word.** "I opened a pull request" is a
   sentence in a transcript. `gh pr list --head <branch>` is a fact, and it is
   one blocking call. The comment on the issue says which of the two happened.
   **It is asked on every ending**, not only a turn ending: an agent can open the
   PR and then time out, park or lose its adapter while tidying up, and a
   comment saying "no pull request" beside a pull request is the worst answer
   available. A PR found makes the outcome `Opened`, with why the run stopped as
   a note under it.
6. **A run is one turn.** The first `TurnEnded` ends it. An agent that stops to
   ask in prose ("should I do A or B?") rather than through a parked ask slips
   past rule 1, so a `NoPr` comment carries the end of the agent's answer
   (`Chat::answer_tail`) — which is where that question is — and a person
   reading the issue sees it without opening the transcript.
7. **A person acting in the session takes it over.** The run's session is in the
   rail and can be opened, typed into, or have its ask answered. The moment a
   prompt or an answer comes from anywhere but the run itself — the composer, or
   a chat on the remote bridge that `/use`d it, since which channel it came
   through changes nothing about who is now driving — the run stops watching:
   no cancel, no close, and comments `TakenOver`. From then on it is an ordinary
   session, and whatever it produces is that person's, not the run's. **Its root
   is saved into the workspace at that moment**, the one write a run's root ever
   gets: a root kept off disk was transient because the run would drop it, and
   the run no longer will — left unsaved, the work somebody just took on would
   vanish from the rail at the next launch.

## A run does not take the window

`Shell::start_session` cannot be reused as it stands, and this is the one piece
of the feature that is new architecture rather than wiring. It mints on the
*active* root and then calls `show_active_session`, because sessions connect
lazily — showing a session is what spawns its adapter. Driven from a tick, that
means every run selects the worktree in the rail and swaps the conversation the
user is reading for one they did not start, mid-sentence if they were typing.

So a run needs the other half of lazy connect: **a session minted on a named root,
connected, and not shown.** That is smaller than it sounds: `ChatPane::connect`
spawns the adapter and subscribes to it without a `Window` — it is
`show_active_session` that needs one, and only to put the session on screen. So
the shell mints onto the run's root by index without selecting it, and the pane
connects that uid directly. The
lazy rule itself stays as it is for everything else — it exists so a workspace of
a dozen roots does not launch a dozen agents at boot, and a run is one agent that
was asked for.

The run's root is added **by path and marked transient**, which
`Workspace::to_config` leaves out. `Shell::add_root` opens a folder picker and
saves; a run's root is transient, and a crash between add and drop would
otherwise leave a worktree of an issue in the workspace for good — and marking
the root rather than skipping one save is what keeps any *other* save in the
meantime from writing it.

Dropping it is `Shell::forget_root`, the half of `remove_root` that asks nothing
and moves nothing. `remove_root` itself asks a person twice before losing live
sessions — the run has already decided — and puts the active session back on
screen afterwards, which takes the caret out of whatever the user was typing in.
Only when the run's own project was the one being looked at is anything shown in
its place.

## Base branch

A worktree is branched off **`origin/<default branch>`, fetched at claim time**,
and never off whatever the user's checkout has at HEAD. The user is usually on a
feature branch; a run branched off it opens a PR against `main` carrying every
commit of that branch, which is a PR nobody can review and nobody asked for.
`worktree::add_blocking` gains a start point to say so, and the default branch is
`gh repo view --json defaultBranchRef`, one more of the blocking `gh` calls. A
`base` key in the config is still not added: the default branch is the answer
the repository already gives.

## Disk

Each worktree is a cold build, and the worktrees are kept on purpose (see
*Outcome*), so a `target/` per run is gigabytes per issue with nothing that ever
cleans it. Runs therefore build into **one shared `CARGO_TARGET_DIR`** under the
user's cache directory (`onehand/unattended-target`), set in the adapter's
environment for runs only. That also turns the second run's build from cold to
incremental, which is most of what the timeout would otherwise be spent on.

It is set by starting the run's adapter through `env` rather than by threading
an environment down through the ACP spawn: the adapter is the one process a run
starts, and everything the agent runs inherits from it. The cost is that it is
POSIX-only, which is marked where it is done.

## The prompt

One prompt per run, built by `prompt_for(&Issue)`: the issue's number, title and
body, the branch it is on, and four instructions — read the repository's own
CLAUDE.md for conventions, run the repo's checks before committing, open the pull
request with `gh pr create`, and if the issue turns out to need a decision, say
so and stop rather than guessing.

It deliberately does **not** restate the commit convention, the test commands or
the PR format. Those are in the repository's own instructions, which the agent
reads anyway, and a second copy in a template is a copy that goes stale silently.

The prompt is sent on the first session event that shows a live adapter, not
immediately: `Chat::set_mode` and `Chat::submit` both need the request channel
the handshake installs. The modes arrive ahead of that event, so the mode is
checked against what is offered at the same moment. There is no separate
connect timeout: an adapter that never comes up is a run that never finishes,
and the run's own timeout already bounds that.

**A mode the agent does not offer pauses the feature**, not just the run. Every
later run would fail the same way on a fresh issue, each one spending a claim
to say so, so the first says it on the issue and on stderr and the tick stops
until the config is fixed and the app restarted.

## Outcome

How a run stopped is an `Ending`; the comment is `report(ending, pr, branch)`.
With a pull request found, every ending reads *"onehand opened <url>."*, with
why the run stopped as a note under it. Without one:

```
TurnEnded(tail) → "The turn ended with no pull request on <branch>. It ended on: <tail>"
Asked(q)        → "onehand stopped: it needs a decision. <q>"
LinkLost        → "The agent stopped answering; there is no pull request on <branch>."
TimedOut        → "No pull request after <timeout>; the run was cancelled."
TakenOver       → "Taken over by hand; the run stopped watching <branch>."
Failed(why)     → "onehand could not start the run: <why>"
```

`Failed` is the one the design did not have: everything between the claim and
the prompt — the default branch, the fetch, the worktree, a window to put the
session in, the mode — can refuse, and each of those is the issue's to hear
about, since the claim has already taken its label.

**A cancel winds down before the session closes.** An ask or a timeout cancels
the turn, and it is the turn ending that writes its transcript — closing the
session on the spot would lose the one turn the run was about. So the run waits
for that turn to end, or thirty seconds, whichever is first.

**A card a run is about to cancel is not announced.** The pane would otherwise
send a desktop notification for a parked ask nobody is looking at, which is
every ask a run sees — pointing somebody at a question that is gone by the time
they arrive.

All of them are commented on the issue, and all of them leave the trigger label
off.
Re-arming is a human putting the label back, which is the same gesture as asking
for the run in the first place. The worktree and its branch are **left on disk**
in every case: a run that got half way has work in it, and removing a worktree to
save the user a `git worktree remove` is the app throwing away work nobody asked
it to.

The transcript needs no special handling — it is written at the end of every
turn, under the conversations directory, exactly like a conversation somebody
had by hand.

## Where it lives

On `Shared`, one per process, for the reason the remote bridge is there: two
windows each running a tick would be two agents on one issue. The tick therefore
has no window, and finds one the way every remote path does — ask each window
whether it holds the root, and the one that does answers.

While a run is live the rail shows it: a project root for the worktree, with one
session under it carrying the ordinary signal marks. That is deliberate — an
unattended agent that cannot be seen or stopped is worse than no unattended
agent — and the root is dropped when the run ends, so the rail does not
accumulate one row per issue ever worked.

## Not built, on purpose

- **Concurrency.** One run at a time; a tick during a run does nothing. Add when
  one run at a time is measurably the bottleneck, which it will not be while the
  issues are small.
- **Cron expressions, quiet hours, a calendar.** An interval and an `enabled`
  flag. Add when somebody actually wants runs only at night.
- **Telegram announcements of a run.** The three announced moments are a closed
  set with no wildcard arm, so a fourth kind of news is a decision about what the
  badge, the desktop and the chat each do with it. The issue comment is the
  report for now. Add when the report is wanted on a phone.
- **Retry.** A failed run is commented and left. Add when the same issue is seen
  to fail transiently.
- **An in-app queue view.** The rail shows the live run; GitHub shows the rest.
- **Anything but GitHub.** `gh` is the whole API layer. A second forge is a
  second set of five functions, not an abstraction to write first.
- **Issue triage.** Whether an issue is small enough is the label, set by a
  human. The app does not judge.

## The checks worth writing

Core, pure, no fixtures:

- `parse_every` — `"30m"`, `"2h"`, `"90s"`, and a refusal for `"soon"`.
- `prompt_for` — the issue number and branch appear; the body is not truncated
  into the middle of a code fence.
- an empty `label` yields no candidate issue, and a missing `enabled` reads false.
- `report`, one case per `Ending` with and without a PR, matched exhaustively so
  an ending cannot be added without a sentence being checked for it.
- a PR found on an ending that was not a turn ending still reads `Opened`.
- the branch name a run derives passes `validate_branch` for a title that is
  nothing but punctuation, and for one that is 300 characters long.

The `gh` calls themselves are not unit-tested; they are `Command`
invocations whose failure is a string that gets commented, the same shape
`worktree::add_blocking` already has.

## Settled

- **Which root goes first** when several open roots have a candidate issue: the
  first in the workspace's display order. Pinning a project is already how a
  user says it matters more, so it is also how it gets worked first — and it
  needs no state, where taking turns between roots would.
- **A run is allowed while the user is at the keyboard.** The worktree is
  isolated, the run's session is never shown unasked (see *A run does not take
  the window*), and a person can take it over, so "away" is about announcements
  rather than about safety. A queue that only moves when nobody is looking is a
  queue that never moves.
