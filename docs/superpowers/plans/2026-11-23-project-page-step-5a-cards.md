# Project page, step 5a: Cards — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A session on this machine answers its own cards at the level it is set to: *Manual*
(every card waits for the person), *Allow edits* (a permission to read or edit files is allowed
with its narrowest grant; everything else waits), *Bypass* (every card answered as a host's
bypass answers it). A project keeps a default, picked on its page; a session the person sends
from the page starts on it, and its own chip under its composer changes it for that session alone.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *Cards (the approval
level)*. Step 5 of 6, split: 5a here (this machine), 5b the host project's chip in place of its
*Answer cards on the host* box.

## Global Constraints

- Only sessions the person opens take the project's default (from the page); every other session
  (task, workflow, schedule, worker, control tools, fork, resume from a past conversation) starts
  on *Manual*. A resume keeps the level its conversation was saved with.
- Independent of the agent's own mode.
- Words: chip *Cards: <level>*; levels *Manual*, *Allow edits*, *Bypass*; menu rows say what each
  does (*Every card waits for you*; *Reading and editing files is allowed; the rest waits for
  you*; *Every card is answered without you*); a level other than *Manual* says its cost under the
  chip in the warning ink (*The agent can read and change files in the folder without asking* /
  *The agent can do anything your user can on this machine, git push and deleting files
  included*).
- A card answered by the level raises no notice (no `AwaitingUser`), and is drawn answered.
- Host sessions have no chip in 5a.

## Tasks

1. **Core: the level and what it answers.** `chat::Cards` (serde, `Manual` default) on `Chat`;
   a permission or a question that arrives while connected is answered at once when the level
   answers it (`PermissionRequest::bypass_answer`'s grant, through `answer_permission`; a question
   as `Elicitation::bypass_answer` answers it, through the ask's own fields), the outcome's
   `asked_user` cleared; `SessionDrive::set_cards(level)` sets it and answers cards already
   waiting; the level is kept in the conversation's `Prefs` and taken back on load. Tests:
   `allow_edits_allows_an_edit_and_leaves_a_command`, `bypass_answers_a_question`,
   `manual_answers_nothing`, `raising_the_level_answers_what_waits`, `the_level_is_kept_with_the_conversation`.
2. **Core: the project's default.** `Project.cards: Cards` (skipped when *Manual*),
   `ProjectsDrive::set_cards(id, level)` as `set_check` is. Test `a_projects_cards_default_is_kept`.
3. **App: the chips.** The page: right of *Start in*, *Cards: <level>* (local projects only),
   its menu sets the project's default; the cost line under the strip; `PageSend.cards`; the shell
   sets it on the session before its first prompt. A session on this machine: the same chip at the
   end of the strip under its composer, setting that session's level. Tests:
   `the_page_sends_its_projects_cards`, `a_sessions_chip_sets_its_level`. Docs: `docs/screens.md`,
   `CONTEXT.md` (*Cards*).

## After review

- *Allow edits* allows a read or an edit only when the request names its paths and every one is
  absolute, inside the session's folder with no step out, and outside `.git`, `.claude`, `.codex`
  and `.gemini` (`PermissionRequest.locations`, read off the tool call). Test
  `allow_edits_stays_inside_the_folder`.
- A level set on a kept conversation is written at once (`save_meta`). Test
  `a_cards_level_is_written_when_it_changes`.
- The session's cost line under its strip; its chip's menu opens above it.
- `docs/control.md` and `docs/known-gaps.md` say a session's own level may answer its cards.
