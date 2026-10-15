# UI patterns

The patterns every screen repeats, in the app and its built-in plugins: how a screen is put
together, and how it behaves while it waits, fails, refuses or removes something. `DESIGN.md`
owns the window's layout and the visual rules these patterns are drawn with; a feature's own
document holds its screens' detail. Code never cites this file.

## Layout

### Choose the container

Choose by the content's role before choosing its decoration. These rules apply to app pages and
plugin views alike; the feature contract still decides what content and actions exist.

| Content | Container | Separation |
|---|---|---|
| Related settings or a page subsection | Plain section | Heading, shared text axis and section gap |
| Independent summary of one thing | Box | One outer hairline; no nested boxes for its fields |
| Repeated items to scan or pick | List or table | Stable columns and row states; table hairlines |
| Code, output or a diff | Well | Quiet fill; overflow stays inside it |
| One selected item read in full | Detail | Title, metadata, content; no extra enclosing card |
| A temporary choice or confirmation | Menu, popover or dialog | Floating surface and lift |

The transcript and composer keep their specific treatments in `DESIGN.md`. A page containing
several groups is not automatically a grid of cards. Keep the explicit boxes required by a
feature contract; use plain sections for their internal groupings.

### Hierarchy

The neutral inks of `DESIGN.md` (*Type, space and shape*), applied on a neutral surface:

| Where | Full ink | Middle ink | Muted ink |
|---|---|---|---|
| A row in content: a page, a Workbench list, a menu | its name | the detail on its line: a folder, a number, a kind | its meta, trailing or on a line under it; the label over a run of rows |
| Navigation: the rail, a section nav | the current location, in its fill's paired ink | none | every other row at rest |
| A box, a detail | its title, its content | none | its count, its line of facts |
| A page head, a strip | what it names | none | a count beside the name |

On a fill, text takes the fill's paired ink instead: the rail's current row
`sidebar_accent_foreground`, a selected row elsewhere `accent_foreground`, a badge its `Tint`'s
ink, a button the ink its variant sets. A quieter step on a fill is mixed from that pair toward
the muted ink, its contrast asserted on the fill, never an alpha. A state, a link and code in
prose keep their semantic inks on any surface.

Hover changes a row's fill, never its inks or its layout. Weight follows the role (`DESIGN.md`,
*Type, space and shape*), never a row's urgency: what needs the person says so in its state's ink
and a word.

### Separation

Space, then a line, then a fill, then a shadow (`DESIGN.md`, *Principles*):

| Step | For | Not for |
|---|---|---|
| Space | sections, groups of rows, a heading from what it heads | |
| Hairline | a box's or a table's edge; a table's rows and its head; groups space alone runs together (Settings) | framing a subsection, one field or one row |
| Fill | a well, the bubble, hover and selection, a dock's surface | a section's background; a box that already has an edge |
| Shadow | what floats over other content, through `lift` | anything in the flow |

A boundary is drawn once: no line along a change of surface, no edge doubled by a neighbour's.

### Spacing and density

| Relationship | Default |
|---|---|
| Compact metadata or a tightly coupled mark and label | `gap_1` |
| Icon and label in a row the app draws; adjacent controls; parts within a box | `gap_2` |
| Row inset | `px_2` |
| Strip inset; box padding | `px_3`; `p_3` |
| Form fields | `gap_4` |
| Page sections; page outer padding | `gap_6`; `px_8` `py_6` |
| A tree row's inset; its step per level | `tokens::ROW_INSET`; `tokens::TREE_INDENT` |
| A live grid (the terminal, Neovim) | `px_3` `py_2` |

Smaller steps are for component internals; the largest step is for a deliberate major grouping,
not a second page-padding convention. Use the named pattern's defaults before another legal step.
Equal relationships keep equal gaps across screens. Space between groups exceeds space within
them; align titles, row labels and field labels to consistent text axes. Marks down one column
(icons, state dots, the workspace tile) share one centre line, each in a slot of one width, so the
labels after them start on one edge. A number or a time in a column takes a fixed-width slot, so a
longer value does not push its neighbours.

Density follows the task: chrome and lists are compact, forms leave room for labels, and prose
keeps its reading size and line height. Do not make a crowded panel fit by shrinking its font,
icons or hit targets. Use the existing overflow, wrapping and list/detail patterns instead. The
Issues page's list is the one exception: its rows read at `.text_xs()`, a whole list scanned at a
glance, its title carried by weight and ink rather than size.
Same-role peers keep the same control size (*Control sizes*). A single-line row is `ROW_H` tall; a
row with a second line takes `ROW_H` as its minimum and grows to fit its lines at shared padding
and leading, rather than clipping to that height.

### Control sizes

A size names a role (`DESIGN.md`, *Components*). Each role, the component that serves it, and the
size it takes; `design-tokens/reference.md` holds what each size measures in each component.

| Role | Component | Size |
|---|---|---|
| The Issues page's head: its search, its icon-only actions and its creating action | Button, Input | `.tall()` |
| A form's fields and buttons; a dialog's footer; a page's filter bar: its search and pickers | Button, Input | `.short()` |
| A page head's actions; a strip's controls; a box's control; a row's actions; a secondary action inside dense content (a line of meta text, a transcript block, a chip) | Button, Input | `.xsmall()`, well under a row, so the controls never outweigh what they act on |
| A field in a strip or the rail | Input | `.xsmall()` |
| An icon in a button | Icon | the button's own: `small` in a `.tall()` button, `xsmall` in a `.short()` or `.xsmall()` one |
| An icon leading a label the app draws: a row, a menu item | Icon | `.small()` |
| A state's mark leading a row's label (a session's) | Icon | `.xsmall()`, a step under the label |
| A mark inside a line of meta text | Icon | `.xsmall()` |
| A badge | Tag | `.small()`, at its own height |
| A checkbox, a switch | Checkbox, Switch | `.small()` beside chrome text, at the library's own geometry |
| A single-line row; a row with a second line | drawn by the app | `ROW_H`; at least `ROW_H` |
| A strip along the top of a region | `strip` (plugin host) | `BAR_H` |

- **One size, one measure.** `.xsmall()` already gives a Button and an Input the same height and
  words; the library's default does not, so it is never used. `.short()` is the wrapper that gives
  both the small height with words at `.text_xs()`, so they breathe inside it, and `.tall()` the
  one that gives both `FIELD_H` at the chrome size. A size a component
  gets wrong is corrected in its shared wrapper, never at the call site. The library's `.small()`
  is no control's size; a control holding a badge keeps it, being the one tall enough.
- **The hit area is not the icon.** A taller control keeps the icon its role takes: a `.short()`
  button carries the same extra small icon as an `.xsmall()` one. Nothing is enlarged to fill its
  box.
- **Some components keep their own geometry**: a badge, a checkbox, a switch, a key cap. Centre
  them in their row (`items_center`); never stretch them to the height of the controls beside
  them.
- **Spacing inside a library control is the library's** (a small button's icon and label sit
  `gap_1` apart). The table above governs the space between controls, `gap_2` between two.
- **Zoom.** What the app draws is in rems and follows per-panel zoom. Some library sizes are
  pixels (a switch's track and thumb, a field's inner padding, the library's table rows), so a
  part equal to one of them at 100% parts from it at another zoom: line such parts up by centring
  them, never by matching a number.

### Window

The window keeps the rail's minimum beside the conversation's: it is never smaller than 720 by
480 pixels. Its regions are `DESIGN.md`'s.

### Strips

A strip along the top of a region is `BAR_H` tall and padded `px_3`, its parts `gap_2` apart,
at the chrome size (`strip`): what it names at the start, in full ink, its controls at the end as
`xsmall` ghost buttons (`strip_control` for an icon alone). An icon-only control carries a tooltip.
No line runs under a strip: it and what it heads are one surface, parted by space. The one
exception is the pane header over the Issues page, whose list runs the pane's whole width under
a `faint_rule` line (*Pages*). Keep identity
at the leading edge and actions at the trailing edge in every equivalent strip; a control that
shows a region the strip's own panel hid may lead (the way back to the rail, a list's toggle).
Filters belong below the region header with the content they filter. Reserve the action group's
width while labels truncate; showing hover actions must not move the text beside them.

### Pages

A page takes the agent pane, its name in the pane's header. Its body scrolls as one column centred
in the pane, padded `px_8` at its sides and `py_6` at its ends, its sections `gap_6` apart, as wide as one of the widths below. A page
with a filter bar keeps its head, filters and a table's column names in place while its content
scrolls under them, so the filters stay in reach beside what they filter.

Widths:

- `PAGE_NARROW`: one column of fields or rows (a project's page);
- `PAGE_PROSE`: an editor, or one thing read in full (Workflows, an issue);
- `PAGE_WIDE`: boxes side by side (the overview, Tasks).

The Issues page's list takes the pane's whole width instead, the one page that does: inset
`px_6`, its rows' fills on that edge and their words on the header's, the pane header, the filter
bar and the foot each parted from it by a `faint_rule` line the pane's whole width; over that line its
name in the header stays at the strip's chrome size, a bar over the list rather than a heading.

A page holding sections of its own (Settings) puts their nav column at its start.
The pane header owns the page name, inset `px_8` as the body is; do not repeat it as a large
heading in the body. A section
heading identifies its own content. Reuse the same column width for the same kind of page.

### Page heads and filters

A page that has a head and filters draws them in one order, the Issues page's:

- **The head**: the search first, `.tall()`, with no edge on the well's `muted` fill (an edge in
  that fill is left for focus), as wide as its cap and narrower only when the row runs short; at
  the end its icon-only ghost buttons (*Refresh*) and its creating action, `.tall()` on the
  secondary fill (*New issue*); `gap_6` over the filter bar, a group apart. The head holds no primary, so a
  panel floating over the page never has one to outweigh.
- **The filter bar**, under the head: where the content splits into a few states that each count
  (*Open*, *Closed*), those first, each a `.short()` `choice`, the shown one filled, its count a
  slim `badge` in the hue of that state (open green, closed magenta) sitting outside its top-right corner, its own lower corner a notch over the button's so it
  never covers the words, the choices `gap_8` apart so a count keeps clear of the next; the filters kept to the row's end, wrapping under the choices
  when narrow. Each filter is a `.short()` `picker` with its caret, its label naming what it is
  set to: a ghost in the muted ink while it is where it starts, on the secondary fill in that
  fill's ink once it narrows the list. After them, only while a filter or the search is set,
  *Clear filters*: a word in the danger ink on no fill, underlined under the pointer, that puts
  the search and every filter back (the choice is not a filter); last in its group, so nothing
  moves as it comes and goes.
- **The content** they filter; a list's foot says how many it shows of how many match. A filter's choice and the search are kept while the page is left
  and shown again.

### Boxes

A box groups the facts about one thing on a page: a hairline at `radius`, padded `p_3`, its
parts `gap_2` apart (`card_box`). Its title row (`card_title`) holds the title in semibold, a
muted count, and at most one control at the end, at a fixed height so boxes side by side line up.
Boxes sit side by side while the measured column holds two, and stack when it does not.

### Lists

A single-line row is `ROW_H` tall, padded `px_2` with `gap_2`, at `radius`. Hover fills it with
`list_hover`, the selected row with `accent`. A row's own actions are `.xsmall()` ghost buttons, shown
on hover, on the selected row, on keyboard focus and while their action runs. Every list has a named cap, and its last line says how many more
there are, in meta text. A table is rows split by hairlines, each with one under it, under a head row in
meta text on the page's own surface, its rows square `row`s at the row inset.
Keep metadata and actions in stable trailing slots. Multi-line row variants align their primary
labels and secondary lines across peers. A hovered selected row remains recognizably selected;
keyboard focus remains separately visible, except where the arrow keys move the selection itself
(the Issues page's list): there the selected fill is where the keys are, and no ring is drawn. Hover actions are also available on keyboard focus, so
a row whose actions hide at rest is a tab stop; a focus a click leaves does not show them.

### List and detail

`list_detail` sets a list beside its detail while the measured container holds both widths; else
it shows one at a time, the detail under a `back_link` naming the list. Picking shows the detail;
going back keeps the list's place.

### Forms

Each input is a `field`: its title in chrome text, an optional hint under it in meta text, `gap_2`
to the control. Fields stand `gap_4` apart. An input is built with `text_input`, at a control's
corner and the `.short()` size its buttons take. Problems stand above the submit button in the danger
ink. The submit is the form's one primary action; while the form cannot be sent it refuses, its
tooltip saying why.

### Dialogs

A dialog is the library's `Dialog` at its own width, titled through `title_row`, its ✕ muted and
named in its tooltip. Its fields are `field`s, `gap_4` apart; what blocks it stands above the
footer in the danger ink. Its footer (`footer_row`) sits on the right: the dismissing button
first, then the one primary action or the danger word, at `.short()`. A third action (*Use the
automatic title*, *Put it somewhere else…*) stands at the footer's start, apart from the two that
close it. It opens with focus on its first field and closes on Esc. A question without fields (a
retry, *Answer the review*, a removal) is an alert dialog, as the confirmation below is, never a
dialog of its own; a start that fails is said once, in an error toast.

### Details

A detail shows one thing in full (an issue, a task): a trail naming where it sits, with its
actions beside it (one primary, one other, `⋯`); its title in `text_xl` semibold; one line of
facts in meta text; what it waits on; then its tabs.

## Behaviour

### Waiting

State changes keep the surrounding layout stable: retain titles, content axes and action slots.
Feedback belongs to the affected content or action; it does not restyle the whole page. Status
uses a word or recognizable mark as well as colour. A quieter state must still be readable.

- **Content on its way** is one meta line where the content will appear, naming the work and
  ending on an ellipsis: *Reading the workflows…*. A panel never sits blank while a read runs.
- **An action in flight** shows on the button that started it, `.loading(true)`, which refuses a
  second press; its label stays.
- A `Spinner` stands only inside a line or a control already on screen.

### Immediate feedback

- **A change a person can take back** (a rename, a pin, a fold, a reorder, a filter, a pick)
  shows in the frame it is made; saving it runs after, off the UI thread.
- **Saves of one thing run one at a time, in the order its changes were made**, so an older save
  never lands over a newer one.
- **A save that fails** matters only while no later save carries that thing: the screen goes
  back to the last value known saved, not the one before the change, and says so once, as an
  `error` toast (*Renaming the session failed: <why>*). A later save that carries it supersedes
  the failure, and that save decides what the screen shows.
- **What goes back is the one change, not the file**: one write of a file can carry several
  things (the pin, the order, the theme), and each goes back alone to its own last saved value.
  A write that carries a thing and lands moves its saved value forward.
- **A save that is an act rather than a value** (closing an issue, adding a comment, a write that
  runs a step on the file as it then stands) is superseded by nothing: a newer act does not carry
  the older one, so each failure is said once, and the screen reloads what the file holds.
- **A pick the agent applies** (a mode, a model, an effort, *Fast*) shows at once. The agent
  refusing it is its failed save: back to the value the agent last confirmed, one toast.
- **A dragged size and a dock's open state** are saved but never put back, since the layout would
  move under the pointer; a failed save still says so once.
- **A rename and a shortcut show at once; a form that writes a file** (a workflow, an issue, an
  agent) waits on its *Save*.
- **The rule lives in core** (`onehand_core::saving`): a writer per file that runs saves in the
  order sent, `Saving` for one value and `Saves` for several things in one file, tested without
  a GUI. The app keeps the spawn and the toast.
- **Nothing queued is lost on leaving**: a closing window's last workspace save and the settings
  saves still queued at quit go through the same ordered writer, waited for up to a bound.
- **Work that leaves the machine or takes its time** (git, the network, an agent) is never drawn
  as done before it is: it waits on the button that started it, as *Waiting* says.

### Motion

- Nothing animates. A hover, a selection, a fold or a new state lands in the frame it happens in.
- The one motion is a `Spinner` on work still running, where *Waiting* puts it. The library's own
  overlays (dialogs, popovers, toasts) and controls (a switch's thumb) keep what they do
  themselves.
- A size or a position never animates, and nothing pulses or slides to draw the eye (a field's
  caret blinks on its own); a state that needs the person says so in ink and a word.

### Nothing to show

One meta line, in sentence case, without a period: at the list's place, left-aligned, inside a
list; centred (`hint`) when it fills the whole panel. *No <things> yet* when there have never been
any; *No <things> match* when a search or a filter hides them all; *Nothing <state>* for a status
box (*Nothing needs you*).

### Failing and refusing

- **An action a person started that failed** is an `error` toast: *<What> failed: <why>*.
- **A condition that stands until something answers it** (a save refused because the file changed
  on disk, an editor that would not start) is a `status_line` under the view, in the ink of its
  state, cleared by whatever answers it.
- One failure is said once: a toast or a line, never both.
- **A press that cannot act yet** refuses (`.refuses(Some(why))`), its tooltip saying why, and
  says nothing else.
- Every failure of something a person started reaches the screen; a log line alone is not enough.

### Toasts

Transient news goes in a toast, `window.push_notification` with a notification built by
the plugin host's `toast`, `crate::toast` in the app (which gives a failure its form and trims
the period), its kind chosen by what it
means:

- `error`: an action failed;
- `warning`: done only in part, or something now needs the person;
- `success`: a long or background action finished, which the screen does not already show;
- `info`: news that asks for nothing.

A toast is one sentence without a trailing period.

### Confirming a removal

A removal that cannot be undone asks first: data taken off disk, a process ended, unsaved work
thrown away. It asks through `Confirm` (`crates/plugin-host/src/confirm.rs`), the same question
in the app and in a plugin:

- the title asks the question naming the thing: *Remove <name> from the workspace?*;
- the description says what goes and what stays;
- the buttons read *Keep*, then the danger word, the same verb as the control that asked.

What can be undone or resumed goes ahead without asking: stopping a task that offers *Resume*,
taking an item off the queue, unbinding a key, closing an issue. Closing an issue a run is working
on asks (*Close <title>?*), since it ends that run too. The control starting a removal is a word in the
danger tint.

### Keys and focus

- **Every key that does something is a command** in the keymap (`crates/app/src/keymap.rs`), so
  it can be rebound; `on_key_down` is for typing only.
- **A control bound to a command shows its key** in its tooltip (a `Kbd`); a field shows it as a
  `Kbd` at its end. A label's text never spells a key.
- **Keyboard focus shows the ring**, only when the keyboard moved it there. A field shows focus by
  its edge alone, turned the `ring` ink; one with no edge at rest gains one while focused.
- A dialog, a popup or a floating panel opens with focus on its first field, or on its primary
  action when it has none. A panel floating beside the list it was picked from (the Issues
  page's issue) leaves focus on the list, so ↑ and ↓ keep stepping it. Each closes on Esc, asking first when a form in it holds unsaved
  changes. On closing, focus goes back to what opened it, or to the conversation when that is gone.
- Esc closes the innermost layer only. A page is a place, not a layer: Esc leaves it where it is.
- A key with no modifier that a region handles never fires while a field inside it holds the
  caret.

### Words

- Sentence case everywhere.
- A label ends on an ellipsis when its action asks something before it acts: a dialog, a
  confirmation, a picker or a form (*Retry…*, *Delete conversation…*).
- *Delete* takes a thing off disk; *Remove* takes it out of a list and leaves it; *Uninstall* is
  for a plugin; *Discard* throws away unsaved changes.
- *Keep* dismisses a confirmation; *Cancel* leaves a form or a dialog; *Close* puts a panel away.
- A command reads as a plain sentence, its article kept: *Restart the agent*, *Hide the rail*.
- Hints, empty lines and toasts end without a period; a dialog's description is whole sentences.
- On-screen words are the glossary's (`CONTEXT.md`), never its _Avoid_ words.
