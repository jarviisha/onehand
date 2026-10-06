# Architecture of the proposal

- Status: proposal, the shape of every piece together. Part of [the proposal](README.md).
- The split is the one the code already has: **core decides, the app does, a plugin draws.** Every
  new rule below is a pure function in `crates/core`; the app gathers its inputs and carries out its
  answer; the Issues plugin draws what it is told through `onehand-plugin-host`.

## Where each part goes

Existing parts are plain; what the proposal adds is marked `+`, what it changes `~`.

```
crates/core (GUI-free, blocking)            crates/app (GPUI)                       plugins/builtin/workbench-issues
───────────────────────────────────         ─────────────────────────────────       ────────────────────────────────
workflow::run                               task (the Tasks global)                 view/detail.rs
  Run, Visit, Marks, Outcome                  rows, attention, issue_runs             ~ runs_view → Work section
  ~ approved(visit) / revised(visit, note)    ~ approve / revise by task + visit        + three facts, steps, next action
  ~ Outcome::Failed { kind, why }  (B)        + work_summary(task) → plugin           + Refresh, "read Xm ago"
  + Run::pull_request              (B)        + retry_with_current(task)              + review block (piece 4)
  + Visit::command                 (B)      task::driver                              + template picker in the form
  retry_of, retry_plan, recheck               ~ routes actions by task id
task                                          ~ records failure kind, PR, command     crates/plugin-host (workbench.rs)
  Task, group, history                      unattended                                Request::IssueRuns
  + next_action(task, working)                ~ placed() on a lost window (fix)       ~ IssueRun grows, or
  + work facts: run diff vs branch diff       + slots() for who holds each            + Request::IssueWork { key, .. }
+ preflight                                 + agent_modes (per spec, process only)    + IssueRun actions carry visit id
  + preflight(kind, facts) → findings       shell/workflows.rs
issues                                        launcher, begin_retry
  LocalIssue, Draft, open_across              ~ draws preflight findings
  + templates (shipped, project's own)        + Retry with current settings dialog
  + lacks(body, template) → headings        dialogs.rs (pick_issue)
  + across(state)            (pages)          + preview + instructions for this run
unattended                                    + preflight findings
  brief_for, Verdict, report                workbench/panel.rs
  ~ brief_for(.., extra instructions)         ~ broadcast + answer keyed reads
connector                                   + issue_reads (background, keyed)
  pull request by branch, merged head         PR state, branch counts, diffs
```

`(B)` is piece 1 part B: new persistence, optional fields in the task file.

## The parts and who calls whom

```mermaid
flowchart LR
    subgraph core["crates/core: decides"]
        Run["workflow::Run<br/>approved(visit), revised(visit)<br/>outcome, visits, marks"]
        Task["task::Task<br/>group, next_action"]
        Pre["preflight(kind, facts)"]
        Lacks["issues::lacks(body, template)"]
        Brief["unattended::brief_for"]
        Conn["connector<br/>pull request, merged head"]
    end
    subgraph app["crates/app: does"]
        Tasks["Tasks global<br/>work_summary, approve, retry"]
        Driver["task::driver<br/>the only reporter"]
        Reads["issue_reads<br/>background, keyed"]
        Modes["agent_modes<br/>per spec, this process"]
        Launch["launcher, pick_issue,<br/>Retry dialog"]
        Unatt["unattended<br/>tick, slots, placed"]
    end
    subgraph plugin["Issues plugin: draws"]
        Work["Work section<br/>facts, steps, next action,<br/>review block, Refresh"]
    end

    Work -- "Request: IssueWork / Approve(task, run, visit)" --> Tasks
    Tasks -- "summary" --> Work
    Tasks --> Task
    Tasks --> Driver
    Driver --> Run
    Work -- "Refresh, focus" --> Reads
    Reads --> Conn
    Reads -- "keyed answer" --> Work
    Launch --> Pre
    Unatt --> Pre
    Modes --> Pre
    Driver -- "agent came up: modes" --> Modes
    Launch --> Lacks
    Launch --> Brief
```

## Flow: an issue opened

What the Work section draws, and where each line comes from. In memory per frame, except the reads
that need git or the forge.

```mermaid
sequenceDiagram
    participant V as Issues plugin
    participant P as workbench panel
    participant T as Tasks global
    participant C as core
    participant R as issue_reads (background)

    V->>P: IssueWork(root, number, key n)
    P->>T: tasks of the issue
    T->>C: Task::group, next_action(task, working)
    C-->>T: facts, steps, next action
    T-->>V: summary (in memory)
    P->>R: PR state, branch counts, run diff (key n)
    Note over V: draws at once, slow lines say reading
    R-->>P: answer (key n, read at)
    P-->>V: answer, if key is still n
    Note over V: issue changed meanwhile, key n+1, answer dropped
```

Asked again on *Refresh*, on the task moving (`IssueRuns` broadcast), and on the window regaining
focus when what is shown is older than a minute.

## Flow: an approval from anywhere

One path for the strip, the task detail and the issue. The visit id is what keeps a stale view from
approving an answer it never showed.

```mermaid
sequenceDiagram
    participant A as Window A (issue)
    participant B as Window B (strip)
    participant T as Tasks global
    participant D as driver
    participant R as Run (core)

    A->>T: shows answer of visit 4
    B->>T: Revise(task, run, visit 4, note)
    T->>D: by task id
    D->>R: revised(4, note)
    R-->>D: Prompt (plan again)
    Note over R: plan runs, waits at approval visit 6
    A->>T: Continue(task, run, visit 4)
    T->>D: by task id
    D->>R: approved(4)
    R-->>D: Idle (visit 4 closed)
    D-->>A: refused: the answer changed
    A->>T: reload, shows visit 6
```

## Flow: a start, through the preflight

Every kind of start asks the same function with its own configuration; only a block stops it.

```mermaid
flowchart TD
    S["a start is asked for"] --> K{"kind"}
    K -->|new run| F1["Settings' agent, workflow picked,<br/>worktree off HEAD"]
    K -->|new issue run| F2["[unattended] config, workflow by pick/label,<br/>worktree off origin/default"]
    K -->|Resume| F3["the run's own snapshot and setup"]
    K -->|Retry| F4["the last run's setup,<br/>snapshot chosen in the dialog"]
    K -->|Retry with current settings| F5["Settings now, with old → new shown"]
    K -->|answer a review| F6["the task's snapshot and setup"]
    F1 & F2 & F3 & F4 & F5 & F6 --> P["core::preflight(kind, facts)"]
    M["agent_modes: current only if<br/>learned this process, same spec"] --> P
    G["gh sign-in as last seen,<br/>slots and who holds them"] --> P
    P --> B{"any block?"}
    B -->|yes| X["refused where it was asked,<br/>nothing claimed, cut or started"]
    B -->|no| Q["task::request: claim (issue), cut,<br/>then the place queue"]
    Q --> W{"place free?"}
    W -->|yes| Run["driver starts the run"]
    W -->|no| Wait["Queued behind the task holding it"]
```

A full slot is a block, never a wait: only a place queues.

## Flow: Retry with current settings

```mermaid
flowchart LR
    Old["last run<br/>setup + snapshot"] --> Diff["what changed:<br/>agent, mode, check command,<br/>timeout, workflow version"]
    Now["Settings / [unattended] now,<br/>workflow by id"] --> Diff
    Diff --> Start["start = retry_plan(new snapshot)<br/>check command changed →<br/>recheck: back to last command step"]
    Start --> Pre["preflight(kind = Retry with current)"]
    Pre --> Dlg["dialog: old → new, where it starts,<br/>findings"]
    Dlg --> New["Run::retry_of with the new setup"]
```

*Retry* itself is unchanged: `retry_of` with the last run's setup.

## What is stored, and where

```
<config_dir>/onehand/tasks/<id>.json        one per task, as today
  runs[]
    outcome          ~ Failed { kind, why }            kind absent in older files → other
    pull_request     + { number, url }                 absent → looked up by branch
    visits[]
      command        + { passed, exit, tail, at }      absent → "not recorded"

<workspace storage>/issues/<project>.json   unchanged: no progress kept on an issue (issues::file_for)

in memory, this process only
  agent_modes        + spec fingerprint → modes offered
  issue reads        + key → (answer, read at)
```

Nothing else is persisted. Progress, the next action, the three facts and every preflight finding
are worked out each time from what is above.
