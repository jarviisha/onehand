# onehand

A desktop host for AI coding agents. This glossary holds the words whose meaning is
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

**Card**:
A permission or a question an agent parked in its session, waiting for a person to answer.
_Avoid_: prompt, dialog, popup

**Take over**:
A person putting a prompt of their own into the session of a run or an unattended run; the
run stops and the session becomes an ordinary one. Answering a card the run's agent parked is not
taking over.
_Avoid_: hijack, adopt

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
A session whose turn failed, whose agent went away or waits on a person, as the rail counts it; or
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
