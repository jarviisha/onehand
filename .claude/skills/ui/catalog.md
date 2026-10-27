# What draws what

Reuse what is listed here before writing a builder of your own. When this file is out of date, fix
it in the same change.

This is an implementation inventory. Choose the container through
[UI patterns](../../../docs/ui-patterns.md#choose-the-container), then find its builder here.
An existing use may still need migration to the contract; copying it is not a consistency check.

## gpui-component

The app uses the library for: `Button` (always through `action`), `Input` (always through
`text_input` or `search_input`) and `InputState`, `Textarea` (always through `text_area`), `Switch`
(always through `toggle`),
`Tag`, `Kbd`, `Spinner`, `Tooltip`, `Dialog` (`window.open_dialog`, `window.open_alert_dialog`, with
`DialogFooter` and `DialogClose`), `Notification` (`window.push_notification`), `PopupMenu` and
`DropdownMenu` (through the menu builders below), `Popover`, `Scrollbar`, the resizable panels,
`DockArea` (drawn by `crate::shell::dock_skin`), `Root` and `TextView`; gpui's own `list` draws the transcript. Icons are
`gpui_component::IconName`; what that set lacks is `crate::icons` (picked from the kit's Lucide
catalog, or synced through `assets/icons/manifest.toml`).

Draw none of these by hand: a `div` that looks like a button, a label, a key cap spelled in text.

## The app and its UI crate (`crates/ui`)

| Need | Use | Where |
|---|---|---|
| Any button | `action` (sets the pointer and the theme's corner) | `crates/app/src/controls.rs` (the app), `crates/ui/src/lib.rs` (a feature crate) |
| Any text field | `text_input(&state, cx)` (sets the theme's corner, on `field_fill` with no edge at rest) | `crates/app/src/controls.rs` (the app), `crates/ui/src/lib.rs` (a feature crate) |
| A search field | `search_input(&state, cx)`: a `text_input` with the search mark and the clear button; the caller sizes it. A page's search is `page_search(&state, cx)`: `.tall()`, its width between its least and its cap | `crates/ui/src/lib.rs`, re-exported by `crates/app/src/controls.rs` |
| A field of several lines | `text_area(&state, cx)`: the library's `Textarea` on a field's corner and fill; the composer's and a pinned card's typed answer take `appearance(false)` and an explicit `.bg(cx.theme().transparent)`, since the fill is set apart from the library's appearance and their card or well is the field | `crates/ui/src/lib.rs`, re-exported by `crates/app/src/controls.rs` |
| An icon alone at a page head's or a page form's scale | `.tall_icon()` (the `TallIcon` trait): square at `FIELD_H`, no padding; `.tall()` on an icon alone squeezes it to a few pixels | `crates/ui/src/lib.rs` |
| A control at a dialog's, a form's or a page filter bar's scale | `.short()` (the `Short` trait, for a button or a field; re-exported by `crates/app/src/controls.rs`) | `crates/ui/src/lib.rs` |
| An icon-only button's name | `.named(name)` or `.named_with_action(name, action, context)`: the tooltip and the accessible name in one call (the `Named` trait, re-exported by `crates/app/src/controls.rs`) | `crates/ui/src/lib.rs` |
| A ghost button | `.quiet(cx)`; an icon with no words `.quiet_icon(on, cx)`, muted at rest and in its fill's ink when hovered or `on` (the `Quiet` trait, re-exported by `crates/app/src/controls.rs`) | `crates/ui/src/lib.rs` |
| A button that can refuse | `.refuses(why)`: refusing while `why` is `Some`, its reason the tooltip; called after the control's own tooltip (the `Refuses` trait, re-exported by `crates/app/src/controls.rs`) | `crates/ui/src/lib.rs` |
| A small label, a badge | `badge(text, tint)`: the library's `Tag` at `.small()`, `rounded_full`, its words `py_1` off its edges, edged in `badge_edge`, in a `Tint`'s fill and ink (`tint(base, cx)` for a hue) | `crates/ui/src/lib.rs` |
| A mark down a column (an icon, a state's mark, a dot) | `mark_slot(mark)`: one icon wide, centred, so marks share a line and labels an edge | `crates/ui/src/lib.rs` |
| A row's actions shown on hover | `hover_slot(group, shown, child)`, then `.focus(&row_handle)`: a trailing slot keeping its width, visible while the row named `group` is hovered, while the keyboard holds the row, or while `shown` (selected, or its action running). A hidden slot's controls are not tab stops, so the row itself is one | `crates/ui/src/row_actions.rs` |
| An issue's label | `label_badge(label, cx)`: a badge tinted by the label's kind; `label_tint(label, cx)` the kind's tint, whose ink writes a label inside a line of meta text | `crates/ui/src/lib.rs` |
| A block held folded | `fold_toggle(id, label, open)`: the chevron first, on the secondary fill, `.short()` | `crates/ui/src/lib.rs` |
| A box on a page | `card_box(cx)`: a control's edge (`input`), no fill, `p_3`, `gap_2` | `crates/ui/src/lib.rs` |
| A radio or checkbox at a choice row's head | `choice_mark(on, single, cx)`: drawn in ink, round for one choice and square for several, so it shows on the selected step where the library's ring vanishes | `crates/ui/src/choice_mark.rs` |
| A picker's trigger | `picker(id, label, set, size, cx)`: a ghost in the muted ink naming what it picks, on the secondary fill once set (in a form, whatever it holds), with the caret | `crates/ui/src/lib.rs` |
| A list or table row's surface | `row(picked, cx)`: `ROW_H`, `px_2`, the hover and picked fills with their inks; the caller lays it out. | `crates/ui/src/lib.rs` |
| A tree's row (Files, Markdown) | `tree_row(id, depth, picked, cx)`: a `row` at `TREE_ROW_H` a pitch, its fill `TREE_ROW_FILL` tall and `ROW_GUTTER` off the dock's side edges, its content at `ROW_INSET` and `TREE_INDENT` per level; the caller adds the mark slot, the name and one trailing slot | `crates/ui/src/lib.rs` |
| A menu, its rows, a menu under its control | `menu_item`, `menu_row`, `menu_below`; `menu_command(label, action)` for a row that runs a keymap command and shows its key; `menu_still` for a row that refuses or is only read, `menu_label` for a group's heading or what the menu leaves out (never the library's `label`, `PopupMenuItem::new` or bare `element`, which letter a step larger); every row's words at `CHROME_TEXT` | `crates/ui/src/menu.rs` |
| An action joined to a menu of its alternatives (*New* and its caret) | `split_action(id, main, caret, surface, anchor, build)`: both halves built by the caller through `action` at their role's size and one variant, joined with square inner corners and parted by a seam in `surface`, the fill under it, the caret's menu opened below it | `crates/ui/src/menu.rs` |
| A strip along the top of a region, and its icon control | `strip(cx)`, `strip_control(id, icon, on, cx)`; a bar inside the Workbench `workbench_strip(cx)`, inset `ROW_INSET`; a mode's own bar under it `mode_strip(cx)`, `MODE_BAR_H` tall | `crates/ui/src/strip.rs` |
| One of a few choices, the taken one filled (filter chips) | `choice` (a `quiet` ghost button, `selected` when taken, at the `ControlSize` its role takes) | `crates/ui/src/lib.rs` |
| A setting turned on or off | `toggle(id, on)`: the library's `Switch` at `.small()`, with no focus ring a pointer press would leave | `crates/ui/src/toggle.rs` |
| A split, or a dock's edge | `h_resizable`, `v_resizable`, `edge_handle`: nothing drawn at rest or under the pointer, one `ring` line while held | `crates/ui/src/resize.rs` |
| Rendered Markdown | `no_code_label(view)` on every `TextView`; `unedged_code()` as the style of one with none of its own (a code block is a well, no edge) | `crates/ui/src/prose.rs` |
| A choice between a few views, every one the same width | `switch` (a `choice` per view at `flex_1`, padded `p_0p5` and parted `gap_0p5` in one edged track, at one `ControlSize`) | `crates/ui/src/lib.rs` |
| One of more choices than a switch holds, each at its own width (a setting's values) | `choice_track` (`choice`s in the switch's edged track, padded `p_0p5` and parted `gap_0p5`, wrapping) | `crates/ui/src/lib.rs` |
| An agent's setting picked from what it offers (a mode, a model, an effort): a first choice leaving it to what runs, the values, one not offered marked | `setting_row(id, none_label, offered, current, strict, refused, pick, cx)` (a `choice_track`, capped, *N more*) | `crates/app/src/controls/setting_row.rs` |
| Tabs that fit their strip, the older ones in a menu | `tab_strip` (the newest that fit at their own widths, the current one among them), `measure_tab` and `tab_width` (each tab's width as last drawn, `TabWidths`), `tab_chip`, `tab_close`, `tab_more` (*N more*, with the picker's caret, opening those left out, `TAB_MENU_CAP` listed), `tab_select` (the Workbench's modes when narrow), `measure_width` | `crates/ui/src/tabs.rs` |
| A list beside its detail, or one at a time | `list_detail`, `ListWidths`, `back_link` | `crates/ui/src/list_detail.rs` |
| A line filling an empty panel | `hint` | `crates/ui/src/lib.rs` |
| A condition standing under a view | `status_line(message, ink)`, in the ink of its state | `crates/ui/src/lib.rs` |
| Status ink, the middle ink, a dock's surface | `status_ink`, `meta_ink` (`middle_ink` on a fill), `dock_surface` | `crates/ui/src/lib.rs` |
| Chat or Workbench in the area | `presentation` | `crates/app/src/shell/presentation.rs` |
| A toast | `toast::failed(what, why)`, `toast::error`, `warning`, `success`, `info` | `crates/ui/src/toast.rs`; the app says `crate::toast` |
| A confirmation | `Confirm` with `confirm_on`, or `Shell::confirm` from the shell | `crates/ui/src/confirm.rs`; `crates/app/src/shell/confirm.rs` |
| A change shown at once and saved in the order made, a failure one toast | `OrderedWriter`, `Saving` and `Saves`, as `ChatPane::save_title` uses them; `saves::write_config` for `onehand.toml`, `Shell::save_change` for the workspace file | `crates/core/src/saving.rs`; `crates/app/src/chat/pane/sessions.rs`, `crates/app/src/saves.rs`, `crates/app/src/shell/storage.rs` |
| A dialog's title row | `title_row` | `crates/app/src/dialogs.rs` |
| A form field | `field` (its line about the setting in meta text) | `crates/ui/src/lib.rs`, re-exported by `crates/app/src/settings.rs` |
| A Settings group, a Settings row | `section`, `list_row` | `crates/app/src/settings.rs` |
| A page's head, and the top it stays in over what scrolls | `page_head(title, facts, actions, cx)`, `page_top(width, parts, cx)` | `crates/ui/src/page.rs`, re-exported by `crates/app/src/chat/pane/page_parts.rs` |
| A page's box and title row, a list section (a box too), rows parted by hairlines, two boxes to a row | `page_card`, `card_title`, `ruled(rows, cx)`, `pair` | `crates/app/src/chat/pane/page_parts.rs` |
| A page form's problems, or that it is only read, along the page's foot | `foot_bar(icon, base, lines, cx)` (the tint of `base`, the mark slotted on the first line) | `crates/app/src/chat/pane/page_parts.rs` |
| A dialog's footer (a third action at its start) | `footer_row` | `crates/app/src/dialogs.rs` |
| A run dialog's sections parted by hairlines; a preflight finding with its state's mark | `sections`, `finding_line` | `crates/app/src/dialogs.rs` |
| A Settings sub-page's scope line | `page_head` | `crates/app/src/settings.rs` |
| A scrollbar on a region the app scrolls itself | `scrollbar` (its thumb at `SCROLLBAR_THUMB`) | `crates/ui/src/lib.rs` |
| A sunk box of machine text | `well` (sets the code text size; callers set padding) | `crates/ui/src/lib.rs` |
| A mono well on a page | `mono_well`, built on `well` | `crates/app/src/chat/pane/tasks_page/detail.rs` |
| Shared sizes and the floating shadow | `tokens` | `crates/ui/src/tokens.rs` |
| A transcript well, a pinned card, a row's note | `plain_box`, `floating_card`, `row_note` | `crates/app/src/chat/transcript/parts.rs` |
| Transcript sizes | the constants | `crates/app/src/chat/transcript/metrics.rs` |
| The composer card | `card` | `crates/app/src/chat/composer/card.rs` |
| An attached file's plate (a picture's preview), pressable; a picture whole in a dialog | `plate`, `opens`; `show_image` | `crates/app/src/chat/transcript/user.rs`, `image_view.rs` |
| The composer popup and its parts | `popup_surface`, `popup_header`, `popup_title`, `candidate_row`, `choice_row` | `crates/app/src/chat/composer/popup.rs`, `rows.rs` |
| A rail row's fills, inks and controls | `row_surfaces`, `row_inks`, `hover_fill`, `chosen_fill`, `key_ring`, `row_control`, `meta_control`, `menu_button` | `crates/app/src/rail/row.rs` |
| A page row (the rail's foot, Settings' nav) | `page_fill(row, on, cx)` | `crates/app/src/rail/row.rs` |

A page part not listed above: take the nearest one, and when a second page needs the same
thing, lift it into `page_parts.rs` in that change rather than copying it.

## Keys and focus

How a key and a focus are wired (the rules: UI patterns, *Keys and focus*):

- A new command goes into `COMMANDS` in `crates/app/src/keymap.rs` (id, label, default keys,
  action, context), never into an `on_key_down` arm.
- A window command's context starts `Shell`, and its key has a modifier. A region's starts with
  its own context (`Rail`, `Issues`, `TasksPage`, `WorkflowsPage`, `PluginList`,
  `QuestionCard`, `PermissionCard`): a key without a modifier adds `&& !Input`, and Escape
  `&& !Dialog && !Input`. One key serves one command per region; `validate` refuses the rest.
- A control bound to one shows its key through the library's lookup, so a rebound key shows:
  `.tooltip_with_action(tip, &action, None)` on a button, a `Tooltip::new(tip).action(&action,
  None)` built in `.tooltip(…)` on anything else.
- An element you draw that takes focus tracks it and rings for the keyboard only:
  `.track_focus(&handle).focus_visible(|s| s.border_color(cx.theme().ring))`. Esc in a field
  reaches the view as the library's `gpui_component::input::Escape` action.

## Patterns

The screen to build from for each pattern in [UI patterns](../../../docs/ui-patterns.md); it may
need migrating itself.

| Building | Pattern | Example |
|---|---|---|
| Fields, boxes or one thing read in full, in the agent pane | Column page | `tasks_frame` in `crates/app/src/chat/pane/tasks_page.rs` |
| One long list to scan, filter and pick from, in the agent pane | List page | the Issues page, `crates/issues/src/view/page.rs` |
| The top of a page: its head, then its search and filters | The top of a page, Filter bar | `crates/issues/src/view/page/head.rs`; `tasks_frame` for a top with no search |
| Related fields or a subsection | Plain section | `section` in `crates/app/src/settings.rs` |
| The facts about one thing on a page | Box | `card_box` (`onehand_ui`); `page_card`, `card_title` and `pair` in `crates/app/src/chat/pane/page_parts.rs` |
| A bar along the top of a region | Strip | `strip` and `strip_control` (`onehand_ui`) |
| Rows to pick from | List | the Files tree, `crates/files/src/view.rs` |
| Items opened one at a time, each with its own actions | Grid of boxes | `grid_page` in `crates/app/src/workflows_page/grid.rs` |
| A list beside its detail | List and detail | `list_detail` in `crates/ui/src/list_detail.rs`; the Markdown mode |
| Inputs and a submit | Form | `field` in `crates/app/src/settings.rs`; the Workflows form |
| A modal question or form | Dialog | `crates/app/src/dialogs.rs` |
| One thing read in full | Detail | the issue, `crates/issues/src/view/detail.rs` |
