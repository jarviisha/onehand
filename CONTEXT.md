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
A person putting a prompt of their own into the session of a pipeline run or an unattended run; the
run stops and the session becomes an ordinary one. Answering a card the run's agent parked is not
taking over.
_Avoid_: hijack, adopt

## Pipelines

**Pipeline template**:
The steps a pipeline takes, in order, with each step's prompt, gates, command or approval, and the
template's place, allowance of misses and timeout. Kept as one TOML file each; the ones onehand
ships are read-only and are duplicated to be changed.
_Avoid_: workflow, recipe, playbook

**Pipeline run**:
One pass of a pipeline template over a brief: a snapshot of the template as it was when the run
began, the step it is at, its marks, what its steps kept, and the history of its transitions. Its
file is kept only while it can still be resumed.
_Avoid_: job, execution, instance

**Step**:
A named stretch of a pipeline run. An agent step prompts the session and is judged by its gates; a
command step runs a command in the work, onehand itself; an approval step waits for a person to
approve what an earlier step answered, or to send it back with a note.
_Avoid_: stage, phase, task

**Gate**:
A condition onehand checks itself when an agent step's turn ends, read from git and the transcript
and never from what the agent says it did: answered, code unchanged, code changed, committed,
uncommitted. A turn that fails one is a miss.
_Avoid_: check (that is the project's command), assertion, guard

**Miss**:
A turn that failed a gate, or a command that failed. Past the template's allowance in one stretch of
steps, the run ends exhausted.
_Avoid_: retry, failure

**Brief**:
What a pipeline run is asked to do: a title, the details, and instructions asked of every step.
_Avoid_: task, ticket, prompt

**Mark**:
A point in the work a run measures from: where an agent step started (the commit and the
fingerprint of the uncommitted work), and the commit a command last passed on.
_Avoid_: baseline, checkpoint, snapshot

**Place**:
Where a pipeline's work happens: the checkout the project is open on, left uncommitted, or a new
branch in a worktree of its own, committed there.
_Avoid_: mode, target

**Check command**:
A project's own command for "the work is sound", run by onehand in a command step that names no
command of its own.
_Avoid_: test command, CI

**Outcome**:
How a pipeline run ended: done, stopped (and why), exhausted at a step, or failed.
_Avoid_: result, status

**Resume**:
Carrying on a pipeline run whose agent stopped or whose session went, from the step it was at, with
its marks kept. Nothing resumes a run by itself.
_Avoid_: restart, retry

## Unattended runs

**Unattended run**:
One issue worked by one session, in a worktree of its own, with one prompt and nobody watching,
ending in a pull request or commits.
_Avoid_: job, task, batch, auto run

**Tracker**:
Where an issue lives: on a forge, kept by onehand, or kept by onehand in step with a forge.
_Avoid_: backend, provider

**Connector**:
What onehand reaches a forge through: its account, its issues, labels, comments and pull requests.
_Avoid_: integration, adapter (that is an agent's)

**Tick**:
One look for work, made every configured interval; it starts nothing while a run is working.
_Avoid_: poll, schedule

**Trigger label**:
The label whose presence on an issue asks for an unattended run.
_Avoid_: tag, auto label

**Claim**:
Removing the trigger label from an issue and commenting that a run started on it. Adding the label
back is how a person asks for the issue to be tried again.
_Avoid_: lock, lease

**Slot**:
The one place a working unattended run holds. A run waiting on a card gives it up, so the next tick
can start another.
_Avoid_: lock, worker

**Ending**:
How an unattended run stopped: its turn ended, a card was left unanswered, the adapter went, the
session closed, it timed out, it was taken over, or it never got as far as a prompt.
_Avoid_: outcome (a pipeline run's), status

**Verdict**:
What an unattended run left behind, judged by onehand rather than the agent: a pull request or
none, or, without a forge, commits on its branch or none.
_Avoid_: result, review
