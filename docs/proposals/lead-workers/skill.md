# The lead skill

- Status: written on 2026-10-29 as `leading-worker-agents` (`SKILL.md` and `protocol.md`), installed
  as a personal skill in `~/.agents/skills/`, which Codex reads and Claude Code reaches through a
  link in its own skills folder. Packaged on 2026-10-29 as the plugin `skills` in github.com/jarviisha/skills, installable by Claude Code and Codex.
- Run end to end on 2026-10-29 (step 2) on a scratch repository: two jobs, a Claude Code worker
  and a Codex worker on the command line, both merged. Two faults were caused on purpose and both
  held: an approved plan changed afterwards (the worker refused to build) and a worker killed
  mid-build (confirmed stopped, its work kept by a recovery turn). The run corrected the skill's
  transport: the Claude prompt goes on stdin, `--strict-mcp-config`, the repository's `.git`
  writable for a worktree, `codex exec resume` taking `-c` overrides only, and the lead keeping
  a copy of each approved artifact. A second run tried the other two cases, and both held: a turn
  started with no pid recorded (found by its session id, waited for, not sent again) and the person
  prompting a worker directly (found by counting the prompts in the session's transcript against
  the turns sent; the job held until the person answered and gave the session back). They added
  to the skill: finding a turn's process without a pid, the transcript check, and running `lsof`
  from outside the worktree.
- Compared on 2026-10-29 with Claude Code agent teams, workmux, oh-my-claudecode, crew,
  claude-squad, uzi and Anthropic's multi-agent research write-up. Six ideas from them were
  tested against the skill as it stood; four already held without a new rule (a handoff note: the
  reasons are already in `decisions.md`; a watchdog for a live but stuck worker; the lead stopping
  early; a dirty base and a worker's claim of the person's consent). Two were added: a limit of
  three `revise` rounds per stage, after which the job is `failed` and the person chooses, and a
  required `check:` line before a build may say `done`.
- Run on a real repository on 2026-10-29 (darkvoid-sandbox, three GitHub issues, three workers,
  three pull requests with CI green). Four lessons were tested against the skill; two already held
  when the scenario named the risk, and two failed when it did not and were added: run the check on
  the base and then in two worktrees at once before opening workers (a tool's machine-wide lock,
  golangci-lint's, made parallel lint fail), and make turn 1 run the check and a throwaway commit
  (Codex's sandbox denied loopback sockets and, on resume, writes to `.git`; both surfaced only at the
  build). Writing a decision's invariant and using a strong model for a measuring re-review held
  without a new rule.
  Where the skill and this document differ, the skill is the newer.
- Tested with subagents, without and then with the skill:

  | Scenario | Without | With |
  |---|---|---|
  | A spec with three small faults, under time pressure | approved it "with fixes, no new revision" | `revise`, then approve the next revision's hash |
  | The lead back after a crash, the worker's process not found, a file changed after the crash | resumed once `pgrep` found nothing | held the job, told the person what to stop; other jobs went on |
  | The first prompt and files for a Codex worker | sound layout, but no turn numbers, no revision or hash in approvals, no run id, no access check | all present; the repository's base branch, merge style and no-push rule taken over the defaults |
  | The lead's own fix to a semantic conflict, CI green, the person asking to merge | asked for a review of the fix first | not rerun: it passed without the skill |
  | A plan edited in place after approval, its hash no longer matching | not run | caught the mismatch, sent `revise`, held review |

A skill that lets one agent session coordinate several worker sessions on one repository: hand out
the jobs, approve each stage, review, and merge. It is the method of the run in
[README.md](README.md), with its lessons turned into rules.

## Where it lives and what it runs on

- **Its own plugin, not this repository**, packaged the way `house-style` is, so any repository
  can install it.
- **Any agent.** The lead and the workers may be Claude Code, Codex or another agent. The skill
  relies on nothing one agent alone has: no `SendMessage`, `ListAgents` or subagent tool is
  required, though the lead may use them where it has them.
- **Written with `superpowers:writing-skills`**: pressure scenarios run on subagents before the
  skill exists, then again against it, so it is checked by how agents behave rather than by a
  person reading it. Where a step has a skill of its own (`using-git-worktrees`,
  `requesting-code-review`, `finishing-a-development-branch`), the lead skill uses it when it is
  installed and carries a short fallback of its own when it is not.

## Defaults and requirements

The source run's habits are **defaults**, not rules. The lead reads the target repository's own
instructions (its `AGENTS.md`, `CLAUDE.md`, contributing guide) and asks the person for what they
do not say, and records the result in `LEAD.md` before opening anyone:

| Default | Taken from the repository or the person when they say otherwise |
|---|---|
| Base branch `main` | the branch the person names, or the repository's default branch |
| One pull request per job, squash merge | the repository's merge rule; no forge at all means local branches merged by the lead |
| Push at every checkpoint | a repository that forbids pushing work in progress: commit at every checkpoint, push at the end |
| Shared files are append-only | the repository's rule for its shared files |
| Brainstorm, spec, plan, test-first | the repository's own process; the stages below are renamed to match it |

**Required** whatever the repository says: the protocol below, the independent review, the scoped
re-review, and that a lead's judgement about code cites what it read.

## The transport

How the lead opens a worker and gives it its next turn. **A worker keeps one transport for its
whole life.** Switching transport mid-life (opened through onehand, woken on the command line) is
not done: nothing shows that a host's session id is one an agent's command line can resume, and it
would need its own specification and a test.

| Transport | Opens with | Next turn with | Available |
|---|---|---|---|
| The command line | the agent's non-interactive command in the worktree, recording the session id it prints | the same command resuming that id, for example `claude -p --resume <id>` or `codex exec resume <id>`; the exact command per agent is checked before it is written into the skill | now; the first version |
| onehand | `open_session` on the worktree | `send_prompt` | once the pieces in [onehand.md](onehand.md) exist |
| Claude Code agent teams | a teammate | a message to it | where enabled; Claude Code only |

## The protocol

### Where

One folder for the whole run, **outside every worktree**, at an absolute path the lead chooses and
writes into every first prompt, for example `/home/me/dev/mmerp-lead/`. A worker may read anything
in it and write only its own files below.

**Reaching the folder is a precondition, not something the skill sets up.** An agent whose sandbox
keeps it inside its worktree must be given the folder as an extra writable directory by whoever
configures that agent (its own flags or settings; onehand's tools do not set it in their first
version). Before the first stage, the lead's first turn to each worker is an **access check**: read
`LEAD.md`, write `status.md` in its own folder; the lead confirms both before handing out any work.
A worker that fails it is not given a stage, and the lead tells the person what to configure. So
the skill works with **any agent that can read and write the run folder**, not with every agent as
it comes.

```
<run>/
  run.md                 lead: the run id, base branch and SHA, the job table
  LEAD.md                lead: the shared rules and the log of cross-branch decisions
  <job>/
    session.md           lead: transport, session id, run id, turn number sent
    status.md            worker: the stage, the turn it answers, the SHA, the artifact and its hash
    ask.md               worker: questions, appended
    decisions.md         lead: answers and approvals, appended
    artifacts/           worker: spec-r1.md, spec-r2.md, plan-r1.md …
  integration/           lead: one record per tested combination
  archive.md             lead: the summary kept after cleanup
```

### Writing

- **One writer per file**, as marked above. Nobody edits another's file.
- `status.md` and `session.md` are **replaced whole**: written to a temporary file in the same
  folder, then renamed over the old one, so a reader never sees half of one.
- `ask.md` and `decisions.md` are **appended**, one entry at a time, each between a line
  `begin <id>` and a line `end <id>`. A reader takes only complete pairs and ignores everything
  else.
- **An interrupted append** leaves a tail with no `end` line. The writer never edits it: its next
  entry starts on a fresh line, and takes a new id, one above the highest id in the file, finished
  or not, so an id is never given twice. The unfinished one is simply never read; if it mattered,
  the writer writes it again under the new id.
- Every file the lead or a worker writes carries the **run id**, so a file left by an older run in
  the same folder is never read as this one's.

### Identifiers

- A question is `<job>-q<n>`, numbered by the worker. A decision is `<job>-d<n>`, numbered by the
  lead, and names the questions it answers.
- A worker's next `status.md` lists the decisions it has taken in (`read: b-d3, b-d4`), so the lead
  knows what was seen.
- An artifact is versioned by file (`spec-r2.md`) and by its SHA-256.

### Approval

An approval is one decision naming exactly what it approves:

```
b-d7 approve job=b stage=spec rev=2 sha256=9f2c… run=2026-10-28-a
end b-d7
```

- **The latest decision about a stage holds.** A later `revise` or `approve` of another revision
  replaces it.
- A worker goes on past a stage only while the latest decision about it approves the revision and
  hash of the artifact as it is now. Revising a spec after its approval makes its hash differ, so
  the old approval no longer matches.
- **Fixes are asked for before approval, never with it.** The lead answers a spec it wants changed
  with `revise`, listing the fixes; the worker writes the next revision; the lead checks the fixes
  (a quick check is enough when they are small) and approves **that** revision's hash. The stage
  always ends on an approval of the artifact as it finally is.
- **This is a rule the agents follow, not an enforcement.** Nothing stops a worker that ignores it
  from writing code; the lead's check (`status.md` naming an approved revision, and the worktree's
  diff matching the stage) catches it after the fact. A real gate would need a mechanism of its
  own, outside this skill.

### A turn, and coming back after a stop

- **Turns are numbered per job**, from 1, across every session the job ever has. The first prompt,
  the access check, is turn 1. The lead records the number in `session.md` before sending and puts
  it in the prompt; the worker writes it into `status.md` when it ends the turn.
- **A `status.md` answering a turn proves only that the result was written**, not that the turn and
  its background work have ended. Before sending the next turn the lead also confirms the turn is
  over: on the command line, the process has exited; on onehand, the state says the turn ended and
  no background work runs.

After any stop, of the app, the lead or a worker, the lead reads `session.md` and `status.md`, and
takes the first case that fits:

1. **`status.md` answers the turn last sent, and the turn is confirmed over**: the lead reads the
   result and goes on.
2. **The session is alive and the lead does not know whether its last send arrived**: it sends
   **the same turn again, with the same number and the same text**. onehand answers `already sent`
   when it had arrived, and refuses a turn number sent with another text. On the command line,
   where nothing dedupes, the lead never resends blindly: it first establishes whether the process
   for that turn ran (case 3 or 4).
3. **The old turn is confirmed over, without an answer** (ended, failed or cancelled, its process
   gone, no background work): the lead looks at what it did anyway (the worktree since the last
   recorded SHA, new artifacts, entries in `ask.md`), records what it found as a decision, and only
   then sends a **new recovery turn** with the next number, saying what was found. A worker given a
   stage it has already finished answers with its existing result rather than doing it again.
4. **The lead cannot confirm the old worker has stopped**, it or anything it started: the session
   is unreachable, or its process may outlive it. **The lead does nothing on that worktree**: it
   sends nothing, opens no replacement, and tells the person, who stops the old worker. Two workers
   on one worktree is worse than a stalled job.
5. **The old worker is confirmed stopped and its session is gone** (closed by the person, or onehand
   restarted cleanly, see onehand's part): the lead opens a new worker on the same worktree, records
   it in `session.md` with the next turn number, and its first prompt points it at the job folder,
   its artifacts and the decisions so far; then case 3 applies to what the old one left.

## The method

### Before opening anything

- Settle the defaults table above with the repository and the person.
- Hand out every number branches would race for (ADRs, migrations) and record them in `LEAD.md`.
- List shared resources and say for each whether parallel use is safe. Where it is not, give each
  branch its own (a database and ports through environment variables), or say plainly that the
  lead runs it one branch at a time.
- **Check free disk space**, and limit workers to the tests that concern them while they work.
- One worktree per job, cut from the base branch, with whatever local config the tests need.
- **Ask the person for merge permission now**, with its conditions (green CI, end-to-end passing,
  a clean review, no conflicts), so the lead can run to the end without stopping.
- Write `LEAD.md`: the job table (worktree, branch, numbers, scope and its border with the other
  jobs), work only in your worktree, the commands no worker runs, the shared-file rule, generated
  files only through their generator, a domain the person does not know is answered with a cited
  default marked as an assumption, stop at the end of the job and never merge.

### The first prompt

The same frame for every worker: the job and the problem behind it; the run folder's absolute path
and its own files; read `LEAD.md` first; the repository's process as settled above; the job's own
constraints; the protocol, its approvals and its turns; where to stop; the language to answer the
person in.

### Stage by stage

| The worker leaves | The lead |
|---|---|
| Questions, each with a proposal | Answers each, adding the conditions the worker could not know |
| A spec | Reads the artifact and the real code, checks the claims that could be wrong (the web for a new regulation, the migrations for a value, the repository's own way of doing a thing), answers `revise` with the fixes, then approves the revision that has them (see *Approval*) |
| A plan | Recomputes the numbers its tests rely on; sets checkpoints where a stage touches shared files |
| A SHA at a checkpoint | Diffs the shared files; runs an integration test (below); sends a shared rule when one is needed |
| A question mid-stage | Decides at once and says why; when the worker shows the lead wrong, says so and lets it go on |

- **Every judgement about code cites the `file:line` the lead read.** All three wrong calls in the
  source run came from concluding first.
- **Commit at every checkpoint** (and push, by default). A lost session then loses no work.
- Every decision that crosses branches goes into `LEAD.md`'s log.

### Integration tests

Workers keep committing while the lead tests, so a test names exactly what it tested:

- The lead records the base SHA and each worker's SHA, merges those commits into a **checkout of
  its own** (a worktree no worker uses), and runs the tests there.
- The result is kept in `integration/` under that combination: the SHAs, the commands, what
  passed and failed. A later checkpoint is a new combination, never an update of the old one.

### Review

- For each job, in parallel: the end-to-end suite, one branch at a time unless each has its own
  resources, and an **independent review** by a fresh read-only reviewer, of a fixed SHA. Its prompt
  says what the feature does, where the spec is, how to get the diff, which risks to look at, and
  asks **which existing tests or jobs this change affects**. It never carries the lead's
  conclusions. Findings come back as Critical, Important or Minor, each with `file:line` and the
  case that breaks.
- The lead confirms each finding in the code before handing it on, and decides the direction of a
  fix itself when one has to be chosen.
- After the fix, a **scoped re-review** of the new SHA: ADDRESSED or NOT ADDRESSED per finding,
  plus anything the fix broke.

### Merge

- In a fixed order. Before each next job, merge the base branch into its branch (merge, not
  rebase, by default, so nothing is force-pushed), regenerate, and run the tests, lint and
  end-to-end again.
- The lead fixes semantic conflicts itself, and **its fix gets a scoped review of its own** before
  the merge, as a worker's fix does.
- Update the pull request's description before merging when the repository squashes; it becomes
  the commit.

### Cleanup

Only what is finished is cleaned up. For each job, its worktree and branch go only when:
- its work is merged (or the person has dropped it),
- the worktree has nothing uncommitted, and the branch has nothing that is not in the base
  branch, judged by how the job was merged:
  - **a merge commit or a fast-forward**: the branch's tip is an ancestor of the base branch;
  - **a squash or a rebase**, where the worker's commits never become ancestors: the forge says the
    pull request is merged, and the branch has no commit after the one that was merged, whose SHA
    the lead recorded when it merged;
  - **no remote**: nothing is asked about pushing; the local merge is judged as above;
- its session has no turn and no background work running, and is closed.

Before the run folder goes, the lead writes `archive.md`, kept where the person says (the
repository's own notes, or beside the run folder): per job, the approvals with their revisions and
hashes, the integration combinations and their results, the review findings and how each ended,
and what is left undone. A job that does not meet the conditions keeps its worktree, branch and
folder, and the archive says why.

### Models

The policy: a strong model for the lead, specs and reviews, a cheaper one for implementation, and
the strong one for a worker whose job is heavy in law or money.

- **Per stage** where the transport takes a model per turn (an agent's command line usually does).
- **Per worker** where it is fixed when the session opens, as with onehand; the lead then picks
  by the worker's hardest stage.
- A model the agent does not accept is not guessed around: the lead records the model actually
  used in `session.md` and tells the person.

### When things break

- Disk full or a shared service down: investigate read-only, let the person choose what to clear,
  then tell every worker to run its tests again.
- A stop of any kind: the turn rule above.
