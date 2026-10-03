# onehand

A desktop host for AI coding agents. This glossary holds the words whose meaning is
particular to onehand; general programming terms are left out.

## Pipelines

**Pipeline template**:
The steps a pipeline takes, in order, with each step's prompt, gates, command or approval, and the
template's place, allowance of misses and timeout. Kept as one TOML file each; the ones onehand
ships are read-only and are duplicated to be changed.
_Avoid_: workflow, recipe, playbook

**Pipeline run**:
One pass of a pipeline template over a brief: a snapshot of the template as it was when the run
began, the step it is at, its marks, what its steps kept, and the history of its transitions. Kept
in a file until it ends, so a restart can offer to resume it.
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

**Source**:
Where a brief comes from. A person typing it into the launcher is the only source in this build; an
issue is meant to be another, and nothing about a pipeline run depends on which.
_Avoid_: input, origin, trigger

**Check command**:
A project's own command for "the work is sound", run by onehand in a command step that names no
command of its own.
_Avoid_: test command, CI

## Runs

**Run**:
The agent, the session and the resources that carry out work on onehand's behalf: the worktree it
works in, the clock that times it out. A pipeline run is carried out by a run; so is an unattended
run on an issue. An issue has at most one active run.
_Avoid_: job, task, queue entry, controller, worker

**Take over**:
A person putting a prompt of their own into a run's session; the run stops and the session becomes
an ordinary one. Answering a card the run's agent parked is not taking over.
_Avoid_: hijack, adopt

**Outcome**:
How a pipeline run or an unattended run ended, from a closed set.
_Avoid_: result, status

## Unattended runs

**Attempt**:
One go at an unattended run. A retry is a new attempt of the same run: it keeps the same branch,
worktree and pull request.
_Avoid_: retry run, second run

**Repair**:
A session started on a run's worktree to fix checks that failed on its pull request's current head.
_Avoid_: fix-up run, rerun

**Tick**:
One look for work, made every configured interval; it starts nothing while a run is working.
_Avoid_: poll, schedule

**Trigger label**:
The label whose presence on an issue asks for an unattended run.
_Avoid_: tag, auto label

**Claim**:
Removing the trigger label from an issue and commenting that a run started on it.
_Avoid_: lock, lease

**Ready for review**:
A run's pull request is open, every commit is pushed, its required checks have passed on the current
head and it has no conflicts. Whether the change meets the issue is for the reviewer to judge.
_Avoid_: done, delivered, complete
