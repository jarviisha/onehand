# Project page, step 1: the page is always reachable — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pressing a project's row on the rail, or its tile on the overview, shows the project's
page even while the project has sessions; the page drops its *Tasks* and *Schedules* boxes.

**Architecture:** "Which session is on screen" becomes optional in the workspace model:
`ProjectRoot.active_session: Option<usize>`, `None` meaning the project's page. A new drive call,
`WorkspaceDrive::show_project_page`, selects the root and sets `None`; the app's existing
no-session path (`arrive_at_active_root` → `ChatPane::clear_active`) then draws the page as it
does today for an empty project. Every other way of selecting a root (Ctrl+Tab, palette, issues,
docks) keeps showing the last session.

**Tech Stack:** Rust, GPUI, `onehand-core` (GUI-free), `onehand` app crate.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md` (section *Reaching the
page*, and the Goals line on which boxes stay). The spec's step order: this is step 1 of 5;
*Settings ▸ Projects* (moving the Settings box out) is step 2, so the page's *Settings* box and the
host's *Answer cards on the host* box **stay** in this step.

## Global Constraints

- Core stays GUI-free; the rule (what is on screen) is core's, the app carries `Effect`s.
- Match our own enums exhaustively; no wildcard arms (`crates/app/src/guards.rs`).
- No comment or string cites a document.
- `make fmt`, `cargo test --workspace --locked`, `make clippy CLIPPY_EXTRA="-- -D warnings"` pass.
- A visible change: answer the `ui` skill's checklist (`.claude/skills/ui/`) before the PR.
- Files stay under ~800 lines of code; `project_page.rs` is at 792 and only shrinks here.

## Review Focus

1. Closing the session on screen while others remain still shows a neighbour, not the page
   (today's behaviour); closing a session while the page is shown leaves the page shown.
2. Reordering sessions (`move_session`) while the page is shown keeps the page shown.
3. Merging a bare row into its project (`drive/projects.rs`) with the page shown keeps `None`.
4. Ctrl+W / *Close session* with the page shown does nothing (no session is on screen).
5. A session that keeps running while its project's page is shown still raises its notices,
   badge and rail state (it is left running, as a page does today).

Tests for 1–4 are in Task 1; 5 is behaviour the app already has for pages (`leave_shown_session`)
and is checked by hand in the PR's test notes.

---

### Task 1: `active_session` becomes optional in core

**Files:**
- Modify: `crates/core/src/workspace/root.rs:34,132,273-275`
- Modify: `crates/core/src/workspace.rs:505-525` (`move_session`), `:672-711` (`add_session_at`,
  `close_session`, `select_session`)
- Modify: `crates/core/src/workspace/drive/projects.rs:230-246`
- Modify: `crates/core/src/workspace/drive/sessions.rs` (new `show_project_page` beside
  `select_project`, line ~392)
- Test: `crates/core/src/workspace/tests.rs`, `crates/core/src/workspace/drive/tests.rs`

**Interfaces:**
- Produces: `ProjectRoot.active_session: Option<usize>` (pub); `ProjectRoot::active_session(&self)
  -> Option<&Session>` unchanged in signature; `Workspace::show_page(&mut self)` (pub(crate)) sets
  the active root's `active_session = None`;
  `WorkspaceDrive::show_project_page(&mut self, root: usize) -> Vec<Effect>` returning
  `vec![Effect::Show, Effect::Changed]`, `#[must_use = "its effects are to be carried"]`.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/workspace/tests.rs`

```rust
#[test]
fn the_page_is_shown_with_sessions_open() {
    // a root with two sessions, the second on screen
    // ws.show_page();
    assert_eq!(ws.active_root().unwrap().active_session, None);
    assert!(ws.active_root().unwrap().active_session().is_none());
    assert_eq!(ws.active_root().unwrap().sessions.len(), 2);
}

#[test]
fn closing_a_session_under_the_page_keeps_the_page() {
    // two sessions, show_page(), close_session(root, 0)
    assert_eq!(ws.roots[root].active_session, None);
}

#[test]
fn closing_the_session_on_screen_shows_a_neighbour() {
    // three sessions, select_session(1), close_session(root, 1)
    assert_eq!(ws.roots[root].active_session, Some(1));
    // close the only one left after closing the others → None
}

#[test]
fn moving_sessions_under_the_page_keeps_the_page() {
    // two sessions, show_page(), move_session(root, 0, 1)
    assert_eq!(ws.roots[root].active_session, None);
}
```

and in `crates/core/src/workspace/drive/tests.rs`:

```rust
#[test]
fn showing_a_project_page_selects_the_root_and_no_session() {
    // two roots, a session on root 1 shown; drive.show_project_page(1)
    assert_eq!(fx, vec![Effect::Show, Effect::Changed]);
    assert_eq!(drive.active_uid(), None);
    assert_eq!(tree(&mut drive).active_root, 1);
}
```

Also update the existing assertions that compare `active_session` with a bare index
(`tests.rs:289,293,474,476,479,690,701,714`) to `Some(n)`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p onehand-core workspace`
Expected: compile errors (`show_page`, `show_project_page` not found; `Option` mismatches).

- [ ] **Step 3: Implement**

- `root.rs`: the field becomes `Option<usize>`, `None` in `new`; `active_session()` is
  `self.active_session.and_then(|i| self.sessions.get(i))`. Its doc says `None` is the project's
  page.
- `add_session_at` sets `Some(len - 1)`; `select_session` sets `Some(idx)`.
- `close_session`: with `None`, nothing moves. With `Some(a)`: empty → `None`; `a >= len` →
  `Some(len - 1)`; `session_idx < a` → `Some(a - 1)`.
- `move_session`: map the same arms over `Some`.
- `drive/projects.rs`: `target.active_session = active.map(|a| first + a)` when `shown && moved`
  (with `None` the target keeps the page).
- `Workspace::show_page` and `WorkspaceDrive::show_project_page(root)`: `select_root(root)`, then
  `show_page()`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p onehand-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(workspace): a project's page can be on screen while it has sessions"
```

### Task 2: the rail and the overview show the page

**Files:**
- Modify: `crates/app/src/shell/roots.rs:105-109` (new `Shell::show_project_page` beside
  `select_root`), `:628-637` (`E::ShowProject` calls it)
- Modify: `crates/app/src/rail.rs:236-243` (`shown` reads the `Option`), `:499-508`
  (`rail_click_project` calls `show_project_page`; its doc and `rail/project.rs:156` say a click
  shows the page)
- Modify: `crates/app/src/shell/sessions.rs:190-201` (`close_active_session` does nothing on
  `None`)
- Test: `crates/app/src/shell/tests.rs`

**Interfaces:**
- Consumes: `WorkspaceDrive::show_project_page(root) -> Vec<Effect>` (Task 1).
- Produces: `pub fn show_project_page(&mut self, idx: usize, window: &mut Window, cx: &mut
  Context<Shell>)` on `Shell`, carrying the effects through `carry_workspace`.

- [ ] **Step 1: Write the failing test** in `crates/app/src/shell/tests.rs`, in the style of the
  test around line 213 (`select_root_session`):

```rust
#[gpui::test]
fn a_project_row_shows_its_page_over_open_sessions(cx: &mut TestAppContext) {
    // a shell with one project and one session shown
    // shell.show_project_page(0, window, cx)
    assert_eq!(shell.active_session_uid(), None);
    assert_eq!(project.sessions.len(), 1);          // still open
    assert!(pane.read(cx).project_page_shown());     // the page is what the pane holds
}
```

If `ChatPane` has no reader saying the project page is up, add
`pub(crate) fn project_page_shown(&self) -> bool { self.empty.is_some() }` in
`chat/pane/pages.rs`.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p onehand a_project_row_shows_its_page`
Expected: FAIL to compile (`show_project_page` not found).

- [ ] **Step 3: Implement** the three call sites above. `rail_click_project` keeps moving the
  keyboard's row to the project. `Ctrl+Tab`, the palette, issues and docks keep `select_root`.

- [ ] **Step 4: Run the app's tests**

Run: `cargo test -p onehand`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "feat(rail): a project's row shows its page"
```

### Task 3: the page drops *Tasks* and *Schedules*, and the documents follow

**Files:**
- Modify: `crates/app/src/chat/pane/project_page.rs:23-27,581-586,627-628,735-792` (remove
  `tasks_box`, `schedules_box` and their `.children`)
- Delete: `crates/app/src/chat/pane/project_page/schedules_box.rs`
- Modify: `docs/screens.md:90-92` (a project row's press shows its page; session rows show
  sessions), `:348` (*A project's page shows*, not *A project with no session*), `:375-379`
  (the *Tasks* and *Schedules* boxes go)
- Modify: `docs/workflows.md:628` (the check row: *Open the project's page*)
- Modify: `docs/hosts.md:232` only if its wording assumes the page means no session (it reads
  "no session shown and no page"; leave it if still true)

- [ ] **Step 1: Remove the boxes** and whatever only they used (events, helpers): let
  `cargo build -p onehand` and the `dead_code`/`unused` warnings point at the rest.

- [ ] **Step 2: Update the documents** listed above, in their own words, English, no code
  pointing at them.

- [ ] **Step 3: Full checks**

Run: `make fmt && cargo test --workspace --locked && make clippy CLIPPY_EXTRA="-- -D warnings"`
Expected: all pass, no warnings.

- [ ] **Step 4: Commit**

```bash
git add -A crates/app docs
git commit -m "feat(projects): the page leaves tasks and schedules to their own pages"
```

## After the tasks

- Opus review of `origin/main..HEAD`; fix what it confirms.
- PR `feat(projects): a project's page is reachable while it has sessions`, plain-text body;
  the PR carries the spec and this plan (they leave with the last step's PR).
