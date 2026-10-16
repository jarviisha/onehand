# Token reference

A lookup, not the rules: which token serves which role is ruled in `DESIGN.md`. Values here are
the ones the code holds; when this file and the code disagree, the code is right and this file is
fixed in the same change.

For role selection, start with [the UI contract](../../../DESIGN.md); for spacing relationships
and density, use [UI patterns](../../../docs/ui-patterns.md#spacing-and-density).

## Colour

Read from `cx.theme()`. The ramp in `crates/app/src/theme.rs` writes them for both palettes, with
contrast tests; the terminal's sixteen colours are `onehand_terminal_ui::ansi`. Roles, the
palette's source, keys and values: `DESIGN.md`, *Colour and state*.

Slot names that mislead (the library's names, kept because its own controls read them):

| Slot | Holds |
|---|---|
| `accent`, `accent_foreground` | the selected fill and its ink: a fill, not a hue |
| `link` (also `info`) | the accent hue: what runs, and links |
| `muted` | the well's fill, and the user's bubble |
| `secondary` | a step past the well: code inside the bubble |
| `button_secondary`, `button_secondary_foreground` | the secondary button's own fill at rest (its pointed-at step under the pointer or pressed) and its ink |
| `primary` | the primary action: a blue, with white on it |
| `danger`, `warning`, `success` | fills with white on them; their text is `status_ink(cx)` |
| `tiles` | the docks' surface, read through `dock_surface(cx)` |
| `sidebar` | the rail's fill: the side bar's grey, the docks' too |
| `sidebar_accent` | the rail's hovered row; its chosen row is `rail_chosen(cx)` |

Helpers:

| Helper | Gives | Lives in |
|---|---|---|
| `status_ink(cx).danger`, `.warning`, `.success`, `.closed` | status text and marks; `closed` for what is over and settled (a closed issue) | `crates/plugin-host/src/lib.rs`; the app says `crate::theme::status_ink` |
| `tint(base, cx)` | a label's `Tint { fill, ink }` by its hue | `crates/plugin-host/src/lib.rs` |
| `dock_surface(cx)` | the Workbench's and the terminal's surface | `crates/plugin-host/src/lib.rs` |
| `badge_edge(tint)` | a badge's edge: halfway from its `Tint`'s fill to its ink | `crates/plugin-host/src/lib.rs` |
| `control_fill(cx)` | a ghost control's fill hovered, pressed or chosen; between `list_hover` and `accent` | `crates/plugin-host/src/lib.rs` |
| `faint_rule(cx)` | a hairline a step fainter, halfway to the surface: the lines across the pane parting a list page's list from its header, filter bar and foot | `crates/plugin-host/src/lib.rs` |
| `meta_ink(cx)` | the middle ink: a detail beside a name, between prose and muted | `crates/plugin-host/src/lib.rs`; the app says `crate::theme::meta_ink` |
| `middle_ink(full, meta)` | the same step under another full ink: on a fill, under that fill's paired ink | `crates/plugin-host/src/lib.rs` |
| `hue_ink(base, cx)` | a hue tempered for ink: code inside prose | `crates/app/src/theme.rs` |
| `raised(cx)` | the composer card's fill | `crates/app/src/theme.rs` |
| `inset(cx)` | a field with no edge on the rail: its search; a badge in the rail | `crates/app/src/theme.rs` |
| `rail_chosen(cx)` | the rail's chosen row | `crates/app/src/theme.rs` |
| `floating_well(cx)` | a well on a floating card: the command a pinned permission asks to run | `crates/app/src/theme.rs` |

Inks (roles: `DESIGN.md`, *Type, space and shape*; by place: UI patterns, *Hierarchy*). The three
neutral ones, for text on a neutral surface:

| Ink | Read as | A plugin reaches it |
|---|---|---|
| Full | `cx.theme().foreground` | yes |
| Middle | `meta_ink(cx)`; on a fill, `middle_ink(pair, muted_foreground)` | yes |
| Muted | `cx.theme().muted_foreground`, which the rail's names take too | yes |

Text on a fill takes the ink paired with it:

| Fill | Its ink |
|---|---|
| `primary`, a solid `danger` | `primary_foreground`, `danger_foreground`, set by the button's variant |
| `button_secondary`; a `quiet` ghost | `button_secondary_foreground`; `secondary_foreground`, set by the builder |
| a ghost's control fill (hovered, pressed or chosen), under an icon alone (`quiet_icon`) | `secondary_foreground`; at rest, with no fill, the icon is muted |
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
| `.text_xs()` | 0.75 | 12 |
| `tokens::CODE_TEXT` | 0.8125 | 13 |
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
| `cx.theme().radius` | 4 (`tokens::CORNER`) |
| `.rounded_full()` | a badge, a dot, a status mark |

The theme sets `radius` and `radius_lg` both to `tokens::CORNER`, so the library's tooltips, menus,
popovers, dialogs and toasts take the one corner too. What the library draws at half its corner
(`radius_tokens().sm`: checkboxes, `Kbd`, its tabs) comes out at 2px and is left to it. A button
(`action`, `ButtonRounded::Medium`) and a text field (`text_input`, `search_input`, `text_area`)
set `radius` where they are built; a `Tag` takes `rounded_full` through `badge`.

## Elevation

| Write | For | Lives in |
|---|---|---|
| `tokens::lift(cx)` | anything the app floats | `crates/plugin-host/src/tokens.rs` |
| `composer_lift(cx)` | the composer card | `crates/app/src/theme.rs` |

## Control sizes

What each size measures in each component, as the pinned `gpui-component` draws it (its
`sizing.rs`, `button/button.rs`, `icon.rs`, `tag.rs`, `checkbox.rs`, `switch.rs`, `kbd.rs`); which
role takes which size is UI patterns, *Control sizes*. Pixels at the 16px base. **px** marks a size
the library holds in pixels, which per-panel zoom does not scale; everything else is in rems.
Moving the library's rev means checking this table.

| Size | Button: height / words / icon / icon–label gap / inset | Input: height / words / inset | Icon alone |
|---|---|---|---|
| `.xsmall()` | 20 / 12 / 12 / 4 / 4 | 20 / 12 / 4 **px** | 12 |
| `.short()` (`CONTROL_H`) | 24 / 12 / 12 / 4 / 8 | 24 / 12 / 8 **px** | 12 |
| `.tall()` (`FIELD_H`) | 30 / 14 / 14 / 4 / 12 | 30 / 14 / 8 **px** | 14 |
| `.small()`: no control's size | 24 / 14 / 14 / 4 / 8 | 24 / 14 / 8 **px** | 14 |
| the library's default | 32 / 16 / 16 / 8 / 10: never used | 32 / 14 / 10 **px** | 16 |

An icon-only button is square at its height. `.short()` is the extra small size given the small
height and inset (`Short` in `crates/plugin-host/src/lib.rs`), so its words and icon are the extra
small ones; a field keeps the small size and takes `.text_xs()`. `.tall()` is the small size
given `FIELD_H` (`Tall`, beside it), so its words and icon stay the small ones. A builder that sizes the controls
it makes (`choice`, `switch`, `picker`) takes one of them as a `ControlSize`.

A field of several lines (`text_area`) is the library's `Textarea`, which takes no size: its text
sits at the default's inset, 10 **px**, which no wrapper reaches, so it starts 2px inside a
`.short()` field's above it.

Components that keep geometry of their own, centred in their row rather than sized to it:

| Component | Geometry |
|---|---|
| `Tag` at `.small()` through `badge` | words 12, inset 6 × 4; its height follows its line |
| `Checkbox` | box 12 / 14 / 16 at `.xsmall()` / `.small()` / default; its label 12 / 14 / 16 |
| `Switch` | track 28 × 16 **px** at `.xsmall()` and `.small()`, 36 × 20 **px** at the default; thumb 12 / 16 **px** |
| `Kbd` | words 12, inset 4 × 2 |
| The library's table rows | 26 / 30 / 32 **px** at `.xsmall()` / `.small()` / default; the app draws its own rows at `ROW_H` |

## Shared sizes

All in `onehand_plugin_host::tokens` (`crates/plugin-host/src/tokens.rs`), which the app names
`crate::tokens`.

| Token | Value | What |
|---|---|---|
| `BAR_H` | 2.75rem | every strip along the top of a region |
| `ROW_H` | 1.875rem | a list row |
| `CONTROL_H` | 1.5rem | a small control |
| `FIELD_H` | 1.875rem | a control a step over `.short()`: a list page's head (`.tall()`) |
| `CODE_TEXT` | 0.8125rem | machine text |
| `LEADING` | 1.6 | reading text |
| `LEADING_TIGHT` | 1.4 | a wrapped title |
| `PAGE_NARROW` | 35rem | a one-column page |
| `PAGE_PROSE` | 48rem | an editor, or one thing read in full |
| `PAGE_WIDE` | 64rem | boxes side by side, a table |
| `ROW_INSET` | 0.5rem | where a tree row's content starts from a dock's edge (Files, Markdown) |
| `TREE_INDENT` | 0.875rem | how far each level of a tree steps in |
| `lift(cx)` | | the floating shadow |

Equal values kept apart on purpose, each one decision beside its one use: `CHAT_MIN`,
`NARROW_PANEL` and `MODES_MIN_W` (30rem); `CONTENT_COLUMN` and `COMPOSER_COLUMN` (44rem).
