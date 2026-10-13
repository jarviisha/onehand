# What draws what

Reuse what is listed here before writing a builder of your own. When this file is out of date, fix
it in the same change.

This is an implementation inventory. Choose the container through
[UI patterns](../../../docs/ui-patterns.md#choose-the-container), then find its builder here.
An existing use may still need migration to the contract; copying it is not a consistency check.

## gpui-component

The app uses the library for: `Button` (always through `action`), `Input` (always through
`text_input` or `search_input`) and `InputState`, `Textarea` (always through `text_area`), `Switch`,
`Tag`, `Kbd`, `Spinner`, `Tooltip`, `Dialog` (`window.open_dialog`, `window.open_alert_dialog`, with
`DialogFooter` and `DialogClose`), `Notification` (`window.push_notification`), `PopupMenu` and
`DropdownMenu` (through the menu builders below), `Popover`, `Scrollbar`, the resizable panels,
`DockArea`, `Root` and `TextView`; gpui's own `list` draws the transcript. Icons are
`gpui_component::IconName`; what that set lacks is `crate::icons` (synced through
`assets/icons/manifest.toml`).

Draw none of these by hand: a `div` that looks like a button, a label, a key cap spelled in text.

## The app and its plugin host

| Need | Use | Where |
|---|---|---|
| Any button | `action` (sets the pointer and the theme's corner) | `crates/app/src/controls.rs` (the app), `crates/plugin-host/src/lib.rs` (a plugin) |
| Any text field | `text_input(&state, cx)` (sets the theme's corner, on no fill of its own) | `crates/app/src/controls.rs` (the app), `crates/plugin-host/src/lib.rs` (a plugin) |
| A search field | `search_input(&state, cx)`: a `text_input` with the search mark and the clear button; the caller sizes it | `crates/plugin-host/src/lib.rs`, re-exported by `crates/app/src/controls.rs` |
| A field of several lines | `text_area(&state, cx)`: the library's `Textarea` on a field's corner and surface | `crates/plugin-host/src/lib.rs`, re-exported by `crates/app/src/controls.rs` |
| A control at a dialog's or a form's scale | `.tall()` (the `Tall` trait, for a button or a field; re-exported by `crates/app/src/controls.rs`) | `crates/plugin-host/src/lib.rs` |
| A ghost button | `.quiet(cx)`; an icon with no words `.quiet_icon(on, cx)`, muted at rest and in its fill's ink when hovered or `on` (the `Quiet` trait, re-exported by `crates/app/src/controls.rs`) | `crates/plugin-host/src/lib.rs` |
| A button that can refuse | `.refuses(why)`: refusing while `why` is `Some`, its reason the tooltip; called after the control's own tooltip (the `Refuses` trait, re-exported by `crates/app/src/controls.rs`) | `crates/plugin-host/src/lib.rs` |
| A small label, a badge | `badge(text, tint)`: the library's `Tag` at `.small()` on `tokens::BADGE_CORNER`, in a `Tint`'s fill and ink (`tint(base, cx)` for a hue) | `crates/plugin-host/src/lib.rs` |
| A mark down a column (an icon, a state's mark, a dot) | `mark_slot(mark)`: one icon wide, centred, so marks share a line and labels an edge | `crates/plugin-host/src/lib.rs` |
| A picker's trigger | `picker(id, label, size)`: a secondary button naming its setting, with the caret | `crates/plugin-host/src/lib.rs` |
| A list or table row's surface | `row(picked, cx)`: `ROW_H`, `px_2`, the hover and picked fills with their inks; the caller lays it out | `crates/plugin-host/src/lib.rs` |
| A menu, its rows, a menu under its control | `menu_item`, `menu_row`, `menu_below`; `menu_command(label, action)` for a row that runs a keymap command and shows its key | `crates/plugin-host/src/menu.rs` |
| A strip along the top of a region, and its icon control | `strip(cx)`, `strip_control(id, icon, on, cx)` | `crates/plugin-host/src/strip.rs` |
| One of a few choices, the taken one filled (filter chips) | `choice` (a `quiet` ghost button, `selected` when taken, at the `ControlSize` its role takes) | `crates/plugin-host/src/lib.rs` |
| A choice between a few views, every one the same width | `switch` (a `choice` per view at `flex_1`, at one `ControlSize`) | `crates/plugin-host/src/lib.rs` |
| Tabs that fit their strip, or one select | `tab_strip`, `tab_chip`, `tab_close`, `tab_select` (at a strip's size, with the picker's caret), `tab_menu_rows`, `measure_width` | `crates/plugin-host/src/tabs.rs` |
| A list beside its detail, or one at a time | `list_detail`, `ListWidths`, `back_link` | `crates/plugin-host/src/list_detail.rs` |
| A line filling an empty panel | `hint` | `crates/plugin-host/src/lib.rs` |
| A condition standing under a view | `status_line(message, ink)`, in the ink of its state | `crates/plugin-host/src/lib.rs` |
| Status ink, the middle ink, a dock's surface | `status_ink`, `meta_ink` (`middle_ink` on a fill), `dock_surface` | `crates/plugin-host/src/lib.rs` |
| Chat or Workbench in the area | `presentation` | `crates/app/src/shell/presentation.rs` |
| A toast | `toast::failed(what, why)`, `toast::error`, `warning`, `success`, `info` | `crates/plugin-host/src/toast.rs`; the app says `crate::toast` |
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
| A rail row's fills, inks and controls | `row_surfaces`, `row_inks`, `hover_fill`, `chosen_fill`, `key_ring`, `row_control`, `meta_control`, `menu_button` | `crates/app/src/rail/row.rs` |

The app has no shared page section yet: take the nearest one above, and when a second
page needs the same thing, lift it into a shared module in that change rather than copying it.
