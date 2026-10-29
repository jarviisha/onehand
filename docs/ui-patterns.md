# UI patterns

The patterns every screen repeats, in the app and its feature crates: how a screen is put
together, and how it behaves while it waits, fails, refuses or removes something. `DESIGN.md`
owns the window's frame and the visual rules these patterns are drawn with, `docs/screens.md`
what each region holds, and a feature's own document its screens' detail. Code never cites this
file.

## Layout

### Choose the container

Choose by the content's role before choosing its decoration. These rules apply to app pages and
feature crate views alike; the feature contract still decides what content and actions exist.

| Content | Container | Separation |
|---|---|---|
| Related settings (Settings), a group inside a form | Plain section | Heading, shared text axis and section gap |
| A group on a page: one thing's facts, a list, a status, what a project is set to | Box | One outer edge in a control's edge, no fill; no nested boxes for its fields |
| Repeated items to scan or pick | List or table | Stable columns and row states; table hairlines |
| Repeated items opened one at a time, each with actions of its own (Workflows) | Grid of boxes | A gap; a box's edge raised on hover, never a fill |
| Code, output or a diff | Well | Quiet fill; overflow stays inside it |
| One selected item read in full | Detail | Title, metadata, content; no extra enclosing card |
| A temporary choice or confirmation | Menu, popover or dialog | Floating surface and lift |

The transcript and composer keep their specific treatments in `DESIGN.md`. On a page each group
stands in a box, so what belongs together reads as one object rather than as text loose on the
page; what groups inside a box (a form's fields, a box's rows) takes the plain treatments, never
a box of its own.

### Hierarchy

The neutral inks of `DESIGN.md` (*Type, space and shape*), applied on a neutral surface:

| Where | Full ink | Middle ink | Muted ink |
|---|---|---|---|
| A row in content: a page, a Workbench list, a menu | its name | the detail on its line: a folder, a number, a kind | its meta, trailing or on a line under it; the label over a run of rows |
| Navigation: the rail, a section nav | the current location, in its fill's paired ink | none | every other row at rest |
| A box, a detail | its title, its content | none | its count, its line of facts |
| A page's head | its name, at the title size | its line of facts | none |
| A strip | what it names | none | a count beside the name |

Text on a fill, and text that carries meaning, takes the ink `DESIGN.md` names for it (*Type,
space and shape*). A row's hover and selected fills are steps from the surface under it
(`DESIGN.md`, *Colour and state*).

Hover changes a list row's fill, never its inks or its layout; a line row answers by its ink
instead (*Line rows*). Weight follows the role (`DESIGN.md`,
*Type, space and shape*), never a row's urgency: what needs the person says so in its state's ink
and a word.

### Spacing and density

| Relationship | Default |
|---|---|
| Compact metadata or a tightly coupled mark and label | `gap_1` |
| Icon and label in a row the app draws; adjacent controls; parts within a box | `gap_2` |
| Row inset | `px_2` |
| Strip inset; box padding | `px_3`, inside the Workbench `tokens::ROW_INSET`; `p_3` |
| A box whose body scrolls inside it (a pinned card) | `p_4`, a step more so the scrollbar's thumb has room beside the text |
| Form fields | `gap_4` |
| Page sections; page outer padding | `gap_6`; `px_8` `py_6` |
| A list page's inset | `px_6` |
| A tree row's inset; its fill off the dock's sides; its pitch, fill and room around the fill; its step per level | `tokens::ROW_INSET`; `tokens::ROW_GUTTER`; `tokens::TREE_ROW_H`, `TREE_ROW_FILL`, `TREE_ROW_ABOVE`/`BELOW`; `tokens::TREE_INDENT` |
| A live grid (the terminal, Neovim) | `px_3` `py_2` |
| Text beside the library's scrollbar | `tokens::SCROLLBAR_W` on the scrolling side, so the thumb never lies over words |

Smaller steps are for component internals; the largest step is for a deliberate major grouping,
not a second page-padding convention. Use the named pattern's defaults before another legal step.
Equal relationships keep equal gaps across screens. Space between groups exceeds space within
them; align titles, row labels and field labels to consistent text axes. Marks down one column
(icons, state dots, the workspace tile) share one centre line, each in a slot of one width, so the
labels after them start on one edge. A number or a time in a column takes a fixed-width slot, so a
longer value does not push its neighbours.

Density follows the task: chrome and lists are compact, forms leave room for labels, and prose
keeps its reading size and line height. Chrome breathes; reading text stays dense:

- A line that is itself content (an activity entry, a conflict, a fact's value, a row's
  description) takes `CHROME_TEXT`; `.text_xs()` is for meta beside something (a time, a count,
  a hint, the label over a run of rows).
- Inside the Workbench, the parts of one group stand `gap_3` apart and groups `gap_6`, so an
  issue's head reads as one thing and what follows it as the next.
- Entries of a chrome list (an issue's activity, a plugin's skills) stand at least `gap_2` apart.
- Reading text (an issue's description, Markdown, code) keeps the reading size on `LEADING`. Do not make a crowded panel fit by shrinking its font,
icons or hit targets. Use the existing overflow, wrapping and list/detail patterns instead. A
list page's rows are a density of their own, not a shrunk panel (*List pages*).
Same-role peers keep the same control size (*Control sizes*). A single-line row is `ROW_H` tall; a
row with a second line takes `ROW_H` as its minimum and grows to fit its lines at shared padding
and leading, rather than clipping to that height.

### Control sizes

A size names a role (`DESIGN.md`, *Components*). Each role, the component that serves it, and the
size it takes; `.claude/skills/ui/tokens.md` holds what each size measures in each component.

| Role | Component | Size |
|---|---|---|
| A page's head and the row under it: its actions, its search, its filters' choices and pickers, a split action's both halves | Button, Input | `.tall()` |
| An icon-only button in a page's head or a page's form | Button | `.tall_icon()`, square at `FIELD_H` |
| A page's form: its fields, its pickers, its switch and its buttons | Button, Input | `.tall()` |
| A form in a dialog at `DIALOG_FORM` (the agent form, the workflow launcher, the issue picker): its fields, its pickers and its footer | Button, Input | `.tall()` |
| A dialog that asks one thing (a rename, a worktree): its fields and footer; a dock's filter bar, its search, choices and pickers | Button, Input | `.short()` |
| Every other button with words: a strip's, a box's, a row's, a secondary action inside dense content (a line of meta text, a transcript block, a chip), a tab, a fold toggle, a pinned card's answers | Button | `.short()`, under a row, so the controls never outweigh what they act on |
| Every other icon-only button: a strip's, a row's, a copy control, the composer's send | Button | `.small()`, square at `CONTROL_H`, so an icon alone is as tall as a worded peer beside it |
| A field in a strip or the rail | Input | `.short()`, as tall as the buttons beside it |
| An icon in a button | Icon | the button's own: `small` in a `.tall()` button and in an icon-only `.small()` one, `xsmall` beside words in a `.short()` one |
| An icon leading a label the app draws: a row, a menu item | Icon | `.small()` |
| A state's mark leading a row's label (a session's) | Icon | `.xsmall()`, a step under the label |
| A mark inside a line of meta text | Icon | `.xsmall()` |
| A badge: a tag standing on its own (a state, a count, *Default*) | Tag | `.small()`, at its own height |
| A label inside a line of meta text (an issue row's) | text | its kind's ink (`label_tint`), the one part of the line that truncates; the row's quieter ink on the selected fill |
| A checkbox, a switch | Checkbox, Switch | `.small()` beside chrome text, at the library's own geometry |
| A single-line row; a row with a second line | drawn by the app | `ROW_H`; at least `ROW_H` |
| A strip along the top of a region | `strip` (`onehand_ui`) | `BAR_H` |
| A mode's own bar under the Workbench's strip | `mode_strip` | `MODE_BAR_H` |

- **`.tall()` is the ceiling.** No control stands taller than `FIELD_H`; a page gives a field
  room by its size, never by a larger control.
- **One size, one measure.** The library's default stands a button 32px tall, over `.tall()`'s
  ceiling, so it is never used. `.short()` is the wrapper that gives a Button and
  an Input the small height with words at `.text_xs()`, so they breathe inside it, and `.tall()`
  the one that gives both `FIELD_H` at `.text_sm()`, the reading size. A size a component gets wrong is corrected
  in its shared wrapper, never at the call site. The library's `.small()` is an icon-only
  button's size, square at the same height as `.short()`; a worded button never takes it, as its
  words would stand at `.text_sm()`. A control holding a badge keeps it, being the one tall
  enough. `.xsmall()` is no field's or worded button's size: an icon takes it, and an icon-only
  button inside a small object (an attachment's plate), which scales with the object it sits on.
- **One row of actions, one height.** Every control standing in one row of actions (a button, both
  halves of a split action, a field, a picker, a choice, an icon alone) takes the same size, so
  their tops and bottoms meet on one line. The row takes the size its leading role takes, and
  every other control in it follows; a control whose role elsewhere takes another size takes
  the row's here. Only the components that keep their own geometry (below) stand apart, centred.
- **The hit area is not the icon.** A worded `.short()` button carries the extra small icon
  beside its words; an icon alone at `.small()` takes the small one, its box no larger than a
  worded peer's. Nothing is enlarged to fill its box.
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
ghost buttons at their control sizes (`strip_control` for an icon alone). An icon-only control is named through `.named(..)`: its
tooltip, and the same words as its accessible name.
Keep identity at the leading edge and actions at the trailing edge in every equivalent strip; a control that
shows a region the strip's own panel hid may lead (the way back to the rail, a list's toggle).
Filters belong below the region header with the content they filter. Reserve the action group's
width while labels truncate; showing hover actions must not move the text beside them.

Inside the Workbench a bar is a `workbench_strip`: padded `tokens::ROW_INSET` rather than `px_3`,
so its words and the tree rows' names read down one edge. Each Workbench region draws one
hairline (`border`) where its head ends and its content begins: under the Workbench's own strip,
under a mode's head (the editor's tabs, the reader's bar, an issue's trail). A mode's own bar
under the Workbench's strip is a `mode_strip`, `tokens::MODE_BAR_H` tall, a step under the strip
over it so it reads as the mode's; where two bars stack, only the lower one draws the line over
the content. The conversation's header and the terminal's strip draw no line and keep
`px_3`.

### Cuts

What runs past its room is cut by a fade: a gradient from the clear surface to the solid one
over its end, which says something is cut. It serves a rail name too long for its row, a long
command folded in the transcript, the conversation where it runs under the composer, and the
staged files beside *Show all N*. Text inside a row that has no fade truncates on an ellipsis.
Nothing else is drawn see-through.

### Pages

A page takes the agent pane. Every page names itself in its head, in the body (`page_head`): its
name at the title size, a line of facts under it in the middle ink where it has facts to give, its
actions at the end; the pane's header above it carries chrome alone. A box's heading identifies its
own content. A page is one of two kinds, chosen by what it holds, and the kind decides its width
and its density, never its head:

- a **column page** holds fields, boxes or one thing read in full (a project's page, the
  overview, Tasks, Schedules, a task, Settings, Workflows' grid of cards); a workflow opened from
  it takes a page of three columns of its own;
- a **list page** holds one long list to scan, filter and pick from (Issues).

### The top of a page

A workspace-wide page (the overview, Tasks, Issues, Workflows, Schedules) and Settings keep their
top in place while the rest scrolls under it (`page_top`): the head, then, where the page has them,
a row of its search and its filters. The top is inset `px_8`, close under the pane's header
(`pt_2`), its parts `gap_6` apart, in the same column as the body under it (the whole pane on a
list page, and on Settings, whose name starts on its nav column's edge); under it a `faint_rule`
line runs the pane's whole width, drawn always, so every page's top ends on one line whatever the
width of its column.

- **The head.** Its actions are `.tall()`, at one height; its icon-only ghost buttons
  (*Refresh*) stand before its creating action. The creating action (*New session*, Workflows'
  *New*, *New schedule*, *New issue*) is the view's one primary, stepping down to `secondary`
  while a form it opened, or an item floating over the list, is open and holds it. A page that
  only sets values (Settings) holds no primary, every button on it secondary, and parts its many
  peer groups by hairlines with semibold headings, as space alone would run them together.
- **The row under it.** The search first (`page_search`: `.tall()`, as wide as its cap and
  narrower only when the row runs short), then the filter bar's choices and filters (*Filter
  bars*), kept to the row's end; the row wraps when narrow. A page with filters and no search
  keeps them at the row's end all the same.
- A table's column names, where a page has them, stay in the top too.

A page reached from one of these (a task, a schedule's form, a project's page) scrolls as one
column under its trail back.

### Column pages

The body scrolls as one column centred in the pane, padded `px_8` at its sides and `py_6` at its
ends, its sections `gap_6` apart, as wide as one of these:

- `PAGE_NARROW`: one column of fields or rows;
- `PAGE_PROSE`: an editor, or one thing read in full (a workflow, an issue, a task, a project);
- `PAGE_WIDE`: boxes side by side, or a workspace-wide list in a box (the overview, Tasks,
  Workflows' cards, Schedules).

The pane header is inset `px_8`, as the body is. A page holding sections of its own (Settings)
puts their nav column at its start. Reuse the same column width for the same kind of page.

### List pages

The list takes the pane's whole width, a whole list scanned at a glance; its top is every page's.

- **Edges.** Inset `px_6`, its rows' fills on that edge and their words on the top's `px_8`. The
  top and the foot are each parted from the list by a `faint_rule` line the pane's whole width, so
  the edge-to-edge list reads as one surface.
- **Rows** on one line at `.text_xs()`, no lines between them: the name and the column read down
  the list (the age) in `font_medium`, carried by weight and ink rather than size; the rest at the
  regular weight. Trailing slots drop, least needed first, where the pane is too narrow to leave
  the name room. Rows keep their place while a person reads.
- **Keys.** The arrow keys move the selection itself, so the selected fill marks where the keys
  are and no ring is drawn.
- **The foot**: *Showing N of M <things>*.
- **The detail.** Nothing is open on arrival. A picked item floats over the list's right side,
  its width dragged for the session, leaving focus on the list so ↑ and ↓ keep stepping it; `×`
  and Esc close it.
- The filters, the search, the selection, the open item and the scroll are kept while the page is
  left and shown again.

### Filter bars

Whatever filters its content draws its filters in one order. On a page they stand in the row under
its head, after the search, at `.tall()` as the search is, so the row keeps one height. In a dock, which is dense chrome with no head, the bar leads with its
search and takes the same order, all at `.short()` (the Plugins mode's marketplaces as choices). A
dock's bar that stands over a list and its detail both (the Issues tab) leaves the choices out:
they split the list, so they stand over its rows, under the bar's line.

- where the content splits into a few states that each count (*Open*, *Closed*), those first,
  each a `choice` at the bar's size, the shown one filled, its count a slim `badge` in the hue of that
  state (neutral at zero, which has nothing to draw the eye to) hung from its top-right corner, its lower-left corner `2` in from that point each way,
  over the button's inset and above its words, the choices `gap_6` apart so a count keeps clear
  of the next;
- the filters, kept to the row's end after the choices, wrapping under them when narrow. Each is a
  `picker` at the bar's size with its caret, its label naming what it is set to: a ghost in the muted
  ink while it is where it starts, on the secondary fill in that fill's ink once it narrows the
  content;
- after them, where more than one thing narrows the content and only while one is set, *Clear
  filters*: a ghost at the bar's size that
  puts the search and every filter back (a choice is not a filter); last in its group, so nothing
  moves as it comes and goes.

The content's foot says how many it shows of how many match. A filter's choice and the search are
kept while the page is left and shown again.

### Boxes

A box groups one thing on a page: edged in a control's edge (`input`) at `radius`, on no fill,
padded `p_3`, its parts `gap_2` apart (`card_box`). Every fill under a page read as a slab too
heavy for it or swallowed the fields and lines drawn on it, and the hairline sits too near the
page's fill in Dark Modern to group anything. Its title row (`card_title`) holds the title in semibold, a
muted count, and at most one control at the end, at a fixed height so boxes side by side line up.
Boxes sit side by side while the measured column holds two, and stack when it does not. A box
that opens what it shows (a card in a grid) is pressed on its body: it takes the pointer, raises
its edge on hover to the muted ink rather than filling, and rings on focus, Enter opening it. Its
actions stand in its foot, since its title row holds at most one control, and a press on them
stays theirs. Cards in a grid stand in rows of as many as the measured column holds at their
least width, up to four, a short last row keeping the column's width.

### Lists

A single-line row is `ROW_H` tall, padded `px_2` with `gap_2`, at `radius`. Hover fills it with
`list_hover`, the selected row with `accent`. A row standing on a region's bare surface, with no
frame of its own around the list, keeps those fills off that region's edges: a tree in a dock by
`tokens::ROW_GUTTER`, any other list in a dock by its own padding, a list page by its inset. In a
list where a row can be selected, two fills never touch: a hovered row beside the selected one
stays parted from it, a tree's fills by `tokens::ROW_GUTTER` inside the row's pitch, any other
list by a gap between its rows; a switch's choices likewise (`switch` pads its track `p_0p5` and
parts its choices `gap_0p5`). A list nothing in it is ever selected (Plugins) may run its hover
fills together. Rows
inside a box are the box's parts: their fills run to its edge, as the hairlines between them do. A row's own actions are ghost buttons at their control sizes, shown
on hover, on the selected row, on keyboard focus and while their action runs. Every list has a named cap. Its last line, in meta
text, says why the list is short: a list the cap cut says how many more there are (*N more*); a
list a search or a filter narrows says how many it shows of how many there are (*Showing N of
M*). A table is rows split by hairlines, each with one under it, under a head row in
meta text on the page's own surface, its rows square `row`s at the row inset.
Rows in a box are parted by a hairline (`ruled`), drawn on a wrapper of the row's own so its
focus ring moves nothing; so are a Settings group's list rows (`list_rows`), whose group stands in
no box. A row's trailing facts reach the box's edge; where a row has them, its
hover actions are laid over them, hiding them while they show, rather than holding a slot beside
them. A row without trailing facts keeps its actions in a held slot. Keep metadata and actions in
stable trailing slots. Multi-line row variants align their primary
labels and secondary lines across peers. A hovered selected row remains recognizably selected;
keyboard focus remains separately visible, but on a list page, whose arrow keys move the
selection itself (*List pages*). Hover actions are also available on keyboard focus, so
a row whose actions hide at rest is a tab stop; a focus a click leaves does not show them.

A row's actions follow what the person must do next. A row waiting on the person (*Needs
attention*) keeps its actions drawn, since they are its point. The keys passing a row whose
content is open on screen, already holding those actions, lay none, so a row the keys only pass
through stays still.

### Line rows

Rows inside reading content (a fold line, an activity row in the transcript) are lines, not list
rows: as tall as their text, taking no focus, they answer the pointer by raising their ink rather
than with a fill, which would break the reading column into tiles. Their actions are the block's
(one copy control, one show-more control).

### List and detail

`list_detail` sets a list beside its detail while the measured container holds both widths; else
it shows one at a time, the detail under a `back_link` naming the list. Picking shows the detail;
going back keeps the list's place. A detail read as prose (an issue, a Markdown document) is a
column up to `PAGE_PROSE`, centred with equal gutters, so hiding the list widens it rather than
leaving the freed width empty; its scroll spans the detail, the scrollbar on the detail's edge.

### Forms

Each input is a `field`: its title in chrome text, an optional hint under it in meta text, `gap_2`
to the control. Fields stand `gap_4` apart. An input is built with `text_input`, at a control's
corner and the size of the place it stands in: `.tall()` in a page's form and in a dialog at
`DIALOG_FORM`, `.short()` in a dialog that asks one thing; its buttons, pickers and switch take the
same. Problems stand above the submit button in the danger
ink; in a page's form, in one box on the danger tint's fill led by the warning mark, each a
sentence. A page form whose submit stands in the page's head (the workflow editor, whose columns
leave no one place above it) sets them instead in a bar along the page's foot, the page's whole
width, on its kind's tint led by its mark: the problems on the danger tint with the warning mark,
a standing note (read-only) on the accent's tint with the info mark. A new form nobody has changed yet shows none: its submit refuses, its tooltip saying
the first, until the first change; one opened on something (a file, a copy, an import) says them
at once. The submit is the form's one primary action; while the form cannot be sent it refuses, its
tooltip saying why.

### Dialogs

A dialog is the library's `Dialog` at its own width, titled through `title_row`, its ✕ muted and
named in its tooltip, over a backdrop that dims the window. Its fields are `field`s, `gap_4`
apart; what blocks it stands above the footer in the danger ink. Its footer (`footer_row`) sits
on the right: the dismissing button first, then the one primary action or the danger word, at
its fields' size. The size follows the dialog: one that asks one thing (a rename, a worktree)
stands at its own width with `.short()` controls; one that holds a form (the agent form, the
workflow launcher, the issue picker) stands at `DIALOG_FORM` with `.tall()` ones. A third action (*Use the automatic title*, *Put it somewhere else…*) stands at the
footer's start, apart from the two that close it, on the secondary fill, as it starts its line. It opens with focus on its first field and
closes on Esc. A question without fields (a retry, *Answer the review*, a removal) is an alert
dialog, as the confirmation below is, never a dialog of its own; a start that fails is said once,
in an error toast.

A dialog that starts a run (the workflow launcher, the issue picker) says what the run is before
its fields, so it stands at `DIALOG_FORM` and parts its sections (what runs, where it works, what
stands in the way, what the person adds) with a hairline (`sections`). The branch it cuts stands
in a well beside its branch mark. Each thing the preflight found leads with its state's mark: a
warning in the danger ink when it blocks the start, an information mark in meta ink when it only
tells; an action beside one takes the secondary fill.

### Details

A detail shows one thing in full (an issue, a task): a trail naming where it sits, with its
actions beside it (one primary, one other, `⋯`; an issue's next one secondary and the other a
ghost, since *New issue* holds its view's primary); its title in `text_xl` semibold; one line of
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
- The motions are two. A `Spinner` on work still running, where *Waiting* puts it. And a card
  tucked behind another, its edge showing past it: pressed, it slides out from under that card to
  its open height, and slides back when closed, short and eased (about 200ms), moving only then.
  The library's own overlays (dialogs, popovers, toasts) and controls (a switch's thumb) keep
  what they do themselves.
- Beside the tucked card's slide, a size or a position never animates, and nothing pulses or
  slides to draw the eye (a field's caret blinks on its own); a state that needs the person says
  so in ink and a word, never in motion: a tucked card does not slide open by itself.

### Nothing to show

One meta line, in sentence case, without a period: at the list's place, left-aligned, inside a
list; centred (`hint`) when it fills the whole panel. *No <things> yet* when there have never been
any; *No <things> match* when a search or a filter hides them all; *Nothing <state>* for a status
box (*Nothing needs you*). An empty state that blocks every action in the window (no agent set up)
may carry, under its line, what fixes it: commands in the code face and one secondary button to
the place it is fixed, all centred in one column.

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
the UI crate's `toast`, `crate::toast` in the app (which gives a failure its form and trims
the period), its kind chosen by what it
means:

- `error`: an action failed;
- `warning`: done only in part, or something now needs the person;
- `success`: a long or background action finished, which the screen does not already show;
- `info`: news that asks for nothing.

A toast is one sentence without a trailing period.

### Confirming a removal

A removal that cannot be undone asks first: data taken off disk, a process ended, unsaved work
thrown away. It asks through `Confirm` (`crates/ui/src/confirm.rs`), the same question
in the app and in a feature crate:

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
  `Kbd` at its end. A label's text never spells a key; a pinned card's answers carry theirs beside
  their words, in the button's own ink, since those keys reach the card from the composer.
- **A tooltip stands for what is under the pointer, one at a time.** A key, a press, or a dialog
  or sheet opening or closing puts it away; the control just pressed shows none again until the
  pointer moves from where it was pressed, so none stands over the menu it opened. What a row's
  hover actions lie over shows no tooltip while the pointer is on them.
- **Keyboard focus shows the ring**, only when the keyboard moved it there. A field shows focus by
  its edge alone, turned the `ring` ink: edgeless at rest, it gains one while focused.
- A dialog, a popup or a floating panel opens with focus on its first field, or on its primary
  action when it has none. A panel floating beside the list it was picked from (the Issues
  page's issue) leaves focus on the list, so ↑ and ↓ keep stepping it. Each closes on Esc, asking first when a form in it holds unsaved
  changes. On closing, focus goes back to what opened it, or to the conversation when that is gone.
- Esc closes the innermost layer only. A page is a place, not a layer: Esc leaves it where it is.
- A key with no modifier that a region handles never fires while a field inside it holds the
  caret. A field inside a region hands the keys back to that region on Esc.
- **A card pinned over the composer** (a permission, a question) takes the keyboard once, as it is
  pinned, and only from nowhere, an empty composer (no words, no staged files) or the card answered
  before it; never from a prompt being put together. A question's choices answer to their number on the card and to Alt with the
  number anywhere in the conversation, and each row's `Kbd` names the Alt one, which works
  wherever the caret is. The footer's answers answer to Alt anywhere in the conversation too: a permission's
  Alt+Y once, Alt+A always, Alt+N deny (refusing once where the agent offers both refusals,
  and only that button carries the key); a question's Alt+Enter on, Alt+S skip.
  A question's forward answer (*Next*, *Submit* on the last) refuses until the question showing
  has an answer. On a multi-select none is an answer: moving on with nothing picked sends an empty
  list, where *Skip* sends no answer for it. A card asking one pick-one question answers on the
  pick, so its footer holds *Skip* alone, and *Submit* beside it once an answer is typed; every
  question card carries *Skip*, so passing on one never takes *Stop*.

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
