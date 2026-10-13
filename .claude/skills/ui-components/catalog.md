# What draws what

Reuse what is listed here before writing a builder of your own. When this file is out of date, fix
it in the same change.

## gpui-component

The app uses the library for: `Button` (always through `action`), `Input` (always through `text_input`) and `InputState`,
`Textarea`, `Switch`, `Tag`, `Kbd`, `Spinner`, `Tooltip`, `Dialog` (`window.open_dialog`,
`window.open_alert_dialog`, with `DialogFooter` and `DialogClose`), `Notification`
(`window.push_notification`), `PopupMenu` and `DropdownMenu` (through the menu builders below),
`Popover`, `Scrollbar`, the resizable panels, `DockArea`, `Root` and `TextView`; gpui's own `list`
draws the transcript. Icons are `gpui_component::IconName`; what that set lacks is `crate::icons`
(synced through `assets/icons/manifest.toml`).

Draw none of these by hand: a `div` that looks like a button, a label, a key cap spelled in text.

## The app and its plugin host

| Need | Use | Where |
|---|---|---|
| Any button | `action` (sets the pointer and the control's `sm` corner) | `crates/app/src/controls.rs` (the app), `crates/plugin-host/src/lib.rs` (a plugin) |
| Any text field | `text_input(&state, cx)` (sets the control's `sm` corner) | `crates/app/src/controls.rs` (the app), `crates/plugin-host/src/lib.rs` (a plugin) |
| A button that can refuse | `.refuses(bool)` (the `Refuses` trait, re-exported by `crates/app/src/controls.rs`) | `crates/plugin-host/src/lib.rs` |
| A small label, a badge | `Tag` at `.small()` and `.rounded(cx.theme().radius_lg)`; tinted through `Tag::custom` and `tint(base, cx)` | gpui-component; `crates/plugin-host/src/lib.rs` |
| A menu, its rows, a menu under its control | `menu_item`, `menu_row`, `menu_below` | `crates/plugin-host/src/menu.rs` |
| One of a few choices, the taken one filled (filter chips) | `choice` (a ghost button, `selected` when taken) | `crates/plugin-host/src/lib.rs` |
| A choice between a few views, every one the same width | `switch` (a `choice` per view at `flex_1`) | `crates/plugin-host/src/lib.rs` |
| Tabs that fit their strip, or one select | `tab_strip`, `tab_select`, `tab_menu_rows`, `measure_width` | `crates/plugin-host/src/tabs.rs` |
| A list beside its detail, or one at a time | `list_detail`, `ListWidths`, `back_link` | `crates/plugin-host/src/list_detail.rs` |
| A line filling an empty panel | `hint` | `crates/plugin-host/src/lib.rs` |
| A condition standing under a view | `status_line` | `crates/plugin-host/src/lib.rs` |
| Status ink, a dock's surface | `status_ink`, `dock_surface` | `crates/plugin-host/src/lib.rs` |
| Chat or Workbench in the area | `presentation` | `crates/app/src/shell/presentation.rs` |
| A toast | `toast::failed(what, why)`, `toast::error`, `warning`, `success`, `info` | `crates/app/src/toast.rs` |
| A confirmation | `Confirm` with `confirm_on`, or `Shell::confirm` from the shell | `crates/plugin-host/src/confirm.rs`; `crates/app/src/shell/confirm.rs` |
| A dialog's title row | `title_row` | `crates/app/src/dialogs.rs` |
| A form field | `field` (its line about the setting in meta text) | `crates/plugin-host/src/lib.rs`, re-exported by `crates/app/src/settings.rs` |
| A Settings group, a Settings row | `section`, `list_row` | `crates/app/src/settings.rs` |
| A page's box and its title row | `card_box`, `card_title` | `crates/app/src/chat/pane/workspace_page.rs` |
| A sunk box of machine text | `well` (callers set size and padding) | `crates/plugin-host/src/lib.rs` |
| A mono well on a page | `mono_well`, built on `well` | `crates/app/src/chat/pane/tasks_page/detail.rs` |
| Shared sizes and the floating shadow | `tokens` | `crates/plugin-host/src/tokens.rs` |
| A transcript well, a pinned card, a row's note | `plain_box`, `floating_card`, `row_note` | `crates/app/src/chat/transcript/parts.rs` |
| Transcript sizes and corners | the constants and `radius_*` | `crates/app/src/chat/transcript/metrics.rs` |
| The composer card | `card` | `crates/app/src/chat/composer/card.rs` |
| The composer popup and its parts | `popup_surface`, `popup_header`, `popup_title`, `candidate_row`, `choice_row` | `crates/app/src/chat/composer/popup.rs`, `rows.rs` |
| A rail row's fills and controls | `row_surfaces`, `hover_fill`, `chosen_fill`, `rail_control`, `menu_button` | `crates/app/src/rail/row.rs` |

The app has no shared page section yet: take the nearest one above, and when a second
page needs the same thing, lift it into a shared module in that change rather than copying it.
