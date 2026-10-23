# R4 step 2: tool sets, `list_issues` and `create_issue`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A session can stand with a set of tools rather than all or none, and two new tools read
and write a project's own issues.

**Spec:** `docs/superpowers/specs/2026-10-23-schedules-design.md` (Goals 4–5; *Set per schedule*,
labels). Step 1: `2026-10-23-schedules-step-1-core.md`.

## Global Constraints

- The set is decided in core `control`: `tools/list` lists only the set, a call outside it is
  refused naming the tool. The composer's switch stays all or none.
- `create_issue` takes no labels; the issue gets the labels the calling session's schedule names
  (none until step 3 gives sessions a schedule), so no agent labels its own issue.
- An open issue whose title is the same (trimmed, case-insensitive) is not written again; its
  number is answered. Checked under `issues::update_blocking`'s lock, so two writers cannot both
  write it.
- The file is read and written off the UI thread; after a write the Issues tab and page rescan.
- `project` defaults to the calling session's; an outside call must name one.

## Review Focus

1. Two agents writing the same title at once → one issue. → core test (the check runs inside
   `update_blocking`).
2. A workspace with no storage directory → `create_issue` refused with why, nothing written.
3. `list_issues` on a synced project lists the forge's open issues too, so a scan does not
   duplicate one it imported.
4. A title only of spaces → refused (core's `title_problem`).
5. The tab showing the file when the tool writes → the new issue appears (Rescan).

---

### Task 1: Core

- `control::Standing::Only(Vec<String>)` (done, with `a_session_given_some_tools_lists_and_calls_only_those`).
- `Call::ListIssues { project }`, `Call::CreateIssue { project, title, body }` in the catalogue
  (done, `the_issue_tools_parse_their_input`).
- `issues::Created { New(u64), Already(u64) }` and
  `pub fn create_unless_open_blocking(file: &Path, draft: Draft) -> Result<Created, String>` —
  `update_blocking` whose change looks for an open issue with the same title first. Tests:
  `an_open_issue_with_the_same_title_is_not_written_twice` (case and spaces ignored; a closed one
  with the title does not count), `a_blank_title_is_refused`.

### Task 2: App

- `perform_now`: `ListIssues` and `CreateIssue` go off the UI thread (the `answer.take()` path the
  starts use): resolve the project (`pick_project`, else the caller's, else
  `Err("name a project")`), its file (`Shell::issues_file`, else
  `Err("this workspace keeps no issues of its own")`), then read or write.
  - list: open issues from `Issues::listed()`, `"#<n> — <title>"`, `control::listed` capped.
  - create: `Draft { title, body, labels: control::labels_of(caller) }` → `New(n)` →
    `"Wrote issue #n: <title>"`, `Already(n)` → `"Issue #n is already open with that title"`.
  - after a write, on the UI thread: `Shell::issues_changed(cx)` (new, in `shell/issues_page.rs`):
    the Workbench's `rescan` and the page's `Request::Rescan`.
- `control::labels_of(uid) -> Vec<String>` reads a per-session label list in `Sessions`, empty
  for now; step 3 fills it.
- The socket test counts eleven tools.

### Task 3: Documents, checks, review, PR

- `docs/control.md`: tool sets, the two tools. `CHANGELOG.md`.
- `make fmt`, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `cargo test --workspace --locked`;
  Opus review; PR; CI; ask before merging.
