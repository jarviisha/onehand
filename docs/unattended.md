# Unattended runs

**One small issue, one task, one session, one branch, nobody watching.**

An audit leaves behind more issues than anyone wants to work by hand, and the
obvious reflex — paste them all into one session — is the one thing that cannot
work: a session that holds twenty unrelated fixes runs out of context window
before it runs out of issues. So the unit of work is one issue, the unit of
context is one session, and the session is thrown away when the issue is done.
A run is worth having only because it is *disposable*.

An issue is worked as a **task** of a workflow, driven like every other task.
What is particular to an unattended run is how the issue is found and claimed,
where its work is cut, and what the issue is told at the end. On a project a
forge serves, the run goes on past its branch: onehand pushes the commit the
check passed on, opens a draft pull request and waits for its status checks,
repairing a failing one. Putting the label back on an issue whose pull request
is open answers its review.

This describes what is built. Where the build settled something the design left
open, the section says so.

## The shape

```
tick (every N minutes, one process): send the reports not yet delivered, then,
while a slot is free under at_once,
  └─ the project's own issues, then its forge's (gh issue list --label <trigger> --author @me)
     → the first labelled issue, or nothing
     └─ claim it: remove the trigger label + a comment (a note, for one kept in onehand)
        └─ with a forge:    git fetch; git worktree add -b <branch> ../<dir> origin/<default>
           without a forge: git worktree add -b <branch> ../<dir> <the branch checked out>
           └─ a task of the [unattended] workflow, its brief the issue; ask for its place
              └─ its place given: a transient project and a session on it, *not shown*;
                 the agent comes up and is put in the run's mode
                 └─ the workflow's steps (builtin:issue: Plan → Implement → Verify →
                    Push → Pull request → Status checks; without a forge the last three pass)
                    (a card or an approval waits for a person, and status checks for the
                    forge; each gives up the slot)
                    (a failing status check or a conflict goes back to Implement)
                    └─ the run ends with an outcome: done · stopped · exhausted · failed
                       └─ verdict: the forge's pull request on the branch, else its
                          commits past where it was cut
                          └─ drop the project, keep the report in the task, send it
                             (picked by hand or taken over: the project stays)
```

Every arrow is a state a run can die in, and each one ends the same way: a
report on the issue saying what happened, and the trigger label already gone so
nothing picks it up again.

## What it reuses

The feature is mostly wiring. A run is a task, so everything a task already
does — steps, gates, the timeout, Stop, Resume, Retry, the Tasks page — a run
gets without code of its own.

| What a run needs | What answers it |
|---|---|
| Run the steps and judge each turn by its gates | the workflow engine (`onehand_core::workflow::Run`) and its one driver (`crate::task::driver`) |
| Keep the run, list it, stop, resume and retry it | a task (`onehand_core::task::Task`, `Source::Issue`) on the Tasks page |
| One task at a time in a checkout | `crate::task::request` and its queue |
| A session on a named project, connected and not shown | `Shell::run_unattended` → `Workspace::add_transient_root` + `ChatPane::open_unshown` — **not** `Shell::start_session`, see *A run does not take the window* |
| Stop asking for permission | `Setup::mode`, set by the driver through `Chat::set_mode` when the agent first comes up |
| An isolated checkout | `worktree::branch_off_blocking` (plus a start point, see *Base branch*), `worktree_dir`, `validate_branch` |
| Find a window that holds a project | the walk over `Shared::windows` every remote path does |
| Talk to GitHub | the `gh` CLI, shelled out blocking like `gitstat` and `worktree` already do, behind `onehand_core::connector::Connector` in `plugins/builtin/connector-github` |
| Config that is off until asked for | `[remote.telegram]`, copied wholesale as a shape |

Two consequences worth stating. **No new dependency**: `gh` carries the auth, the
API version and the JSON, so there is no HTTP client, no token in the config and
no OAuth dance. And **no scheduler engine**: one repeating timer on the
background executor, one interval in the config, no cron expressions.

## Where the code is

| File | What |
|---|---|
| `crates/core/src/unattended.rs` | `Issue`, the claim and search over a connector, `branch_for`, `brief_for`, `claim_comment`, `IssueSource` and `TrackerRef` (what a task keeps of its issue), `room`, `Budget`, `Verdict`, `report`, `could_not_start`, `parse_every`, `target_dir` |
| `crates/core/src/workflow/builtin/issue.toml` | *Work an issue*, the workflow runs use by default |
| `crates/core/src/connector.rs` | `Connector`, what a run asks of the system a project lives on, and `serving`, which picks the first one that takes a project |
| `plugins/builtin/connector-github/src/lib.rs` | the `gh` calls, the `origin` and ssh-alias check, and the account check |
| `crates/core/src/process.rs` | `output_within`: a command with a limit on its exit *and* its output, stopped with its whole process group |
| `crates/core/src/task/history.rs` | `over_cap`, which never lets go of a task whose report is unsent |
| `crates/app/src/unattended.rs` | the tick, the cap (`at_cap`, the tasks still `starting`), `waiting` |
| `crates/app/src/unattended/launch.rs` | claiming an issue, cutting its worktree, and making it a task |
| `crates/app/src/unattended/report.rs` | `spec_for`, `refuse_mode`, `started`, `opening`, and the end: `keep`, `card_question`, `ended`, `deliver`, `deliver_all` |
| `crates/app/src/task.rs` | `freed`, which calls `unattended::ended`; `issues_working`, `live_issues`, `update_issue`, `undelivered` |
| `crates/app/src/task/driver.rs` | `came_up` (the mode, the note on the issue), the clock, the take-over |
| `crates/app/src/shell/workflows.rs` | `drive_task`, which brings an issue's run up off screen |
| `crates/core/src/config.rs` | `UnattendedConfig` |
| `crates/core/src/workspace.rs` | `ProjectRoot::transient`, left out by `to_config`, and `add_transient_root`, which adds one with its session without selecting it |
| `crates/core/src/worktree.rs` | `branch_off_blocking`, a new branch from a named start, and `fetch_blocking`; `worktree add` and the fetch both bounded and never prompting |
| `crates/app/src/shell/remote_runs.rs` | `run_unattended`, `end_unattended`, `adopt_unattended` |
| `crates/app/src/shell/roots.rs` | `forget_root`, the half of `remove_root` that asks nothing and moves nothing |
| `crates/app/src/chat/pane/sessions.rs` | `open_unshown` |

The split is the one the crate boundary already forces: everything that can be
decided without a window — which issue, what branch, what the run is asked, what
the report says, how long `"30m"` is — is core's and is tested there; the app
holds the parts that need a `Window`, an entity or a timer.

## The name

`[unattended]`, `core::unattended`, `app::unattended`, and one issue worked is a
**run** of a task. Not "queue": the task queue already means tasks waiting for
their place, and one word for two things inside one app is how two answers end
up swapped. Not "scheduler" either — what it schedules is one tick; the
interesting noun is the run.

The rest of the vocabulary, one meaning each:

- **tick** — one look for work, every `every`; it sends the reports not yet
  delivered, and starts nothing while every slot is taken.
- **slot** — one of `at_once` places, across every window, a working run holds.
- **trigger label** — the label (`label`) whose presence on an issue asks for a
  run. Its removal is the claim.
- **claim** — removing the trigger label and commenting that a run started.
- **report** — what the issue is told when a run ends: the verdict, the outcome,
  and what the last step ended on.
- **take over** — a person acting inside a run's session. The run stops and the
  session becomes an ordinary one.

## Config

```toml
[unattended]
label = "auto"              # the trigger label (the default); empty picks nothing
every = "30m"               # how often to look
timeout = "45m"             # how long a run may work, put over the workflow's own
mode = "acceptEdits"        # the ACP session mode a run starts in; empty leaves the agent's own
agent = "Claude Code"       # which agent spec; the default agent when unset
workflow = "builtin:issue"  # the workflow an issue is worked with, by id
at_once = 1                 # runs working at once, across every window
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

**`workflow` is any workflow, by its id.** *Work an issue* (`builtin:issue`)
plans in an answer, changes and commits the code, then runs the project's check
command, sending a failure back to the change. A workflow that is not there, or
one that would not pass validation, stops every run, said the way a bad config
is (below), and so does having no agent to run it. One that runs the project's
check command is never claimed for on a project with none: that project's row
says so, a pick there is refused, and no issue is claimed for a run that cannot
start. A run whose session cannot start (its folder gone, its agent gone) ends
failed, and its issue is told it could not start rather than left claimed.

**Two places an issue can live, and a project with no forge is still worked.**
An issue is either on the project's forge — GitHub today, through its connector
— or kept in onehand, in the project's Issues tab (`onehand_core::issues`), and
a run carries which as a `Tracker`: that is where it is claimed and where the
report goes, as a comment on the forge or a note on the issue. The project's
own issues are searched before its forge's, since they are the ones written for
onehand to work. **Where the work goes is a separate question** — a run also
carries the forge its verdict looks for a pull request on, if any. A project
**no connector serves** — no `origin`, or a remote elsewhere — is worked on its
own issues only, with the worktree cut from the branch checked out (a detached
HEAD is refused). What refuses a project is having nothing to work — no forge
and a workspace that keeps no issues — or not being a git repository.

**The branch says where the issue lives.** It is
`onehand/<tracker>-<number>-<title words>`: `onehand/github-57-…` for an issue
on the forge, `onehand/local-3-…` for one kept in onehand, and one kept in step
with a forge goes by the forge's name and number. A number alone names nothing,
and an issue kept here and one on the forge can both be 3; two issues must never
share a branch or a worktree.

**A synced project is searched through the sync, and only there.** When a
project's issues are kept in step with its forge (the pause and resume control
in the Issues tab's footer), a run looks at the project's own issues alone — the
forge's are already among them — syncing first so a label added on the forge is
seen. The claim takes the label off here and the sync takes it off the forge;
every note is also a comment on the forge's issue, except the one naming the
run's session (`unattended::started`), which only an issue kept here has
anywhere to keep and which means nothing on the forge. An issue brought in from
the forge is taken only if the forge says the user wrote it, the rule the
forge's own search keeps.

**Said when the forge cannot be reached.** A project is served by GitHub when
its `origin` is on github.com. That is read locally, before anything asks
GitHub, so a project somewhere else costs nothing per tick. An ssh remote is
judged by the host ssh would reach (`ssh -G`, which connects to nothing), not by
the word in the URL. The host there can be an alias from `~/.ssh/config`, such
as `git@github-work:me/repo`, which is how one machine keeps two GitHub accounts
apart, and reading the alias refused every such project. `gh` is asked who it
is signed in as. Anything that stops a switched-on project from being worked —
`gh` missing, signed out or unanswered on a project GitHub serves, or nothing to
work at all — is kept per project and shown on its row, in the warning ink with
the reason on hover. It is never left on stderr, where a switch that is on while
nothing can happen would look exactly like one that is working. Settings ▸
Connections shows the signed-in line for each connector and a *Check again*. A
project is looked at when it is switched on, when its window opens, and on every
tick — a run in progress included. A tick with nothing switched on asks GitHub
nothing, so the feature costs nobody who has not turned it on. Another forge is
a second implementation of `Connector`, listed in `plugins::connectors`, and is
not built yet.

**A config that cannot work is said, not only printed.** An empty label, an
interval that does not parse, a workflow that cannot run, or a mode the agent
does not offer stops every run. The reason is kept, and every switched-on row
and Settings show it in the warning ink, while the projects and `gh` are still
looked at.

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
- what stops every run — a mode the agent does not offer, once learned, or a
  workflow that cannot run — refuses the pick *before* the claim, rather than
  claiming an issue for a run that would fail.

**`at_once` runs work at a time**, picked and found alike, counted across every
window (`task::issues_working`: issue tasks running or queued). A run waiting on
a card or an approval does not count, and starting to wait looks for the next
issue at once (`unattended::waiting`) rather than at the next tick. A pick while
the cap is reached is refused with the issues being worked, and so is a Resume
or a Retry of an issue's task from the Tasks page (`unattended::over_cap`, asked
in `task::request`, which every start goes through); a refused Retry drops the
run it was about to start. Beside *Check
again* in Settings, *Look now* runs the search at once and always says what came
of it: nothing switched on, the cap reached, what blocks every run, or that no
issue of yours carries the label.

**The transcript is the run's log**, in short lines, one fact each. A remark in
the transcript is one line down the middle of the column, cut where the column
ends. A run opens on which issue and how it was chosen, then the branch and what
it was cut from, then the worktree's folder (`unattended::opening`). The driver
then says when the mode is set, each step, each command, and how the run ended.
A found run's project is dropped when it ends, so its last line is only read by
somebody who opened it in time; the report on the issue is the record that
stays, beside the task in the Tasks page.

**A run is an ordinary task on the Tasks page**, named after its issue and its
workflow (`#57 · Work an issue`): Stop, Resume, Retry, Dismiss and the task
detail all work on it. A Resume or a Retry brings it up off screen the same way
(`Shell::drive_task` → `run_unattended`), and its issue is told again when that
run ends.

**The rail says what is switched on and what is running.** A switched-on project
row carries a pill reading `auto`, `auto · #N` while a run is working issue N of
that project, and `auto · #N waiting` while that run waits on a person — a
working run is named ahead of a waiting one on the same project. The run's own
session is on a worktree's row of its own, so without the pill the project the
issue belongs to would say nothing about it. Both read `task::live_issues`, the
issue tasks with a session under way.

**Only issues you opened.** The issue body goes into the brief word for word,
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
claimed, commented on and worked.

**`mode` is the adapter's id, not ours.** Modes come back from `session/new` and
are the adapter's to name, so the config names one rather than the app mapping a
word of its own onto a moving set. It is the run's `Setup::mode`, and the driver
puts the agent in it when the agent first comes up, before the first prompt. An
id the agent does not offer fails the run — a run that silently fell back to the
mode that asks questions would park on the first write.

**`acceptEdits` covers file edits and nothing else.** Under it Claude Code still
asks before every command — `cargo test`, `git commit` — so with rule 1 below
every run would stop at the first check it tried to run and wait for somebody
to allow it. What a run may execute is therefore the **repository's own
permission allowlist** (`.claude/settings.json`: the build, the tests, `git`,
`gh`), which the adapter reads on `session/new`. That keeps the list next to the
code it builds, reviewed like the code, and keeps rule 1 meaningful: a command
outside the list is exactly the thing that should wait for a person.
`bypassPermissions` would also work and is not the recommendation — under it
rule 1 never fires, and "unattended" becomes "unsupervised with full rights".
A project with no allowlist is not refused up front; its first run parks on the
first command and waits for somebody to allow it, which says what to add.
**The project list is not the whole grant**: the adapter also reads the user's
own `~/.claude/settings.json`, so what a run may do is the union of the two.
That is stated rather than worked around — a second, app-supplied list would be
one more place a permission can hide — and it is why the project list should be
a deliberate one, not a list of whatever was once approved by hand.

## The seven rules that do not get simplified

1. **A parked ask waits for a person. The run never answers it.** If the agent
   asks for permission or asks a question, the card stays up and is announced
   like any other — on the desktop, and to a remote chat following the session —
   and the run waits; so does an approval step. Auto-granting is the one
   shortcut that turns "unattended" into "an agent with no supervision and full
   rights"; the mode set at the start is what keeps asks rare, and this is what
   happens when it is not enough. **Answering is not taking over**: the card is
   a request the adapter is waiting on inside the turn, so an answer from
   anywhere simply lets the turn carry on. **A waiting run gives up the slot** —
   it keeps its issue and its session, and the search looks for the next issue
   at once — so one unanswered question does not stop every other issue from
   being worked. When its card is answered it carries on beside whatever started
   meanwhile: the turn is already under way, and the only way to hold it back
   would be to cancel the work the answer was for. An adapter lost or a session
   closed while waiting ends the run, and the report says so.
2. **Never the user's checkout.** Every run is a fresh `git worktree` beside the
   repository, on a branch of its own, cut by the claim whatever the workflow's
   place says. An agent writing to the tree somebody is working in, while they
   are working in it, is not a risk worth anything this would save. It is
   branched off the remote's default branch and never off the user's HEAD; see
   *Base branch*.
3. **Claim by removing the trigger label, before the task is made.** The label
   that would cause a second pick is gone before any work begins, so a loop is
   impossible by construction. From then on the task's file is the run's state:
   a run cut off by a quit or a panic is a task cut off on the Tasks page, to be
   resumed or retried like any other. A claim that never became a task leaves
   an issue with no trigger label and the claim comment, which is written to
   read correctly when orphaned: *"onehand started an unattended run on this
   issue. If no outcome follows, the run was interrupted — re-add `<label>` to
   retry."* The comment's own timestamp says when, so the sentence carries no
   time. It never says a run *is* happening, since that is the one sentence a
   crash makes false.
4. **One timeout per run, counting working time, and it cancels.** An agent
   that neither finishes nor asks is the expensive failure, and it is the one
   nobody is watching for. `[unattended] timeout` is put over the workflow's
   own, and the driver keeps it per run rather than per turn, because a turn
   that ends is progress and a run is what is being bounded. **Time spent
   waiting on a person does not count** (`unattended::Budget`): a run standing
   still because nobody has answered yet is not the failure the timeout is for.
5. **The verdict is the forge's and git's, not the agent's word.** "I committed
   the fix" is a sentence in a transcript. The forge's pull request on the
   branch, or else `git rev-list --count <base>..HEAD` in the worktree, is a
   fact. **It is asked on every ending**, not only a finished one: a run that
   timed out may still have left commits, and a report about the timeout alone
   would hide them. A lookup that failed is said as a failure, never as
   "nothing".
6. **A run is a task of a workflow.** Its steps are judged by their gates, read
   from git and the transcript, so *Implement* ends only when the code changed
   and was committed, and a run that keeps missing ends exhausted rather than
   looping. The brief tells the agent to ask a decision through its question
   tool rather than in prose, so a question becomes a card that waits. One
   asked in prose anyway slips past rule 1, so a report on a run that did not
   finish quotes what its last step ended on, which is where that question is.
7. **A person acting in the session takes it over.** The run's session is in the
   rail and can be opened and typed into. The moment a prompt comes from
   anywhere but the driver — the composer, or a chat on the remote bridge that
   `/use`d it, since which channel it came through changes nothing about who is
   now driving — the run ends as taken over: no cancel, no close. From then on
   it is an ordinary session, and whatever it produces is that person's, not
   the run's. **Its project is saved into the workspace** (`adopt_unattended`),
   the one write a run's project ever gets: it was transient because the run
   would drop it, and the run no longer will — left unsaved, the work somebody
   just took on would vanish from the rail at the next launch.

## A run does not take the window

`Shell::start_session` cannot be reused as it stands. It mints on the *active*
project and then calls `show_active_session`, because sessions connect lazily —
showing a session is what spawns its adapter. Driven from a tick, that would
mean every run selects the worktree in the rail and swaps the conversation the
user is reading for one they did not start, mid-sentence if they were typing.

So when `Shell::drive_task` is handed an issue task whose worktree is not a
project in the window, it calls `run_unattended`: **a session minted on a named
project, connected, and not shown.** `ChatPane::connect` spawns the adapter and
subscribes to it without a `Window` — it is `show_active_session` that needs
one, and only to put the session on screen. The lazy rule itself stays as it is
for everything else — it exists so a workspace of a dozen projects does not
launch a dozen agents at boot, and a run is one agent that was asked for.

The run's project is added **by path and marked transient**, which
`Workspace::to_config` leaves out, so a crash between add and drop never leaves
a worktree of an issue in the workspace for good — and marking the project
rather than skipping one save is what keeps any *other* save in the meantime
from writing it.

Dropping it is `Shell::end_unattended` over `forget_root`, the half of
`remove_root` that asks nothing and moves nothing, and only for a project the
run added itself. `remove_root` asks a person twice before losing live sessions
— the run has already decided — and puts the active session back on screen
afterwards, which takes the caret out of whatever the user was typing in. Only
when the run's own project was the one being looked at is anything shown in its
place.

## Base branch

A worktree is branched off **`origin/<default branch>`, fetched at claim time**,
and never off whatever the user's checkout has at HEAD. The user is usually on a
feature branch; a run branched off it carries every commit of that branch, which
is work nobody can review as the issue's and nobody asked for. The default
branch is `gh repo view --json defaultBranchRef`, one more of the blocking `gh`
calls. A `base` key in the config is still not added: the default branch is the
answer the repository already gives.

The fetch is the connector's (`Connector::fetch_blocking`). GitHub's retries a
failed fetch over HTTPS with `gh`'s own sign-in when `origin` is an ssh remote:
an app opened from the desktop often has no ssh agent to reach while `gh` is
already signed in. The HTTPS URL is built from `origin` itself, an ssh alias
resolved through `ssh -G`, never from `gh`'s default repository, which on a fork
can be upstream. The retry is gated on the remote and not on the kind of
failure — telling an auth failure from a dead network means matching git's
wording — so a network that is down costs both fetches' time.

## Disk

Each worktree is a cold build, and the worktrees are kept on purpose (see
*The report*), so a `target/` per run is gigabytes per issue with nothing that
ever cleans it. Runs therefore build into **one shared `CARGO_TARGET_DIR`**
under the user's cache directory (`onehand/unattended-target`), set in the
adapter's environment for runs only (`unattended::spec_for`). That also turns
the second run's build from cold to incremental, which is most of what the
timeout would otherwise be spent on.

It is set by starting the run's adapter through `env` rather than by threading
an environment down through the ACP spawn: the adapter is the one process a run
starts, and everything the agent runs inherits from it. The cost is that it is
POSIX-only, which is marked where it is done.

## The brief

`brief_for(&Tracker, &Issue)` gives the run's brief: the issue's title and body
word for word, and instructions asked of every step that name the issue and say
nobody is watching, so a decision the issue needs is asked through the agent's
question tool and not guessed. Each step's prompt is the workflow's, with the
brief filled in; onehand adds what it adds to every step — where the work is (a
branch of its own, *do not push or open a pull request*: onehand does that), to
read the repository's own instructions, and the rule each gate checks.

It deliberately does **not** restate the commit convention or the test
commands. Those are in the repository's own instructions, which the agent reads
anyway, and a second copy here is a copy that goes stale silently.

The first prompt waits for the adapter to come up: `Chat::set_mode` and
`Chat::submit` both need the request channel the handshake installs. There is no
separate connect timeout: an adapter that never comes up is a run that never
finishes, and the run's own timeout already bounds that.

**A mode the agent does not offer pauses the feature**, not just the run
(`unattended::refuse_mode`). Every later run would fail the same way on a fresh
issue, each one spending a claim to say so, so the first fails and the tick
stops until the config is fixed and the app restarted. **That first issue is
spent**: modes are only known once the adapter is up, which is after the claim,
so the check cannot run before one is made. Its report names the mode and what
was offered, and re-adding the label once the config is fixed is the way to try
again.

## The report

A run's end is kept as a `PendingReport` the moment the driver ends it
(`unattended::keep`): the run, its outcome, whether it asked its agent anything,
what its last step ended on, and the question of a card still waiting. When its
task has given its place up and pinned its last mark, `unattended::ended` drops
the project unless it was picked by hand or taken over, and the report is sent.
The verdict is looked for as it is sent, and `report(pending, verdict, branch)`
is what the issue is told. It opens on the verdict —
*"onehand opened <url>."*, *"onehand left N commits on `<branch>`."*, *"onehand
left no commit on `<branch>`."*, or *"onehand could not tell what the run left
on `<branch>`: <why>"* — then one sentence per outcome:

```
Done               → "Every step of the workflow passed."
Stopped(ByPerson)  → "The run was stopped by hand."
Stopped(TakenOver) → "The run was taken over by hand, and stopped watching."
Stopped(TimedOut)  → "The run hit its timeout and was cancelled."
Stopped(LinkLost)  → "The agent stopped answering."
Stopped(Closed)    → "The run's session was closed before it finished."
Exhausted { step } → "The run stopped after too many misses at the <step> step."
Failed(why)        → "The run failed: <why>"
```

then the question of a card nobody answered, and, for any outcome but `Done`,
what the last step ended on, quoted. A run that failed before it asked its agent
anything (a mode the agent does not offer) is told only *"onehand could not start
the run: <why>"*. A run cut off by a quit and then dismissed is told it was cut
off and let go, and a queued task stopped before its run began is told it was
stopped by hand, so no claim is left with nothing after it. A claim that then
could not become a task is told `could_not_start` directly, best effort, since
there is no task to keep it in.

**The report is kept before it is sent.** It goes onto the task's
`IssueSource::unsent`, and the task's file is saved, before anything reaches the
network: even the verdict is looked for only when the report is sent. `deliver` sends the unsent reports oldest first and drops each only once
the issue has it; the first that fails stops the rest, so the issue never hears
them out of order. What is left is sent again at every tick, whether or not any
project is switched on, and at the next
start (`deliver_all`), and the history cap never removes a task whose report is
unsent. A forge that cannot be reached, or a quit, never loses one.

**What can reach a network cannot hang a run.** Every `gh` call is stopped after
a minute, and the fetch and `git worktree add` after five — the second because a
checkout can run hooks and fetch large files. The limit covers the output as
well as the exit, since a program can finish while something it started (an ssh
connection kept for reuse) still holds its pipe; the whole process group is
stopped with it. Standard input is closed, `gh` and `git` are told never to
prompt, and ssh runs in batch mode unless the user configured an ssh command of
their own. The quick local reads left — whether a branch exists, where the
repository's top is — are not bounded. A panic in the claim is caught too:
before the claim it is treated as nothing found, after it the issue is told.
Any of these would otherwise leave the one claim in flight forever, and no issue
would be looked for again until a restart.

**A run's card reaches a remote chat only if that chat follows the session**,
as every card does — and a run's session is new, so by default nobody does.
The desktop notification is the one that always goes.

Every report leaves the trigger label off. Trying again is a human putting the
label back, which is the same gesture as asking for the run in the first place,
or Retry on the Tasks page. The worktree and its branch are **left on disk** in
every case: a run that got half way has work in it, and removing a worktree to
save the user a `git worktree remove` is the app throwing away work nobody asked
it to.

The transcript needs no special handling — it is written at the end of every
turn, under the conversations directory, exactly like a conversation somebody
had by hand.

## Answering a review

Before it claims an issue, `launch::taking_blocking` looks for the issue's newest
task with a branch of its own. **One still working is never taken again**: the
search passes the issue over for the next labelled one
(`core::candidates_blocking`), and a pick is refused, so an issue is never worked
twice on two branches. A pull request that cannot be looked up is said on the
issue, after its claim, like any other reason a run could not start. Otherwise it asks the forge for the pull request on that
task's branch:

- **Open**: the label put back is a reviewer asking for changes, and the claim
  comment says it answers the review. No worktree is cut: the old one is fetched
  and fast-forwarded to the branch on the forge, which a reviewer may have
  pushed to (one that went its own way is refused, never pushed over); then the
  task is retried (`Template::repair_step`, `crate::task::retry`) from the step
  its status checks send back to, its new run told how to read the review
  (`core::review_note`, `Connector::read_review_with`) as a revision note. It
  pushes to the same pull request.
- **Closed without being merged**: refused. The issue is told onehand will not
  open another, and to reopen it to have its review answered.
- **Merged, or none**: a fresh branch, as for any issue.

## Where it lives

The tick is on `Shared`, one per process, for the reason the remote bridge is
there: two windows each running a tick would be two agents on one issue. The
tick therefore has no window, and finds one the way every remote path does — ask
each window whether it holds the project, and the one that does answers. The
runs themselves are tasks, in the task global with every other.

While a run is live the rail shows it: a project for the worktree, with one
session under it carrying the ordinary signal marks. That is deliberate — an
unattended agent that cannot be seen or stopped is worse than no unattended
agent — and the project is dropped when the run ends, so the rail does not
accumulate one row per issue ever worked.

## Not built, on purpose

- **Parking a run while its status checks run.** Its session stays open and the
  driver looks at the pull request every minute; a restart waits afresh. Park it
  with no session when idle adapters are seen to cost something.
- **Cleaning up after a merged pull request.** The worktree and its branch stay
  on disk; a merged pull request is the first signal clear enough to act on.
- **A cap on waiting runs.** `at_once` counts working runs only; each waiting
  one keeps an adapter alive. Add when a pile of unanswered runs is seen to
  cost something.
- **Cron expressions, quiet hours, a calendar.** An interval and a switch per
  project. Add when somebody actually wants runs only at night.
- **Telegram announcements of a run.** The three announced moments are a closed
  set with no wildcard arm, so a fourth kind of news is a decision about what the
  badge, the desktop and the chat each do with it. The report on the issue is
  enough for now. Add when it is wanted on a phone.
- **Anything but GitHub.** `gh` is the whole API layer. A second forge is a
  second implementation of `Connector`, not an abstraction to write first.
- **Issue triage.** Whether an issue is small enough is the label, set by a
  human. The app does not judge.

## The checks worth writing

Core, pure, no fixtures:

- `parse_every` — `"30m"`, `"2h"`, `"90s"`, and a refusal for `"soon"`.
- `brief_for` — the body is whole, not cut into the middle of a code fence, and
  the instructions name the issue.
- an empty `label` yields no candidate issue; no project is switched on by
  default, and the switch survives the workspace file by path.
- `report`, one case per `Outcome`, matched exhaustively so an outcome cannot be
  added without a sentence being checked for it; the work leads whatever the
  outcome; the last step's output is quoted unless the run was done.
- `branch_for` passes `validate_branch` for a title that is nothing but
  punctuation and for one 300 characters long, and a kept issue and a forge
  issue of one number never share a branch.
- `room` counts working runs against `at_once`.
- `history::over_cap` never lets go of a task whose report is unsent.

The `gh` calls themselves are not unit-tested; they are `Command`
invocations whose failure is a string that gets reported, the same shape
`worktree::add_blocking` already has.

## Settled

- **Which project goes first** when several have a candidate issue: the first in
  the workspace's display order. Pinning a project is already how a user says it
  matters more, so it is also how it gets worked first — and it needs no state,
  where taking turns between projects would.
- **A run is allowed while the user is at the keyboard.** The worktree is
  isolated, the run's session is never shown unasked (see *A run does not take
  the window*), and a person can take it over, so "away" is about announcements
  rather than about safety. A tick that only starts runs when nobody is looking
  is one that never starts any.
