---
name: design-contract
description: Read before changing anything the user sees — a panel, a dock, a rail row, a colour, a radius, a size, an icon, a transcript block, a dialog, a composer control. DESIGN.md is the whole-app visual contract, DESIGN-ANSWER.md the transcript's design language, DECISIONS.md (D1–D8) the choices reading the code will not explain. All three are binding.
---

# The UI contracts

Read before the edit, not after:

- `DESIGN.md` — the whole-app visual contract: chrome, docks, rail, dialogs, surfaces.
- `DESIGN-ANSWER.md` — the transcript's design language, one section per block kind.
- `DECISIONS.md` — D1–D8, read before changing theme, appearance, editor or Neovim scope,
  icons, plugins, GPUI source identity or the vendored-terminal strategy.

## What they no longer carry

Neither design document holds a palette. gpui-component's theme is the look, so both describe
*structure and behaviour* only — every colour, radius and size is read from `cx.theme()` at the
call site. Sizes are rems, never pixels, because zoom overrides the rem base per panel.

## They are living, and that cuts both ways

These contracts move with the UI. So:

- **Read them at the version in the working tree**, never from memory of an earlier read.
- **A change that alters structure or behaviour they describe updates them in the same change.**
  A contract left behind the code it binds is one the next reader follows into a frame that no
  longer exists, which is worse than no contract at all.
- **A block the contract asks for and the code does not draw is marked *(not rendered)* with the
  reason**, not quietly deleted from the document. A missing feature nobody wrote down reads as a
  bug in the ones that exist.

## Code describes; it never cites

Source files — comments, doc comments, runtime strings — may not name these documents or any
section, anchor or item code belonging to one. Say the reason in the comment's own words, in full,
so the comment stands alone. Pointing at *code* stays fine. The traffic runs one way: documents
point at code, code does not point back. A test enforces this.
