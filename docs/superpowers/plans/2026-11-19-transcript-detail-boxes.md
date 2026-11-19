# Detail Boxes and a Turn's Changes: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything a row opens into is one box that never grows past sixteen lines, and a turn's changes say each file once.

**Architecture:** One builder (`detail_box`) frames every detail: a clamped head, a body scrolling in both axes under one two-axis scrollbar on the box's edges, a floating Copy, and a foot with the line count and *Open* past the cap. Core words the file verdicts and counts created files apart.

**Tech Stack:** Rust, GPUI (`line_clamp`, `text_ellipsis_start`, `ScrollHandle::scroll_to_bottom`), gpui-component (`ScrollbarAxis::Both`).

**Spec:** `docs/superpowers/specs/2026-11-18-transcript-working-state-design.md`, section 3.

## Global Constraints

- Stacked on #360 (branch `feat/transcript-turn-endings`).
- The cap is sixteen lines: `DETAIL_OPEN_H` (20rem at a 1.25rem line). A box draws at most `BOX_MAX_LINES` lines and says so when that bites; *Open* reuses the code dialog, which has its own cap.
- The vertical scrollbar stays on the box's right edge whatever the horizontal offset: one scroll container for both axes, the bar a sibling of it in the frame.
- No document named in code; no hard-coded colour or size; words in core.
- Checks: core and app tests, clippy with warnings denied, `make fmt`.

## Review Focus

- A box whose content is shorter than the cap takes no wheel and shows no bar or foot. (Task 2)
- A command's box opens scrolled to its end; a read's to its start; reopening keeps the reader's offset. (Task 2)
- A command of one very long line is still clamped visually, and *Show all* offered. (Task 2)
- Multi-byte paths truncate at their start without panicking. (Task 4)
- A large diff past `LARGE_DIFF` is still offered rather than drawn. (Task 3)

---

### Task 1: Core words file verdicts and counts created files
`FileVerdict::verb()` (*Created*, *Edited*, *Deleted*) and `note()` (*new*, *deleted*, none); `Bucket::Create` so a cluster says *created N files*; `activity::creates(tool)` shared with the row's verb. Tests first.

### Task 2: One detail box
`parts/detail_box.rs`: `detail_box(DetailBox, cx)`, `box_count(total, drawn, from_end) -> Option<String>`, `command_cut(command) -> Option<usize>`. Two-axis `ScrollBody` with `from_end`. Commands, reads and searches move onto it; the show-earlier links go. Tests on the pure functions.

### Task 3: Diffs in the box
`diff_rows(.., fresh)` draws a new file plain, numbered; rows stretch so tints run the box's width; the preview and *Show N more lines* go; *Open* shows the diff whole.

### Task 4: A turn's changes
One file is one line; several are a head and a row a file with the path giving way at its start, *new*/*deleted*, counts in one right column, the ratio bar on changed files only. *Show every diff* and `toggle_every_file` go. An opened diff is the box.

### Task 5: Contract and checks
`DESIGN.md` *Transcript*, `CHANGELOG.md`, `catalog.md`; the spec's *As built*; every check.
