# Project page, step 4c: `@` and `/` on the page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** On a project's page, `@` lists the project's files and folders and `/` the agent's
commands (as a session last said them), in the composer's own list, walked and taken with the same
keys as in a session.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *The page*. Owner's choice
(2026-11-23): a PR of its own, after 4b.

**Architecture:** The composer's lists read *where they come from* (`Lists`) instead of a session:
`Lists::Session(&Entity<ChatSession>)` as today, or `Lists::Page(&PageLists)`: the files and
folders of the page's project (scanned off the UI loop with `completion::scan_files`, as a session
scans them, kept per project) and the commands its agent last offered (`Settings::facts_of`).
Lists only a session has (its mode, its options, the composer's own controls in `/`) are empty on
a page. The pane draws the composer's list over the page's card, and the page's field takes the
list's keys while it is open.

## Tasks

1. **The lists read a source, not a session** (no behaviour change). `chat/composer/complete.rs`,
   `popup.rs`, `presentation.rs`, `card.rs`, `composer.rs`: the functions that read a session for
   a list take `&Lists`. All existing tests pass unchanged.
2. **The page's lists.** `PageLists { files, folders, scanned, commands }` kept per project in the
   pane, scanned when a page first shows (re-read on the next show), commands from the page agent's
   facts. The page card's field: `ChatComposer` context while a list is open, `CompletionNext/Prev/
   Accept` actions; Enter takes a highlighted row before it sends. The pane draws the popup over the
   page's card. Test: `a_slash_on_the_page_offers_its_agents_commands` (facts with a command,
   type `/re`, the composer's matches hold it); `an_at_on_the_page_offers_its_files` (a file in
   the project, type `@no`, after the scan the matches hold it). Docs: `docs/screens.md`.

## After review

- The page's list floats out of the page's scroll (`Composer::page_popup`, a deferred anchored
  box hanging from the card's measured top), so its top is never cut; a press elsewhere closes it.
- The page's `+` is a menu: *Attach files…*, *Mention a file*, *Run a slash command* (typed from
  code, for an input method that swallows the character).
- A page's file scans are numbered; an older one landing late is dropped.
- A session's `/` no longer reads the transcript's touched files; only `@` does, as before.
