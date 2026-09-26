# Unattended runs

**One small issue, one session, one pull request, nobody watching.**

An audit leaves behind more issues than anyone wants to work by hand, and the
obvious reflex — paste them all into one session — is the one thing that cannot
work: a session that holds twenty unrelated fixes runs out of context window
before it runs out of issues. So the unit of work is one issue, the unit of
context is one session, and the session is thrown away when the issue is done.
A run is worth having only because it is *disposable*.

This is a design, not a description: nothing below is built yet.

## The shape

```
tick (every N minutes, one process, one run at a time)
  └─ gh issue list --label <trigger>        → the first issue, or nothing
     └─ claim it: remove the trigger label + comment "onehand is on this"
        └─ git worktree add  ../<repo>-auto-issue-123   (never the user's checkout)
           └─ add it as a project root, mint a session on it, set the mode
              └─ one prompt: the issue, the rules, open the PR yourself
                 └─ watch: turn ended · parked an ask · adapter lost · timed out
                    └─ verdict: gh pr list --head <branch>  → PR or no PR
                       └─ close the session, drop the root, comment the outcome
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
| Mint a session on a root and spawn its adapter | `Shell::start_session` / `spawn_session`, the path `remote_open` takes |
| Push a prompt in with no composer | `Shell::remote_prompt` → `ChatPane::remote_prompt` (already answers sent / queued / refused-with-a-reason) |
| Know a turn settled | `ChatEvent::TurnEnded`, emitted by the session pump |
| Know the agent parked a question | `ChatEvent::AwaitingUser(UserAsk)` |
| Know the adapter died | `ChatEvent::Disconnected`, and `Chat::link` |
| Watch a session from outside the window tree | `App::subscribe` on the `Entity<ChatSession>` |
| Cancel a turn | `Chat::cancel_turn` |
| Stop asking for permission | `Chat::set_mode` → `AcpRequest::SetMode` |
| An isolated checkout | `worktree::add_blocking`, `worktree_dir_in`, `validate_branch`, `slug` |
| Find a window that holds a root | the walk `remote::ask_windows` does |
| End the agent and write the transcript | `ChatPane::close` (the transcript is already written at every turn end) |
| Talk to GitHub | the `gh` CLI, shelled out blocking like `gitstat` and `worktree` already do |
| Config that is off until asked for | `[remote.telegram]`, copied wholesale as a shape |

Two consequences worth stating. **No new dependency**: `gh` carries the auth, the
API version and the JSON, so there is no HTTP client, no token in the config and
no OAuth dance. And **no scheduler engine**: one repeating timer on the
background executor, one interval in the config, no cron expressions.

## What is new

| File | Roughly | What |
|---|---|---|
| `crates/core/src/unattended.rs` | ~250 incl. tests | `Issue`, the four `gh` calls, `prompt_for`, `Outcome` + its sentence, `parse_every` |
| `crates/app/src/unattended.rs` | ~300 | the tick, the live `Run`, the worktree, the subscription, the timeout, the teardown |
| `crates/core/src/config.rs` | ~40 | `UnattendedConfig` |
| `crates/app/src/shell.rs` | ~50 | `run_unattended` (add root → mint → hand back) and `end_unattended` (close → drop root) |
| `crates/app/src/chat/pane.rs` | ~5 | one `pub` accessor for `Entity<ChatSession>` by uid |
| `crates/app/src/state.rs`, `shell::boot` | ~10 | the field on `Shared` and the boot call |

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

## Config

```toml
[unattended]
enabled = false          # off unless asked for
label = "auto"           # the trigger label; empty picks nothing
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

## The five rules that do not get simplified

1. **A parked ask ends the run. It is never answered.** If the agent asks for
   permission or asks a question, the run cancels the turn, closes the session
   and comments the question on the issue. Auto-granting is the one shortcut that
   turns "unattended" into "an agent with no supervision and full rights", and
   it is also the honest signal that the issue was not small: an issue that needs
   a decision needs a person. The mode set at the start is what keeps this rare;
   this is what happens when it is not enough.
2. **Never the user's checkout.** Every run is a fresh `git worktree` beside the
   repository, on a branch of its own. An agent writing to the tree somebody is
   working in, while they are working in it, is not a risk worth the ten lines
   this saves — and `worktree::add_blocking` already exists.
3. **Claim by removing the trigger label, before the prompt goes in.** That is
   the whole of the state: no local queue file, no sqlite, nothing to reconcile
   after a crash. A run interrupted by a quit or a panic leaves an issue with no
   trigger label and a comment saying a run started, which is a state a human can
   read and re-arm. It also makes a loop impossible by construction — the label
   that would cause a second pick is gone before any work begins.
4. **One wall-clock timeout per run, and it cancels.** An agent that neither
   finishes nor asks is the expensive failure, and it is the one nobody is
   watching for. The timeout is per run rather than per turn because a turn that
   ends is progress and a run is what is being bounded.
5. **The verdict is a PR, not the agent's word.** "I opened a pull request" is a
   sentence in a transcript. `gh pr list --head <branch>` is a fact, and it is
   one blocking call. The comment on the issue says which of the two happened.

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
the handshake installs. The pump emits on every event, so the run tries on each
one and gives up after a connect timeout of its own.

## Outcome

```
Opened(url)   → "onehand opened <url>."
NoPr          → "The turn ended with no pull request on <branch>."
Asked(q)      → "onehand stopped: it needs a decision. <q>"
LinkLost      → "The agent stopped answering."
TimedOut      → "No pull request after <timeout>; the run was cancelled."
```

All five are commented on the issue, and all five leave the trigger label off.
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
  second set of four functions, not an abstraction to write first.
- **Issue triage.** Whether an issue is small enough is the label, set by a
  human. The app does not judge.

## The checks worth writing

Core, pure, no fixtures:

- `parse_every` — `"30m"`, `"2h"`, `"90s"`, and a refusal for `"soon"`.
- `prompt_for` — the issue number and branch appear; the body is not truncated
  into the middle of a code fence.
- an empty `label` yields no candidate issue, and a missing `enabled` reads false.
- `Outcome` → sentence, one case each, so a fifth outcome cannot be added without
  a sentence.
- the branch name a run derives passes `validate_branch` for a title that is
  nothing but punctuation, and for one that is 300 characters long.

The `gh` calls themselves are not unit-tested; they are four `Command`
invocations whose failure is a string that gets commented, the same shape
`worktree::add_blocking` already has.

## Open questions

1. **Which root, when several are open and all carry the label?** Proposed: the
   first in the workspace's display order that has a candidate issue, so pinning
   a project is also how it gets worked first.
2. **Should a run be allowed while the user is at the keyboard?** Proposed: yes.
   The worktree is isolated, so "away" is about announcements rather than about
   safety, and a queue that only moves when nobody is looking is a queue that
   never moves.
3. **Base branch.** Proposed: whatever HEAD is at claim time, since
   `worktree::add_blocking` already branches off it. A `base = "main"` key is one
   more knob for a case nobody has hit yet.
