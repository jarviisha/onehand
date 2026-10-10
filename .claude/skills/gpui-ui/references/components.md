# What draws what

Reuse what is listed here before writing a builder of your own. When a list here is out of date,
fix this file in the same change.

## gpui-component draws the controls

Both contexts use the library for: `Button` (primary, ghost, outline, danger; `selected`,
`disabled`, sizes), `Radio`, `Checkbox`, `Kbd`, `Spinner`, `Tooltip`, `Dialog`
(`window.open_dialog`), `Input`/`InputState`, `Switch`, `Root`. The app adds `DockArea`,
`Sidebar`, `Editor`, `TextView`, `PopupMenu`/`DropdownMenu`, and gpui's `list` for the
transcript. Icons are `gpui_component::IconName`; what that set lacks is `crate::icons` in the app
(synced through `assets/icons/manifest.toml`) and `crate::assets` in the lab.

Never draw one of these by hand: a `div` that looks like a button, a hand-made check, a key cap
spelled in text.

## The app and its plugins

| Need | Use | Where |
|---|---|---|
| Any button | `action` | `crates/app/src/controls.rs` (the app), `crates/plugin-host/src/lib.rs` (a plugin) |
| A button that can refuse | `resting`, `.refuses()` | `crates/app/src/controls.rs` |
| A menu, a menu row, a menu under a control | `menu_item`, `menu_row`, `menu_below` | `crates/plugin-host/src/menu.rs` |
| A segmented switch | `switch` | `crates/plugin-host/src/lib.rs` |
| Status text, a status hue | `status_ink`, `status_hue` (never a raw `danger`/`warning` fill as text) | `crates/plugin-host/src/lib.rs` |
| A dock's surface | `dock_surface` | `crates/plugin-host/src/lib.rs` |
| A bar along the top of a region | `BAR_H` | `crates/app/src/controls.rs` |
| Tabs that fit their strip, or one select | `tab_strip`, `tab_select`, `tab_menu_rows`, `measure_width` | `crates/plugin-host/src/tabs.rs` |
| A list beside its detail, or one at a time | `side_by_side`, `back_link`, `DETAIL_MIN` | `crates/plugin-host/src/list_detail.rs` |
| Chat or Workbench | `presentation` | `crates/app/src/shell/presentation.rs` |
| A hint, a one-line status | `hint`, `status_line` | `crates/plugin-host/src/lib.rs` |
| Transcript meta ink, a tempered hue, a floating shadow | `meta_ink`, `hue_ink`, `lift` | `crates/app/src/theme.rs` |
| A Settings group | `section` | `crates/app/src/settings.rs` |
| A page card and its title | `card_box`, `card_title` | `crates/app/src/chat/pane/workspace_page.rs` |
| A pill, a row's note | `pill`, `row_note` | `crates/app/src/chat/transcript/parts.rs` |
| A mono well | `mono_well` | `crates/app/src/chat/pane/tasks_page/detail.rs` |
| The composer popup and its parts | `popup`, `popup_header`, `popup_footer`, `popup_title`, `popup_row` | `crates/app/src/chat/composer/popup.rs`, `rows.rs` |
| The composer card | `card` | `crates/app/src/chat/composer/card.rs` |
| A rail row's surfaces and shape | `row_surfaces`, `row_shape` | `crates/app/src/rail/row.rs` |
| Machine-text size, ask inset | `CODE_TEXT`, `ASK_INSET` | `crates/app/src/chat/transcript/metrics.rs` |

The app has **no shared page section, row list or well**: each page draws its own. Take the
nearest one above; when a second page needs the same thing, lift it to a shared module in that
change rather than copying it.

## The lab

| Need | Use | Where |
|---|---|---|
| Any button | `action` | `labs/ui-labs/src/controls.rs` |
| An icon-only button whose glyph lights on hover | `icon_button` (`IconButton`), `glyph` | `controls.rs` |
| One line that truncates and shows in full on hover | `full` | `controls.rs` |
| A page column, a section, rows in a hairline box, a state badge | `column`, `inner`, `section`, `rows` (`Row`), `badge` | `pages.rs` |
| The composer card and its chips | `card`, `chip`, `plus_chip`, `fast_chip`, `model_chip`, `strip_chip` | `composer.rs` |
| A pinned card, a well, a popup and its rows, a key cap | `pinned`, `well`, `popup` (`item`/`Item`), `key` | `composer.rs` |
| Chat or Workbench, and seams | `presentation`, `seam`, `hairline_v` | `layout.rs` |
| A list beside its detail, the mode strip | `list_detail`, `mode_strip` | `workbench.rs` |
| A confirming dialog | `confirm_delete` (the pattern: title, wrapping name, *Keep* outline, *Delete* danger) | `settings.rs` |
| The palette into the library | `paint`, `install`, `set_mode` | `theme.rs` |

The lab's `Palette` reaches gpui-component only through `paint` writing the theme *config*, then
`Theme::change`. Writing resolved colours onto the theme leaves the library's buttons, radios and
key caps on its own palette; `theme.rs`'s test holds the button states to the palette.
