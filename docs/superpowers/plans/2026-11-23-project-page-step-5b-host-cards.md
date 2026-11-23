# Project page, step 5b: the host's switch as the page's Cards chip — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Goal:** On a host project's page (and an offloaded project's), the composer's *Cards* chip is the
host's switch for the folder: *Manual* (off) or *Bypass* (on), its cost under the strip while on.
The page's *Settings* box with *Answer cards on the host* goes.

**Spec:** `docs/superpowers/specs/2026-11-20-project-page-design.md`, *Cards*: "A host project
offers *Manual* and *Bypass*, and the chip is the host's existing switch; a session on a host has
no chip of its own."

## Task

- `ChatPane::page_cards_chip` (`chat/pane/page_options.rs`): on a host, `host_cards(bypass)` (none
  until the host has said), levels `Manual`/`Bypass`, a pick calls the existing
  `set_host_bypass`; the cost names the host. A session sent from a host's page still starts on
  *Manual* (the host answers). `host::bypass_box` and its place on the page go. Test
  `a_hosts_switch_reads_as_cards`. Docs: `docs/hosts.md`, `docs/screens.md`, `CONTEXT.md`.
