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
| Icon and label; adjacent controls; parts within a box | `gap_2` |
| Row inset | `px_2` |
| Strip inset; box padding | `px_3`; `p_3` |
| Form fields | `gap_4` |
| Page sections; page outer padding | `gap_6`; `p_6` |

Smaller steps are for component internals; the largest step is for a deliberate major grouping,
not a second page-padding convention. Use the named pattern's defaults before another legal step.
Equal relationships keep equal gaps across screens. Space between groups exceeds space within
them; align titles, row labels and field labels to consistent text axes. Marks down one column
(icons, state dots, the workspace tile) share one centre line, each in a slot of one width, so the
labels after them start on one edge. A number or a time in a column takes a fixed-width slot, so a
longer value does not push its neighbours.

Density follows the task: chrome and lists are compact, forms leave room for labels, and prose
keeps its reading size and line height. Do not make a crowded panel fit by shrinking its font,
icons or hit targets. Use the existing overflow, wrapping and list/detail patterns instead.
Same-role peers keep the same control size. Single-line rows use `ROW_H`; a row with secondary
text grows to fit its lines at shared padding and leading, rather than clipping to that height.

### Window

The window keeps the rail's minimum beside the conversation's: it is never smaller than 720 by
480 pixels. Its regions are `DESIGN.md`'s.

### Strips

A strip along the top of a region is `BAR_H` tall and padded `px_3`: what it names at the start,
its controls at the end as `small` ghost buttons. An icon-only control carries a tooltip.
Keep identity at the leading edge and actions at the trailing edge in every equivalent strip.
Filters belong below the region header with the content they filter. Reserve the action group's
width while labels truncate; showing hover actions must not move the text beside them.

### Pages

A page takes the agent pane, its name in the pane's header. Its body scrolls as one column centred
in the pane, padded `p_6`, its sections `gap_6` apart, as wide as one of:

- `PAGE_NARROW`: one column of fields or rows (a project's page);
- `PAGE_PROSE`: an editor, or one thing read in full (Workflows, an issue);
- `PAGE_WIDE`: boxes side by side, or a table (the overview, Tasks, Issues).

A page holding sections of its own (Settings) puts their nav column at its start.
The pane header owns the page name; do not repeat it as a large heading in the body. A section
heading identifies its own content. Reuse the same column width for the same kind of page.

### Page heads and filters

A page that has a head and filters draws them in one order, the Issues page's:

- **The head**: what the page counts at the start, in muted ink (*12 open*); its actions at the
  end at `.tall()`, icon-only ghost buttons first (*Refresh*), the page's one primary last
  (*New issue*).
- **The filter bar**, under the head: the search first, taking the free width, then each filter
  as a `.tall()` secondary picker with its caret, its label naming what it is set to.
- **The content** they filter. A filter's choice and the search are kept while the page is left
  and shown again.

### Boxes

A box groups the facts about one thing on a page: a hairline at `radius_lg`, padded `p_3`, its
parts `gap_2` apart (`card_box`). Its title row (`card_title`) holds the title in semibold, a
muted count, and at most one control at the end, at a fixed height so boxes side by side line up.
Boxes sit side by side while the measured column holds two, and stack when it does not.

### Lists

A single-line row is `ROW_H` tall, padded `px_2` with `gap_2`, at `radius_tokens().sm`. Hover fills it with
`list_hover`, the selected row with `accent`. A row's own actions are `xsmall` ghost buttons, shown
on hover and on the selected row. Every list has a named cap, and its last line says how many more
there are, in meta text. A table is a box of rows split by hairlines, under a head row in meta text.
Keep metadata and actions in stable trailing slots. Multi-line row variants align their primary
labels and secondary lines across peers. A hovered selected row remains recognizably selected;
keyboard focus remains separately visible. Hover actions are also available on keyboard focus.

### List and detail

`list_detail` sets a list beside its detail while the measured container holds both widths; else
it shows one at a time, the detail under a `back_link` naming the list. Picking shows the detail;
going back keeps the list's place.

### Forms

Each input is a `field`: its title in chrome text, an optional hint under it in meta text, `gap_2`
to the control. Fields stand `gap_4` apart. An input is built with `text_input`, at a control's
corner and the `.tall()` size its buttons take. Problems stand above the submit button in the danger
ink. The submit is the form's one primary action; while the form cannot be sent it refuses, its
tooltip saying why.

### Dialogs

A dialog is the library's `Dialog` at its own width, titled through `title_row`. Its footer sits on
the right: the dismissing button first, then the one primary action or the danger word, at
`.tall()`. It opens with focus on its first field and closes on Esc. A removal asks through the
confirmation below, never through a dialog of its own.

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
- **A save that fails** matters only while its change is still the newest to that thing: the
  screen goes back to the last value known saved, not the one before the change, and says so
  once, as an `error` toast (*Renaming the session failed: <why>*). A newer change supersedes it,
  and that change's own save decides what the screen shows.
- **Work that leaves the machine or takes its time** (git, the network, an agent) is never drawn
  as done before it is: it waits on the button that started it, as *Waiting* says.

### Motion

- Nothing animates. A hover, a selection, a fold or a new state lands in the frame it happens in.
- The one motion is a `Spinner` on work still running, where *Waiting* puts it. The library's own
  overlays (dialogs, popovers, toasts) keep what they do themselves.
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
- **A press that cannot act yet** refuses (`.refuses(true)`) with a tooltip saying why, and says
  nothing else.
- Every failure of something a person started reaches the screen; a log line alone is not enough.

### Toasts

Transient news goes in a toast, `window.push_notification` with a notification built by
`crate::toast` (which gives a failure its form and trims the period), its kind chosen by what it
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
taking an item off the queue, unbinding a key. The control starting a removal is a word in the
danger tint.

### Keys and focus

- **Every key that does something is a command** in the keymap (`crates/app/src/keymap.rs`), so
  it can be rebound; `on_key_down` is for typing only.
- **A control bound to a command shows its key** in its tooltip (a `Kbd`); a field shows it as a
  `Kbd` at its end. A label's text never spells a key.
- **Keyboard focus shows the ring**, only when the keyboard moved it there.
- A dialog, a popup or a floating panel opens with focus on its first field, or on its primary
  action when it has none, and closes on Esc, asking first when a form in it holds unsaved
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
