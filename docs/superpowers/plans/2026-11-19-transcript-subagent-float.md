# Subagents as a Line and a Float: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A subagent is one line in the conversation; pressed, its own transcript opens in a float over the pane, and a strip above the composer counts the turn's subagents while any works.

**Architecture:** Core gives a card's items an address of their own (`TranscriptItemId::InCard`), so the transcript's renderer, fold state and run planning work inside a card unchanged. Core also keeps a card's clock and whole task, and says how the turn's subagents stand. The pane holds which card the float shows (a stack, for nested ones) and draws it from planned runs, as it draws the conversation.

**Tech Stack:** Rust, GPUI, gpui-component.

**Spec:** `docs/superpowers/specs/2026-11-18-transcript-working-state-design.md`, section 4.

## Global Constraints

- Stacked on #361 (branch `style/transcript-detail-boxes`).
- Words and what is true live in core: a card's steps, time, standing and the turn's counts. Which card the float shows is view state, kept in the pane.
- The float reads the pane's width: 440px at its top right, at most three fifths of its height, while the pane is 900px or wider; under that, the pane above the composer. Not modal.
- Escape closes the float only where it holds focus: a keymap command in its own context, focus put back on the composer before it leaves the frame.
- A card saved before this reads back with no time and its one task line.
- Checks: core and app tests, clippy with warnings denied, `make fmt`.

## Review Focus

- A card address that no longer resolves (a conversation reloaded, a fork) draws nothing and panics nowhere. (Task 1)
- Fold keys and element ids of items in different cards never collide with each other or with the conversation's. (Task 3)
- Closing the float while it holds focus leaves the keymap working. (Task 4)
- A nested card's line inside the float pushes onto the stack; one from the conversation replaces it. (Task 4)
- The strip leaves when the last subagent ends, even with a failure unread. (Task 5)

---

### Task 1: Core addresses a card's items
`TranscriptItemId::InCard { card, index }`, `SubagentItem::key()`, `Chat::card(key)`, `Chat::list` and every match on the address. Tests first.

### Task 2: Core keeps a card's clock and whole task, and its standing
`SubagentItem { task (whole), started, secs }`, saved as optional fields; `steps()` counts tool steps; `Standing { Waiting, Failed, Running, Done, Stopped }`; `Chat::turn_subagents()` (the turn's top-level cards, in transcript order, each with its standing) and `Chat::subagents_beside(key)` (the cards of that card's turn). Tests first.

### Task 3: The line
The card in the conversation becomes one line: a mark, the name, what it does (shimmering) or how it ended, its steps and its time. Pressed, the session tells the pane to open it. The old card's drawing goes.

### Task 4: The float
`ChatPane` keeps `float: Option<Float { uid, stack }>` and a focus handle; draws the head (name; state, steps, time; back; *n of N*, previous, next; Close), the task (two lines, *Show all*), and the card's runs, planned as the conversation's are. Escape closes it.

### Task 5: The strip
Over the composer, where the lead's strip stands, while any of the turn's subagents runs or waits: *N subagents* and a count per standing, most urgent first. Pressed, the float opens on the most urgent.

### Task 6: Contract and checks
`DESIGN.md`, `docs/screens.md` (*Subagent card*), `CHANGELOG.md`, `catalog.md`, the spec's *As built*. Every check. Last PR of the work: remove `docs/superpowers/` once its behaviour is in the owning documents.
