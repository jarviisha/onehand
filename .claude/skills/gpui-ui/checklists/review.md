# UI review

Answer each question for the change you made, yes or no, with the file and line for every answer
that fails. A question that does not apply is *n/a*, with the reason.

Each question names where its rule is written: *App* is a section of the root `DESIGN.md`, *Labs*
a section of `labs/ui-labs/DESIGN.md`. Read that section before answering; the question is not the
rule. Read the same sections before drawing, for the region you touch.

## Values

1. Any literal size, gap, radius or colour at a call site, rather than from the source in the
   skill's route table (*Every value comes from*)? — *App:* Typography and spacing · *Labs:* the
   paragraph under the title.
2. Any constant named for its arithmetic rather than its role, or used for a role it is not named
   for? — *Labs:* the paragraph under the title.
3. Any number or hex copied into a document, a comment or this skill? — *Labs:* the paragraph under
   the title; *App:* the paragraph under the title.
4. A new surface or ink in the app without a contrast assertion in `theme.rs`'s tests? — *App:*
   Colour and state.

## Components

5. Any control drawn by hand that gpui-component provides? Any rule (a radius, a width) the
   contract sets for a library control that the call site leaves to the library's default? —
   *App:* Components · *Labs:* Components and icons.
6. Any button not made through `action`, or a control that can refuse without saying so? — *App:*
   Components · *Labs:* Components and icons.
7. Any icon that is not an `IconName` or registry SVG? An icon-only button without a tooltip? —
   *App:* Icons · *Labs:* Components and icons.
8. A new builder that duplicates one in `references/components.md`?

## Layout

9. Does a list-or-detail, split-or-stack decision read the window rather than the measured
   container? — *Labs:* Workbench, Pages · *App:* no rule yet.
10. Does any transition (presentation, back link, page switch, a dock hiding) lose state the
    contract keeps, or save a narrow size over the layout? — *App:* Layout (Persistence) ·
    *Labs:* Window.
11. Does focus stay on something no longer mounted? — *App:* the focus gotchas in `CLAUDE.md` ·
    *Labs:* Window.
12. Does any text overflow its container instead of truncating or wrapping as the contract says?
    — *Labs:* Type, spacing and shape (Truncation) · *App:* follow the screen's neighbours.

## Words and state

13. More primary actions than the contract allows where they sit? — *App:* Colour and state ·
    *Labs:* Components and icons (Which button).
14. Colour used for something the contract does not give it, or a shadow on something that does
    not float? — *App:* Principles 2 and 3 · *Labs:* Principles 4, Colour.
15. A label, back link or line of facts written against the contract's wording? — *Labs:* Type,
    spacing and shape; Pages · *App:* no rule written; follow the app's existing screens.
16. A list with no cap, or a capped list that does not say how many it left out? — *App:*
    Principles 7 · *Labs:* Principles 7.
17. A view without its empty, loading or error state? — empty: *App:* Layout (Pages without a
    session) · *Labs:* Pages. Loading and error have no rule in either yet; if the task settles
    how they look, write it into the binding `DESIGN.md`.
18. A weight or a type size the contract does not allow? — *App:* Typography and spacing ·
    *Labs:* Type, spacing and shape.
19. A destructive action that is not confirmed the way the contract says? — *App:* Transcript ·
    *Labs:* Deleting a project, Components and icons (Which button).

## Contract

20. Was a labs rule applied in `crates/app`, or an app rule in the lab, outside a promotion? —
    *Labs:* Departures from the app's DESIGN.md.
21. Does the binding `DESIGN.md` still describe what the change draws, and does a deliberate
    difference in the lab have its Departures row? — *App:* the `design-contract` skill · *Labs:*
    Departures from the app's DESIGN.md.
22. Does any code, comment or runtime string cite a document? — `CLAUDE.md`, Rules.
23. Did a file pass about 800 lines of code? — `CLAUDE.md`, Rules.

## Checks

Run the checks in the *Checks* column of the skill's route table for the context, and quote any
failure.

Do not launch the window. End with a short report: the items that failed and how each was fixed
or why it was left, the checks run and their result, and what the user should look at when they
open it.
