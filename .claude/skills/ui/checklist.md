# UI review checklist

For an implementation change, answer each: yes, no (with file and line) or n/a (with the reason).
Name the evidence: code inspection, an automated check or the user's observation; never call a
visual judgment verified from a source scan. Each item names the section that owns its rule; read
the rule, not just the question. For prose-only work, use the contract path in `SKILL.md`.

1. `cargo test -p onehand --locked` passes, its source guards included? They hold what a scan
   can see (colour, length, spacing, type, corner, shadow, alpha, motion, the button and field
   wrappers, an edged button, the accent fill as text, icons, citations), each with its limits in
   `docs/rules-and-gotchas.md` (*Guard coverage*); the items below are what they cannot see.
2. A same-role component or screen named and compared for height, text axis, ink, icon size and
   slot, spacing, corner and states, and a shared change checked in its callers across app and
   feature crates (`DESIGN.md`, *Principles*)?
3. Each page a column page or a list page by what it holds, its filters in the shared order,
   and each group in the container its role takes (`docs/ui-patterns.md`, *Pages*, *Filter bars*,
   *Choose the container*)?
4. Every state the component can be in drawn, the combined ones (selected and hovered, selected
   and focused) kept apart, and a transition (a page switch, a back link, a dock hiding) keeping
   the state the contract keeps (`SKILL.md`, *States*)?
5. Every control at the size its role takes, and every control in one row of actions at one height (`docs/ui-patterns.md`, *Control sizes*)?
6. Every control drawn by the builder `catalog.md` lists for it, every button's variant named by
   its role, at most one primary in a view, and no ghost alone at the start of its line or
   beside a sentence (`DESIGN.md`, *Components*, *Colour and state*)?
7. A refusing button through `.refuses(why)` (`DESIGN.md`, *Components*), never `.disabled(`,
   which only a menu item or a switch takes, as `.refuses` does not reach them?
8. `rounded_full` only on a badge, a dot or a status mark (`DESIGN.md`, *Type, space and shape*)?
9. Every list capped, saying how many it left out, and text that can run long truncating or
   wrapping as its neighbours do (`docs/ui-patterns.md`, *Lists*, *Cuts*)?
10. Every split or stack reading the measured container, never the window (`docs/ui-patterns.md`,
    *List and detail*, *Boxes*)?
11. Attention signals (a state's hue, a count or badge of what waits, a dot, weight past the
    role's) only where something needs the person or has just changed, or on the mark of the
    control that switched a mode on (`DESIGN.md`, *Principles*)?
12. A change the person can take back shown at once, and every failure of something a person
    started said once: a toast of the kind its meaning takes or a line, never both and never a
    log alone (`docs/ui-patterns.md`, *Immediate feedback*, *Failing and refusing*, *Toasts*)?
13. Every removal that cannot be undone confirmed through `Confirm` (`docs/ui-patterns.md`,
    *Confirming a removal*)?
14. Every new key a keymap command, shown on its control, and focus landing, returning and never
    left on what is no longer drawn (`docs/ui-patterns.md`, *Keys and focus*)?
15. Every new word as *Words* has it, in the glossary's terms (`docs/ui-patterns.md`, *Words*;
    `CONTEXT.md`)?
16. The documents carried in the same change, a treatment that differs written as a rule of its
    role or recorded in `docs/known-gaps.md`, and no file past about 800 lines of code
    (`SKILL.md`, *Finishing*; `AGENTS.md`, *Rules*)?
17. The user's visual checks named: both themes, zoom, narrow and wide containers, long text and
    the states that apply, with what was observed kept apart from what is pending?
