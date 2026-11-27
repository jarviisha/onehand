# UI patterns

The patterns every screen repeats, in the app and its feature crates: how a screen is put
together, how it holds under strain, and how it behaves while it waits, fails, refuses or
removes something. `DESIGN.md` owns the window's frame and the visual rules these patterns are
drawn with (its rules `T` and `C`, and how a rule reads), `docs/screens.md` what each region
holds, and a feature's own document its screens' detail. Code never cites this file.

## Layout

### Choose the container

#### K-01 The container follows the content's role

| Content | Container | Separation |
|---|---|---|
| Related settings | Box | The settings group: one edge, its heading inside at the top |
| A group inside a form | Plain section | Heading, shared text axis and section gap |
| A group on a page: one thing's facts, a list, a status, what a thing is set to | Box | One outer edge in a control's edge, no fill; no nested boxes for its fields |
| Repeated items to scan or pick | List or table | Stable columns and row states; table hairlines |
| Repeated items opened one at a time, each with actions of its own | Grid of boxes | A gap; a box's edge raised on hover, never a fill |
| Code, output or a diff | Well | Quiet fill; overflow stays inside it |
| One selected item read in full | Detail | Title, metadata, content; no extra enclosing card |
| A temporary choice or confirmation | Menu, popover or dialog | Floating surface and lift |

- When: content is given a container, in an app page or a feature crate's view alike.
- Then: choose by the content's role before its decoration, in the least decorated container
  that says each relationship. Never add a card, a badge, an icon or a fill to look finished.
- Unless: the transcript and the input surface keep the treatments `DESIGN.md` and
  `docs/screens.md` give them.
- Checked by: review.

#### K-02 On a page each group stands in a box, and nothing inside it is boxed again

- When: a page holds several groups.
- Then: each stands in a box, so what belongs together reads as one object rather than as
  text loose on the page; what groups inside a box (a form's fields, a box's rows) takes the
  plain treatments.
- Checked by: picture.

### Hierarchy

#### K-03 Each part of a row, a box or a strip takes its ink

The neutral inks of T-04, on a neutral surface:

| Where | Full ink | Middle ink | Muted ink |
|---|---|---|---|
| A row in content: a page, a Workbench list, a menu | its name | the detail on its line: a folder, a number, a kind | its meta, trailing or on a line under it; the label over a run of rows |
| Navigation: the rail, a section nav | the current location, in its fill's paired ink; every other row at rest in the sidebar's ink | none | a row's second line |
| A box, a detail | its title, its content | none | its count, its line of facts |
| A page's head | its name, semibold at the chrome size | none | its line under it, at the meta size: what the page is for, then its facts |
| A strip | what it names | none | a count beside the name |

- Checked by: picture.

#### K-04 Hover changes a fill, never an ink or a layout

- When: the pointer is over a list row.
- Then: its fill changes (T-22); its inks and its layout do not. Weight follows the role
  (T-02), never a row's urgency: what needs the person says so in its state's ink and a word.
- Unless: a line row answers by its ink instead (K-23).
- Checked by: picture.

### Spacing and density

#### K-05 A gap follows the relationship

| Relationship | Default |
|---|---|
| Compact metadata, or a tightly coupled mark and label | `gap_1` |
| Icon and label in a row the app draws; adjacent controls; parts within a box | `gap_2` |
| Row inset | `px_2` |
| Strip inset; box padding | `px_3`, inside the Workbench `tokens::ROW_INSET`; `p_3` |
| A box whose body scrolls inside it | `p_4`, a step more so the scrollbar's thumb has room beside the text |
| Form fields | `gap_4` |
| Page sections; page outer padding | `gap_6`; `px_8` `py_6` |
| A list page's inset | `px_6` |
| A tree row's inset; its fill off the dock's sides; its pitch, fill and room around the fill; its step per level | `tokens::ROW_INSET`; `tokens::ROW_GUTTER`; `tokens::TREE_ROW_H`, `TREE_ROW_FILL`, `TREE_ROW_ABOVE`/`BELOW`; `tokens::TREE_INDENT` |
| A live grid (a terminal) | `px_3` `py_2` |
| Inside the Workbench: the parts of one group; groups | `gap_3`; `gap_6` |
| Entries of a chrome list (an activity log, a list of skills) | `gap_2` at least |

- When: two parts are spaced.
- Then: the named pattern's default before another legal step (T-09). Smaller steps are for a
  component's internals; the largest is for a deliberate major grouping, not a second
  page-padding convention.
- Checked by: tests `tree_row_fill_is_odd_and_rows_stack_at_the_pitch`,
  `words_stand_inside_the_fill` for the tree's; review.

#### K-06 Equal relationships keep equal gaps, and parts share their axes

- When: a view is laid out.
- Then: equal relationships keep equal gaps across screens; space between groups exceeds space
  within them. Titles, row labels and field labels sit on consistent text axes, and text
  below a title shares its axis. Marks down one column (icons, state dots) share one centre
  line, each in a slot of one width. A number or a time in a column takes a fixed-width slot,
  so a longer value does not push its neighbours.
- Checked by: picture.

#### K-07 Density follows the task

- When: text is sized or a panel is crowded.
- Then: chrome and lists are compact, forms leave room for labels, and prose keeps its reading
  size and line height (`LEADING`). A line that is itself content (an activity entry, a fact's
  value, a row's description) takes `CHROME_TEXT`; `.text_xs()` is for meta beside something
  (a time, a count, a hint, the label over a run of rows). A crowded panel is never made to
  fit by shrinking its font, icons or hit targets: it uses overflow, wrapping and the
  list-and-detail pattern.
- Unless: a list page's rows are a density of their own (K-16).
- Checked by: picture.

#### K-08 A row is `ROW_H`, and grows for a second line

- When: a row is drawn.
- Then: a single-line row is `ROW_H` tall; a row with a second line takes `ROW_H` as its
  minimum and grows to fit its lines at shared padding and leading, never clipping to that
  height. Multi-line variants align their primary labels and second lines across peers.
- Checked by: tests `a_project_row_stays_inside_the_rail`,
  `every_history_row_keeps_its_height_and_its_trailing_slot`.

### Control sizes

#### C-16 Each role takes its component and its size

`.claude/skills/ui/tokens.md` holds what each size measures in each component.

| Role | Component | Size |
|---|---|---|
| A page's head and the row under it: its actions, its search, its filters' choices and pickers, a split action's both halves | Button, Input | `.short()`, so the head's controls never outweigh its name |
| An icon-only button in a page's head | Button | `.short_icon()`, square at `CONTROL_H` |
| An icon-only button in a page's form | Button | `.tall_icon()`, square at `FIELD_H` |
| A page's form: its fields, its pickers, its switch and its buttons; a form's *Save* and *Cancel* standing in its head take the head's size | Button, Input | `.tall()` |
| A form in a dialog at `DIALOG_FORM`: its fields, its pickers and its footer | Button, Input | `.tall()` |
| A dialog that asks one thing: its fields and footer; a dock's filter bar, its search, choices and pickers | Button, Input | `.short()` |
| Every other button with words: a strip's, a box's, a row's, a secondary action inside dense content, a tab, a fold toggle, a pinned card's answers | Button | `.short()` |
| Every other icon-only button: a strip's, a row's, a copy control, a split action's caret beside `.short()`, a send button | Button | `.short_icon()`, square at `CONTROL_H` |
| A field in a strip, the rail or the title bar | Input | `.short()`, as tall as the buttons beside it |
| An icon in a button | Icon | the button's own, as the library sizes it for the button's size |
| An icon leading a label the app draws: a row, a menu item | Icon | `.small()` |
| A state's mark leading a row's label; a mark inside a line of meta text | Icon | `.xsmall()`, a step under the label |
| A badge: a tag standing on its own | Tag | micro words on a slim inset, edged in its own hue (T-14) |
| A label inside a line of meta text | text | its kind's ink (`label_tint`), the one part of the line that truncates; the row's quieter ink on the selected fill |
| A checkbox, a switch | Checkbox, Switch | `.small()` beside chrome text, at the library's own geometry |
| A single-line row; a row with a second line | drawn by the app | `ROW_H`; at least `ROW_H` (K-08) |
| A strip along the top of a region | `strip` | `BAR_H` |
| A mode's own bar under a dock's strip | `mode_strip` | `MODE_BAR_H` |

- When: a control is sized (C-11).
- Then: the size its role takes here. `.tall()` is the ceiling: no control stands taller than
  `FIELD_H`; a page gives a field room by its size, never by a larger control.
- Checked by: test `the_sizes_are_the_listed_ones` for the heights; review for the role.

#### C-17 One size, one measure, corrected in its wrapper

- When: a component's size comes out wrong.
- Then: it is corrected in its shared wrapper (`Short`, `Tall`), never at the call site. A
  button's `.short()` is the library's extra small held at `CONTROL_H` and its `.tall()` the
  library's small held at `FIELD_H`, its words at `CHROME_TEXT`; a field takes the library's
  small at both heights. An icon-only button beside `.short()` is `.short_icon()`: the
  library's small icon, held square at `CONTROL_H`.
- Checked by: review.

#### C-18 One row of actions, one height

- When: controls stand in one row of actions (a button, both halves of a split action, a
  field, a picker, a choice, an icon alone).
- Then: every one takes the same size, so their tops and bottoms meet on one line. The row
  takes the size its leading role takes, and a control whose role elsewhere takes another
  size takes the row's here.
- Unless: the components that keep their own geometry (C-20) stand apart, centred.
- Checked by: tests `the_title_bar_controls_share_one_height`,
  `a_page_heads_actions_share_one_height_and_one_line`.

#### C-19 The hit area is not the icon

- When: an icon-only control is drawn.
- Then: it keeps its peers' hit area, its box no larger than a worded peer's, and is named
  with `.named(..)`, its tooltip and its accessible name at once. Nothing is enlarged to fill
  its box, and the hit area is never shrunk to the SVG.
- Checked by: test `the_title_bar_controls_share_one_height`; review.

#### C-20 Some components keep their own geometry

- When: a badge, a checkbox, a switch or a key cap stands in a row of controls.
- Then: it is centred in the row (`items_center`), never stretched to the height of the
  controls beside it. Spacing inside a library control is the library's; the gap between two
  controls is `gap_2`.
- Checked by: picture.

#### C-21 Under zoom, parts line up by centring, never by matching a number

- When: a part the app draws in rems stands beside a library part sized in pixels (a switch's
  track and thumb, a field's inner padding, the library's table rows).
- Then: line them up by centring them; a part equal to one of those at 100% parts from it at
  another zoom.
- Checked by: picture, at a zoom other than 100%.

### Window

#### L-01 The window is never smaller than 720 by 480

- When: the window is sized.
- Then: it keeps the rail's minimum beside the conversation's (`WINDOW_MIN`). Its regions are
  `DESIGN.md`'s.
- Checked by: test `the_sizes_are_the_listed_ones`.

### Strips

#### K-09 A strip names at its start and acts at its end

- When: a strip stands along the top of a region.
- Then: `strip`: `BAR_H` tall, padded `px_3`, its parts `gap_2` apart, at the chrome size. What
  it names stands at the start in full ink; its controls stand at the end as ghost buttons at
  their control sizes (`strip_control` for an icon alone). Filters belong below the region's
  header, with the content they filter. Every bar inside a region's box draws a line where it
  ends.
- Unless: a control that shows a region the strip's own panel hid may lead (the way back to a
  list, a list's toggle).
- Checked by: picture.

### Cuts

#### L-02 What runs past its room is cut, and the cut says so

- When: content is longer than the room it is given.
- Then: a block of reading content that is folded or bounded, a name too long for its
  navigation row, and content that runs under a pinned surface are cut by a fade: a gradient
  from the clear surface to the solid one over its end. Text inside a row that has no fade
  truncates on an ellipsis. Nothing else is drawn see-through.
- Checked by: tests `a_long_workspace_name_stays_inside_its_slot`,
  `a_long_project_name_truncates_and_its_tail_keeps_its_width` for the bound; picture for the
  fade or the ellipsis itself.

### Under strain

These rules hold a view together when its content is long, its container narrow, its zoom
large or its list overfull. Each came from a fault seen in the app.

#### L-03 A control's words stay on one line

- When: a control holds words.
- Then: they never wrap; a control that cannot hold them truncates them or gives way as L-07 says.
- Checked by: test `a_page_heads_actions_share_one_height_and_one_line`; picture, since only
  the real font shows where a line would break.

#### L-04 A long name never moves its neighbours

- When: a name of unknown length stands in a row or a bar beside other parts.
- Then: the name takes the room left (`min_w_0`) and is cut (L-02); the parts beside it do not
  shrink, grow or move. A name that is cut is said whole in its tooltip.
- Unless: the name is the only part on its line.
- Checked by: tests `a_long_workspace_name_leaves_the_toggles_in_place`,
  `a_long_page_title_leaves_its_actions_in_place`,
  `a_long_project_name_truncates_and_its_tail_keeps_its_width`,
  `a_long_task_title_stays_inside_its_column`,
  `the_workspace_hint_says_the_name_whole_before_what_a_press_does`.

#### L-05 A control with a fixed place keeps it

- When: a control's place is part of what a person learns (a bar's toggles, a centred search).
- Then: its bounds are the same whatever stands beside it, at every width the window allows,
  and it stays inside the window.
- Checked by: tests `a_long_workspace_name_leaves_the_toggles_in_place`,
  `the_palette_stays_at_the_centre_whatever_the_name`.

#### L-06 A row's trailing slot keeps its width

- When: a row holds a name and trailing facts or actions.
- Then: the trailing slot keeps its width while the name truncates, and showing a row's hover
  actions moves nothing beside them: where a row has trailing facts its hover actions are laid
  over them, hiding them while they show; a row without them keeps its actions in a held slot.
  Where the row is still too narrow, the facts give way before the actions leave the row.
- Checked by: tests `one_history_row_keeps_its_height_and_its_trailing_slot`,
  `every_history_row_keeps_its_height_and_its_trailing_slot`,
  `a_long_project_name_truncates_and_its_tail_keeps_its_width`.

#### L-07 A row of actions stays on one line

- When: a row of actions is short of room.
- Then: its words shorten, or the least needed actions go behind a menu; the row does not
  break onto a second line.
- Unless: where a cut label would hide what a press grants (a pinned card's answers), the
  answers wrap onto a second line whole before a label is cut.
- Checked by: tests `a_cards_answers_as_worded_stand_on_one_line`,
  `a_question_cards_answers_as_worded_stand_on_one_line`; for the exception,
  `answers_that_do_not_fit_wrap_whole`, `answers_worded_long_wrap_whole`.

#### L-08 The last item stands off its container's end

- When: a list or a column ends inside a box or a region.
- Then: its last item stands off the end by the container's own inset, as its first stands off
  the start.
- Checked by: test `the_last_row_stands_off_the_end_of_its_box`.

#### L-09 What scrolls keeps a lane beside its scrollbar, and its rows their height

- When: content scrolls.
- Then: a lane on the scrolling side stays clear, so the thumb never lies over words, a
  control or a row's fill: `tokens::SCROLLBAR_W`, or the box's own inset (`p_3` at least)
  where the scroll's frame reaches back through it to the box's edge. Rows keep their height
  and are scrolled, never squeezed: a scrolling box is a plain box with the column inside it,
  never itself a flex column.
- Checked by: guard `a_scrolling_box_is_not_a_flex_column`; tests
  `rows_in_a_scrolling_box_keep_a_lane_beside_its_scrollbar`,
  `every_history_row_keeps_its_height_and_its_trailing_slot`.

#### L-10 Content wider than its container scrolls or reflows, never clips silently

- When: columns or content are wider than the container.
- Then: the container scrolls sideways with every column at its own width, or the content
  reflows, as its pattern says. Nothing is cut off with no way to reach it.
- Checked by: test `columns_past_the_width_scroll_sideways_and_keep_their_width`; picture at
  the narrowest window.

#### L-11 A popup never covers the field it serves

- When: a list or a menu opens for a field or a control.
- Then: it opens beside it (under a field, just above a control at the window's foot), at a
  width of its own, and leaves the field in view.
- Checked by: test `the_palettes_list_opens_below_its_field`; picture.

#### L-12 A split or a stack reads its container, never the window

- When: a view decides between side by side and one at a time, or how many boxes a row holds.
- Then: it measures the container it is given: a narrow dock in a wide window still stacks.
- Checked by: test `a_list_and_its_detail_share_a_container_only_when_both_fit`.

### Pages

#### K-10 Every page names itself in its head

- When: a page takes the agent pane.
- Then: `page_head`, in the body: its name semibold at the chrome size, one line under it at
  the meta size in the muted ink (what the page is for, then its facts where it has them), its
  actions at the end, and a hairline under the whole that says where the page's own content
  starts. The pane's header above it carries chrome alone. Weight, not size, sets the name
  apart. A box's heading identifies its own content.
- Checked by: tests `a_long_page_title_leaves_its_actions_in_place`,
  `a_page_head_with_no_actions_gives_the_title_the_row`.

#### K-11 A page is a column page or a list page, by what it holds

- When: a page is laid out.
- Then: a **column page** holds fields, boxes or one thing read in full; a **list page** holds
  one long list to scan, filter and pick from. The kind decides its width and its density,
  never its head.
- Checked by: review.

### The top of a page

#### K-12 A page's top stays while the rest scrolls under it

- When: a page that is reached from the rail, or holds settings, is drawn.
- Then: `page_top`: the head, then, where the page has them, a row of its search and its
  filters, inset `px_8` and `pt_4`, its parts `gap_3` apart, in the same column as the body
  under it. The head's hairline parts it from what the page holds; the body's own top inset
  stands what scrolls clear of the top. A table's column names head its rows and scroll with
  them.
- Unless: a page reached from one of these (one thing's own page, a form) scrolls as one
  column under its trail back.
- Checked by: picture.

#### K-13 A page's head holds its actions in one order

- When: a page's head holds actions.
- Then: all `.short()`, at one height; icon-only ghost buttons stand before the creating
  action. The creating action is the view's one primary (T-25), stepping down to `secondary`
  while a form it opened, or an item floating over the list, is open and holds it.
- Unless: a page that only sets values holds no primary, every button on it secondary, and
  stands its peer groups each in a box, its heading inside.
- Checked by: test `a_page_heads_actions_share_one_height_and_one_line`; review.

#### K-14 The row under a page's head is its search, then its filters

- When: a page has a search or filters.
- Then: the search first (`page_search`: `.short()`, as wide as its cap and narrower only when
  the row runs short), then the filter bar's choices and filters (K-18), kept to the row's
  end; the row wraps when narrow. A page with filters and no search keeps them at the row's
  end all the same.
- Checked by: picture at the narrowest window.

### Column pages

#### K-15 A column page takes one of three widths

- When: a column page is laid out.
- Then: the body scrolls as one column centred in the pane, padded `px_8` at its sides and
  `py_6` at its ends, its sections `gap_6` apart, as wide as `PAGE_NARROW` (one column of
  fields or rows), `PAGE_PROSE` (an editor, or one thing read in full) or `PAGE_WIDE` (boxes
  side by side, or a list in a box). The pane header is inset `px_8`, as the body is. A page
  holding sections of its own puts their nav column at its start. The same kind of page takes
  the same width.
- Checked by: test `the_sizes_are_the_listed_ones` for the widths; review.

### List pages

#### K-16 A list page takes the pane's whole width

- When: a list page is laid out.
- Then: its top is every page's (K-12). The list is inset `px_6`, its rows' fills on that edge
  and their words on the top's `px_8`. Its rows are on one line at `.text_xs()` with no lines
  between them: the name and the column read down the list in `font_medium`, the rest at the
  regular weight. Trailing slots drop, least needed first, where the pane is too narrow to
  leave the name room. Rows keep their place while a person reads. The foot, parted from the
  list by a `faint_rule` line the pane's whole width, says *Showing N of M <things>*.
- Checked by: picture.

#### K-17 A list page's keys move the selection itself

- When: a list page holds the keyboard.
- Then: the arrow keys move the selection, so the selected fill marks where the keys are and
  no ring is drawn (T-24). Nothing is open on arrival. A picked item floats over the list's
  right side, its width dragged for the session, leaving focus on the list so the arrows keep
  stepping it; its close control and Esc close it. The filters, the search, the selection, the
  open item and the scroll are kept while the page is left and shown again.
- Checked by: review.

### Filter bars

#### K-18 Filters stand in one order

- When: something filters its content.
- Then: on a page the filters stand in the row under its head, after the search (K-14); in a
  dock, which is dense chrome with no head, the bar leads with its search. All at `.short()`,
  in this order:
  - where the content splits into a few states that each count, those first, each a `choice`,
    the shown one filled, its count a `badge` after its word in the hue of that state (neutral
    at zero);
  - the filters, kept to the row's end after the choices, each a `picker` with its caret, its
    label naming what it is set to: a ghost in the muted ink while it is where it starts, on
    the secondary fill once it narrows the content;
  - after them, where more than one thing narrows the content and only while one is set,
    *Clear filters*, which puts the search and every filter back (a choice is not a filter);
    last in its group, so nothing moves as it comes and goes.
  A page's row wraps under the choices when narrow; a dock's bar never wraps, its search
  shrinks instead. The content's foot says how many it shows of how many match, and a
  filter's choice and the search are kept while the view is left and shown again.
- Checked by: picture.

### Boxes

#### K-19 A box groups one thing on a page

- When: a group stands on a page (K-02).
- Then: `card_box`: edged in a control's edge (`input`) at `radius_lg`, on no fill, padded
  `p_3`, its parts `gap_2` apart. Its title row (`card_title`) holds the title in semibold, a
  muted count, and at most one control at the end, at a fixed height so boxes side by side
  line up. Boxes sit side by side while the measured column holds two, and stack when it does
  not (L-12).
- Checked by: test `a_box_stands_apart_from_the_window`; picture.

#### K-20 A box that opens what it shows is pressed on its body

- When: a box in a grid opens the thing it shows.
- Then: it takes the pointer, raises its edge on hover to the muted ink rather than filling,
  and rings on focus, Enter opening it. Its actions stand in its foot, and a press on them
  stays theirs. Cards stand in rows of as many as the measured column holds at their least
  width, up to four, a short last row keeping the column's width.
- Checked by: picture.

### Lists

#### K-21 A row is drawn by the row builder, and its fills stand off the edges

- When: a list row is drawn.
- Then: `row`: `ROW_H` tall (K-08), padded `px_2` with `gap_2`, at `radius`, hovered and chosen
  as T-22 and T-23 say. A row standing on a region's bare surface keeps its fills off that
  region's edges: a tree in a dock by `tokens::ROW_GUTTER`, any other list in a dock by its
  own padding, a list page by its inset.
- Unless: rows inside a box are the box's parts: their fills run to its edge, as the hairlines
  between them do (K-25).
- Checked by: test `a_project_row_stays_inside_the_rail`; picture.

#### K-22 Two fills never touch

- When: a row in a list can be selected, or choices stand in a pick-one track.
- Then: a hovered row beside the selected one stays parted from it: a tree's fills by
  `tokens::ROW_GUTTER` inside the row's pitch, any other list by a gap between its rows, a
  track's choices held in from its rim and parted `gap_0p5`.
- Unless: a list in which nothing is ever selected may run its hover fills together.
- Checked by: test `tree_row_fill_is_odd_and_rows_stack_at_the_pitch`; picture.

#### K-23 A row inside reading content is a line, not a list row

- When: a row stands inside reading content (a fold line, an activity row).
- Then: it is as tall as its text, takes no focus, and answers the pointer by raising its ink
  rather than with a fill, which would break the reading column into tiles. Its actions are
  the block's (one copy control, one show-more control).
- Checked by: picture.

#### K-24 A row's actions show when the row is the one in hand

- When: a row has actions of its own.
- Then: ghost buttons at their control sizes, shown on hover, on the selected row, on keyboard
  focus and while their action runs; a focus a click leaves does not show them. They sit
  behind a `⋯` (`row_more`), shown on hover, on the keyboard's row and always on the current
  one, and the same items open on a right-click (`row_menu_items`), one list for both, a
  removal last in the danger ink; a row's own press is offered there by name too. A row whose
  actions hide at rest is a tab stop.
- Unless: a row waiting on the person keeps its actions drawn as buttons beside its `⋯`, since
  they are its point; a row the keys only pass through, whose content is open on screen
  already holding those actions, lays none.
- Checked by: picture, with a hover and with the keyboard on the row.

#### K-25 Rows in a box are parted by hairlines

- When: rows stand inside a box.
- Then: each is parted from the next by a hairline (`ruled`), drawn on a wrapper of the row's
  own so its focus ring moves nothing; a table in a box is such rows under a head row in meta
  text on the page's own surface, its rows square at the row inset. A row's trailing facts
  reach the box's edge.
- Unless: a list page's table keeps the list page's rows, with no lines between them (K-16).
- Checked by: picture.

#### K-26 Every list has a named cap, and says why it is short

- When: a list is drawn.
- Then: it stops at a named cap. Its last line, in meta text, says why it is short: a list
  the cap cut says how many more there are (*N more*); a list a search or a filter narrows
  says how many it shows of how many there are (*Showing N of M*).
- Checked by: tests `past_the_cap_the_page_says_how_many_more`,
  `a_group_past_the_cap_says_how_many_more_until_shown_whole`; review for a new list.

### List and detail

#### K-27 A list stands beside its detail only while the container holds both

- When: a list and the detail of the item picked share a view.
- Then: `list_detail` sets them side by side while the measured container holds both widths;
  else it shows one at a time, the detail under a `back_link` naming the list. Picking shows
  the detail; going back keeps the list's place. A detail read as prose is a column up to
  `PAGE_PROSE`, centred with equal gutters, so hiding the list widens it rather than leaving
  the freed width empty; its scroll spans the detail, the scrollbar on the detail's edge.
- Checked by: tests `a_list_and_its_detail_share_a_container_only_when_both_fit`,
  `the_detail_mirrors_the_lists_range_across_the_container`,
  `a_drag_never_takes_the_detail_under_its_minimum`.

### Forms

#### K-28 A form's input is a `field`

- When: a form holds an input.
- Then: a `field`: its title in chrome text, an optional hint under it in meta text, `gap_2` to
  the control. Fields stand `gap_4` apart. The input is built with `text_input`, at a
  control's corner and the size of the place it stands in (C-16); its buttons, pickers and
  switch take the same.
- Checked by: review.

#### K-29 A form says what blocks it, beside its submit

- When: a form cannot be sent.
- Then: its submit, the form's one primary action, refuses, its tooltip saying why (C-05).
  The problems stand above the submit in the danger ink; in a page's form, in one box on the
  danger tint's fill led by the warning mark, each a sentence. A new form nobody has changed
  yet shows none until the first change; one opened on something (a file, a copy, an import)
  says them at once.
- Unless: a page form whose submit stands in the page's head sets them in a bar along the
  page's foot, the page's whole width, on its kind's tint led by its mark: the problems on the
  danger tint with the warning mark, a standing note on the accent's tint with the info mark.
- Checked by: review.

### Dialogs

#### K-30 A dialog is the library's, titled and footed by the shared builders

- When: a dialog holds fields.
- Then: the library's `Dialog` at its own width over a backdrop that dims the window, titled
  through `title_row` (`DIALOG_TITLE`), its close mark muted and named in its tooltip. Its
  fields are `field`s, `gap_4` apart; what blocks it stands above the footer in the danger
  ink. Its footer (`footer_row`) sits on the right: the dismissing button first, then the one
  primary action or the danger word, at its fields' size. A third action stands at the
  footer's start, apart from the two that close it, on the secondary fill, as it starts its
  line (C-06). One that asks one thing stands at its own width with `.short()` controls; one
  that holds a form stands at `DIALOG_FORM` with `.tall()` ones.
- Unless: a question without fields (a retry, a removal) is an alert dialog (S-13), never a
  dialog of its own.
- Checked by: review.

#### K-31 A dialog that starts a run says what the run is before its fields

- When: a dialog starts a run.
- Then: it stands at `DIALOG_FORM` and parts its sections (what runs, where it works, what
  stands in the way, what the person adds) with a hairline (`sections`). The branch it cuts
  stands in a well beside its branch mark. Each thing found before the start leads with its
  state's mark: a warning in the danger ink when it blocks the start, an information mark in
  meta ink when it only tells; an action beside one takes the secondary fill (C-07). A start
  that fails is said once, in an error toast.
- Checked by: review.

### Details

#### K-32 A detail shows one thing in full

- When: one thing is read in full.
- Then: a trail naming where it sits, with its actions beside it (one primary, one other, `⋯`);
  its title in `text_lg` semibold; one line of facts in meta text; what it waits on; then its
  tabs. No enclosing card (K-01).
- Unless: where another control of the view already holds its primary, the detail's next
  action is secondary and its other a ghost.
- Checked by: review.

## Behaviour

### Waiting

#### S-01 A state change keeps the layout around it

- When: a view changes state (waiting, loaded, empty, failed).
- Then: titles, content axes and action slots stay where they were. Feedback belongs to the
  affected content or action; it does not restyle the whole page. A state is said by a word
  or a recognisable mark as well as colour, and a quieter state is still readable.
- Checked by: picture of both states.

#### S-02 Content on its way is one meta line

- When: a read runs before content can show.
- Then: one meta line where the content will appear, naming the work and ending on an
  ellipsis: *Reading the workflows…*. A panel never sits blank while a read runs.
- Checked by: review.

#### S-03 An action in flight shows on the button that started it

- When: a press starts work that is still running.
- Then: `.loading(true)` on that button, which refuses a second press; its label stays. A
  `Spinner` stands only inside a line or a control already on screen.
- Checked by: review.

### Immediate feedback

#### S-04 A change a person can take back shows in the frame it is made

- When: a person renames, pins, folds, reorders, filters or picks.
- Then: the screen shows it at once; saving it runs after, off the UI thread. A rename and a
  shortcut show at once too.
- Unless: a form that writes a file waits on its *Save*.
- Checked by: review.

#### S-05 Saves of one thing run one at a time, in the order made

- When: several changes to one thing are saved.
- Then: a writer per file runs them in the order sent, so an older save never lands over a
  newer one; a closing window's last save and the saves still queued at quit go through the
  same writer, waited for up to a bound. The rule lives in core (`onehand_core::saving`:
  `Saving` for one value, `Saves` for several things in one file); the app keeps the spawn and
  the toast.
- Checked by: the tests of `onehand_core::saving`.

#### S-06 A save that fails puts back the one change, and says so once

- When: a save fails and no later save carries that thing.
- Then: the screen goes back to the last value known saved, not the one before the change,
  and says so once, as an `error` toast (*Renaming the session failed: <why>*). What goes back
  is the one change, not the file: each thing a write carried goes back alone to its own last
  saved value. A pick the agent applies (a mode, a model) is the same: the agent refusing it
  is its failed save.
- Unless: a later save that carries the thing supersedes the failure. A save that is an act
  rather than a value (closing an issue, adding a comment) is superseded by nothing: each
  failure is said once, and the screen reloads what the file holds. A dragged size and a
  dock's open state are saved but never put back, since the layout would move under the
  pointer; a failed save still says so once.
- Checked by: the tests of `onehand_core::saving`; review.

#### S-07 Work that leaves the machine is never drawn as done before it is

- When: an action reaches git, the network or an agent, or takes its time.
- Then: it waits on the button that started it (S-03) until it has answered.
- Checked by: review.

### Motion

#### S-08 Nothing moves for show

- When: a state changes.
- Then: a hover, a selection, a fold or a new state lands in the frame it happens in. A size
  or a position never animates, and nothing pulses or slides to call for the person: a state
  that needs them says so in ink and a word.
- Unless: a `Spinner` on work still running (S-03); the band of brighter ink that crosses the
  words of what runs in the transcript, which marks work going on by itself and asks for
  nothing; what the library's own overlays and controls do themselves; a field's caret.
- Checked by: review.

### Nothing to show

#### S-09 An empty view says so in one meta line

- When: a list, a box or a panel has nothing to show.
- Then: one meta line, in sentence case, without a period: at the list's place and
  left-aligned inside a list; centred (`hint`) when it fills the whole panel. *No <things>
  yet* when there have never been any; *No <things> match* when a search or a filter hides
  them all; *Nothing <state>* for a status box.
- Unless: an empty state that blocks every action in the window may carry, under its line,
  what fixes it: commands in the code face and one secondary button to the place it is fixed,
  all centred in one column.
- Checked by: tests `a_project_page_with_no_conversations_draws_no_rows`,
  `a_flat_list_names_no_projects_and_says_when_nothing_matches`; review for the words.

### Failing and refusing

#### S-10 A failure of something a person started reaches the screen, once

- When: an action a person started fails.
- Then: an `error` toast: *<What> failed: <why>*. One failure is said once: a toast or a
  line, never both, and a log line alone is not enough.
- Unless: a condition that stands until something answers it (a save refused because the
  file changed on disk, an editor that would not start) is a `status_line` under the view, in
  the ink of its state, cleared by whatever answers it.
- Checked by: review.

#### S-11 A press that cannot act yet refuses, and says nothing else

- When: a control cannot act now.
- Then: it refuses with its reason as its tooltip (C-05). No toast, no line.
- Checked by: lint (C-05); review.

### Toasts

#### S-12 A toast's kind is what it means

- When: transient news is shown.
- Then: `window.push_notification` with a notification built by the shared `toast` builder,
  one sentence without a trailing period: `error` when an action failed; `warning` when it was
  done only in part, or something now needs the person; `success` when a long or background
  action finished that the screen does not already show; `info` for news that asks for nothing.
- Checked by: test `a_toast_is_one_sentence_without_its_period`; review for the kind.

### Confirming a removal

#### S-13 A removal that cannot be undone asks first

- When: an action takes data off disk, ends a process or throws unsaved work away.
- Then: it asks through `Confirm` (`crates/ui/src/confirm.rs`), the same question in the app
  and in a feature crate: the title asks the question naming the thing (*Remove <name> from
  the workspace?*); the description says what goes and what stays; the buttons read *Keep*,
  then the danger word, the same verb as the control that asked. A lesser way ahead stands
  between them, secondary, and then Enter answers the danger word; a question with only that
  word keeps Enter closing it. The control starting a removal is a word in the danger tint
  (C-09).
- Unless: what can be undone or resumed goes ahead without asking (stopping what offers
  *Resume*, taking an item off a queue, unbinding a key, closing what can be opened again),
  except where it also ends something that cannot be.
- Checked by: review.

### Keys and focus

Keyboard focus and its ring are T-24.

#### W-01 Every key that does something is a command

- When: a key acts.
- Then: it is a command in the keymap (`crates/app/src/keymap.rs`), so it can be rebound;
  `on_key_down` is for typing only.
- Checked by: test `every_menu_item_is_a_command`; review.

#### W-02 A control bound to a command shows its key

- When: a control dispatches a command that has a key.
- Then: its tooltip shows the key (a `Kbd`); a field shows it as a `Kbd` at its end. A
  label's text never spells a key.
- Unless: answers whose keys reach them from elsewhere carry theirs beside their words, in
  the button's own ink.
- Checked by: picture, with a hover.

#### W-03 A tooltip stands for what is under the pointer, one at a time

- When: a tooltip could show.
- Then: a key, a press, or a dialog or sheet opening or closing puts it away; the control
  just pressed shows none again until the pointer moves from where it was pressed, so none
  stands over the menu it opened. What a row's hover actions lie over shows no tooltip while
  the pointer is on them.
- Checked by: picture, with a hover after a press.

#### W-04 A layer opens with focus inside it, and gives it back

- When: a dialog, a popup or a floating panel opens or closes.
- Then: it opens with focus on its first field, or on its primary action when it has none.
  Esc closes the innermost layer only, asking first when a form in it holds unsaved changes;
  on closing, focus goes back to what opened it, or to the conversation when that is gone.
  Focus is never left on what is no longer drawn.
- Unless: a panel floating beside the list it was picked from leaves focus on the list
  (K-17). A page is a place, not a layer: Esc leaves it where it is.
- Checked by: review.

#### W-05 Enter in a dialog's one-line field presses its primary action

- When: Enter is pressed in a dialog.
- Then: in a one-line field it presses the primary action, and does nothing while that
  refuses or works; the dialog stays when what was typed is refused, saying why.
- Unless: a field of several lines keeps Enter for a new line, and Enter on a focused button
  presses nothing.
- Checked by: review.

#### W-06 A plain key never fires while a field holds the caret

- When: a region handles a key with no modifier.
- Then: the key never fires while a field inside the region holds the caret, and a field
  hands the keys back to its region on Esc.
- Checked by: review.

### Words

#### W-07 Sentence case, and no period on a short line

- When: words are put on screen.
- Then: sentence case everywhere. Hints, empty lines and toasts end without a period; a
  dialog's description is whole sentences. A command reads as a plain sentence, its article
  kept: *Restart the agent*, *Hide the Workbench*.
- Checked by: test `a_toast_is_one_sentence_without_its_period` for a toast; review.

#### W-08 A label ends on an ellipsis when its action asks first

- When: an action opens a dialog, a confirmation, a picker or a form before it acts.
- Then: its label ends on an ellipsis: *Retry…*, *Delete conversation…*.
- Checked by: review.

#### W-09 Each verb says what goes

- When: a control removes, leaves or dismisses.
- Then: *Delete* takes a thing off disk; *Remove* takes it out of a list and leaves it;
  *Uninstall* is for a plugin; *Discard* throws away unsaved changes. *Keep* dismisses a
  confirmation; *Cancel* leaves a form or a dialog; *Close* puts a panel away.
- Checked by: review.

#### W-10 On-screen words are the glossary's

- When: a thing is named on screen.
- Then: by its term in `CONTEXT.md`, never by one of its _Avoid_ words.
- Checked by: review.
