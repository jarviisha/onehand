# Proposal: workspace-wide issues and unattended runs that see work through

- Date: 2026-09-30.
- Status: step 1 built; the rest proposed.
- The system as built: [Unattended runs](unattended.md).

## The problem

Unattended runs find an issue, cut a worktree, start an agent and report the
outcome. Three things still need a person watching:

1. **A run dies on its first question.** A parked permission or question
   cancelled the run, so an issue that needed one decision was thrown away and
   had to be labelled again.
2. **A run can end before the work is done.** The first `TurnEnded` ends it,
   including a turn that only analysed the problem.
3. **There is nowhere to see the whole workspace.** Issues are per project, and
   the live run shows only as a pill on one rail row.

## What was decided

The first draft of this proposal reached much further — a thirteen-state run
lifecycle, a run store with leases and an outbox, a scheduler with priority,
dependencies and fairness, agent-assisted triage. It was cut back to what a
single-user desktop app with one execution slot can use, and to reversing
**only the rule that costs the most runs**:

| Rule in *Unattended runs* | Decision |
|---|---|
| 1. A parked ask ends the run | **Reversed.** The card waits for a person |
| 3. The claim is the whole of the state | Kept. No run store, no recovery |
| 6. A run is one turn | Kept. No multi-turn controller, no CI loop |

**A waiting run gives up the slot.** It keeps its issue and its session, but
the search may start the next issue, so one unanswered question does not stop
the workspace. That means a working run and any number of waiting ones can each
hold an adapter, and a waiting run whose card is answered carries on beside the
one started meanwhile — a turn under way cannot be held.

**The workspace view is a page in the centre of the window**, not a Workbench
mode or a rail tab: it has to hold a table across every project, and the rail
is too narrow for one.

## Step 1 — an ask waits instead of ending the run (built)

Why the old rule no longer holds: it assumed an issue needing a decision was
not small and needed a person. It does — but the person can answer the card
from wherever they are (the window, the desktop notification, a chat on the
remote bridge), and a run cancelled over one decision costs the whole run.

The mechanism is the protocol's own. A parked card is a request the adapter is
waiting on inside the turn, so leaving it up and letting a person answer it
continues the same turn. A card answers once, so nothing can resume twice, and
nothing needs storing.

- A parked card leaves the run waiting: its timeout stops counting
  (`unattended::Budget` counts working time only), and the search looks for the
  next issue at once.
- The card is announced like any other. Nothing is suppressed for runs any more.
- Answering the card is not a take-over; the run carries on and its clock
  restarts. A prompt of a person's own is still a take-over.
- The "someone is reading it" and "picked by hand" exceptions are gone: every
  run now does what they did, minus the hand-over.
- The prompt tells the agent to ask through its question tool rather than in
  prose, so a question becomes a card rather than the end of the turn.
- An adapter lost, or a session closed, while waiting ends the run as
  `Asked`, and the question goes on the issue. Re-adding the label is the
  retry, as for every other ending — rule 3 is kept, so a card does not
  survive a restart.

What it does not do: a chat on the remote bridge hears a run's card only if it
follows that session, and a run's session is new, so nobody follows it by
default.

## Step 2 — the workspace page (built)

Built as described below, with two choices made on the way. The issue list is
**open issues only**, with a line counting the closed ones, because the page is
read for work. Pressing an issue selects its project and opens the Workbench's
Issues mode on it. *Working* does not say how long a run has been going: a run
carries its remaining budget and not its start, and the rail already names it.

A page in the centre of the window, reached from the rail, reading what already
exists:

- **Waiting on you** — every run with a parked card; a row opens that session,
  where the card is.
- **Working** — the run holding the slot: project, issue, how long.
- **Issues** — every project's own issues in one list (the union of the
  per-project issue files), with project, open/closed and labels, and a
  project filter. A linked issue appears once.

Not in it, because rule 3 is kept: a run history. What a finished run did is
the note or comment on its issue. A Runs tab needs a run store, and that is
the decision to reopen first when one is wanted.

The page goes through the design contract before it is built.

## Deferred, and what would reopen each

| Deferred | Reopen when |
|---|---|
| Nudging a run whose turn ended without a pull request; a CI fix loop | Runs are seen ending early often enough to matter (reverses rule 6) |
| A run store, recovery after a restart, an outbox for forge updates | A run history is wanted on the page, or restarts are seen losing runs (reverses rule 3) |
| Priority, dependencies, parent/child issues | The queue is ever longer than a handful |
| Fairness between projects | There is more than one slot |
| Agent-assisted triage | Issues are seen arriving underspecified |
| A cap on waiting runs | A pile of unanswered runs is seen to cost something |
| A background worker outliving the window | Runs are wanted with the app closed |
