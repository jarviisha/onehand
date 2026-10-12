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
| `secondary` | a step past the well: code inside the bubble, a filled button |
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

The library's controls apply their own: buttons, inputs, tooltips, menus and popovers `radius`;
`Tag` at `.small()`, checkboxes and `Kbd` half of it (`sm`); dialogs and toasts `radius_lg`.

## Elevation

| Write | For | Lives in |
|---|---|---|
| `lift(cx)` | anything the app floats | `crates/app/src/theme.rs`, until `tokens` takes it |
| `composer_lift(cx)` | the composer card | `crates/app/src/theme.rs` |

## Control heights

| Size | Height |
|---|---|
| `.xsmall()` | 1.25rem (20px) |
| `.small()` | 1.5rem (24px), `CONTROL_H` |
| the library's default | 2rem (32px) |

## Shared sizes

`onehand_plugin_host::tokens` does not exist yet: the migration's first step creates it and moves
these into it. Until then each lives where the last column says.

| Token | Value | What | Lives in today |
|---|---|---|---|
| `BAR_H` | 2.75rem | every strip along the top of a region | `crates/plugin-host/src/lib.rs` |
| `ROW_H` | 1.875rem | a list row | `ROW_H` in `crates/app/src/rail.rs`, `workbench-files/src/view.rs`, `workbench-markdown/src/document.rs`; `POPUP_ROW_H` in `chat/composer/popup.rs` |
| `CONTROL_H` | 1.5rem | a small control | `BUTTON_H` and `CONTROL_ROW` in `chat/transcript/metrics.rs`; `CHIP_H` in `chat/composer.rs`; `POPUP_MORE_H` in `chat/composer/popup.rs` |
| `CODE_TEXT` | 0.8125rem | machine text | `WORK_TEXT` in `chat/transcript/metrics.rs`; `CODE_TEXT` in `workbench-issues/src/view/detail.rs` |
| `LEADING` | 1.6 | reading text | `LEADING` in `chat/transcript/metrics.rs` |
| `LEADING_TIGHT` | 1.4 | a wrapped title | `ASK_PROMPT_LEADING` in `chat/transcript/metrics.rs` |
| `PAGE_NARROW` | 35rem | a one-column page | `PAGE_COLUMN` in `chat/pane/workspace_page.rs` |
| `PAGE_PROSE` | 48rem | an editor, or one thing read in full | `EDITOR_COLUMN` in `workflows_page.rs`; `ISSUE_MAX` in `workbench-issues/src/view/detail.rs` |
| `PAGE_WIDE` | 64rem | boxes side by side, a table | `crates/plugin-host/src/lib.rs` |
| `lift(cx)` | | the floating shadow | `crates/app/src/theme.rs` |

Equal values kept apart on purpose, each one decision beside its one use: `CHAT_MIN`,
`NARROW_PANEL` and `MODES_MIN_W` (30rem); `CONTENT_COLUMN` and `COMPOSER_COLUMN` (44rem).

## Migrating old values

What each old value becomes. A change touching one of these lines writes the new form; the
migration sweeps the rest. Paths without a crate are under `crates/app/src/`.

**Sizes and type**

| Old | New |
|---|---|
| the four `ROW_H`-alikes and the four `CONTROL_H`-alikes above | `tokens::ROW_H`, `tokens::CONTROL_H` |
| `WORK_TEXT`, the Issues `CODE_TEXT`, `rems(0.8125)` in `workbench-markdown/src/document.rs` | `tokens::CODE_TEXT` |
| `PAGE_COLUMN`; `EDITOR_COLUMN` and `ISSUE_MAX` | `tokens::PAGE_NARROW`; `tokens::PAGE_PROSE` |
| an `f32` constant wrapped in `rems()` at its call site | a `Rems` constant |
| `CHIP_TEXT` (0.6875rem in `workbench-issues/src/view/list.rs`, 0.75rem in `chat/composer.rs`) | `.text_xs()` |
| `BODY_TEXT` (`workbench-issues/src/view/detail.rs`), `.text_size(TEXT_SM)` | `.text_sm()` |
| the Issues `HEADING_1` and `HEADING_2` (1rem, 0.9375rem) | `.text_lg()`, `.text_base()` |
| the Markdown mode's headings (the renderer's 2, 1.5 and 1.25rem, the first bold) | `.text_xl()`, `.text_lg()`, then the body's size; semibold |
| the Issues page's own head, `text_2xl` semibold (`workbench-issues/src/view/page/head.rs`) | the page's name in the pane's header |
| `LEADING_DOC` 1.65 | `tokens::LEADING` |
| `ASK_PROMPT_LEADING`, `relative(1.3)` in `dialogs.rs` | `tokens::LEADING_TIGHT` |
| a field's hint in `text_sm` (`field` in `settings.rs`) | `.text_xs()` |

**Spacing**

| Old | New |
|---|---|
| `_2p5` (10px) | `_2` |
| `_3p5` (14px) | `_3` |
| `_5` (20px) as a gap or padding | `_4` |
| `pl_5` lining a block up under its chevron (`chat/transcript`) | `FOLD_INDENT`, an icon plus its gap |
| `pl_7`, `ml_5` in the rail | the rail's `INDENT` |
| `BUBBLE_TAIL_GAP` 3px, `ASK_HINT_PAD` 5px, `CHIP_PAD_X` 7px, `ROW_PAD_Y` 9px | 2px, 4px, 6px and 8px: the nearest step, the smaller one on a tie |
| a strip's `px_4` (`chat/pane/header.rs`) or `px_2` (`terminal.rs`) | `px_3` |
| a page's `px_4 py_6` (`workflows_page.rs`) | `p_6` |

**Corners**

| Old | New |
|---|---|
| `radius_tag(cx)`, `radius_control(cx)` (`chat/transcript/metrics.rs`), `rounded_sm()` | `radius_tokens().sm` |
| `radius_block(cx)` | `cx.theme().radius` |
| a small label drawn by hand at `radius` or round: the rail's counts, the issue state badge, an issue's labels, a plugin's chips, the issue picker's labels, the transcript's `pill` | `Tag` at `.small()`, its tint through `Tag::custom` |
| a box at `radius`: `card_box`, a project page's conversation box, a Workflows step's box | `radius_lg` |
| the composer popup at `radius_lg` (`chat/composer/popup.rs`) | `radius` |
| a question card's choice rows at `radius_lg` (`chat/transcript/ask.rs`) | `radius` |
| transcript wells and code blocks at `radius_lg` (`plain_box`, `chat/transcript/prose.rs`) | `radius` |
| the to-bottom button, round (`chat/pane/body.rs`) | `radius_lg` |

**Shadows and colour**

| Old | New |
|---|---|
| `.shadow_lg()` in `chat/pane/body.rs`, `floating_card`, `workbench-issues/src/view/page/panel.rs` | `lift(cx)` |
| `floating_card`'s shadow once its card is answered | none |
| `accent.opacity(0.5)` as a hover (`settings.rs`) | `list_hover` |
