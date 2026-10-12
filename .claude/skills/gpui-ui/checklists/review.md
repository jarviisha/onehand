# UI review

Answer each question for the change you made, yes or no, with the file and line for every answer
that fails. A question that does not apply is *n/a*, with the reason.

Each question names the section of the root `DESIGN.md` where its rule is written. Read that
section before answering; the question is not the rule. Read the same sections before drawing,
for the region you touch.

## Values

1. Any literal size, gap, radius or colour at a call site, rather than from the source the
   skill lists under *Values come from*? — Typography and spacing.
2. Any number or hex copied into a document, a comment or this skill? — the paragraph under the
   title.
3. A new surface or ink without a contrast assertion in `theme.rs`'s tests? — Colour and state.

## Components

4. Any control drawn by hand that gpui-component provides? Any rule (a radius, a width) the
   contract sets for a library control that the call site leaves to the library's default? —
   Components.
5. Any button not made through `action`, or a control that can refuse without saying so? —
   Components.
6. Any icon that is not an `IconName` or registry SVG? An icon-only button without a tooltip? —
   Icons.
7. A new builder that duplicates one in `references/components.md`?

## Layout

8. Does a list-or-detail, split-or-stack decision read the window rather than the measured
   container? — no rule yet.
9. Does any transition (presentation, back link, page switch, a dock hiding) lose state the
    contract keeps, or save a narrow size over the layout? — Layout (Persistence).
10. Does focus stay on something no longer mounted? — the focus gotchas in `CLAUDE.md`.
11. Does any text overflow its container instead of truncating or wrapping? — follow the screen's
    neighbours.

## Words and state

12. More primary actions than the contract allows where they sit? — Colour and state.
13. Colour used for something the contract does not give it, or a shadow on something that does
    not float? — Principles 2 and 3.
14. A label, back link or line of facts written against the contract's wording? — no rule
    written; follow the app's existing screens.
15. A list with no cap, or a capped list that does not say how many it left out? — Principles 7.
16. A view without its empty, loading or error state? — empty: Layout (Pages without a session).
    Loading and error have no rule yet; if the task settles how they look, write it into
    `DESIGN.md`.
17. A weight or a type size the contract does not allow? — Typography and spacing.
18. A destructive action that is not confirmed the way the contract says? — Transcript.

## Contract

19. Does `DESIGN.md` still describe what the change draws? — the `design-contract` skill.
20. Does any code, comment or runtime string cite a document? — `CLAUDE.md`, Rules.
21. Did a file pass about 800 lines of code? — `CLAUDE.md`, Rules.

## Checks

Run the checks the skill lists under *Checks*, and quote any failure.

Do not launch the window. End with a short report: the items that failed and how each was fixed
or why it was left, the checks run and their result, and what the user should look at when they
open it.
