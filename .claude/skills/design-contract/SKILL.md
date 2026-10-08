---
name: design-contract
description: Read before changing anything the user sees — a panel, a dock, a rail row, a colour, a radius, a size, an icon, a transcript block, a dialog, a composer control. DESIGN.md is the binding UI overview.
---

# The UI contract

Read `DESIGN.md` before the edit, at the version in the working tree. It covers layout, the
transcript, typography, colour, components and icons. It describes structure and behaviour, never
values: every colour, radius and size comes from `cx.theme()`, and sizes are rems. Its
*Typography and spacing* says where any other size lives, and a source guard holds new code to
it.

- **It is binding at the level it is written.** Below that, the code and its tests decide, and
  a feature's own document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) holds the
  detail of its screens. Never write the same detail in both places.
- **A visible change updates it in the same pull request.** If a change alters what a screen
  holds, its order, or a rule `DESIGN.md` states, the matching lines change with it.
- **It stays an overview, on a budget.** A new screen or feature gets at most six lines: what it
  holds, in what order, and what it says when empty. The whole file stays under about 250 lines;
  past that, merge or cut before adding.
- **Code never cites it.** Comments, doc comments and runtime strings give their reason in their
  own words. A source guard checks line comments; review block comments and runtime strings too.
