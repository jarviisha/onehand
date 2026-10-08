# ui-labs DESIGN.md: the proposed UI

The UI that `labs/ui-labs` tries out: how its window is allocated and the rules its views follow.
It is **not** the app's contract; `DESIGN.md` at the repository root is, and nothing here binds
`crates/app`. An idea proven here reaches the app in a pull request of its own, which moves the rule
into the root `DESIGN.md` at the same time. Where the two differ on purpose, the table under
[Departures](#departures-from-the-apps-designmd) says so.

**No value lives here.** Every size, gap, radius, width budget and colour is a named constant in
`src/tokens.rs`, with the reason for it beside it. This file names those constants and never
repeats their numbers, so a value has one place to change.

## Principles

1. **Allocate width before spacing.** Each region has a minimum at which its content is still
   useful. When two regions cannot both reach theirs, the layout changes presentation; nothing is
   squeezed into a strip.
2. **The conversation is the primary surface.** The Workbench and terminal appear on request and
   never leave the chat narrower than `CHAT_MIN`.
3. **Density comes from aligned rows and disclosure,** not from more columns or smaller type.
4. **Colour means state.** Warm neutrals carry everything. Blue is running and links, amber waits
   on the person, green is done, red is danger. A selected row is a faint ink tint, never a hue.
5. **One owner per outer padding.** A page inset, a card inset and a list inset never stack on the
   same label; a row's vertical rhythm comes from its own padding or its parent's gap, not both.
6. **Chrome is fixed; reading scales.** Bars, rows, the rail and controls keep their size under the
   reading zoom. Only the `TEXT_READ*` sizes and `CHAT_MIN` scale.
7. **Say nothing twice, and bound every list.** A capped list says how many it left out.

## Window

The rail sits outside the allocation. Everything below is measured on the width left after it.

```
Conversation           Split                          Workbench focus
┌──────┬───────────┐   ┌──────┬──────────┬────────┐   ┌──────┬────────────────────┐
│ rail │ chat      │   │ rail │ chat     │ Work-  │   │ rail │ <- Conversation    │
│      │           │   │      │ >= CHAT_ │ bench  │   │      │ Workbench          │
│      │           │   │      │ MIN      │        │   │      │ (whole content     │
│      │ composer  │   │      │ composer │        │   │      │  area)             │
└──────┴───────────┘   └──────┴──────────┴────────┘   └──────┴────────────────────┘
```

- **Three presentations.** *Conversation* with the Workbench closed; *Split* while the width left
  holds `CHAT_MIN` (times the reading zoom) beside at least `DOCK_MIN`; *Workbench focus*
  otherwise, the Workbench taking the whole content area under a `← Conversation` back link. The
  rule is one pure function, `presentation`, with unit tests. It has slack: a split already
  showing holds down to `DOCK_MIN`, one coming back needs `SPLIT_SLACK` more, so a window resting
  on the line does not flicker between the two.
- **`← Conversation` steps aside; it does not close.** The Workbench stays open while the chat
  takes the area, the header's Workbench button brings it back, and the split returns by itself
  once the window has room for it.
- **Dragged widths are kept, not overwritten.** The rail drags between `RAIL_W` and `RAIL_MAX_W`;
  the Workbench opens at `DOCK_PREF` and drags from `DOCK_MIN` up to wherever the chat would drop
  under `CHAT_MIN`. When the window narrows, the Workbench is *drawn* narrower, down to
  `DOCK_MIN`, and the dragged width returns as soon as there is room again.
- **The rail** hides completely, never to an icon column: from its own workspace row, or
  `Ctrl+Shift+B`. While it is hidden the agent header leads with the button that brings it back.
- **Maximize** gives the Workbench the whole window, the rail included, until it is restored.
- **Regions meet at one hairline**, which is also the resize handle: a seam `SEAM_GRAB_W` wide to
  grab, with the line drawn in its middle, darker under the pointer and thicker while dragged.
  Docks are continuous surfaces on `panel`; there are no inset frames or gutters around them.
- **Bars line up.** The agent header, the Workbench mode strip, every page header and the rail's
  workspace row and the terminal's tabs are `BAR_H` tall. A detail header inside a dock
  (`← Files`) is `SUBBAR_H`.
- **A transition never loses state.** Changing presentation keeps the draft, the open file, the
  selection and scroll positions, and never writes a narrow size over the saved wide layout.
  Streaming output never changes the presentation; focus never stays on something unmounted. The
  lab keeps the draft, the dragged widths, and the Workbench's picks, search, open folders and the
  file list's scroll; the transcript's scroll position is not tracked.
- **Keys:** `Ctrl+\` the Workbench, `` Ctrl+` `` the terminal, `Ctrl+Shift+B` the rail,
  `Ctrl+=` / `Ctrl+-` / `Ctrl+0` the reading size. They are bound in the window's own context, so
  they work with the caret in a field.
- **Reading zoom** scales `TEXT_READ*` (the transcript, the composer's field, documents) and
  `CHAT_MIN`, in steps of `ZOOM_STEP` snapped so stepping back lands on exactly 100%, between
  `ZOOM_MIN` and `ZOOM_MAX`. Bars, the rail, the terminal and every control keep their size.

## Rail

- The workspace row, then the page rows (overview, Tasks with its count, Issues), then
  *New session*, the only filled control in the rail, then a hairline and the tree.
- **A one-line row is `ROW_H`.** A session row is two lines when it has something to say: its name,
  then a muted footnote (agent, or project under *All sessions*), `TIGHT` above and below. Its text starts `RAIL_INDENT` in,
  under the project's name.
- **State sits in a stable column** of `DOT_COLUMN` at the row's end, so names never shift
  beside a dot.
- The selected project's branch and change count take a line of their own under its name, so a
  short name never truncates at `RAIL_W`.

## Chat and composer

- **The transcript** is a column of at most `READ_MAX`, centred with equal gutters. The user's
  prompt is the one filled bubble, against the right edge, at most `BUBBLE_MAX`. Everything the
  agent says starts on a shared left axis. Code and output sit in a `sunken` well with no border.
- **Activity is a summary line** (`Ran 2 commands · 1 failed`): a failure inside it is named in the
  summary in danger ink, not only when it is opened. A click opens it: its chevron turns down and
  each command shows with its outcome (`failed` in danger ink) and the tail of its output in a
  well.
- **The composer stack** is at most `COMPOSER_MAX` wide and reads as one object: the pinned cards,
  then the composer, `STACK_GAP` apart.
- **The composer** is the app's: a card of two rows, then a strip under it. The card holds the
  field (at least `INPUT_MIN_H`, placeholder *Ask the agent…*) and the controls: `+`, Fast, the
  model chip, then *Send* fixed at the end. The strip has the branch at the left and the permission
  mode at the right; below `COMPOSER_SPLIT` it takes two lines, the branch over the mode. Card and
  strip are inset `COMPOSER_PAD`, and the field's text starts at that inset, on the chips' edge. A click
  anywhere in the field's area gives it the caret.
- **Chips** are ghost buttons `CONTROL_H_SM` tall, inset `CHIP_PAD_X`, their words at `TEXT_XS` in
  full ink and their glyphs muted: `+` (its glyph `PLUS_ICON`), Fast (the bolt and *On* or *Off*),
  the model (its name, then the effort in muted ink, and a caret), the branch (the branch glyph)
  and the mode (the shield).
- **The card is the field's edge:** it darkens to `muted` while the field has the caret.
- **Fast** is Lucide's `zap`, embedded by the lab because gpui-component's set has none, beside
  the word for its state.
- **Send and Stop.** *Send* is the region's primary: filled once there is something to send, spent
  (disabled) while there is not.
  While a turn runs it becomes *Stop*, a solid danger button with its word, and *Queue* joins it
  once something is typed.
- **Pinned cards** (permission, questions) share one shape: an icon in the state's ink, the title,
  a muted line saying who asks; the body; then a footer with the keys at the left and the
  answers at the right, the primary last. A long command is bounded in its well. A question's
  description is said once, never again as its placeholder.
- **The queue and a reconnect** are single-line strips in the same stack, not cards: no edge, no
  fill, their text aligned with the composer's.
- **In the chat the composer works**, on canned answers: the field takes text (Enter sends,
  Shift+Enter breaks the line); `@` at the start of a word and `/` heading the field open
  completion, Enter or a click inserts the candidate and replaces only the word being completed;
  `+`, the model chip, the branch and the mode open their menus, and a pick changes the label;
  the Fast bolt toggles; *Attach files…* fills the tray, each chip removable. *Send* starts a turn that
  answers after a moment, with a running line in `accent` meanwhile. Typing during a turn offers
  *Queue*; only words queue, attachments stay in the tray for the next prompt sent. Queued prompts
  are sent in order as turns end on their own, each editable (*Edit* puts it back in the field,
  over the draft) or removable until then. *Stop* ends the turn with a notice and holds the
  queue: nothing queued starts until the person sends. In an open popup the arrows move the
  highlight and Enter takes it, as a click would; Esc or a click outside closes it. The pinned cards are drawn on the *Composer cards* page, not in the
  chat.
- **Popups** open above the field and float over the transcript, with a shadow: a pinned header
  naming what the list is, at most `POPUP_LIST_CAP` rows grouped under labels, a line saying how
  many more there are, and a footer of the keys that work there (the arrows, Enter to insert or
  choose, Esc). A completion spans the stack; a menu opened from a control is `MENU_W` or
  `MENU_WIDE_W` and starts under that control (the mode's at the right). The control that opened
  it, chip in the card or on the strip, shows `chip_on`.

## Workbench

- **The mode strip** has the modes (Editor, Markdown, Issues, Plugins, Neovim), each chip at most
  `TAB_MAX_W`, then maximize and hide fixed at the end, outside anything that scrolls. Below
  `DOCK_PREF` the chips become one control naming the mode and opening the others.
- **List and detail.** Side by side only when the container holds `SPLIT_MIN`
  (`LIST_W` + `DETAIL_MIN`); otherwise one at a time, the detail under a back link naming the list
  (`← Files`, `← Documents`, `← Issues`). The width is the container's, measured each frame,
  never the window's. Going back keeps what was picked; picking again shows it.
- **Editor:** the Files tree with its folders, a search that reaches into closed folders and
  lists matching files flat (and says when nothing matches), then the file.
- **Markdown:** the documents, then the one picked at `DOC_MEASURE` and `LEADING_DOC`.
- **Issues:** search and *New issue* above both halves, so they stay reachable one at a time.
- **Plugins:** a bounded inventory; each row its name, a description that wraps, the metadata on
  its own line, and its switch at the end.
- **Neovim:** the grid takes everything under the strip, its status line at the foot.
- The Workbench's foot shows the measured width and which rule applies: a lab instrument, not a
  proposal.

## Terminal

- **Under the agent pane only**, opening at `TERM_H`, its top a seam that drags. It is drawn no
  taller than leaves `READING_MIN_H` of conversation above it, and no shorter than `TERM_MIN_H`;
  maximized, it takes the pane under the header.
- **The strip** is the shell tabs, each at most `TAB_MAX_W` with the full name on hover and its
  own close, scrolling sideways; `+`, maximize and hide sit outside the scroll. Closing the last
  shell closes the terminal.
- **A dot on the header's terminal button** says a shell is alive while the terminal is hidden.

## Settings

- **A page**, not a dialog: a nav column of `SETTINGS_NAV` while the page holds it beside a form
  that can stay side by side, otherwise one control naming the section and opening the others.
- **The form** is at most `FORM_MAX`, its title at `TEXT_LG`, its rows a hairline group; below
  `FORM_STACK` each row's label stacks over its control. A long description wraps.
- **Sections:** Appearance (theme and reading size as segmented controls, the font as a select),
  Workspace (the check command, switches), Agents, Connections, Shortcuts (key caps).

## Deleting a project

- A project's `⋯` opens gpui-component's `Dialog`, at most `DIALOG_MAX`: the title, the project's
  full name wrapping in the body, what goes and what stays, then *Keep* (outline) before *Delete*
  (solid danger). Deleting takes the project and its sessions out of the rail.

## Pages

- The overview, Tasks and Issues take the content area and put the docks away. A page column is at
  most `PAGE_MAX`, inset `INSET`, its sections `SECTION` apart.
- **A section** is a heading at `TEXT_MD`, an optional control at its end, then its rows in one
  hairline box divided by hairlines. A section that holds work or states (*Waiting on you*,
  *Running*, *Queued*…) shows its count; a reference section (*Projects*, *Recent
  conversations*, *Steps*, *What the work left*) does not.
- **An empty list** is one or two plain sentences saying where its items come from, then the
  action that creates one only where the list has one (a session, an issue, a workflow); a list
  that only fills by itself, such as *Queued*, says where its items come from and stops there.
  No illustration, no joke. It wraps inside the same inset as a full list
  would use, never centred as one unbreakable line across a clipped area.
- **A row** is a state dot in its `DOT_COLUMN`, a title over muted facts joined by ` · `, and its
  actions at the end. Rows fill with `selected` under the pointer.
- **The overview:** *Waiting on you*, *Working*, *Projects* as tiles of `TILE_W` that wrap, then
  *Recent conversations*.
- **Tasks:** *Needs attention* (with the project filter), *Running*, *Queued*, *Finished*. A task's
  detail replaces the list in the same column under `← Tasks`: the title at `TEXT_XL` with its
  main action beside it, its facts with its state badge at their end, its steps, then what
  awaits approval.
- **Issues:** the list beside the issue while the content area holds `ISSUE_LIST_W` +
  `DETAIL_MIN`, otherwise one at a time under `← Issues`. Above the list: search, *New issue*,
  *Open N | Closed N*. A row's labels are muted facts after its reference (`atlas-api #42 · bug ·
  auto`), never pills. The issue: its title, then one line of facts (`Open · atlas-api #42 ·
  opened … · bug`), where *Open* is a plain fact and not a coloured badge; *Where it stands* with
  one primary; the body; *What the work left*.

## Type, spacing and shape

| Role | Constant |
|---|---|
| Metadata, chips, sub-lines | `TEXT_XS`, `muted` |
| Rows, buttons, bars (the UI default) | `TEXT_SM` |
| Section headings | `TEXT_MD`, weight 500 |
| Dialog and form titles | `TEXT_LG`, weight 500 |
| Page title | `TEXT_XL`, weight 500 |
| Transcript, composer, documents | `TEXT_READ` at `LEADING_READ` |
| Activity lines, code | `TEXT_READ_SM`, mono for machine text |

- **Two weights,** 400 and 500. Sentence case in every label, no exclamation marks. A back link
  names where it goes.
- **Spacing is chosen by relationship:** `TIGHT` for an icon and its label, `CONTROL` between
  adjacent controls, `RELATED` inside a card, `INSET` for a gutter, `SECTION` between groups,
  `MAJOR` only between very different parts of a page. Inside a row, `SUBLINE` between its title
  and the line under it; between rows stacked in a list, `ROW_GAP`.
- **Controls:** buttons take gpui-component's small size, which is `CONTROL_H_SM`, so every
  button on a bar or in a row is the same height; `CONTROL_H` is a single-line field and an
  attachment chip. A tab is at most `TAB_MAX_W`.
- **Radii are small:** `RADIUS_SM` for controls and rows, `RADIUS_MD` for wells, list boxes and
  popups, `RADIUS_LG` for the composer and the cards pinned on it, `RADIUS_XL` for the bubble and a
  dialog.
  A pill is only for a status badge.
- **Truncation** is one line with an ellipsis and the full text on hover (names in the rail, the
  Workbench's lists and the terminal's tabs); state and primary actions always stay visible. A long name in a dialog wraps.
- **Lines are hairlines:** `hairline` divides regions and rows, `control` edges a control. No
  thick borders, and no coloured bar down a card's side to mark it. A seam and the rail's rule are
  `HAIRLINE_PX`, the one length in pixels; an element's edge is one pixel.
- **Fonts.** The proposal names Be Vietnam Pro for the UI and JetBrains Mono for machine text.
  The lab draws with the theme's default families instead, because the app embeds no font and a
  family that is not installed fails silently; adopting either means bundling it.

## Interaction

- **Hover** is a fill: `selected` behind a ghost or outline control and behind a row, with the
  pointer cursor on anything a click opens. An icon-only button's glyph is `muted` at rest and
  `text` under the pointer (`icon_button`).
- **Press** changes nothing on its own: no shrink, no colour shift. What was pressed shows in the
  state it leads to.
- **Focus** on a field darkens its edge to `muted` (for the composer, the card's edge). Controls
  draw no ring of their own here.
- **Motion** is limited to what a state change needs: the running spinner turns, and the activity
  line's chevron turns as it opens. Everything else changes at once.

## Colour

`Palette` in `src/tokens.rs` has a light and a dark set with the same roles; geometry and
hierarchy are identical in both.

| Role | Use |
|---|---|
| `page` / `sunken` / `panel` | reading surface / rail, wells, bubble / docks, composer, cards, dialogs |
| `text` / `text2` / `muted` | prose / secondary / metadata |
| `hairline` / `control` | dividers / a control's edge |
| `selected` / `chip_on` | a selected or hovered row / the control whose popup is open |
| `accent` | running, links |
| `warning` | waits on the person |
| `success` | done |
| `danger` / `danger_solid` | danger as ink / as a fill, the same red in both modes |
| `primary_bg` / `primary_fg` | the one primary action in a region |
| `scrim` | under a dialog |

- Solid fills only. Shadows only on what floats: popups, menus, dialogs.
- **One exception to colour meaning state: a diff.** Added lines are `success` and removed lines
  `danger`, because that is how every diff reads; the `+` and `−` signs stay, so the colour is
  never the only difference.
- **The palette reaches gpui-component through the theme config.** `paint` writes it into the
  library's light and dark configs and `Theme::change` applies one, so every button state, radio,
  checkbox and key cap resolves from it. Writing resolved colours onto the theme directly leaves
  those controls on the library's palette; a unit test holds the button states to the palette in
  both modes.

## Components and icons

- **gpui-component draws the controls:** `Button` (primary, ghost, outline, danger; `selected`
  and `disabled`), `Radio`, `Checkbox`, `Kbd`, `Spinner`. The lab adds no component of its own
  for something the library has. Every button goes through `controls::action`, which gives it the
  pointer cursor the library leaves off; a switch and a clickable row set it themselves.
- **Which button.** *Primary* for the one main action in a region, never two side by side.
  *Outline* for the other actions beside it. *Ghost* for back links and for actions on a bar.
  *Danger*, solid, only to confirm something destructive. In a sub-bar a button takes the small
  size. Labels are sentence case.
- **An icon-only button** always carries a tooltip naming what it does, and sits square at the
  library's size for its button size, so neighbouring icon buttons match.
- **Badge:** only a state is a badge; labels and an issue's open or closed are plain facts. A
  pale pill with its word, in a stable column at the end of a row or line: `warning` on
  `warning_bg` (waits on the person), `accent` on `accent_bg` (running), `success` on `success_bg`
  (done), `danger` for a failure.
- **State dot:** `DOT` in its `DOT_COLUMN`: `warning` waits on the person, `accent` is running,
  `success` is done and idle. The lab also marks a failed turn in `danger`.
- **Segmented control:** two or three short labels; the chosen one takes the `selected` fill.
- **Field:** a hairline `control` edge at `CONTROL_H`, its placeholder in sentence case.
- **Form row:** a label (and its description, wrapping) with the control at its end, a hairline
  under each row and one above the group; a switch always sits in a form row.
- **Key cap (`Kbd`):** mono, used in Settings ▸ Shortcuts, in tooltips and in a card's footer.
- **Tabs:** flat, the chosen one on the `selected` fill, each at most `TAB_MAX_W` and truncating;
  a strip that cannot hold them becomes a select.
- **The lab draws only layout and content:** rows, sections, cards, wells, the composer stack and
  popups, as plain elements on the tokens.
- **Icons** come from `gpui_component::IconName`, never a glyph.
- **Lab instruments:** the palette button in the agent header and in every page header switches
  light and dark; it is a way to look at both, not part of the proposal.

## Departures from the app's DESIGN.md

| Here | In the app |
|---|---|
| Docks are continuous surfaces divided by one hairline | each dock is a card inset on three sides |
| The Workbench takes the content area when the chat would drop under `CHAT_MIN` | docks never crowd the conversation, with no rule for when they would |
| A warm palette of its own, light and dark | gpui-component's theme plus the app's surface ramp |
| Weights 400 and 500 only | titles are semibold |
| *Stop* is a solid danger button with its word | Send and Stop share an icon button |
| A question's description is said once | the description is repeated as the field's placeholder |
| Settings is a page in the content area | Settings is a large dialog |
| One primary per region, so a list and its detail side by side may each have one | one primary per view |
| The composer is a panel card with no shadow | the composer card floats over the transcript with a shadow |
| Spacing and radii are named roles in `src/tokens.rs` | gpui's base-4 scale and the theme's radius steps |

## Backlog

Specified, not drawn yet:

- **Full text on hover** for the names on the pages (overview, Tasks, Issues rows); the rail, the
  Workbench and the terminal have it.
- **The transcript's scroll position** kept across presentation changes.
- **The workflow launcher** behind *Run a workflow…*, and *New issue*'s form.

## Acceptance

What the proposal has to hold before any of it moves into the app. Check each at 1600×1000,
800×1000 and the widths between, in both palettes, at 100% and an enlarged reading size; a short
window too, so a horizontal change never leaves an unreachable vertical stack.

| Scenario | Expected |
|---|---|
| 800×1000, rail showing, Workbench opened | a useful Workbench with an obvious way back; never a thin chat strip |
| 1600×1000, both docks open | a readable chat, useful Workbench content, and a terminal that does not take the whole reading height |
| Resizing across a presentation threshold | no lost draft, buffer, selection, scroll position or keyboard route; no flicker back and forth at the threshold |
| Dragging the Workbench wider | the chat keeps `CHAT_MIN`; a list or a reader never overflows its container |
| Editor, Markdown and Issues in a narrow dock | every item can be picked and read at a useful width, and the list stays reachable |
| Long project, session, branch and document names | a controlled ellipsis or wrap; state and primary actions stay visible |
| The composer narrow and at an enlarged reading size | *Send*/*Stop* stays reachable; the branch and the mode never overlap |
| A long permission command, a question with several fields | the content is bounded; the footer's actions and the answer field stay reachable |
| `Ctrl+=` twice, then `Ctrl+0` | the reading content scales and comes back; bars keep their height |
| From the Workbench or the terminal to a session or a page | focus lands on something mounted; application shortcuts keep working |
| Settings with long descriptions | an even section rhythm, readable labels, reachable controls, no sideways overflow |
| Light and dark | identical geometry and hierarchy; spacing never depends on the palette |
