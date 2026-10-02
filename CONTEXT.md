# onehand

A desktop host for AI coding agents. This glossary holds the words whose meaning is
particular to onehand; general programming terms are left out.

## Unattended runs

**Run**:
All of the work onehand does on one issue, from the claim until the issue's pull request is
ready for review or the run stops. An issue has at most one active run.
_Avoid_: job, task, queue entry, controller, worker

**Attempt**:
One go at a run. A retry is a new attempt of the same run: it keeps the same branch, worktree
and pull request.
_Avoid_: retry run, second run

**Step**:
A named stretch of an attempt with its own prompt and a condition the app checks itself before
moving on: Plan, Implement, Verify, Open PR.
_Avoid_: stage, phase, task

**Repair**:
A session started on a run's worktree to fix checks that failed on its pull request's current
head. A repair belongs to the attempt that is running when the checks fail.
_Avoid_: fix-up run, rerun

**Tick**:
One look for work, made every configured interval; it starts nothing while a run is working.
_Avoid_: poll, schedule

**Trigger label**:
The label whose presence on an issue asks for a run.
_Avoid_: tag, auto label

**Claim**:
Removing the trigger label from an issue and commenting that a run started on it.
_Avoid_: lock, lease

**Outcome**:
How a run or an attempt ended, from a closed set, always told on the issue.
_Avoid_: result, status

**Ready for review**:
A run's pull request is open, every commit is pushed, its required checks have passed on the
current head and it has no conflicts. Whether the change meets the issue is for the reviewer
to judge, not part of this state.
_Avoid_: done, delivered, complete

**Take over**:
A person acting inside a run's session; the run stops watching and the session becomes an
ordinary one.
_Avoid_: hijack, adopt
