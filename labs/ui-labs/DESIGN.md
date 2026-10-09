# ui-labs DESIGN.md: the proposed UI

The UI that `labs/ui-labs` tries out: how its window is allocated and the rules its views follow.
It is **not** the app's contract; `DESIGN.md` at the repository root is, and nothing here binds
`crates/app`. An idea proven here reaches the app in a pull request of its own, which moves the rule
into the root `DESIGN.md` at the same time. Where the two differ on purpose, the table under
[Departures](#departures-from-the-apps-designmd) says so.

**No value lives here.** Gaps, paddings, the UI's type sizes, control and icon heights and radii
come from gpui's base-4 scale (`gap_2`, `text_xs`, `h_6`), gpui-component's sizes (`small`,
`xsmall`) and the theme's `radius` and `radius_lg`, as in the app. Everything they do not give
(chrome heights, reading sizes, width budgets, colour) is a named constant in `src/tokens.rs`, with
the reason for it beside it. This file names those steps and constants and never repeats their
numbers, so a value has one place to change.

## Principles

1. **Allocate width before spacing.** Each region has a minimum at which its content is still
   useful. When two regions cannot both reach theirs, the layout changes presentation; nothing is
   squeezed into a strip.
2. **The conversation is the primary surface.** The Workbench and terminal appear on request and
   never leave the chat narrower than `CHAT_MIN`.
3. **Density comes from aligned rows and disclosure,** not from more columns or smaller type.
4. **Colour means state.** Neutral greys carry everything. Blue is running and links, amber waits
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
- **Dragged widths are kept, not overwritten.** The rail opens at `RAIL_W` and drags between `RAIL_MIN_W` and `RAIL_MAX_W`;
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
  (`← Files`) is `SUBBAR_H`. The agent header draws no rule under it; the transcript runs up to it on the
  same surface.
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

Top to bottom; only the list scrolls, so the header, the pages and the foot stay put. The blocks
breathe: `px_3` on the header rows, `pb_3` under the search and the pages, `pt_2` over
the list, `gap_0p5` between rows, and `h_2` of air between projects.

- **The workspace row** (`BAR_H`): a letter tile (the workspace's initial on `chip_on`), its
  name, a `ChevronsUpDown` button opening the workspace menu (the recent workspaces with the
  current one checked, *Open workspace…*, *New workspace…*) and *Hide the rail*.
- **Search and New:** a small borderless field (its fill shows it), *Search…* (short, so it never truncates), with its `Ctrl+K` key cap,
  filters the list as it is typed: a title or a project's name, ignoring case. A search opens
  every project so no match hides behind a fold; while it is on, a click on a project's row
  only moves the cursor there, and folding waits until the search is cleared. Beside it *New* and its caret are one ghost
  group without an edge (the library's `DropdownButton`): *New* starts a session in the cursor's project, else the
  chat's, with the default agent; the caret chooses *Start in* another project or *With agent*
  another agent. With no such project *New* is disabled and the caret is the way in.
- **The page rows:** *Overview*, *Tasks* with its count in a pill, *Issues*, *Workflows*, marked
  with `selected` while their page shows; each says what it holds in a tooltip.
- **Sessions,** with a hairline under it that the list scrolls beneath: the word, *N need
  attention* in `warning`, and a `list-filter` menu: *By project*, *All sessions*, *Needs
  attention*, *Recent activity* (the session whose status changed last first), the chosen one checked. Its glyph is `text` while a filter other
  than *By project* is on. *N need attention* is also a switch: a click shows only the sessions
  that need attention, a second click restores the filter it replaced. While on it sits on
  `chip_on` in medium weight and stays even at none; off and at none it is hidden.
- **A session row** is two lines at `py_1`: its status in a `DOT_COLUMN` on the title's line,
  then the title (fading at its room's end, in full on hover; medium weight when selected or done unread), and
  under the title, past the status's column, a `text_xs` line: `<status> · <agent> · <time>`
  muted, in that order for every status, and the diff, when there is one, at the line's end:
  `+N` in `success`, `−N` in `danger`. The time is how long the session has been in its status,
  said `waiting 4m` for one that needs input. In a flat list the project takes the agent's
  place. Under a project it starts `RAIL_INDENT` in. Its `⋯` shows on hover, and always on the
  selected row.
- **Hover actions on a session** always cover the end of its metadata line, on the row's own
  fill, whether or not the line is full, so they are in one place on every row:
  *Stop* (`square`) while it runs, *Retry* (`Redo`) once it failed, *Archive* (`archive`) always.
  The lab carries them out: Stop makes it idle, Retry running, Archive takes it off the list.
- **Closing or archiving the session on screen** hands the chat to the row drawn below it, or
  above it at the end of the list, never to a row the person cannot see; the cursor follows. With
  no session left the overview shows.
- **A capped list.** A project shows at most `SESSION_CAP` sessions, a flat list the same; past
  it a muted *N more* row shows how many it left out, and a click on it shows them all.
- **A project row** (`ROW_H`): the fold chevron, its folder, the name fading at its room's end, then its branch and only the git parts that are not zero, muted: a `DOT`
  and the count of uncommitted changes, `ArrowUp` and the commits ahead of the remote,
  `ArrowDown` and those behind. Each part names itself in full on hover (*3 uncommitted
  changes*). The name's tooltip says the full name, branch, changes, pinned, the unattended run
  and the path. Clicking the row folds it. On hover, `+` starts a session there and `⋯` opens
  its menu; they lie over the end of the row on its hover fill, so they take no room from the
  name while hidden. A folded project shows a badge, the status mark of its most urgent session that
  needs attention (Failed before Needs input), else the Running mark while any session runs, else
  nothing; it goes when the project opens. An open project with no sessions says *Empty*. Pinned projects come
  first. A project opens with the rail only when something in it needs attention; after that
  folding is the person's.
- ***Add project…*** is the list's last row.
- **Menus:** `⋯` and right-click open the same menu on a row. A project's menu has *Pin to top* or
  *Unpin*, *Work labelled issues* (checked while an unattended run works), *Work an issue…*,
  *New session*, *New worktree…*, *Open terminal*, *Copy project path*, *Refresh Git status*, then
  *Remove from workspace* in `danger`. A session's menu has *Rename…*, *Restart the agent*,
  *Export as Markdown…*, then *Close session* in `danger`. The lab carries out pin, new session,
  open terminal, copy, remove and close; the other entries are there to be seen.
- **The foot** is one row: *Labs*, muted, opens the lab's own pages (*Composer cards*), which are
  not part of the proposal; the Settings button at its end is `selected` while its page shows.

**Status.** One per session, a shape and a colour each, so none is told by colour alone; each
says its word in a tooltip. Every mark is still: nothing in the rail turns.

| Status | Mark | Ink |
|---|---|---|
| Needs input | `hand` | `warning` |
| Running | `LoaderCircle`, still | `accent` |
| Failed | `TriangleAlert` | `danger` |
| Done (unread) | `CircleCheck` | `success` |
| Idle | `circle` | `muted` |

Needs input and Failed need attention: they are what the count, the filter and a folded
project's badge report, the most urgent first (Failed, then Needs input).

**Words on a session.** The rail uses three words the glossary gives to tasks; here they mean:
*Needs attention*, a session that needs input or failed; *Retry the last turn*, send the turn
that failed again; *Restart the agent*, start the session's agent process again. The glossary
is unchanged: the lab is not the app.

**Accessible names.** A status mark, a badge and a git part carry the image role and their words
(a badge: *atlas-api: Failed*). An icon-only button's name is the same as its tooltip, set on a
wrapper: gpui-component's button takes its accessible name only from a text label, so a setter
for an icon-only button belongs upstream.

**Keys** (Ctrl where the app's other keys are): `Ctrl+K` searches, `Ctrl+N` starts a session as
*New* does, from anywhere in the window, a field included, `Alt+↑` / `Alt+↓` open the session before or after in the list's order. While the
list holds the focus (a click on a row gives it), Enter opens the cursor's row or folds its
project, and `←` / `→` fold and unfold the cursor's project. The cursor's row takes `selected`.

## Chat and composer

- **The transcript** is a column of at most `READ_MAX`, centred with equal gutters. The user's
  prompt is the one filled bubble, against the right edge, at most `BUBBLE_MAX`. Everything the
  agent says starts on a shared left axis. Code and output sit in a `sunken` well with no border.
- **Activity is a summary line** (`Ran 2 commands · 1 failed`): a failure inside it is named in the
  summary in danger ink, not only when it is opened. A click opens it: its chevron turns down and
  each command shows with its outcome (`failed` in danger ink) and the tail of its output in a
  well.
- **The composer stack** is at most `COMPOSER_MAX` wide and reads as one object: the pinned cards,
  then the composer, `gap_2p5` apart.
- **The composer** is the app's: a card of two rows, then a strip under it. The card holds the
  field (at least `min_h_10`, placeholder *Ask the agent…*) and the controls: `+`, Fast, the
  model chip, then *Send* fixed at the end. The strip has the branch at the left and the permission
  mode at the right; below `COMPOSER_SPLIT` it takes two lines, the branch over the mode. Card and
  strip are inset `p_1p5`, and the field's text starts at that inset, on the chips' edge. A click
  anywhere in the field's area gives it the caret.
- **Chips** are extra-small ghost buttons `h_6` tall, inset `px_1p5`, their words at `text_xs` in
  full ink and their glyphs muted: `+` (its glyph `size_5`), Fast (the bolt and *On* or *Off*),
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
- **The form** is at most `FORM_MAX`, its title at `text_base`, its rows a hairline group; below
  `FORM_STACK` each row's label stacks over its control. A long description wraps.
- **Sections:** Appearance (theme and reading size as segmented controls, the font as a select),
  Workspace (the check command, switches), Agents, Connections, Shortcuts (key caps).

## Deleting a project

- A project's `⋯` opens gpui-component's `Dialog`, at most `DIALOG_MAX`: the title, the project's
  full name wrapping in the body, what goes and what stays, then *Keep* (outline) before *Delete*
  (solid danger). Deleting takes the project and its sessions out of the rail.

## Pages

- The overview, Tasks, Issues and Workflows take the content area and put the docks away. A page column is at
  most `PAGE_MAX`, inset `px_4`, its sections `gap_6` apart.
- **A section** is a heading at `text_sm`, weight 500, an optional control at its end, then its rows in one
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
  detail replaces the list in the same column under `← Tasks`: the title at `text_xl` with its
  main action beside it, its facts with its state badge at their end, its steps, then what
  awaits approval.
- **Issues:** the list beside the issue while the content area holds `ISSUE_LIST_W` +
  `DETAIL_MIN`, otherwise one at a time under `← Issues`. Above the list: search, *New issue*,
  *Open N | Closed N*. A row's labels are muted facts after its reference (`atlas-api #42 · bug ·
  auto`), never pills. The issue: its title, then one line of facts (`Open · atlas-api #42 ·
  opened … · bug`), where *Open* is a plain fact and not a coloured badge; *Where it stands* with
  one primary; the body; *What the work left*.
- **Workflows:** one section of the workspace's workflows, each with *Run*, and *New workflow* at
  its head. The lab draws the list only; running or writing one is the app's.

## Type, spacing and shape

| Role | Size |
|---|---|
| Metadata, chips, sub-lines | `text_xs`, `muted` |
| Rows, buttons, bars (the UI default, set once on the window) | `text_sm` |
| Section headings | `text_sm`, weight 500 |
| Dialog and form titles | `text_base`, weight 500 |
| Page title | `text_xl`, weight 500 |
| Transcript, composer, documents | `TEXT_READ` at `LEADING_READ` |
| Activity lines, code | `TEXT_READ_SM`, mono for machine text |

- **Two weights,** 400 and 500. Sentence case in every label, no exclamation marks. A back link
  names where it goes.
- **Spacing is chosen by relationship,** on gpui's base-4 scale: `_1` for an icon and its label,
  `_2` between adjacent controls, `_3` inside a card, `_4` for a gutter (`GUTTER` where a width sum
  subtracts it), `_6` between groups, `_8` only between very different parts of a page. Inside a
  row, `_0p5` between its title and the line under it; between rows stacked in a list, `_0p5`.
- **Controls:** buttons take gpui-component's small size, `h_6`, so every button on a bar or in a
  row is the same height; a single-line field and an attachment chip take the same `h_6`. Icons
  take the library's `small` beside a row's text and `xsmall` in a chip. A tab is at most
  `TAB_MAX_W`.
- **Radii come from the theme:** `radius` for controls and rows, `radius_lg` for wells, list
  boxes, popups, the composer and the cards pinned on it, the bubble and a dialog.
  A pill is only for a status badge.
- **Truncation** is one line and the full text on hover: names in the rail fade at their room's
  end, the Workbench's lists and the terminal's tabs end on an ellipsis; state and primary actions always stay visible. A long name in a dialog wraps.
- **Lines are hairlines:** `hairline` divides regions and rows, `control` edges a control. No
  thick borders, and no coloured bar down a card's side to mark it. A seam and the rail's rule are
  `HAIRLINE_PX`, the one length in pixels; an element's edge is one pixel.
- **Fonts.** Inter for the UI and JetBrains Mono for machine text: the families the app ships, read
  from the same files in `assets/fonts/`, so the lab and the app draw in the same faces.

## Interaction

- **Hover** is a fill: `selected` behind a ghost or outline control and behind a row, with the
  pointer cursor on anything a click opens. An icon-only button's glyph is `muted` at rest and
  `text` under the pointer (`icon_button`).
- **Press** changes nothing on its own: no shrink, no colour shift. What was pressed shows in the
  state it leads to.
- **Focus** on a field darkens its edge to `muted` (for the composer, the card's edge). Controls
  draw no ring of their own here.
- **Motion** is limited to what a state change needs: the running spinner in the chat turns (the rail's marks are still), and the activity
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
- **One exception to solid fills: the fade.** A name too long for its room in the rail fades
  out over `FADE_W` at the room's edge instead of ending on an ellipsis (`controls::faded`). The
  band is painted in the row's own opaque fill (the rail, or `selected` over it while hovered or
  chosen), so it is invisible where a name already ended; the whole name shows on hover.
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
- **Status mark:** an icon in its `DOT_COLUMN`, a shape and an ink per status (the rail's table).
  A page row's state dot is `DOT` in the same column.
- **Segmented control:** two or three short labels; the chosen one takes the `selected` fill.
- **Field:** a hairline `control` edge at `h_6`, its placeholder in sentence case.
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
| Bubble and well share `sunken`; hover and selected share `selected` | the app's palette keeps each pair a step apart, its other values the lab's |
| Weights 400 and 500 only | titles are semibold |
| *Stop* is a solid danger button with its word | Send and Stop share an icon button |
| A question's description is said once | the description is repeated as the field's placeholder |
| Settings is a page in the content area | Settings is a large dialog |
| One primary per region, so a list and its detail side by side may each have one | one primary per view |
| The composer is a panel card with no shadow | the composer card floats over the transcript with a shadow |
| A session's meta line ends on its diff, `+N −N` | no diff: nothing reports one per session |
| A failed session's hover action is *Retry*, and every session's *Archive* | *Send the last prompt again* and *Close*: Retry is a task's word, and closing keeps the conversation |

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
| Long project, session, branch and document names | a fade in the rail, elsewhere a controlled ellipsis or wrap; state and primary actions stay visible |
| The composer narrow and at an enlarged reading size | *Send*/*Stop* stays reachable; the branch and the mode never overlap |
| A long permission command, a question with several fields | the content is bounded; the footer's actions and the answer field stay reachable |
| `Ctrl+=` twice, then `Ctrl+0` | the reading content scales and comes back; bars keep their height |
| From the Workbench or the terminal to a session or a page | focus lands on something mounted; application shortcuts keep working |
| Settings with long descriptions | an even section rhythm, readable labels, reachable controls, no sideways overflow |
| Light and dark | identical geometry and hierarchy; spacing never depends on the palette |
