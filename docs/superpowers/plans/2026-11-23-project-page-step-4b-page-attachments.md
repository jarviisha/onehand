# Project page, step 4b: files staged on the page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A project's page stages files (its `+`, a drop on the card, a paste) and they go with
the session's first prompt. `@` and `/` lists on the page are step 4c (owner's choice, 2026-11-23:
they need the composer's lists taken off the session first).

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *The page*.

## Tasks

1. **Core: a held prompt keeps what was staged with it.** `SessionDrive::prompt_when_up(text,
   staged, origin, now)`; `Held.staged`; sent with it once the agent is up. Test
   `a_held_prompt_takes_what_was_staged_with_it`.
2. **The page stages files.** `Composer::page_card`: `+` (*Attach files*, the picker), drop with
   the session card's ring, paste; the tray over the card; Send refused only while nothing is
   written *or staged*; `PageSend.staged` → the shell's `Ask` → `prompt_when_up`. What is staged
   is set aside per project with the words (the composer's `Draft` already holds both). Test
   `a_file_staged_on_a_page_goes_with_the_send`. Docs: `docs/screens.md`.
