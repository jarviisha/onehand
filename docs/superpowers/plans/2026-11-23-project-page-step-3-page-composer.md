# Project page, step 3: the composer on the page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The project page carries a composer under *What are we building?*: text, an agent chip,
Send, and under it a *Start in* chip (the project's folder, each worktree, *New worktree…* with a
branch field). Sending makes the session (and the worktree first when asked) with the prompt as
its first turn. Every *New session* lands on the page.

**Architecture:** The pane's one `Composer` is drawn on the page as well, its text set aside per
project (`page_drafts`) as it is per session. A new card, `Composer::page_card`, draws the field,
the agent chip and Send without a session. Send raises `ChatPaneEvent::StartFromPage`; the shell
makes the worktree if asked (the async half of `commit_worktree`, extracted), adds the session
in the chosen folder with the chosen agent, and hands the text to the session's drive with
`prompt_when_up(text, Origin::Person)`, which sends it once the agent is up. A person's held
prompt that never goes (the agent did not come up) is handed back by a new
`chat::drive::Effect::Unsent(String)`, which the app puts into that session's draft.

**Tech Stack:** Rust, GPUI, gpui-component; `onehand-core` chat drive; `onehand` app crate.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *The page*, *Reaching the
page*. Step 3 of 6. Out of scope here: model, mode, effort and ⚡ chips, `+`, `@`/`/` completion
and attachments on the page (step 4, with what agents are remembered to offer); *Cards* (step 5);
*Continue* (step 6); removing the empty-session screen (left as it is: a session made from the
page always starts with a prompt).

## Global Constraints

- Words: heading *What are we building?* (`chat::pane::greeting::GREETING`); placeholder as the
  session composer's; chips *Start in*'s label is the branch of the place picked, its menu
  headed *Start in*; *New worktree…*; the branch field's placeholder *Branch name*; status
  *Making worktree <branch>…*; agent menu headed *Agent*.
- The page holds one primary: Send. *Run check* stays secondary in the head; the head's
  *New session* button and its caret go.
- Worktrees in the *Start in* menu capped at `crate::projects::WORKTREES_SHOWN` with
  `more_not_shown`.
- The branch name is checked with `onehand_core::worktree::validate_branch` as it is typed; Send
  refuses (`.refuses(why)`) while it fails or is empty, and while a worktree is being made.
- A host project's *Start in* offers its folder alone; an offloaded project's too (its sessions
  run in the mirror). *New worktree…* only where `ProjectFacts.is_repo`.
- No comment cites a document; files under ~800 lines (`project_page.rs` is near: the new card
  and chips go in files of their own); `make fmt`, workspace tests, clippy `-D warnings`; the
  `ui` checklist.

## Review Focus

1. Typing on project A's page, going to B's page and back: A's text is there, B's empty; a
   session's draft and the page's never mix.
2. Send with *New worktree* whose `git worktree add` fails (branch exists): the page says why,
   the text stays, no session is made.
3. The agent never comes up (bad command): the session shows its failure and its composer holds
   the text that was to be sent.
4. Pressing Send twice quickly makes one session.
5. The agent chip after the picked agent is removed in Settings: falls back to the first agent.

---

### Task 1: a person's held prompt that never goes comes back

**Files:**
- Modify: `crates/core/src/chat/drive.rs` (`Effect::Unsent(String)`; where `held` is taken at
  `:544`, a held prompt whose origin is `Origin::Person` and whose link is `Lost` yields it)
- Modify: `crates/app/src/chat/session.rs` (carry `Effect::Unsent` as a `ChatEvent` the pane
  hears), `crates/app/src/chat/pane/…` (the pane puts it into that conversation's draft, or the
  composer when it is the one shown)
- Test: `crates/core/src/chat/drive/tests.rs`

- [ ] Failing test `a_persons_held_prompt_comes_back_when_the_agent_never_comes_up`: drive
  connecting; `prompt_when_up("hi", Origin::Person)` → `Intake::Held`; apply the link lost →
  effects contain `Effect::Unsent("hi".into())`. And `an_issues_held_prompt_is_dropped`: same
  with `Origin::Issue` → no `Unsent`.
- [ ] Run → FAIL; implement; run `cargo test -p onehand-core chat` → PASS.
- [ ] App: carry the effect (exhaustive matches will point at every arm). `cargo test -p onehand`.
- [ ] Commit `feat(chat): a person's prompt held for an agent that never came up comes back`.

### Task 2: the page's composer and its chips

**Files:**
- Create: `crates/app/src/chat/composer/page_card.rs`: `impl Composer { pub(in crate::chat) fn
  page_card(&mut self, agent: AgentChip, refuses: Option<SharedString>, cx) -> impl IntoElement }`
  — the field (`text_area` on `self.state`), the agent chip, Send (primary, `ComposerEvent::SendPressed`).
- Create: `crates/app/src/chat/pane/project_page/start_in.rs`: the *Start in* chip and the branch
  field under the card.
- Modify: `crates/app/src/chat/pane.rs` (`page_drafts: HashMap<PathBuf, PageDraft>`,
  `branch: Entity<InputState>`), `chat/pane/sessions.rs` (`clear_active` restores the page's
  draft into the composer; leaving the page sets it aside — beside `stash_draft`/`restore_draft`),
  `chat/pane.rs:394` (`SendPressed` on the page → `submit_page`)
- Modify: `crates/app/src/chat/pane/project_page.rs` (the heading and the card under the head;
  `start.rs`'s *New session* goes from the head)
- Delete: `crates/app/src/chat/pane/project_page/start.rs`
- Test: `crates/app/src/chat/pane/tests.rs` or `crates/app/src/shell/tests.rs`

**Interfaces:**
- `pub(crate) enum StartIn { Project, Folder(PathBuf), NewWorktree }` and
  `pub(crate) struct PageDraft { pub text: Draft, pub agent: Option<SharedString>, pub start: StartIn }`
  in `chat/pane/project_page.rs` (or a small `page_draft.rs`).
- `ChatPaneEvent::StartFromPage { text: String, agent: Option<SharedString>, start: StartIn,
  branch: Option<String> }`.
- `ChatPane::set_page_start(&mut self, start: StartIn, agent: Option<SharedString>, window, cx)`
  (used by Task 4), `ChatPane::page_failed(&mut self, why: String, cx)`,
  `ChatPane::page_busy(&mut self, busy: Option<String>, cx)`, `ChatPane::page_sent(&mut self, root:
  &Path, window, cx)` (clears that project's page draft).

- [ ] Failing tests: `a_page_draft_stays_with_its_project` (type on A's page, show B's page,
  show A's: text back; B empty) and `send_on_the_page_asks_for_a_session_with_what_was_picked`
  (pick a worktree and an agent, press Send → the pane emits `StartFromPage` with them and the
  text).
- [ ] Run → FAIL; implement; run `cargo test -p onehand` → PASS.
- [ ] Commit `feat(projects): the page carries a composer`.

### Task 3: the shell starts what the page sends

**Files:**
- Modify: `crates/app/src/shell/drafts.rs` (extract from `commit_worktree` the background make —
  `fn make_worktree(root, branch, dir, top, parent) -> Task<Result<(PathBuf, Option<String>),
  String>>` — and from `open_split` the record without a session, `fn record_split(...)`; both
  used by the dialog as before)
- Modify: `crates/app/src/shell/boot.rs` (the `StartFromPage` arm → `Shell::start_from_page`)
- Create: `crates/app/src/shell/page_start.rs`: `pub(super) fn start_from_page(&mut self, ...)`:
  refuse a second while one is under way; for `NewWorktree`, `page_busy(Some("Making worktree
  <branch>…"))`, make it (folder from `worktree::worktree_dir[_in]` and the project's
  `worktree_parent`), on failure `page_failed(why)`; then `session_in_folder(idx, folder, agent,
  StartsOn::Fresh)` → `prompt_when_up(text, Origin::Person)` on its session → `page_sent(root)`.
- Test: `crates/app/src/shell/tests.rs`

- [ ] Failing tests: `sending_from_the_page_starts_a_session_holding_the_prompt` (project
  folder, rig agent `nowhere`: a session exists in the folder, shown, its chat holds the prompt
  as held or failed-and-unsent — assert the page draft was cleared and the session's composer or
  draft holds nothing lost) and `a_worktree_that_cannot_be_made_keeps_the_page` (a project that
  is no repository, `NewWorktree` with branch `x` → no new session, page draft kept, the pane's
  failure line set).
- [ ] Run → FAIL; implement; `cargo test -p onehand` → PASS.
- [ ] Commit `feat(projects): sending from the page starts the session`.

### Task 4: every *New session* lands on the page; documents

**Files:**
- Modify: `crates/app/src/shell/sessions.rs` (`new_session`, `new_session_in`,
  `new_session_at`, `new_session_beside` — the person's entries — show the project's page with
  the agent and place set and the composer focused; `session_in_folder`/`start_session` stay for
  resumes, runs, tools and issue work)
- Modify: `crates/app/src/shell/worktrees.rs` (`WorktreeAct::NewSession(folder)` → the page with
  that folder picked), `crates/app/src/rail/project/menu.rs` (*New session*), the rail's `+` and
  its *Start in … with …* menu (`rail/workspace.rs` `new_session_menu`), `shell/drafts.rs`
  `open_split` (a split from the dialog lands on the page in its new worktree instead of a
  session)
- Modify docs: `docs/screens.md` (the page's head and the composer under it; the rail's `+` and
  *New session* land on the page), `docs/hosts.md` if it says *New session* opens a session on
  a host project, `docs/tasks.md` (*Run check* beside *New session* → in the head),
  `CONTEXT.md` only if a term changes.
- Test: `crates/app/src/shell/tests.rs`

- [ ] Failing test `new_session_lands_on_the_page_with_its_place_picked`: a project with a
  session shown; `new_session_beside(root)` → no new session, the page shown, its draft's start is
  the folder of the session that was on screen.
- [ ] Run → FAIL; implement; update the existing tests that expected a session from these
  entries (they now expect the page; a session comes from `start_from_page`).
- [ ] Full checks: `make fmt && cargo test --workspace --locked && make clippy
  CLIPPY_EXTRA="-- -D warnings"` → pass.
- [ ] Commit `feat(projects): a new session starts on its project's page`.

## After the tasks

Opus review; PR `feat(projects): a project's page starts its sessions`.
