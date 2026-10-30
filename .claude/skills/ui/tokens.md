# Token reference

A lookup, not the rules: which token serves which role is ruled in `DESIGN.md`. Values here are
the ones the code holds; when this file and the code disagree, the code is right and this file is
fixed in the same change. Its last section says how to add a token.

For role selection, start with [the UI contract](../../../DESIGN.md); for spacing relationships
and density, use [UI patterns](../../../docs/ui-patterns.md#spacing-and-density).

## Colour

Read from `cx.theme()`. They are gpui-kit's Default Light and Default Dark as the library ships
them; `crates/app/src/theme.rs` installs them unchanged, with contrast tests. The terminal's
sixteen colours are `onehand_terminal_ui::ansi`. Roles: `DESIGN.md`, *Colour and state*.

Slot names that mislead (the library's names, kept because its own controls read them):

| Slot | Holds |
|---|---|
| `accent`, `accent_foreground` | the selected fill and its ink: a fill, not a hue |
| `link` | a link, in the prose ink |
| `info` | the library's blue: what runs |
| `muted` | the well's fill, and the user's bubble |
| `secondary` | a step past the well: code inside the bubble |
| `button_secondary`, `button_secondary_foreground` | the secondary button's own fill at rest (its pointed-at step under the pointer or pressed) and its ink |
| `primary` | the primary action: near black with white on it (light), near white with black on it (dark) |
| `danger`, `warning`, `success` | fills with white on them; their text is `status_ink(cx)` |
| `sidebar` | the rail's fill, the docks' too, read for a dock through `dock_surface(cx)` |
| `sidebar_accent` | the rail's hovered row, and its chosen row through `rail_chosen(cx)` |

Helpers:

| Helper | Gives | Lives in |
|---|---|---|
| `status_ink(cx).danger`, `.warning`, `.success`, `.closed` | status text and marks; `closed` for what is over and settled (a closed issue) | `crates/ui/src/ink.rs`; the app says `crate::theme::status_ink` |
| `tint(base, cx)` | a label's `Tint { fill, ink }` by its hue | `crates/ui/src/ink.rs` |
| `dock_surface(cx)` | the Workbench's and the terminal's surface | `crates/ui/src/surface.rs` |
| `badge_edge(tint)` | a badge's edge: halfway from its `Tint`'s fill to its ink | `crates/ui/src/badge.rs` |
| `control_fill(cx)` | a ghost control's fill hovered, pressed or chosen; between `list_hover` and `accent` | `crates/ui/src/ink.rs` |
| `faint_rule(cx)` | a hairline a step fainter, halfway to the surface: the line under a page's top, and a list page's line over its foot | `crates/ui/src/ink.rs` |
| `meta_ink(cx)` | the middle ink: a detail beside a name, between prose and muted | `crates/ui/src/ink.rs`; the app says `crate::theme::meta_ink` |
| `middle_ink(full, meta)` | the same step under another full ink: on a fill, under that fill's paired ink | `crates/ui/src/ink.rs` |
| `hue_ink(base, cx)` | a hue tempered for ink: code inside prose | `crates/app/src/theme.rs` |
| `raised(cx)` | the composer card's fill: the page's | `crates/app/src/theme.rs` |
| `inset(cx)` | a badge in the rail: the page's fill | `crates/app/src/theme.rs` |
| `rail_chosen(cx)` | the rail's chosen row | `crates/app/src/theme.rs` |
| `floating_well(cx)` | a well on a floating card, the command a pinned permission asks to run: the library's `muted` | `crates/app/src/theme.rs` |

Inks (roles: `DESIGN.md`, *Type, space and shape*; by place: UI patterns, *Hierarchy*). The three
neutral ones, for text on a neutral surface:

| Ink | Read as | A feature crate reaches it |
|---|---|---|
| Full | `cx.theme().foreground` | yes |
| Middle | `meta_ink(cx)`; on a fill, `middle_ink(pair, muted_foreground)` | yes |
| Muted | `cx.theme().muted_foreground`, which a rail row's second line takes | yes |

Text on a fill takes the ink paired with it:

| Fill | Its ink |
|---|---|
| `primary`, a solid `danger` | `primary_foreground`, `danger_foreground`, set by the button's variant |
| `button_secondary`; a `quiet` ghost | `button_secondary_foreground`; `secondary_foreground`, set by the builder |
| a ghost's hover or selected fill, under an icon alone (`quiet_icon`) | `secondary_foreground`, as at rest |
| `accent`, the selected row | `accent_foreground` |
| `rail_chosen(cx)`, the rail's current row | `sidebar_accent_foreground` |
| `muted` (a well, the bubble) | the neutral inks, as on the surface: a neutral fill, not a paired one |
| a badge's `Tint` fill | that `Tint`'s `ink` |

Semantic inks, on any surface: `status_ink(cx)` for a state, `link` for a link or what runs,
`hue_ink(base, cx)` for code in prose.

## Spacing

gpui's methods (`p_*`, `px_*`, `pt_*`…, `m*_*`, `gap_*`) on these steps; `0`, `auto` and `full`
are not steps and stay allowed. A named spacing constant (`BLOCK_GAP`, `TIGHT_GAP`…) equals one.

| Step | rem | px at the 16px base |
|---|---|---|
| `0p5` | 0.125 | 2 |
| `1` | 0.25 | 4 |
| `1p5` | 0.375 | 6 |
| `2` | 0.5 | 8 |
| `3` | 0.75 | 12 |
| `4` | 1 | 16 |
| `6` | 1.5 | 24 |
| `8` | 2 | 32 |

## Type

| Write | rem | px |
|---|---|---|
| `tokens::MICRO_TEXT` | 0.6875 | 11 |
| `.text_xs()` | 0.75 | 12 |
| `tokens::CODE_TEXT` | 0.8125 | 13 |
| `tokens::CHROME_TEXT` | 0.875 | 14 |
| `.text_sm()` | 0.875 | 14 |
| `.text_base()` | 1 | 16 |
| `.text_lg()` | 1.125 | 18 |
| `.text_xl()` | 1.25 | 20 |

- Weights: `.font_medium()` (500), `.font_semibold()` (600).
- Line height: gpui's default is 1.618; `tokens::LEADING` 1.6; `tokens::LEADING_TIGHT` 1.4.
- Families: `cx.theme().font_family` (the bundled UI family, `fonts::UI_FAMILY`) and
  `cx.theme().mono_font_family` (the bundled mono family, `fonts::MONO_FAMILY`, unless
  `[font].monospace` names another).

## Corners

| Write | px |
|---|---|
| `cx.theme().radius` | 6, the library's |
| `cx.theme().radius_lg` | 8, the library's: large surfaces |
| `.rounded_full()` | a dot, a status mark |

The theme leaves both at the library's values, so its tooltips, menus, popovers, dialogs and
toasts take them. What the library draws at half its corner (`radius_tokens().sm`: checkboxes,
`Kbd`, its tabs) comes out at 3px and is left to it. A button
(`action`, `ButtonRounded::Medium`) and a text field (`text_input`, `search_input`, `text_area`)
set `radius` where they are built; a `Tag` keeps the library's corner through `badge`.

## Elevation

| Write | For | Lives in |
|---|---|---|
| `tokens::lift(cx)` | anything the app floats | `crates/ui/src/tokens.rs` |
| `composer_lift(cx)` | the composer card | `crates/app/src/theme.rs` |

## Control sizes

What each size measures in each component, as the pinned `gpui-component` draws it (its
`sizing.rs`, `button/button.rs`, `icon.rs`, `tag.rs`, `checkbox.rs`, `switch.rs`, `kbd.rs`); which
role takes which size is UI patterns, *Control sizes*. Pixels at the 16px base. **px** marks a size
the library holds in pixels, which per-panel zoom does not scale; everything else is in rems.
Moving the library's rev means checking this table.

| Size | Button: height / words / icon / icon–label gap / inset | Input: height / words / inset | Icon alone |
|---|---|---|---|
| `.xsmall()`: an icon-only button inside a small object only | 20 / 12 / 12 / 4 / 4 | 20 / 12 / 4 **px** | 12 |
| `.short()` (`CONTROL_H`): the library's small | 24 / 14 / 14 / 4 / 8 | 24 / 14 / 8 **px** | 14 |
| `.small()`: an icon-only button beside `.short()` | 24 / 14 / 14 / 4 / 8 | 24 / 14 / 8 **px** | 14 |
| `.tall()` (`FIELD_H`): the library's medium | 32 / 14 / 16 / 8 / 10 | 32 / 14 / 10 **px** | 16 |

An icon-only button is square at its height. `.short()` is the library's small size held at
`CONTROL_H` and `.tall()` its medium at `FIELD_H` (`Short` and `Tall` in
`crates/ui/src/control_size.rs`); their words, icons and insets are the library's. A builder that
sizes the controls it makes (`choice`, `switch`, `picker`) takes one of them as a `ControlSize`.

A field of several lines (`text_area`) is the library's `Textarea`, which takes no size: its text
sits at the default's inset, 10 **px**, which no wrapper reaches, so it starts 2px inside a
`.short()` field's above it.

Components that keep geometry of their own, centred in their row rather than sized to it:

| Component | Geometry |
|---|---|
| `Tag` at `.small()` through `badge` | words 12 set solid (`badge` overrides the library's 10 at 1.25), inset 6 × 4; its height follows its line |
| `Checkbox` | box 12 / 14 / 16 at `.xsmall()` / `.small()` / default; its label 12 / 14 / 16 |
| `Switch` | track 28 × 16 **px** at `.xsmall()` and `.small()`, 36 × 20 **px** at the default; thumb 12 / 16 **px** |
| `Kbd` | words 12, inset 4 × 2 |
| The library's table rows | 26 / 30 / 32 **px** at `.xsmall()` / `.small()` / default; the app draws its own rows at `ROW_H` |

## Shared sizes

All in `onehand_ui::tokens` (`crates/ui/src/tokens.rs`), which the app names
`crate::tokens`.

| Token | Value | What |
|---|---|---|
| `BAR_H` | 2.75rem | every strip along the top of a region |
| `MODE_BAR_H` | 2rem | a mode's own bar under the Workbench's strip (the editor's tabs, the reader's bar) |
| `ROW_H` | 2rem | a list row (the rail, the composer's popup) |
| `TREE_ROW_H` | 1.5rem | a Workbench tree's row pitch (Files, Markdown) |
| `TREE_ROW_FILL` | 1.3125rem | a tree row's fill: 21px, odd so the name centres on a whole pixel |
| `TREE_ROW_ABOVE`, `TREE_ROW_BELOW` | 0.0625rem, 0.125rem | the room over and under a tree row's fill; between two fills, their sum |
| `CONTROL_H` | 1.5rem | a small control: `.short()`, and an icon-only `.small()` button |
| `FIELD_H` | 2rem | the tallest control, a step over `.short()`: a page's head and form (`.tall()`); nothing stands taller |
| `SCROLLBAR_THUMB`, `SCROLLBAR_THUMB_ACTIVE` | 4px, 6px | a scrollbar's thumb at rest, and hovered or dragged |
| `CHROME_TEXT` | 0.875rem | chrome text: a panel's rows, boxes, forms and strips, at the library's control size |
| `MICRO_TEXT` | 0.6875rem | a caption inside a small object: an attachment's plate |
| `CODE_TEXT` | 0.8125rem | machine text |
| `LEADING` | 1.6 | reading text |
| `LEADING_TIGHT` | 1.4 | a wrapped title |
| `PAGE_NARROW` | 35rem | a one-column page |
| `PAGE_PROSE` | 48rem | an editor, or one thing read in full |
| `PAGE_WIDE` | 64rem | boxes side by side, a table |
| `DIALOG_FORM` | 40rem | a dialog that holds a form (the agent form, one that starts a run), its controls `.tall()`, never wider than the window less its margins |
| `ROW_INSET` | 1rem | where a tree row's content starts from a dock's edge (Files, Markdown), and a Workbench bar's words |
| `ROW_GUTTER` | 0.5rem | how far a tree row's fill stands off the dock's side edges, inside `ROW_INSET` |
| `TREE_INDENT` | 0.875rem | how far each level of a tree steps in |
| `SCROLLBAR_W` | 16px | room held clear beside text the library's scrollbar scrolls (the transcript's boxes, the Markdown reader) |
| `lift(cx)` | | the floating shadow |

Equal values kept apart on purpose, each one decision beside its one use: `CHAT_MIN`,
`NARROW_PANEL` and `MODES_MIN_W` (30rem); `CONTENT_COLUMN` and `COMPOSER_COLUMN` (44rem).

## Adding a token

- **A colour**: first a library token for the role. A step no token holds is derived from the
  tokens with `mix_oklab`, as `meta_ink` and `tint` are, in `crates/app/src/theme.rs` (or
  `onehand_ui::ink` when a feature crate needs it), its contrast asserted in that file's tests
  against every surface it lands on; never a value of its own.
- **A size one module uses**: a `const` of type `Rems` beside its use, its reason in its doc
  comment. A spacing value equals a step.
- **A size a second module needs**: move the constant into `onehand_ui::tokens` in that
  change, and point both modules at it.
- **A new step in a scale** (a type size, a corner, a spacing step, an elevation): a system change.
  Use the user's authorized scope; when it covers the system, update the owning contract and
  implementation together. Otherwise use an existing role or raise the specific unresolved decision.
- List the token in this file in the same change.
