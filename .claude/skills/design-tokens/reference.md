# Token reference

A lookup, not the rules: which token serves which role is ruled in `DESIGN.md`. Values here are
the ones the code holds; when this file and the code disagree, the code is right and this file is
fixed in the same change.

## Colour

Read from `cx.theme()`. The ramp in `crates/app/src/theme.rs` writes them for both palettes, with
contrast tests. Roles: `DESIGN.md`, *Colour and state*.

Slot names that mislead (the library's names, kept because its own controls read them):

| Slot | Holds |
|---|---|
| `accent`, `accent_foreground` | the selected fill and its ink: a fill, not a hue |
| `link` (also `info`) | the accent hue: what runs, and links |
| `muted` | the well's fill, and the user's bubble |
| `secondary` | a step past the well: code inside the bubble |
| `button_secondary`, `button_secondary_foreground` | the secondary button: the selected step and its ink, as `accent` |
| `primary` | the primary action: ink on light, light on dark, not a hue |
| `danger`, `warning`, `success` | fills with white on them; their text is `status_ink(cx)` |
| `tiles` | the docks' surface, read through `dock_surface(cx)` |
| `sidebar_accent` | the rail's hovered row; its chosen row is `rail_chosen(cx)` |

Helpers:

| Helper | Gives | Lives in |
|---|---|---|
| `status_ink(cx).danger`, `.warning`, `.success` | status text and marks | `crates/plugin-host/src/lib.rs`; the app says `crate::theme::status_ink` |
| `tint(base, cx)` | a label's `Tint { fill, ink }` by its hue | `crates/plugin-host/src/lib.rs` |
| `dock_surface(cx)` | the Workbench's and the terminal's surface | `crates/plugin-host/src/lib.rs` |
| `meta_ink(cx)` | the transcript's meta text, between prose and muted | `crates/app/src/theme.rs` |
| `hue_ink(base, cx)` | a hue tempered for ink: code inside prose | `crates/app/src/theme.rs` |
| `raised(cx)` | the composer card's fill | `crates/app/src/theme.rs` |
| `rail_chosen(cx)` | the rail's chosen row | `crates/app/src/theme.rs` |

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
- Families: `cx.theme().font_family` (Inter) and `cx.theme().mono_font_family` (JetBrains Mono,
  unless `[font].monospace` names another).

## Corners

| Write | px |
|---|---|
| `cx.theme().radius_tokens().sm` | 3 |
| `cx.theme().radius` | 6 |
| `cx.theme().radius_lg` | 8 |
| `.rounded_full()` | round |

The library's controls apply their own: tooltips, menus and popovers `radius`; checkboxes and
`Kbd` `sm`; dialogs and toasts `radius_lg`. Two are overridden where they are built: a button
(`action`) and a text field (`text_input`) take `sm` rather than the library's `radius`, and a
`Tag` takes `radius_lg` through `.rounded(cx.theme().radius_lg)` after `.small()`.

## Elevation

| Write | For | Lives in |
|---|---|---|
| `tokens::lift(cx)` | anything the app floats | `crates/plugin-host/src/tokens.rs` |
| `composer_lift(cx)` | the composer card | `crates/app/src/theme.rs` |

## Control heights

| Size | Height |
|---|---|
| `.xsmall()` | 1.25rem (20px) |
| `.small()` | 1.5rem (24px), `CONTROL_H` |
| `.tall()` | 2rem (32px), `FIELD_H`: the small size at the default height, its words at `text_sm` |
| the library's default | 2rem (32px); not used: its button letters at `text_base` |

## Shared sizes

All in `onehand_plugin_host::tokens` (`crates/plugin-host/src/tokens.rs`), which the app names
`crate::tokens`.

| Token | Value | What |
|---|---|---|
| `BAR_H` | 2.75rem | every strip along the top of a region |
| `ROW_H` | 1.875rem | a list row |
| `CONTROL_H` | 1.5rem | a small control |
| `FIELD_H` | 2rem | a control at a dialog's or a form's scale (`.tall()`) |
| `CODE_TEXT` | 0.8125rem | machine text |
| `LEADING` | 1.6 | reading text |
| `LEADING_TIGHT` | 1.4 | a wrapped title |
| `PAGE_NARROW` | 35rem | a one-column page |
| `PAGE_PROSE` | 48rem | an editor, or one thing read in full |
| `PAGE_WIDE` | 64rem | boxes side by side, a table |
| `lift(cx)` | | the floating shadow |

Equal values kept apart on purpose, each one decision beside its one use: `CHAT_MIN`,
`NARROW_PANEL` and `MODES_MIN_W` (30rem); `CONTENT_COLUMN` and `COMPOSER_COLUMN` (44rem).
