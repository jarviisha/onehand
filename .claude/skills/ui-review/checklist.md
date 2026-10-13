# UI review checklist

Answer each for the change: yes, no (with file and line) or n/a (with the reason). Each names the
document owning its rule; read it before answering.

## Tokens: `DESIGN.md` (*Type, space and shape*, *Colour and state*), the `design-tokens` skill

1. Every colour a theme slot or helper, with no hex, `rgb` or `hsla` outside `theme.rs`?
2. Every length a gpui step or a named constant, with no number inside `rems(` or `px(` at a call
   site, and pixels only for lines, rings, shadows, window bounds, an inset cancelling the
   library's, and the terminal's grid?
3. Every spacing on the steps `0p5`, `1`, `1p5`, `2`, `3`, `4`, `6`, `8`?
4. Every text size from the six roles, weights only `medium` and `semibold`, line heights only
   gpui's, `LEADING` and `LEADING_TIGHT`?
5. Every corner by role: `sm` a button, a text field, a checkbox or a key cap (buttons through
   `action`, fields through `text_input`, a `choice` included), `radius` a row, menu or chip
   picked among others and a block inside a column, `radius_lg` a standing box and a badge,
   `rounded_full` a dot?
6. Shadows only on what floats, through `lift` (the composer through its own)?
7. A new colour in both palettes, its contrast asserted in `theme.rs`'s tests?
8. No number or hex copied into a document or a comment?

## Components: `DESIGN.md` (*Components*, *Icons*), the `ui-components` skill

9. Nothing drawn by hand that the library or `catalog.md` draws?
10. Every button through `action` and with a variant by role (`primary`, `secondary`, ghost as
    `.quiet(cx)`, `danger`, or `choice` in a pick-one row); no button edged (never the library's
    default nor `outline()`); every field through `text_input`; every refusal through `.refuses()`,
    with a tooltip saying why?
11. Every state the component can be in: hover, selected or open, keyboard focus, refusing,
    loading?
12. Every control at the size its place takes: `.tall()` in a dialog's footer, a form and a page's
    head, `small` in a box or a strip, `xsmall` on a row, in a transcript block and as a chip; never
    the library's default size? 13. Every icon an `IconName` or registry SVG; every icon-only button
    with a tooltip? 14. A new builder listed in `catalog.md`, duplicating none listed there?

## Layout: `DESIGN.md` (*Layout*), `docs/ui-patterns.md` (*Layout*), the `layout-patterns` skill

15. The screen follows one pattern: page, box, strip, list, list and detail, form, dialog, detail?
16. Every split or stack reads the measured container, never the window?
17. Every list capped, saying how many it left out?
18. Text that can run long truncates or wraps, as its neighbours do?
19. A transition (presentation, back link, page switch, a dock hiding) keeps the state the
    contract keeps, and saves no narrow size over the layout?

## Behaviour: `docs/ui-patterns.md` (*Behaviour*), the `ux-patterns` skill

20. Waiting, empty, failed and refusing states each drawn by their pattern?
21. Every failure of something a person started reaches the screen once: a toast or a line, never
    both, never a log line alone?
22. Every toast's kind chosen by what it means?
23. Every removal that cannot be undone confirmed through `Confirm`: the thing named, *Keep*, then
    the verb in the danger tint?
24. Every new key a keymap command, every control bound to one showing its key?
25. Esc closes what opened; focus lands on the first field and returns on close; no focus left on
    something no longer drawn?
26. Every new word in sentence case, its ellipsis, verb and dismissal per *Words*, and the
    glossary's (`CONTEXT.md`)?

## Contract: `DESIGN.md`, `CLAUDE.md` (*Rules*)

27. At most one primary action per view (none is fine)?
28. Every departure from a rule listed under `DESIGN.md`'s *Exceptions*, agreed with the user?
29. `DESIGN.md`, `docs/ui-patterns.md` and the feature's own document still describe what the
    change draws?
30. No code, comment or runtime string citing a document?
31. No file past about 800 lines of code?
