---
name: design-contract
description: Read before changing anything the user sees — a panel, a dock, a rail row, a colour, a radius, a size, an icon, a transcript block, a dialog, a composer control. DESIGN.md is the binding UI overview.
---

# The UI contract

Read `DESIGN.md` before the edit, at the version in the working tree. It covers layout, the
transcript, typography, colour, components and icons. It describes structure and behaviour, never
values: every colour, radius and size comes from `cx.theme()`, and sizes are rems.

- **It stays an overview.** If a change alters the layout or a rule it states, update it in the
  same change. Do not add per-widget detail; that belongs in the code and its tests.
- **Code never cites it.** Comments, doc comments and runtime strings give their reason in their
  own words. A test enforces this.
