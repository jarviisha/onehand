# onehand

A desktop app that runs AI coding agents. This glossary holds the words whose meaning is
particular to onehand; general programming terms are left out.

## Workspace

**Workspace**:
What one window holds: the projects it groups, and their sessions.
_Avoid_: solution, folder set

**Project**:
A folder in a workspace, usually a repository or a worktree of one, with the sessions started on it.
_Avoid_: repo (a project can be a folder inside one), root

**Session**:
One conversation with one agent on one project, carried over the Agent Client Protocol.
_Avoid_: chat, thread, tab

**Background work**:
What an agent started that runs on after its turn ends: a background command. The session stays
running while it lives, and while the agent answers it once it ends, and it can be stopped one
item at a time without stopping the turn.
_Avoid_: task, job, process

**Card**:
A permission or a question an agent parked in its session, waiting for a person to answer.
_Avoid_: prompt, dialog, popup

**Take over**:
A person putting a prompt of their own into the session of a run or an unattended run; the
run stops and the session becomes an ordinary one. Answering a card the run's agent parked is not
taking over.
_Avoid_: hijack, adopt

**Lead**:
A session whose agent controls onehand and opened other sessions to work for it, its workers.
_Avoid_: parent, manager, orchestrator

**Worker**:
A session a lead opened with `open_worker`, listed under its lead while the lead is open. The
lead holds it until the person sends it a prompt; *Give back to the lead* returns it.
_Avoid_: child, subagent, job

## Agents

**Built-in agent**:
Claude Code or Codex, named in the agent list by its preset: onehand chooses and tries its adapter,
runs the agent on the person's machine, and promises it every feature.
_Avoid_: official agent, supported agent

**Declared agent**:
Any other agent, written out in the agent list with its command; it gets what the protocol gives.
_Avoid_: custom agent, third-party agent

**Preset**:
What onehand knows about one built-in agent: its adapter and the version tried, the binary it
drives, and the defaults it needs. Kept in code; the agent list names it by id.
_Avoid_: template, profile

## Workflows

**Workflow**:
The steps a run takes, in order, with each step's prompt, gates, command or approval, and the
workflow's place, allowance of misses and timeout. Kept as one TOML file each; the ones onehand
ships are read-only and are duplicated to be changed.
_Avoid_: pipeline, template on its own, recipe, playbook

**Workflow id**:
What names a workflow whatever it is called: a person's own takes its file's name when first saved,
a shipped one `builtin:<name>`. A duplicate or an import gets a new one. Retry finds the newer
workflow by it, so a rename does not lose it.
_Avoid_: template id, key, slug

**Workflow version**:
A counter on a workflow, 1 when its file is made and up by one at each save that changes what it
says. Only the latest is kept; a run's snapshot holds the one it ran.
_Avoid_: template version, revision

**Run**:
One pass of a workflow over a brief: a snapshot of the workflow as it was when the run began, the
step it is at, its marks, what its steps kept, its step visits, its outcome once it has one, and
the history of its transitions. Kept in its task's history whatever its outcome.
_Avoid_: workflow run, pipeline run, job, execution, instance

**Task**:
The work onehand does on a person's behalf: a brief on a place, and every run of it. Kept as one
file each, unfinished or as history; it reads as its last run's outcome. Dismissing an ended task
keeps it as history and never offers it again. The project's check command run on its own is a
task too, with one command step and no session, and so is an issue worked unattended. Each project keeps its newest 200 finished tasks;
older ones are removed, with their marks.
_Avoid_: job, ticket, workflow run

**Retry**:
A new run of a task, after its last run ended. It starts at the first step it cannot carry over
from the last run, or at an earlier one a person picks, keeping what the steps before it answered,
and counts its misses from zero. A finished or dismissed task can be retried from its detail, and
is live again. Unlike Resume, which carries the same run on.
_Avoid_: rerun, restart

**Needs attention**:
A session whose last turn failed, until a prompt goes out again (a restart or a relaunch does not
answer it), whose agent went away or waits on a person, as the rail counts it; or
a task a person should act on, of two kinds: one **waiting** on a person (an approval, or a card its
agent parked), and one **ended** on something nobody chose (cut off, exhausted, failed, timed out,
or its agent or session gone). Done, stopped by a person and dismissed tasks are finished instead.
_Avoid_: failed (one kind only), errors, inbox

**Next action**:
What an issue's work waits on, in one sentence, and the one thing to press about it, if any: worked
out each time from the task, its runs and the issue, never stored. The issue's state only gates a
new start.
_Avoid_: todo, call to action, status

**Step visit**:
One stay of a run at a step, with its times, its marks at start and end, what it kept and how it
came out. Going back to a step, or resuming at one, is a new visit, never a rewrite of the last.
_Avoid_: attempt, iteration

**Step**:
A named stretch of a run. An agent step prompts the session and is judged by its gates; a
command step runs a command in the work, onehand itself; an approval step waits for a person to
approve what an earlier step answered, or to send it back with a note; a forge step takes a
worktree's branch to the forge.
_Avoid_: stage, phase, task

**Agent settings**:
An agent step's mode and config options (model, effort, whatever else the agent names), by the
agent's own ids and values. Each visit to the step runs with them; a setting the step does not
name goes back to what the run started with.
_Avoid_: agent config, step config

**Forge step**:
A step onehand does on the forge itself, never the agent: *push* (the commit the check passed on),
*pull request* (a draft, or the one open on the branch) and *status checks*. With no forge serving
the project each passes at once, and the branch is the result.
_Avoid_: integration step, deploy step

**Status checks**:
What a forge runs on a pull request's head and reports back. A forge step waits on them: all
passing takes the pull request out of draft; one failing, or a conflict, goes back to an earlier
step to repair it.
_Avoid_: check (that is the project's command), CI, build

**Gate**:
A condition onehand checks itself when an agent step's turn ends, read from git and the transcript
and never from what the agent says it did: answered, code unchanged, code changed, committed,
uncommitted. A turn that fails one is a miss.
_Avoid_: check (that is the project's command), assertion, guard

**Miss**:
A turn that failed a gate, or a command that failed. Past the workflow's allowance in one stretch of
steps, the run ends exhausted.
_Avoid_: retry, failure

**Brief**:
What a run is asked to do: a title, the details, and instructions asked of every step.
_Avoid_: task, ticket, prompt

**Acceptance** (in a brief):
How a person will judge the work, written into the issue under its own heading; read by the agent,
never checked by onehand as a gate. An issue written from a template that leaves it empty is told
so, as advice.
_Avoid_: definition of done, criteria on their own

**Issue template**:
A form a new issue is written from: the project's own under `.github/ISSUE_TEMPLATE/`, or the ones
onehand ships. It fills the body in and puts its labels on; its headings are what the issue's body
is read against, as advice.
_Avoid_: template on its own (that is a workflow)

**Mark**:
A point in the work a run measures from: where an agent step started (the commit and the
fingerprint of the uncommitted work), and the commit a command last passed on. Each step visit's
start and end also carries a pinned commit of the work, untracked files included, kept under
`refs/onehand/tasks/<task>/<run>/<visit>/<start|end>`.
_Avoid_: baseline, checkpoint, snapshot

**Place**:
Where a run's work happens: the checkout the project is open on, left uncommitted, or a new
branch in a worktree of its own, committed there. Also what the queue locks: the checkout git sees
(its canonical top level, or a folder's own path outside git), held by one task at a time.
_Avoid_: mode, target

**Queued**:
A task waiting for its place while another task works there. It starts by itself, first in first
out, once that task's work has stopped; *Stop* calls the start off.
_Avoid_: pending, scheduled

**Check command**:
A project's own command for "the work is sound", run by onehand in a command step that names no
command of its own.
_Avoid_: test command, CI

**Outcome**:
How a run ended: done, stopped (and why), exhausted at a step, or failed.
_Avoid_: result, status

**Resume**:
Carrying on a run whose agent stopped, whose session went or that a restart cut off, from the
step it was at, with its marks kept. It goes through the queue like any start. Nothing resumes a
run by itself.
_Avoid_: restart, retry

**Preflight**:
What onehand checks before a start claims an issue, cuts a worktree or starts an agent, and what it
found: each finding blocks the start or only says something, and says where it is changed. It
judges the configuration that will actually run, which for a Resume or a Retry is the run's own.
_Avoid_: precheck, validation (that is the workflow's own)

**Schedule**:
A project, what to start there (a session with a prompt, or a workflow on a brief), the agent and
how it runs, the tools its agent may use, and when, as a cron expression; it starts a task at each
due time while onehand is open, and once for the times it missed while it was not.
_Avoid_: cron job, timer, job

## Unattended runs

**Unattended run**:
One issue worked as a task of the configured workflow, in a worktree and on a branch of its own,
in a session nobody is watching, found by its trigger label or picked by hand. It ends on its
branch, and its issue is told how.
_Avoid_: job, batch, auto run

**Tracker**:
Where an issue lives: on a forge, kept by onehand, or kept by onehand in step with a forge.
_Avoid_: backend, provider

**Connector**:
What onehand reaches a forge through: its account, its issues, labels, comments and pull requests.
_Avoid_: integration, adapter (that is an agent's)

**Tick**:
One look for work, made every configured interval; it starts nothing while every slot is taken.
It also sends the reports not yet delivered.
_Avoid_: poll, schedule

**Trigger label**:
The label whose presence on an issue asks for an unattended run.
_Avoid_: tag, auto label

**Workflow label**:
A label on an issue, beside the trigger label, that chooses the workflow its run works with.
_Avoid_: tag, workflow tag

**Claim**:
Removing the trigger label from an issue and commenting that a run started on it. Adding the label
back is how a person asks for the issue to be tried again.
_Avoid_: lock, lease

**Slot**:
One of the places, `at_once` across every window, a working unattended run holds. A run waiting on
a card, an approval or its pull request's status checks gives it up, so the next tick can start
another.
_Avoid_: lock, worker

**Verdict**:
What an unattended run left behind, judged by onehand rather than the agent: the pull request on
its branch if one was opened, or how many commits its branch has past where it was cut.
_Avoid_: result, review

**Report**:
What an issue is told when an unattended run ends: the verdict, the run's outcome, and what its
last step ended on. Kept in the task's file until the issue has it, and sent again at every tick
until it lands.
_Avoid_: notification, status update

## Hosts

**Host**:
Another machine onehand reaches over SSH and runs sessions on, named the way `ssh` names it: an
alias from the person's SSH config, or `user@machine`. It keeps working while the laptop is off.
_Avoid_: server, remote, cloud, VPS (one kind of host)

**Host project**:
A project whose folder lives on a host; its sessions run there. The laptop holds no copy of it.
_Avoid_: remote project

**Offloaded project**:
A project whose folder is on this machine and whose sessions run on a host, against that host's
mirror of it. Its tree, editor, terminal and git stay this machine's. Offloading converts the
project in place; *Run here again* puts it back.
_Avoid_: remote project, synced project

**Mirror**:
The host's copy of an offloaded project's folder, which its sessions there run in: what git does
not ignore, plus `.git`. It belongs to the laptop that made it, and stays on the host when the
project runs here again.
_Avoid_: clone (that is git's), copy (that is a host session's journal kept here)

**Sync mode**:
How an offloaded project's code comes back, picked when it is offloaded: *into this folder*, a
mirror with a key, or *on a branch*. Changing it means running the project here again first.
_Avoid_: sync strategy, mode (alone, it names the Workbench's)

**Branch mode**:
The sync mode with no key: the host's sessions work on the host's branch in a mirror of its own,
and this machine's folder is never locked. Its code goes up only on *Update host from laptop*, as
its last commit merged into that branch.
_Avoid_: branch sync, PR mode

**Host's branch**:
`onehand/<host>`, the branch an offloaded project's sessions work on in branch mode. The host
commits what each turn leaves onto it; this machine fetches it into the remote-tracking ref of
that name, and merging it is the person's.
_Avoid_: remote branch (that is any `origin/…`), work branch

**Key**:
Which side may change an offloaded project's code: this machine, or the host. It goes to the host
with the code before a prompt, and comes back with the code once nothing works on it there. While
the host holds it, this machine's editor, terminals and git actions on the project are off.
_Avoid_: lock (that is what holding it does here), owner (that is the laptop a mirror belongs to)

**Host service**:
The onehand process on a host, with no window. It runs that host's sessions and their agents,
keeps a journal for each, and carries on with nobody attached. One per user on a host.
_Avoid_: daemon, server, agent (that is what it runs)

**Attach**:
The laptop connecting to a host service to show its sessions; **detach** is letting go. Only one
laptop is attached to a host at a time, and the newer attach wins. Detaching never stops anything
on the host.
_Avoid_: connect (that is a session reaching its agent), log in

**Journal**:
A host session's transcript as the host keeps it: one line per entry, only ever added to, holding
what the agent sent and what the person asked, in order. The laptop rebuilds the conversation by
replaying it. A host service that restarts reads its sessions back from their journals.
_Avoid_: log, history, transcript file (that is `items.jsonl` on the laptop)

**Copy** (of a host session):
The laptop's own copy of a host session's journal, the same lines with the host's numbers, added
to as entries arrive. It lets a session be read while its host is away and a later attach fetch
only what it lacks. The host's journal stays the truth: a copy that disagrees is dropped and taken
again.
_Avoid_: cache, backup, mirror (that is an offloaded project's folder)

**Permission bypass**:
A host project's switch, on by default, under which the host service answers every card its
sessions park: a permission is allowed, and a question is told to decide on its own and say what
it chose. Kept on the host, so it holds while the laptop is away.
_Avoid_: auto-approve, YOLO mode

**Build**:
Which copy of the host service a host runs, named by a hash of the binary itself. The laptop puts
its own build on a host beside any other, and restarts the service onto it only when no session
there is mid-turn.
_Avoid_: version

## How onehand is built

**Drive**:
The part of core that decides what happens next for one thing (a session, a run, or every task
and the places they work in): it is told
commands and what came of its effects, and answers with effects. It does no I/O and holds no
timer. Not the `drive` skill, which runs the app in a headless session for a by-hand test.
_Avoid_: controller, service, manager, orchestrator, use case

**Carrier**:
What carries out a drive's effects in one front end and reports what came of them; it decides
nothing.
_Avoid_: driver, adapter (that is an agent's), glue, bridge (that is the remote bridge)

**Effect**:
One thing a drive asks its carrier to do: a write, a send, a wait, a signal to whatever watches.
_Avoid_: action (that is a run engine's), command (that is what a drive is told)

**Application state**:
A fact about the work, which changes what onehand does or must read the same from every front
end: a queued prompt, a card's answer, a setting. Every change to it goes through a drive.
_Avoid_: model state, business state

**View state**:
How one viewer looks at the work, which changes nothing onehand does: what is open, scrolled to
or focused. It stays in the front end, kept across restarts or not.
_Avoid_: UI state
