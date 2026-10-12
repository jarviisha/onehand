# UI review

Answer each question for the change you made, yes or no, with the file and line for every answer
that fails. A question that does not apply is *n/a*, with the reason.

Each question names the section of the root `DESIGN.md` where its rule is written. Read that
section before answering; the question is not the rule. Read the same sections before drawing,
for the region you touch.

## Values

1. Any literal size, gap, radius or colour at a call site, rather than from the source in the
   skill's route table (*Every value comes from*)? — Typography and spacing.
2. Any constant named for its arithmetic rather than its role, or used for a role it is not named
   for?
3. Any number or hex copied into a document, a comment or this skill? — the paragraph under the
   title.
4. A new surface or ink without a contrast assertion in `theme.rs`'s tests? — Colour and state.

## Components

5. Any control drawn by hand that gpui-component provides? Any rule (a radius, a width) the
   contract sets for a library control that the call site leaves to the library's default? —
   Components.
6. Any button not made through `action`, or a control that can refuse without saying so? —
   Components.
7. Any icon that is not an `IconName` or registry SVG? An icon-only button without a tooltip? —
   Icons.
8. A new builder that duplicates one in `references/components.md`?

## Layout

9. Does a list-or-detail, split-or-stack decision read the window rather than the measured
   container? — no rule yet.
10. Does any transition (presentation, back link, page switch, a dock hiding) lose state the
    contract keeps, or save a narrow size over the layout? — Layout (Persistence).
11. Does focus stay on something no longer mounted? — the focus gotchas in `CLAUDE.md`.
12. Does any text overflow its container instead of truncating or wrapping? — follow the screen's
    neighbours.

## Words and state

13. More primary actions than the contract allows where they sit? — Colour and state.
14. Colour used for something the contract does not give it, or a shadow on something that does
    not float? — Principles 2 and 3.
15. A label, back link or line of facts written against the contract's wording? — no rule
    written; follow the app's existing screens.
16. A list with no cap, or a capped list that does not say how many it left out? — Principles 7.
17. A view without its empty, loading or error state? — empty: Layout (Pages without a session).
    Loading and error have no rule yet; if the task settles how they look, write it into
    `DESIGN.md`.
18. A weight or a type size the contract does not allow? — Typography and spacing.
19. A destructive action that is not confirmed the way the contract says? — Transcript.

## Contract

20. Does `DESIGN.md` still describe what the change draws? — the `design-contract` skill.
21. Does any code, comment or runtime string cite a document? — `CLAUDE.md`, Rules.
22. Did a file pass about 800 lines of code? — `CLAUDE.md`, Rules.

## Checks

Run the checks in the *Checks* column of the skill's route table, and quote any failure.

Do not launch the window. End with a short report: the items that failed and how each was fixed
or why it was left, the checks run and their result, and what the user should look at when they
open it.
