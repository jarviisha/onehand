# A lighter, bluer, roomier visual language

Spec for work under way. It says how the change is to be built; the owning documents
(`DESIGN.md`, `docs/ui-patterns.md`, `.claude/skills/ui/tokens.md`) say what the build does once
each step lands.

## Intent

The owner works in the dark mode and finds it too dark and dated: the library's Default Dark
puts every region on near black (`neutral-950`) a hair apart, so the rail, the conversation and
the docks read as one black slab held apart only by hairlines. The reference they picked, from
mockups, is the current Jira dark look. They named four things they want from it:

1. **Its surfaces**: neutral charcoal, clearly lighter than black, each region a visible step.
2. **Its blue as the mark of where you are**: the selected item stands on a blue tint in blue
   ink, an open tab is lettered in blue, and the same blue is the primary action.
3. **Its room**: taller rows and bars.
4. **Its type size is not wanted**: chrome and reading text stay at 13px.

## Goals

- Both modes take a palette of onehand's own, installed through the library's theme configs so
  library controls and the app's parts still read one palette.
- Selection reads as blue everywhere a row, a rail entry or a tab is the chosen one of several.
- Rows, tree rows, strips and controls stand a step taller.
- The contract documents say the new rules in the same PR that makes them true.

## Non-goals

- The window's frame: regions stay flush and parted by 1px lines. No floating panels, no gaps
  between regions, no moved region.
- Type sizes, weights, font families, the corner radii (6px and 8px stay).
- Status badges with fills, or chips round tool names. State stays a dot or an ink, as now.
- The terminal's sixteen colours, a chart's series, a label kind's hues.
- A theme picker, or keeping the old palette selectable. Git keeps it.
- New motion.

## Anti-goals

- A hue that means nothing. Blue gains exactly one meaning (here you are) beside the one it has
  (press this); surfaces and inks stay neutral, and hover stays a neutral fill.
- Two modes under two rule sets. Light gets the same rules as dark.
- A hex literal at a call site. The palette's values are written in one place and nowhere else.
- Any pair of ink and surface falling under what `crate::theme`'s contrast tests hold today.

## Constraints

- `gpui-component`'s own controls read the theme's slots, and `accent` is the library's *hover*
  as much as its selection: a ghost button's hover (every quiet button), a popup menu item's
  hover and highlight, a toggle's pressed fill, the command list's highlighted row. Those cannot
  be restyled from outside. So **`accent` stays a neutral step** and the blue selection is a role
  of onehand's own (*Blue marks where you are*). The library's `list_active` slot, its own
  selected-list fill, takes the blue tint so a library list agrees with the app's rows.
- `onehand_ui::row` is the one builder for a picked row and `onehand_ui`'s tab builder for a tab;
  the rail's chosen row goes through `theme::rail_chosen`. About ten call sites read
  `theme.accent` directly for a chosen item (the stepper, the rail's workspace row, the composer's
  rows, the issue dialog, a workflow step, the issues views); each is sorted by what it means,
  chosen or merely pointed at, and a chosen one moves to the shared role.
- Sizes are rems; fixed chrome heights stay outside the zoom wrapper.
- Code never cites a document. Every `.md` is English.
- The UI skill's implementation path applies to each PR: its checklist, the checks, the
  documents carried, and no claim of visual verification. The owner looks in `make dev`.

## Approaches

1. **Theme first, tokens second** (picked). Install the palette once in `theme::install`, retarget
   the selection role, then raise the height tokens. Roles already route through theme slots and
   `onehand_ui::tokens`, so most screens follow without edits; the app is never half old.
   Cost: each step changes the whole window at once, so the owner reviews the whole app twice.
2. **Region by region** (rail, chat, Workbench, pages). Each region tuned on its own, but regions
   disagree while the work runs, against "same role, same treatment", and it needs temporary
   parallel tokens.
3. **A second, selectable theme.** Reversible, but the selection rule cannot be both grey and
   blue, so two rule sets and two test sets would be kept for no asked-for benefit.

## Design

### Palette

Written as hex in the theme configs inside `crate::theme` (or one JSON asset that module loads:
builder's call), the only place a colour value lives. `DESIGN.md` stops saying the palette is the
library's; it says the palette is onehand's own, installed through the library's theme so library
parts read it, and that a derived step is still mixed from tokens, never written.

Starting values, from the approved mockup. They are targets to tune against the contrast tests,
not figures to defend: where a test fails, move the ink, keep the surface.

| Role (slot) | Dark | Light |
|---|---|---|
| Reading surface (`background`) | `#1f1f21` | `#ffffff` |
| Rail and docks (`sidebar`) | `#18191a` | `#f8f8f8` |
| Title bar, tab bar (`title_bar`, `tab_bar`) | `#18191a` | `#f8f8f8` |
| Well, quiet fill (`muted`) | `#2b2c2f` | `#f0f1f2` |
| Past the well (`secondary`) | `#303134` | `#e7e8ea` |
| Floating surface (`popover`) | `#2b2c2f` | `#ffffff` |
| Hairline (`border`) | `#303134` | `#dddee1` |
| A box's and a field's edge (`input`) | `#3a3b3f` | `#c9cace` |
| Content ink (`foreground`) | `#e2e3e6` | `#292a2e` |
| Muted ink (`muted_foreground`) | `#96999e` | `#6b6e76` |
| Hover on a row (`list_hover`, `sidebar_accent`) | a neutral step over its surface | same rule |
| Primary (`primary`, hover, active) | `#669df1`, lighter under the pointer | `#1868db`, darker under the pointer |
| Ink on the primary | `#1f1f21` | `#ffffff` |
| Hover and highlight on a control or a menu row (`accent`, `accent_foreground`) | a neutral step, the content ink | same rule |
| Chosen fill (onehand's own role; also `list_active`) | `#1c2b42` | `#e9f2fe` |
| Chosen ink (onehand's own role) | `#669df1` | `#1868db` |

- The composer card (`theme::raised`), the waiting card, the floating well and every other
  derived step keep their derivations; their tests keep their thresholds. Where a derivation
  assumed near-black surfaces (a floating surface told apart by its edge, not its shadow), the
  builder re-derives it and rewrites the sentence in `DESIGN.md` to match.
- Status, info, link, ring and the base hues stay the library's, unless a contrast test fails on
  a new surface; then the ink is tempered as `status_ink` already does.

### Blue marks where you are

- The chosen fill and the chosen ink are two helpers in `onehand_ui::ink`, beside `control_fill`,
  their values in the palette with the rest; feature crates reach them there.
- A picked row (`onehand_ui::row`): the chosen fill and the chosen ink. A quieter ink on it is
  `middle_ink(chosen ink, muted_foreground)`.
- The rail's chosen row: the same fill and ink (`rail_chosen` returns the chosen fill, or goes
  away in favour of the shared helper: builder's call).
- An open tab: its 2px primary mark stays where it is, and its label takes the chosen ink.
- The row the keys are on inside a popup (the composer's completion rows, a menu, the command
  list) is a cursor, not a location: it stays the neutral highlight.
- Hover stays neutral and distinct from selected; a hovered selected row stays blue.
- Keyboard focus stays the grey ring.
- `DESIGN.md` Principle 2 and *Colour and state* are rewritten: blue is the primary action and
  the current location; `accent` becomes the neutral hover and highlight, and the chosen pair is
  named as the selection. The slot table and the helper table in tokens.md follow.

### Room

Heights rise one step; text sizes do not move. Starting values, tuned by the owner in `make dev`:

| Token | Now | Target |
|---|---|---|
| `ROW_H` | 24px | 28px |
| `TREE_ROW_H` (and its fill and room, kept in proportion) | 22px | 26px |
| `BAR_H` | 35px | 40px |
| `MODE_BAR_H` | 30px | 34px |
| `CONTROL_H` (`.short()`) | 22px | 24px |
| `FIELD_H` (`.tall()`) | 26px | 28px |

Gaps and paddings keep their steps (*Spacing and density* is unchanged) unless a taller row or
bar leaves something off its axis; that is fixed in the shared builder. More room than this is a
later request once the owner has seen it.

## Steps

One PR each; the contract text moves with the code that makes it true, so no document claims
what the build does not do.

1. **Palette and selection.** `theme::install`, the chosen role and its call sites, the tab label; contrast tests
   extended to the new pairs (selected ink on selected fill, primary ink on primary, inks on each
   new surface) in both modes; `DESIGN.md`, tokens.md, `docs/known-gaps.md`'s contrast list.
   This PR also carries this spec.
2. **Room.** The height tokens and whatever builders they unsettle; `DESIGN.md` *Components*,
   `docs/ui-patterns.md` *Control sizes*, tokens.md. This PR removes this spec and its plans.

## Left to the builder

- Hex in Rust or a JSON asset; whether `rail_chosen` survives.
- The exact neutral hover steps, and every derived step.
- Final values wherever a contrast test or an axis forces a move from the starting ones.
- Which contrast pairs need new tests beyond those named.
