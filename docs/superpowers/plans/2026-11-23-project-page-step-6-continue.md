# Project page, step 6: continue a past conversation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Goal:** A past conversation on a project's page continues in a new session, with the same agent
or another: *Continue in a new session…* on its row puts it on the page's composer as a chip (its
title, *Summarise first*, ×); on send, the conversation is written as Markdown to a file the new
session can read, and its first prompt points the agent at it.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *Continuing a past
conversation*. Step 6 of 6: this PR also removes the spec and the plans (the work is done; git
keeps them).

## Calls (the spec's builder's calls)

- The file goes in the session's folder at `.onehand/continued/<title>-<id>.md`, beside a
  `.onehand/.gitignore` holding `*`, so git ignores it without a line in the person's own files.
- The person's words lead the first prompt and the hand-off follows them, so the new
  conversation's title comes from what the person wrote.
- Offered on a page whose sessions run on this machine (its *Past conversations*); a host's or an
  offloaded project's page does not offer it yet (`docs/known-gaps.md`).
- The new conversation keeps the title it continued, shown on its row as *Continued from <title>*.

## Tasks

1. **Core.** `chat::handoff`: `write_blocking(kept: &Path, folder: &Path) -> Result<PathBuf,
   String>` (the conversation kept in `kept`, read and exported with `export_markdown`, written
   as above), `lead_in(text: &str, file: &Path, title: &str, summarise: bool) -> String`.
   `Prefs.continued: Option<String>`, `ConvMeta.continued`, `SessionDrive::continues(title)`.
   Tests: `a_hand_off_is_written_where_git_ignores_it`, `the_lead_in_follows_the_words`,
   `a_continued_conversation_says_what_it_continued`.
2. **App.** The row's `⋯` and right-click gain *Continue in a new session…*: the page's draft takes
   `Continuing { dir, title, agent, summarise }`, its agent chip the conversation's agent; a chip
   over the field (title, *Summarise first* toggle, ×). `PageSend.continuing`; the shell writes the
   file off the UI loop, then starts the session with `lead_in(...)` and `continues(title)`; a
   failed write stands on the page. The row's line reads *Continued from <title>* where it has one.
   Tests: `continue_puts_the_conversation_on_the_page`, `the_page_sends_what_it_continues`.
3. **Docs, and the work's end.** `docs/screens.md`; `docs/known-gaps.md` (host/offloaded pages);
   remove `docs/superpowers/specs/2026-11-20-project-page-design.md` and this step's plans, their
   behaviour now in the owning documents.
