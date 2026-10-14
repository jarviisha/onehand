# UI review checklist

For an implementation change, answer each: yes, no (with file and line) or n/a (with the reason).
Name the evidence: code inspection, automated check or user observation. Do not call a visual
judgment verified from a source scan. For prose-only work, use the contract path in `SKILL.md`.
Each section names the document owning its rules; read it before answering.

## Tokens: `DESIGN.md` (*Type, space and shape*, *Colour and state*), the `design-tokens` skill

1. Every colour a theme slot or helper, with no hex, `rgb` or `hsla` outside `theme.rs`?
2. Every length a gpui step or a named constant, with no number inside `rems(` or `px(` at a call
   site, and pixels only for lines, rings, shadows, window bounds, an inset cancelling the
   library's, and the terminal's grid?
3. Every spacing on the steps `0p5`, `1`, `1p5`, `2`, `3`, `4`, `6`, `8`?
4. Every text size from the six roles, weights only `medium` and `semibold`, line heights only
   gpui's, `LEADING` and `LEADING_TIGHT`?
5. Every corner the one `radius`, a badge at `tokens::BADGE_CORNER` and a dot `rounded_full`, with
   no `radius_lg` or `radius_tokens().sm` written?
6. Shadows only on what floats, through `lift` (the composer through its own)?
7. A new colour in both palettes, its contrast asserted in `theme.rs`'s tests?
8. Token values kept in code and its token reference, with no duplicate palette or scale in
   contracts, skills or call-site comments?

## Components: `DESIGN.md` (*Components*, *Icons*), `docs/ui-patterns.md` (*Control sizes*), the `ui-components` skill

9. Nothing drawn by hand that the library or `catalog.md` draws?
10. Every button through `action` and with a variant by role (`primary`, `secondary`, ghost as
    `.quiet(cx)`, `danger`, or `choice` in a pick-one row); no button edged (never the library's
    default nor `outline()`); every field through `text_input`, `search_input` or `text_area`; every
    refusal through `.refuses(why)`, its reason the tooltip?
11. Every state the component can be in: hover, selected or open, keyboard focus, refusing, loading?
12. Every control at the size its role takes: `.tall()` in a form and a dialog's footer;
    `.small()` in a page's head, a filter bar, a strip, a box and for a row's actions; `.xsmall()` only for a
    secondary action inside meta text, a transcript block or a chip; never the library's default;
    an icon at its role's size, not grown with the hit area; a badge, checkbox or switch at the
    library's own geometry, centred; any size correction in the shared wrapper, not the call site?
13. Every icon an `IconName` or registry SVG; every icon-only button with a tooltip?
14. A new builder listed in `catalog.md`, duplicating none listed there?

## Layout: `DESIGN.md` (*Layout*), `docs/ui-patterns.md` (*Layout*), the `layout-patterns` skill

15. Each group uses the right pattern: plain section, box, strip, list, list and detail, form,
    dialog or detail within its page, without unnecessary nested frames?
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
28. Every departure from a rule covered by the user's authorized scope and recorded under
    `DESIGN.md`'s *Exceptions*, without silently changing the contract to fit existing code?
29. `DESIGN.md`, `docs/ui-patterns.md` and the feature's own document still describe what the
    change draws?
30. No code, comment or runtime string citing a document?
31. No file past about 800 lines of code?

## Consistency: `DESIGN.md` (*Principles*, *Colour and state*, *Icons*), `docs/ui-patterns.md` (*Layout*, *Waiting*)

32. An equivalent component elsewhere identified and compared for height, text axis, icon size,
    spacing, corner and states; shared changes checked in their affected app and plugin callers?
33. Main content remains the visual anchor, with neutral chrome, meaningful local colour and
    no extra surface for an ordinary nested group?
34. Spacing follows relationships as well as the legal scale; equivalent strips align actions,
    page names are not repeated and related pages use the same column role?
35. Density follows the content: compact chrome, readable prose, multi-line rows allowed to
    grow, and narrow views handled without shrinking type or hit targets?
36. Hover actions keep their slots and are available through keyboard focus; loading does not
    shift neighboring content; selected + hovered and selected + focused remain distinct?
37. Required user visual checks named for both themes, zoom, narrow containers, long text and
    relevant empty/loading/failure states, with observed results separated from pending checks?

## Calm: `DESIGN.md` (*Principles*, *Type, space and shape*, *Icons*), `docs/ui-patterns.md` (*Hierarchy*, *Separation*, *Page heads and filters*, *Immediate feedback*, *Motion*)

38. Text on a neutral surface in one of the three neutral inks by its role, text on a fill in
    that fill's paired ink, a state, a link and code in their semantic inks, and no alpha on
    text or on a container to quieten it?
39. Attention signals (a state's hue, a count or badge of what waits, a dot, weight past the
    role's) only where something needs the person or has just changed, while hierarchy and
    identity (a heading's weight, a list's muted count, a label's kind) stay as their roles give?
40. Separation by the ladder (space, then a line, then a fill, then a shadow), and no boundary
    drawn twice?
41. A page with a head and filters drawing them in the shared order?
42. A change the person can take back shown at once; saves of one thing run in order; a failed
    save, only while its change is the newest, put back to the last saved value with one toast;
    and nothing animated but a `Spinner` on running work (the library's overlays and controls
    aside)?
43. Icons at one size per role, in slots of one width on one centre line, none on a tile of its
    own; numbers and times in fixed-width slots?
