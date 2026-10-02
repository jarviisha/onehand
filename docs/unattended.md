# Unattended runs

**One small issue, one branch, one pull request ready for review, nobody watching.**

An audit leaves behind more issues than anyone wants to work by hand, and the
obvious reflex — paste them all into one session — is the one thing that cannot
work: a session that holds twenty unrelated fixes runs out of context window
before it runs out of issues. So the unit of work is one issue, the unit of
context is one session, and a session is thrown away when its part is done.
A run may take several sessions — the first, a repair when its checks fail, an
answer to a review — but each starts clean, from the branch and the issue, and
none depends on what an earlier one remembered.

This describes what is built. Where the build settled something the design left
open, the section says so.

## The shape

```
tick (every N minutes, one process, one run started while none is working)
  └─ the project's own issues, then its forge's (gh issue list --label <trigger> --author @me)
     → the first labelled issue, or nothing
     └─ claim it: remove the trigger label + a comment (a note, for one kept in onehand)
        └─ with a forge:    git fetch; git worktree add -b <branch> ../<dir> origin/<default>
           without a forge: git worktree add -b <branch> ../<dir> <the branch checked out>
           └─ add it as a project root, mint a session on it *without showing it*, set the mode
              └─ first prompt: the issue, then the rules of the step the run is at
                 └─ watch: turn ended · adapter lost · timed out · taken over
                    (a parked ask waits for a person, and gives up the slot while it does)
                    └─ turn ended: read the branch, judge the step's gate
                       ├─ gate failed, turns left → say what, same session
                       ├─ Plan: an answer, nothing changed → post the plan on the issue
                       │   ├─ project approves plans → close the session, wait for a person
                       │   │   (Approve → Implement, Revise… → Plan, each in a new session)
                       │   └─ otherwise → Implement, same session
                       ├─ Implement: committed, clean → the project's check command
                       │   (Verify: onehand runs it; a failure goes back to the agent)
                       │   └─ none, or it passed → Open PR, same session
                       ├─ no forge, commits → the outcome is the commits
                       └─ Open PR: pushed to an open PR → close the session, wait on its checks
                          └─ each tick: gh pr list --head <branch> with its check rollup
                             ├─ pending → wait
                             ├─ failed or conflicting → a repair: a new session
                             └─ all passed (or none ran) → gh pr ready → Ready
                                └─ tell the issue the outcome, drop the run's file
                                   (taken over, or picked by hand: the session and root stay)
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
| Talk to GitHub | the `gh` CLI, shelled out blocking like `gitstat` and `worktree` already do, behind `onehand_core::connector::Connector` in `plugins/builtin/connector-github` |
| Config that is off until asked for | `[remote.telegram]`, copied wholesale as a shape |

Two consequences worth stating. **No new dependency**: `gh` carries the auth, the
API version and the JSON, so there is no HTTP client, no token in the config and
no OAuth dance. And **no scheduler engine**: one repeating timer on the
background executor, one interval in the config, no cron expressions.

## Where the code is

| File | What |
|---|---|
| `crates/core/src/unattended.rs` | `Issue`, the claim and search over a connector, `branch_for`, `prompt_for`, `claim_comment`, `Ending` and `report`, `parse_every`, `target_dir` |
| `crates/core/src/unattended/run.rs` | `Facts` and `after_turn`, `carry_on`, `after_checks`, `Start`, `Progress`, and the run's file (`Record`) |
| `crates/core/src/connector.rs` | `Connector`, what a run asks of the system a project lives on, and `serving`, which picks the first one that takes a project |
| `plugins/builtin/connector-github/src/lib.rs` | the `gh` calls, the `origin` and ssh-alias check, and the account check |
| `crates/core/src/process.rs` | `output_within`: a command with a limit on its exit *and* its output, stopped with its whole process group |
| `crates/app/src/unattended.rs` | the tick, the live `Run`, the parked runs, the look at their checks, and carrying on after a restart |
| `crates/app/src/unattended/launch.rs` | claiming an issue, cutting its worktree and starting the run |
| `crates/app/src/unattended/turn.rs` | the subscription, what follows a turn, parking, the timeout, the wind-down, the teardown |
| `crates/core/src/config.rs` | `UnattendedConfig` |
| `crates/core/src/workspace.rs` | `ProjectRoot::transient`, left out by `to_config`, and `add_transient_root`, which adds one with its session without selecting it |
| `crates/core/src/worktree.rs` | `branch_off_blocking`, a new branch from a named start, and `fetch_blocking`; `worktree add` and the fetch both bounded and never prompting |
| `crates/app/src/shell/remote_runs.rs` | `run_unattended`, `end_unattended`, `adopt_unattended` |
| `crates/app/src/shell/roots.rs` | `forget_root`, the half of `remove_root` that asks nothing and moves nothing |
| `crates/app/src/chat/pane/sessions.rs` | `open_unshown` |
| `crates/app/src/chat/viewport.rs` | `reading` |

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
- **claim** — removing the trigger label and commenting that a run started.
- **attempt** — one go at a run, from a claim to an outcome. Putting the label
  back starts another attempt on the same branch and pull request.
- **repair** — a session started to fix checks that failed on the pull
  request's head, or a conflict with its base.
- **outcome** — how a run ended, one of a closed set, always commented.
- **take over** — a person acting inside a run's session. The run stops watching
  and the session becomes an ordinary one.

## Config

```toml
[unattended]
label = "auto"           # the trigger label (the default); empty picks nothing
every = "30m"            # how often to look
timeout = "45m"          # a run that neither finishes nor asks is cancelled
mode = "auto"            # the ACP session mode; also chosen in Settings ▸ Workspace
agent = "Claude Code"    # which agent spec; the default agent when unset
turns = 3                # turns of one session that may fail their step's gate
repairs = 2              # repairs one attempt may start on failing checks
```

**The switch is per project, and there is none in this table.** Every project
starts switched off, and a run looks only in the projects somebody switched on —
from the project's ••• menu (on the rail and on the project page), or from the
list of switches in Settings ▸ Workspace. Kept in the workspace file by path,
like a pin. A feature that starts an agent writing to a repository on the
strength of a file nobody edited is not a default anybody chose, and neither is
one that reaches every repository the user happens to have open. A file still
carrying the old global `enabled` key keeps loading; the key is ignored. An
**empty `label` still picks nothing at all**, and the project's hover and the
Settings list both say so, since a switch that is on while nothing can happen is
the one state that looks exactly like working.

**Two more per-project settings gate a run's steps**, from the same two places
the switch is, beside it: *Approve plans before work*, a checked entry in both
project menus and a switch in Settings ▸ Workspace, and a *Check command*, a
field under the same switch. Both are kept in the workspace file by path
(`approve_plans`, `checks`) and both are read at the moment a run needs them,
so a change counts from the next plan or the next check. Neither needs the
project switched on: a run picked by hand is gated the same way. A project
with no check command skips Verify.

**Two places an issue can live, and a project with no forge is still worked.**
An issue is either on the project's forge — GitHub today, through its connector
— or kept in onehand, in the project's Issues tab (`onehand_core::issues`), and
a run carries which as a `Tracker`: that is where it is claimed and where the
outcome is told, as a comment on the forge or a note on the issue. The project's
own issues are searched before its forge's, since they are the ones written for
onehand to work. **Where the work goes is a separate question** — a run also
carries the forge its pull request goes to, if any. An issue kept in onehand in
a project on GitHub still ends in a pull request, which does **not** reference
`#N`: that number is onehand's, and on the forge it names some other issue. A
project **no connector serves** — no `origin`, or a remote elsewhere — is worked
on its own issues only: the worktree is cut from the branch checked out (a
detached HEAD is refused), the agent is told to commit and not to push, and the
verdict is the commits past that start. What refuses a project is having nothing
to work — no forge and a workspace that keeps no issues — or not being a git
repository.

**A synced project is searched through the sync, and only there.** When a
project's issues are kept in step with its forge (the pause and resume control in the Issues tab's footer), a
run looks at the project's own issues alone — the forge's are already among
them — syncing first so a label added on the forge is seen. The claim takes the
label off here and the sync takes it off the forge; every note is also a comment
on the forge's issue, except the one naming the run's session (`turn::name_session`),
which only an issue kept here has anywhere to keep and which means nothing on the forge; the pull request references the forge's number. An issue
brought in from the forge is taken only if the forge says the user wrote it,
the rule the forge's own search keeps.

**Said when the forge cannot be reached.** A project is served by GitHub when
its `origin` is on github.com. That is read locally, before anything asks
GitHub, so a project somewhere else costs nothing per tick. An ssh remote is
judged by the host ssh would reach (`ssh -G`, which connects to nothing), not by
the word in the URL. The host there can be an alias from `~/.ssh/config`, such
as `git@github-work:me/repo`, which is how one machine keeps two GitHub accounts
apart, and reading the alias refused every such project. `gh` is asked who it
is signed in as. Anything that stops a switched-on project from being worked —
`gh` missing, signed out or unanswered on a project GitHub serves, or nothing to
work at all — is kept per project and shown on its row, in the warning ink with the reason on hover. It is
never left on stderr, where a switch that is on while nothing can happen would
look exactly like one that is working. Settings ▸ Connections shows the
signed-in line for each connector and a *Check again*. A project is looked at when it is
switched on, when its window opens, and on every tick — a run in progress
included, since a tick that only looked when it was about to search left the rows
as stale as the run was long. A tick with nothing switched on asks GitHub
nothing, so the feature costs nobody who has not turned it on. Another forge is a
second implementation of `Connector`, listed in `plugins::connectors`, and is not
built yet.

**A config that cannot work is said, not only printed.** An empty label, an
interval that does not parse, or a mode the agent does not offer stops every run.
The reason is kept, and every switched-on row and Settings show it in the warning
ink, while the projects and `gh` are still looked at. It used to leave the state
unset, so a bad interval read on screen as a missing label and the GitHub line
waited for an answer that was never going to come.

**A run can be picked by hand, and started now.** A project's ••• menu (on the
rail and on the project page) offers *Work an issue…* on any repository that is
not a run's own worktree. It lists every open issue — anybody's, not only yours,
since a person reading the list is the check the automatic search stands in for
— with who opened each one on its row, because the body is handed to the agent
word for word. The list holds the newest 100 and says so when it was cut.
Picking one runs the same path as a found issue, with four differences:
- the claim takes the trigger label off only if the issue carries it, and its
  comment says to pick the issue again to retry, since re-adding a label means
  nothing for an issue the search would never take;
- the session is put on screen as it starts, in the window it was picked from,
  so the person who picked it is reading it as it works;
- when it ends, the session stays where it is and its project is kept for good,
  as a taken-over one is, since somebody who watched it end may carry on in it;
- a mode the agent does not offer, once learned, refuses the pick *before* the
  claim, rather than claiming an issue for a run that would fail at its prompt.

**A run can also be started from the Issues mode's *Work*.** *In a new
worktree…* with *Work in steps* checked is a run picked by hand on that issue,
the same path as *Work an issue…*, so the branch, the claim, the timeout and
everything after are a run's. The modal's box is then *Extra instructions*:
whatever is typed there goes into every step's prompt, and is kept in the run's
file so a resumed run still has it. A refusal (a run is working, the issue is
busy, the mode is not offered) is said in a notification.

A run is started only while none is working, for picked and found alike, and a
pick while one is working is refused with the issue it is waiting on. A run
waiting on a card does not count. Beside *Check again* in Settings,
*Look now* runs the search at once instead of at the next tick, and always says
what came of it: nothing switched on, a run already going, what blocks every
run, or that no issue of yours carries the label.

**The transcript is the run's log**, in short lines, one fact each. A remark in
the transcript is one line down the middle of the column, cut where the column
ends, so a sentence carrying the issue, the folder and the branch lost all but
its start. A run opens on which issue and how it was chosen, then the branch and
what it was cut from, then the worktree's folder. It says when the mode is set
and the issue sent, and when a turn is cancelled and why. It ends on one line:
the pull request if one was opened, or why there is none. The issue comment
carries the full account. A found run's session is taken down when it ends, so
its last line is only read by somebody who opened it in time; the issue comment
is the record that stays.

**The rail says what is switched on and what is running.** A switched-on project
row carries a pill reading `auto`, `auto · #N · <Step>` while a run is working
issue N of that project, `auto · #N waiting` while that run waits on a card, and
`auto · #N · plan` while its plan waits to be approved — a working run is named
ahead of a waiting one on the same project. The run's own session is on a worktree's row of its own, so
without the pill the project the issue belongs to would say nothing about it.
The run's session shows every step in a strip under its header, the ones behind
it checked off and the one it is at in full ink, and the workspace page's
*Working* row names the step after the project.

**Only issues you opened.** The issue body goes into the prompt word for word,
and the agent it goes to may run `git` and `gh` with your credentials. Anybody
with triage rights can put a label on an issue, and on a public repository
anybody at all can write the issue it goes on — so the label alone would let a
stranger's text drive an agent holding your token. The tick therefore lists
`--author @me` only. An `authors` key is the way to widen it when an audit bot
files the issues; not added until one does.

**No `repo` key.** `gh` reads the repository out of the directory it runs in, so
the switched-on projects *are* the list — there is no second one to keep in step.
Being open in the rail used to be the whole of it, which made every open project
fair game; a label as common as `auto` then meant a repository somebody else owns,
where you had filed an issue carrying that label for another reason, could be
claimed, commented on and pushed to.

**`mode` is the adapter's id, not ours.** Modes come back from `session/new` and
are the adapter's to name, so the config names one rather than the app mapping a
word of its own onto a moving set. An id the agent does not offer is complained
about once and the run does not start — a run that silently fell back to the
mode that asks questions would park on the first write.

**`auto` is the default, because a run has nobody to ask.** Under it Claude
judges each command itself and stops only the ones that look risky, so a run
gets through its build, its tests, `git` and `gh` without waiting on anybody.
The default used to be `acceptEdits`, which covers file edits and nothing else:
Claude Code still asked before every command, so with rule 1 below every run
stopped at the first check it tried to run, in any repository whose allowlist
was thin. What `auto` gives up is a list that can be read ahead of time — what
a run may do is decided command by command — which is why the search still
takes only issues the user wrote, since the issue body is the prompt.
`bypassPermissions` is still not the recommendation: under it rule 1 never
fires, and "unattended" becomes "unsupervised with full rights". Setting
`mode = "acceptEdits"` puts back the old contract, where the **repository's own
permission allowlist** (`.claude/settings.json`) is what a run may execute and
a command outside it waits for a person. An agent that offers no `auto` mode
stops the feature as any missing mode does, and says which modes it offers.
**Where an allowlist applies, the project list is not the whole grant**: the adapter also reads the user's
own `~/.claude/settings.json`, so what a run may do is the union of the two.
That is stated rather than worked around — a second, app-supplied list would be
one more place a permission can hide — and it is why the project list should be
a deliberate one, not a list of whatever was once approved by hand.

## The seven rules that do not get simplified

1. **A parked ask waits for a person. The run never answers it.** If the agent
   asks for permission or asks a question, the card stays up and is announced
   like any other — on the desktop, and to a remote chat following the session —
   and the run waits. Auto-granting is the one shortcut that turns "unattended"
   into "an agent with no supervision and full rights"; the mode set at the
   start is what keeps asks rare, and this is what happens when it is not
   enough. **Answering is not taking over**: the card is a request the adapter
   is waiting on inside the turn, so an answer from anywhere simply lets the
   turn carry on, and a card answers once, so nothing can resume twice.
   **A waiting run gives up the slot** — it keeps its issue and its session,
   and the search looks for the next issue at once — so one unanswered question
   does not stop every other issue from being worked. When its card is
   answered it carries on beside whatever started meanwhile: the turn is
   already under way, and the only way to hold it back would be to cancel the
   work the answer was for. An adapter lost or a
   session closed while waiting ends the run on the question, which is put on
   the issue; re-adding the label is the retry.
   This rule used to end the run: it cancelled the turn and commented the
   question, on the reasoning that an issue needing a decision is not small.
   What that cost was the whole run over one click, and the person able to
   make it can do so from wherever they are.
2. **Never the user's checkout.** Every run is a fresh `git worktree` beside the
   repository, on a branch of its own. An agent writing to the tree somebody is
   working in, while they are working in it, is not a risk worth the ten lines
   this saves — and `worktree::add_blocking` already exists. It is branched off
   the remote's default branch and never off the user's HEAD; see *Base branch*.
3. **Claim by removing the trigger label, before the prompt goes in.** A run
   interrupted by a quit or a panic between the claim and its first file leaves
   an issue with no trigger label and the claim comment, which is written to
   read correctly when orphaned: *"onehand started an unattended run on this
   issue. If no outcome follows, the run was interrupted — re-add `<label>` to
   retry."* The comment's own timestamp says when, so the sentence carries no
   time. It never says a run *is* happening, since that is the one sentence a
   crash makes false. It also makes a loop impossible by construction — the
   label that would cause a second pick is gone before any work begins.
   **Past the claim, a run keeps one small file** (`unattended::Record`, under
   `<config_dir>/onehand/runs/`): the phase it is in, the step, the plan and
   a person's note on it, and what it has used of its budget, nothing else.
   A restart resumes at the start of the step; a file from before steps
   existed reads as Implement. The branch, its commits, the pull request and its
   checks are read again whenever they are needed, so the file cannot disagree
   with them, and the forge is always asked before anything is pushed or opened.
   A restart reads every file back: a run waiting on checks goes on waiting, and
   one that was working gets a new session on its worktree once a window holds
   its project, told to look at what is already there. The file goes when the
   issue has been told the outcome. This used to be "no file at all"; it changed
   once a run outlived one turn, and a restart lost the run.
4. **One timeout per attempt, counting working time, and it cancels.** An agent
   that neither finishes nor asks is the expensive failure, and it is the one
   nobody is watching for. The timeout is per run rather than per turn because a
   turn that ends is progress and a run is what is being bounded. **Time spent
   waiting on a card does not count** (`unattended::Budget`): a run standing
   still because nobody has answered yet is not the failure the timeout is for,
   and one that ran through the wait would cancel a run for a slow reply.
5. **The verdict is a PR, not the agent's word.** "I opened a pull request" is a
   sentence in a transcript. `gh pr list --head <branch>` is a fact, and it is
   one blocking call, which also carries the checks on the head it reports.
   The comment on the issue says which of the two happened.
   **It is asked on every ending**, not only a turn ending: an agent can open the
   PR and then time out, park or lose its adapter while tidying up, and a
   comment saying "no pull request" beside a pull request is the worst answer
   available. A PR found makes the outcome `Opened`, with why the run stopped as
   a note under it.
6. **A turn ending is an observation, not an outcome, and each step has its
   own gate.** An attempt goes Plan → Implement → Verify → Open PR
   (`unattended::Step`), in one session; a repair starts at Implement. After
   each turn the branch is read — commits, work no commit holds, the pull
   request and the commit it is at — and `next` judges the step:
   - **Plan** passes on a non-empty answer that changed nothing, measured from
     where the session found the worktree. The plan is posted on the issue
     and kept in the run's file. A project that approves plans then parks the
     run with no session (`Phase::AwaitingApproval`), giving up the slot,
     until the Issues panel's *Approve plan* or *Revise…*; a wait longer than
     the `timeout` ends it as `Exhausted`.
   - **Implement** passes on a clean worktree and a commit made since the step
     started, so the commits an earlier attempt left, or a session cut short by
     a restart, do not pass it for a turn that only read the code. A repair
     counts from the base instead: it is told to leave the code alone when a
     failure is not its change's. The agent is told not to push.
   - **Verify** is onehand running the project's check command itself
     (`verify_blocking`: `sh -c`, fifteen minutes, the shared target
     directory). A failure goes back to the same session with the last 200
     lines of its output.
   - **Open PR** passes when everything is pushed to an open pull request,
     which closes the session and gives up the slot while the checks run.
     With no forge there is no Open PR: commits are the outcome.

   A failed gate sends the same session back with a prompt naming what is
   missing (`carry_on`). **Only a failed gate costs a turn**, since four steps
   each take one; after `turns` of them the run stops as `Exhausted`, naming
   the step it was stuck at. Each step change is noted on an issue kept in
   onehand, and in the transcript; a forge's issue hears only the plan and
   the outcome. What the agent said about its work is never consulted, so "I
   opened a pull request" with none on the branch is sent back to open one. The prompt still tells the
   agent to ask through its question tool rather than in prose, and an ending
   with no pull request still carries the end of its answer
   (`Chat::answer_tail`), which is where a question asked in prose would be.
7. **A person acting in the session takes it over.** The run's session is in the
   rail and can be opened and typed into. The moment a prompt comes from
   anywhere but the run itself — the composer, or
   a chat on the remote bridge that `/use`d it, since which channel it came
   through changes nothing about who is now driving — the run stops watching:
   no cancel, no close, and comments `TakenOver`. From then on it is an ordinary
   session, and whatever it produces is that person's, not the run's. **Its root
   is saved into the workspace at that moment**, the one write a run's root ever
   gets: a root kept off disk was transient because the run would drop it, and
   the run no longer will — left unsaved, the work somebody just took on would
   vanish from the rail at the next launch.

## Checks, repairs and reviews

**A pull request is ready when its checks pass on its head.** Each tick looks at
every parked run's pull request, checks included (`after_checks`). Every check
must pass, required or not; one cancelled or timed out counts as failed, since
none of those is evidence the change works. Pending checks wait, up to the
`timeout`, then the run stops as `Exhausted`. **A pull request with no checks at
all** is ready too, and the issue says *no checks ran* — but only after
`CHECKS_GRACE`, since a forge registers its checks a little after the push and
a look straight after it would read "not yet" as "none". Ready takes the pull
request out of draft (`gh pr ready`), which is what asks its reviewers.

**A failure starts a repair**, at the Implement step: a new session on the same worktree, whose prompt
carries the pull request, the failing checks' names and the end of each one's
log (`Connector::check_log_blocking`, the first three), and tells the agent to
say so and leave the code alone when the failure is not its change's. A
conflict with the base is repaired the same way. Failures are not classified
first: telling a broken test from a dead runner means reading the log, which
the agent does better. The bound is `repairs` per attempt, and **the same check
failing again right after a repair aimed at it stops the run** — that is the
agent being unable to fix it, or the failure not being its to fix, and another
round would only spend time. A due repair goes before any new issue — one a window can give a session to: a
run whose project no open window holds waits without holding up the search.

**A review is answered by putting the label back.** The next claim of an issue
looks for the newest branch under `onehand/issue-<N>` — found by its prefix,
so a title edited since still finds it. Like a fresh one, the attempt starts
with a plan. With an open pull request the attempt
answers its review: the prompt names the pull request and how to read the
review, and the work is counted from the commit the attempt starts at, so a
turn that adds nothing is sent back. With no pull request it carries on the
work there. With a merged one it starts over on a branch of its own. **A pull
request closed without being merged refuses the claim**: somebody decided
against it, and opening another would undo that.

**An issue a run is still on is not claimed again.** Its label stays on, so it
is taken once that run ends, and a pick by hand says what it is waiting on.

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

The fetch is the connector's (`Connector::fetch_blocking`). GitHub's retries a
failed fetch over HTTPS with `gh`'s own sign-in when `origin` is an ssh remote:
an app opened from the desktop often has no ssh agent to reach while `gh` is
already signed in. The HTTPS URL is built from `origin` itself, an ssh alias
resolved through `ssh -G`, never from `gh`'s default repository, which on a fork
can be upstream. The retry is gated on the remote and not on the kind of
failure — telling an auth failure from a dead network means matching git's
wording — so a network that is down costs both fetches' time. Only the fetch
falls back; the agent's own push still goes to `origin`.

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

Every step's prompt is built by `step_prompt`: the issue's number, title and
body, the branch it is on, what this attempt is for beyond the issue (`Start`:
carrying on an earlier attempt, answering a review, repairing checks), two
standing instructions — read the repository's own CLAUDE.md for conventions,
and decide what the code, the tests and the docs let it infer, listing the
assumptions that mattered; only a decision about what the product should do is
asked, through the agent's question tool — then the step's own rules:
- **Plan**: read the code and answer with a plan; do not edit, commit or push.
  A person's note on the last plan comes with that plan.
- **Implement**: follow the plan, which is included, run the repo's checks and
  commit; do not push.
- **Verify**: the end of the check command's output; fix it and commit. Only a
  session resumed at Verify gets this; within a session a failed check comes
  back as a `carry_on`.
- **Open PR**: push and open the draft pull request with `gh pr create
  --draft`, or push to the one already open.

A session moving on to the next step is sent that step's prompt. A failed gate
gets only `carry_on`'s one sentence about what is still missing.

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
until another mode is chosen in Settings ▸ Workspace, which lifts the pause at
once. **That first issue is spent**: modes are only known once the adapter is
up, which is after the claim, so the check cannot run before one is made. Its
comment names the mode and what was offered, and re-adding the label once the
mode is fixed is the retry.

**One mode for every session that works an issue.** The *Mode* picker in
Settings ▸ Workspace sets `unattended.mode`, live and in the config, and a
session started on an issue from its Issues tab takes it as well — there it is
set only when the agent offers it, since a person is at the window to answer
whatever the session's own mode asks. The picker offers Auto, Accept edits, Ask
and Bypass (`unattended::MODES`), with what Bypass gives away said beside it in
the warning ink; an id written into the config by hand shows as a choice of its
own.

## Outcome

How a run stopped is an `Ending`; the comment is `report(ending, pr, branch)`.
With a pull request found, every ending reads *"onehand opened <url>."*, with
why the run stopped as a note under it. Without one:

```
TurnEnded(tail) → "The turn ended with no pull request on <branch>. It ended on: <tail>"
Asked(q)        → "The run ended waiting on a decision nobody answered; there is no pull request on <branch>. <q>"
                  (its adapter was lost, or its session closed, while a card waited)
LinkLost        → "The agent stopped answering; there is no pull request on <branch>."
Closed          → "The run's session was closed before it finished; there is no pull request on <branch>."
TimedOut        → "No pull request after <timeout>; the run was cancelled."
TakenOver       → "Taken over by hand; the run stopped watching <branch>."
Failed(why)     → "onehand could not start the run: <why>"
Exhausted(why)  → "<why>. There is no pull request on <branch>; its work stays there."
                  (turns used up, at the step it was stuck at; repairs used up;
                  the same check failing again; checks still running after
                  the timeout; a plan nobody approved within the timeout)
```

With a pull request, two more: **`Ready`** — its checks passed on its latest
commit, or none ran, and it is marked ready for review — and
**`PullRequestGone`**, closed or merged while its checks were awaited.

`Failed` is the one the design did not have: everything between the claim and
the prompt — the default branch, the fetch, the worktree, a window to put the
session in, the mode — can refuse, and each of those is the issue's to hear
about, since the claim has already taken its label.

**A cancel winds down before the session closes.** A timeout cancels
the turn, and it is the turn ending that writes its transcript — closing the
session on the spot would lose the turn it was cancelling. So the run waits
for that turn to end, or thirty seconds, whichever is first.

**A pull request that could not be looked for is said as that.** A failed
`gh pr list` reads *"onehand could not tell whether a pull request was opened"*
and never as "no pull request" — which is a claim, and one nobody checked.

**What can reach a network cannot hang a run.** Every `gh` call is stopped after
a minute, and the fetch and `git worktree add` after five — the second because a
checkout can run hooks and fetch large files. The limit covers the output as
well as the exit, since a program can finish while something it started (an ssh
connection kept for reuse) still holds its pipe; the whole process group is
stopped with it. Standard input is closed, `gh` and `git` are told never to
prompt, and ssh runs in batch mode unless the user configured an ssh command of
their own. The quick local reads left — whether a branch exists, where the
repository's top is — are not bounded. A panic is caught too: before the claim
it is treated as nothing found, after it the issue is told. Any of these would
otherwise leave the one claim in flight forever, and no issue would be looked
for again until a restart.

**A session that vanishes settles the run at once.** Closing the window a run's
session lives in drops the session without a `Disconnected`; the run watches for
the release instead of waiting out its timeout and reporting the wrong ending.
It is reported as `Closed` — not as the agent having stopped answering — unless
a cancel was already winding down, in which case it keeps the ending it was
heading for.

**A prompt that beats the run's own is a take-over.** Somebody typing between
the adapter coming up and the run's prompt going out owns the session, and the
run settles as taken over rather than failing to send and closing it on them.
Answering a card is not: it is what the run is waiting for.

**A run's card reaches a remote chat only if that chat follows the session**,
as every card does — and a run's session is new, so by default nobody does.
The desktop notification is the one that always goes.

All of them are commented on the issue, all of them leave the trigger label
off, and all of them remove the run's file.
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

- **Concurrency.** One run working at a time; a tick during one does nothing.
  Runs waiting on a card do not count, and are not capped. Add when one run at
  a time is measurably the bottleneck, which it will not be while the
  issues are small.
- **Cron expressions, quiet hours, a calendar.** An interval and a switch per
  project. Add when somebody actually wants runs only at night.
- **Telegram announcements of a run.** The three announced moments are a closed
  set with no wildcard arm, so a fourth kind of news is a decision about what the
  badge, the desktop and the chat each do with it. The issue comment is the
  report for now. Add when the report is wanted on a phone.
- **Retrying a failed call by itself.** A forge that does not answer one look
  at the checks is looked at again next tick; anything else that fails ends the
  attempt and is commented. Putting the label back is the retry, and it carries
  on the same branch.
- **A run waiting on its checks, on screen.** A parked run has no session, so
  the rail shows nothing for it; the issue comment is where it ends. Add when
  waiting runs are wanted at a glance.
- **Running with the app closed.** A run pauses with the app and carries on
  when it is next opened. Add a worker of its own when runs are wanted overnight
  with the window shut.
- **An in-app queue view.** The rail shows the live run; GitHub shows the rest.
- **Anything but GitHub.** `gh` is the whole API layer. A second forge is a
  second set of five functions, not an abstraction to write first.
- **Issue triage.** Whether an issue is small enough is the label, set by a
  human. The app does not judge.

## The checks worth writing

Core, pure, no fixtures:

- `parse_every` — `"30m"`, `"2h"`, `"90s"`, and a refusal for `"soon"`.
- `step_prompt` — the issue number and branch appear; the body is not truncated
  into the middle of a code fence.
- an empty `label` yields no candidate issue; no project is switched on by
  default, and the switch survives the workspace file by path.
- `report`, one case per `Ending` with and without a PR, matched exhaustively so
  an ending cannot be added without a sentence being checked for it.
- a PR found on an ending that was not a turn ending still reads `Opened`.
- `next`: each step's gate — a plan that touched code or said nothing, a
  change with no commits, a claimed but missing pull request — with and
  without approval, a check command or a forge; `after_check`.
- `step_prompt`, one case per step; `verify_blocking` with a passing, a failing
  and a long command.
- `after_checks`: only every check passing is ready; no checks waits out the
  grace first; the same check failing again, no repairs left, or checks that
  never finish stop the run; a conflict is repaired.
- the run's file reads back as written, waiting for approval included; one
  from before steps reads as Implement; an unreadable one is said.
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
