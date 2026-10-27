# DESIGN.md: onehand UI contract

The whole-app UI contract for `onehand`: the principles every view follows, the window's frame,
the transcript and the visual system, as structure and behaviour, not values (the palette names
VS Code's keys, not their values: *Colour and state*). It binds at the level written here; below
it, the code and its tests decide. `docs/screens.md` says what each region holds and in what
order, a feature's own document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) its
screens' detail, and `docs/ui-patterns.md` the patterns every screen repeats and how they behave.
Code never cites this file.

This contract defines onehand's own visual language: quiet navigation, clear content hierarchy,
compact controls and predictable alignment. It keeps no standing exceptions: a treatment that
differs is a rule of the role it belongs to, written where that role is. A departure is a bug,
put right or recorded in `docs/known-gaps.md` with its reason within the change that makes it.

**The palette is VS Code's Dark Modern and Light Modern** (*Colour and state*), installed over
gpui-component's theme at boot (`crate::theme::install`, with its contrast tests). Every colour,
radius and size is a token read at the call site (see `.claude/skills/ui/tokens.md`);
a hex literal outside the ramp is a bug even when it looks right, as no theme switch reaches it.

## Principles

1. **The conversation is the centre while a session shows**, the only region that flexes. Docks
   start closed and open on demand. Beside the Workbench it keeps a minimum, times its zoom; too
   narrow for both, the Workbench takes the area under *← Conversation*, which steps it aside. Pages
   take the agent pane and put the docks away; a page opens over a session only when picked.
2. **Content leads; navigation recedes.** Neutral surfaces carry the layout. Hue identifies a
   state, a link, a label's kind or code in prose, never a decorative section background.
   The palette's blue also marks keyboard focus and the primary action, and in the dark
   palette the selected item's fill. Attention is earned: the signals that call for the person
   (a state's hue, a count or badge of what waits, a dot, weight past what the role takes) mark
   only what needs them or has just changed, so a view at rest is calm. Hierarchy and identity are not attention: a heading keeps
   its weight, a list its muted count, a label its kind's hue. Nor is a mode switched on: the
   control that switches it may carry a state's hue on its own mark while the mode is on, and
   nowhere else. All colours are theme tokens with
   legible contrast on their actual surfaces in both modes.
3. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
4. **Same role, same treatment.** Across app and feature crates, equivalent rows, headers and controls
   share size, spacing, corners and states. A valid token alone does not make a consistent view.
5. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
6. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.
7. **Calm and immediate.** What a person changes shows at once, and the work behind it follows.
   Nothing moves for show: the one motion is a `Spinner` on work still running, beside what the
   library's own overlays and controls do (*Components*). `docs/ui-patterns.md` (*Immediate feedback*, *Motion*) holds how.

## Layout

One window hosts one workspace. The Workbench runs the full height; the terminal sits under the
agent pane only. The diagram is plain ASCII and box-drawing, so it stays aligned in any mono font.

```
┌──────────────────┬────────────────────────────────────┬──────────────────┐
│ W Workspace   v <│ Title (...)    past close term wb  │ modes   max hide │
│ Search...  New v │                                    │                  │
│ Overview         │ workflow > step > step   Stop      │                  │
│ Tasks         2  │                                    │                  │
│ Issues           │                                    │                  │
│ Workflows        │                                    │                  │
│ Sessions  2 need │                                    │    Workbench     │
│ ──────────────── │          agent pane                │  (right dock,    │
│ v project  main  │        (centre, flexes)            │   full height)   │
│   o session      │                                    │                  │
│     Idle . agent │  ┌──────────── composer ──────┐    │                  │
│ + Add project... │  │ +  Fast  model        Send │    │                  │
│                  │  └────────────────────────────┘    │                  │
│                  │    branch                mode      │                  │
│                  ├────────────────────────────────────┤                  │
│                  │ shell tabs  +                      │                  │
│ Settings         │ terminal (bottom dock)             │                  │
└──────────────────┴────────────────────────────────────┴──────────────────┘
        rail                     DockArea
```

What each region holds, and in what order, is `docs/screens.md`'s, under the region's name:

- **Rail**: app chrome outside the dock, from the workspace bar down to *Settings*.
- **Agent pane header**: the conversation's name, its menu and the dock buttons; chrome alone over
  a page that names itself in its head.
- **Step strip**: a run's steps under the header, while it drives the session on screen.
- **Composer**: the input surface under the transcript, and what waits pinned above it.
- **Docks**: the Workbench and the terminal, bare panels with strips of their own.
- **Pages without a session**: a project's page, the overview, Tasks, Issues and Workflows.
- **Settings**: a page of its own, a nav column beside the sub-page it shows.
- **Persistence**: what the layout saves.
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast; modals are
  `Dialog`s.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read at the reading size; what is
  wider scrolls inside its well. A scrollbar runs on the panel's edge down to the composer.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters (tools only), commands,
  diffs, a subagent's card (a bare fold block where the agent handed work off), answered permissions and questions, notices and errors, the files the turn changed,
  and, while it runs, the running line (a spinner, its clock, *N running*, what the agent says it
  is doing). A workflow's steps and an interrupted turn are notices, never prompts.
- **Folding blocks open from one line**, the chevron first: what runs is open until done, the
  plan stays open, and a reader's fold wins. Long output folds and is capped, an opened cluster
  too, each saying what it holds back. Wells are filled with no edge, their text at
  `CODE_TEXT`. A finished turn ends on Copy (its closing paragraph) and how long it took.
- **Its rows are lines, not list rows**: a fold line or an activity row is as tall as its text,
  answers the pointer by raising its ink rather than with a fill, leads with an `.xsmall()` mark
  in one slot (a spinner while it runs, so nothing moves as it settles), and takes no focus. A
  path or a command is mono; the agent's own wording, a folder and what the agent did (a
  thought, a command's output, a read or search list) read in the middle ink. One copy control
  (`.small()`, quiet, named in its tooltip, in a column of its own; a permission's on the line
  under its command, beside where it runs; a prompt's under its bubble, shown while the prompt
  is pointed at or the control holds focus) and one show-more control serve every block. Text
  that scrolls keeps a gutter for its scrollbar.
- **A table keeps its columns readable**: each is as wide as its text down to a floor, and past
  the column the table scrolls sideways in its own track while the vertical wheel stays the
  conversation's. Under every table, Copy (as Markdown) and *Open*, which shows it whole in a
  dialog as wide as the window allows.
- **Calm once it is past**: a failure a later prompt has moved past keeps its words in the
  muted ink, with no fill; a settled *N failed* drops the danger ink; a refusal's badge is
  neutral. A live failure is a solid danger wash with no edge, in the agent's words (*The turn
  failed: …*), what it attached folded under *Show details* in mono; a lost agent's names the key
  that restarts it. A failed turn holds until a prompt goes out; a restart does not answer it. An
  error that failed no turn (one answering a *Stop*, a resume that fell back) is past at once.
- **The pinned cards** (a permission, a question) share one shell: a head, the body, the
  answers at the end at `.short()` (a hairline under the head and over the answers, padded as a box
  whose body scrolls, its cut end fading out while more lies below), the dismissing one a quiet
  ghost. A choice is a row on a well
  of its own at rest and on the selected step when taken, a radio or checkbox drawn in ink at
  its head (the library's rings vanish on the selected step) and its number with Alt as a `Kbd`, at least
  a field's height; a typed answer sits under the hairline, with the answers and no line between them, on the
  choices' well with its `Kbd`: an edgeless field two lines tall at rest that grows as it is
  written (Shift+Enter a newline), ringed while the keys are on it. Each answer in a pinned card's footer shows its key beside its words, always in
  sight, as that key reaches the card from anywhere in the conversation.
  The card holds the one primary while it shows.

## Type, space and shape

| Role | Size |
|---|---|
| Meta, hints, counts, a small label | `.text_xs()`, in `muted_foreground` when it is meta |
| A caption inside a small object (an attachment's plate) | `MICRO_TEXT` (11px), a step under the meta size |
| Machine text | `CODE_TEXT`, in `mono_font_family` |
| Chrome: a panel's rows, boxes, forms and strips, a menu's rows | `CHROME_TEXT` (13px), a step under the reading size |
| A control's words (`.short()`) | `.text_xs()` |
| Reading text: the transcript, the composer's field, a document | `.text_sm()`, on the reading leading; the transcript's `TEXT` |
| The transcript's record: activity, thoughts, notices, a card's question | a step under the reading size, the transcript's `TEXT_SM` (13px) |
| A heading in reading text | a step up the scale per level, two at most; then the body's size |
| A page's or an issue's title | `.text_xl()` |

- **Weight carries hierarchy before size does**: `font_medium` for a page's title and a name in
  a header, `font_semibold` for any other title or heading; nothing bolder. A list page's rows,
  at the meta size, carry their name and the column read down the list in `font_medium`
  (`docs/ui-patterns.md`, *List pages*). Line height is gpui's,
  but `LEADING` for reading text and `LEADING_TIGHT` for a wrapped title.
- **Inks by role.** Text on a neutral surface (the reading surface, a dock, the rail, a floating
  surface, a well, the bubble) takes one of three: `foreground` for content and the current
  location; `theme::meta_ink` for a detail beside a name on its line (its folder, its number, its
  kind); `muted_foreground`, still readable, for labels, counts, hints, placeholders and every
  navigation row but the current one. Text on a fill takes the ink paired with that fill: a
  button's variant and the library's controls set their own, a selected row `accent_foreground`,
  a badge its `Tint`'s ink. Meaning takes its semantic ink: a state `status_ink()`, a link or
  what runs `link`, code in prose `hue_ink`. No fourth neutral ink, and a quieter ink is a mixed
  step, never an alpha on the text or its container: see-through text reads differently on
  every fill under it.
- **The UI and mono font families ship with the app**, so text looks and centres the same everywhere;
  `[font].monospace` can still name another installed mono family.
- **Space** takes gpui's steps `0p5`, `1`, `1p5`, `2`, `3`, `4`, `6` and `8`; a named spacing
  constant equals one of them. Pick by relationship using `docs/ui-patterns.md`'s *Spacing and
  density*, not by what fits one screenshot. Text below a title shares its text axis, and icons,
  labels and buttons in one column share one axis down it.
- **Sizes are rems**, as per-panel zoom overrides the rem base. Pixels are for what zoom must
  not reach: a stroke (a line, a ring, a mark), a shadow, a bound measured against the window, an
  inset cancelling one the library draws in pixels, and a grid that zooms on its own (the
  terminal's). Fixed chrome heights stay outside the
  zoom wrapper. A size the steps lack is a named constant (shared ones in
  `onehand_ui::tokens`, others beside their use), its reason in its doc comment.
- **Two shapes.** What a person presses or what holds content takes `radius`, 4px
  (`tokens::CORNER`): a control, a field, a row, a tab, a chip, a well, the bubble, a box, the
  composer, the terminal, a floating panel, a dialog, a toast. The theme sets `radius_lg` to the
  same, so the library's own surfaces follow. What is only read beside content is
  `rounded_full`: a badge (a `Tag`), a tag standing on its own, so it reads as a tag rather than a
  button (inside a line of meta text a label is written in its kind's ink instead, unframed), its words `py_1` off its edges, its edge halfway from its fill to its ink (`badge_edge`), and a dot or a status mark is `rounded_full` too. The library's small parts keep its half
  corner (*Components*). A part joined to its container's edge takes its corner there.
- **Lines** part regions and are a 1px `border`. A **mark** says where the person is, not where
  a region ends: the one under an open tab is 2px, as a stroke a step heavier than a line. The line under a page's top,
  a list page's lines across the pane and the line between Settings' groups are a step fainter
  (`faint_rule`), so a page's top ends on one line whatever its column, the edge-to-edge list reads
  as one surface, and a group's heading, not its line, says where it starts. A
  box's edge is a step stronger, a control's edge (`input`): in Dark Modern the hairline sits too
  near the page's fill to group anything.

## Colour and state

The palette is VS Code's **Dark Modern** and **Light Modern**. Each role takes the value VS Code
gives the key named beside it, resolved through the theme's include chain (`dark_modern.json` over
`dark_plus.json` over `dark_vs.json`, and the light trio likewise), and for a key no theme file
sets, the default VS Code registers in its source. The key is the value's source; `crate::theme`'s
ramp holds the value (for the terminal, `onehand_terminal_ui::ansi`), already derived where VS
Code derives it. This table names the key, and changes only when a role takes another. A new
colour is the value of a VS Code key, and a colour read anywhere but through a token is a bug.

| Role | VS Code key |
|---|---|
| reading surface (`background`), an open tab | `editor.background`, `tab.activeBackground` |
| prose (`foreground`), the rail's slot ink | `foreground`, `sideBar.foreground` |
| docks, the rail, title, status and tab bars, list and table heads | `panel.background`, `sideBar.background`, `titleBar.activeBackground`, `statusBar.background`, `editorGroupHeader.tabsBackground`, `sideBarSectionHeader.background` |
| every well, the prompt bubble (`muted`), a skeleton | `textCodeBlock.background` |
| meta text (`muted_foreground`), an off switch, a tab's word | `descriptionForeground` (dark), `tab.inactiveForeground` |
| a step past the well (`secondary` and its ink) | `textPreformat.background` (light: black at 12% over `editor.background`), `textPreformat.foreground` |
| hover (`list_hover`) | `list.hoverBackground` |
| the selected item (`accent` and its ink) | `list.activeSelectionBackground`, `list.activeSelectionForeground` |
| the rail's hovered row (`sidebar_accent`) | `list.inactiveSelectionBackground` |
| every hairline (`border`), the window's edge | `panel.border`, `titleBar.border` |
| floating surfaces (`popover`) | `editorWidget.background` |
| the composer card, the rail's search (`theme::raised`, `theme::inset`) | `input.background` |
| a field's fill (`field_fill`) | mixed: `muted` a step toward `foreground`, so it stands off the page, a floating surface and a well alike |
| a control's edge, a box's edge (`input`) | `input.border` |
| a secondary button at rest, pointed at, its word | `button.secondaryBackground` (dark: `input.background`, as Dark Modern's is transparent), `button.secondaryHoverBackground`, `button.secondaryForeground` |
| the open tab's word | `tab.activeForeground` |
| what runs, links (`link`, `info`), at rest, pointed at, pressed | `textLink.foreground`, `textLink.activeForeground` |
| keyboard focus (`ring`), a dragged panel's edge | `focusBorder`, `sash.hoverBorder` |
| where a dragged panel lands | `editorGroup.dropBackground` |
| a dialog's backdrop | `widget.shadow` |
| the scrollbar thumb, pointed at | `scrollbarSlider.background`, `scrollbarSlider.hoverBackground` |
| the caret | `editorCursor.foreground` |
| selected text (`selection`) | `editor.selectionBackground` |
| the primary action (`primary` and its ink), pointed at and pressed, a progress bar | `button.background`, `button.hoverBackground`, `button.foreground`, `progressBar.background` |
| danger as a fill (`danger`) | `statusBarItem.errorBackground` (dark: `errorForeground` darkened by 0.4) |
| danger pointed at and pressed | none: the fill mixed 15% and 25% toward Light Modern's ink |
| the status hues `status_ink()` is derived from | `errorForeground`, `editorWarning.foreground`, `testing.iconPassed` |
| a label's kind (`magenta`, `cyan`) | `terminal.ansiMagenta`, `terminal.ansiCyan` |
| the terminal's sixteen colours | `terminal.ansi*` |
| a chart's series | `charts.blue`, `charts.green`, `charts.yellow`, `charts.orange`, `charts.purple`, `charts.red` |

VS Code's surfaces sit closer than a stepped ramp's would: a region may be as little as 1.01 off
what it sits on, and a hairline or a shadow says where it ends. Its status hues and its focus ring
fall under WCAG AA on some of the surfaces they land on; those pairs are in `docs/known-gaps.md`,
and the ramp's tests hold each at what it reaches.

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `theme::dock_surface` | the Workbench and the terminal, a step off the reading surface |
| `muted` / `muted_foreground` | quiet fills (the prompt bubble, every well), meta text |
| `theme::meta_ink` | the middle ink: a detail beside a name, between prose and meta, contrast-tested |
| `border` | every hairline; a box takes `input` for its edge |
| `ring` | keyboard focus, drawn only when the keyboard moved it; the composer while a file is dragged over it |
| `accent` | the one item selected among several (a fill, not a hue) |
| `link` | the accent hue: what runs, and links |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text, and `closed` for what is over and settled (a closed issue) |
| `popover` | floating surfaces |
| `theme::raised` | the composer card |
| `secondary` | inline code in the prompt bubble, a step off its fill; a document's code on a dock |

- Reading content is the visual anchor; rail and dock chrome remain quieter. A nested container
  inherits its surface unless it has a distinct role: well, selection or floating content.
- Boxes are borders; wells and the bubble are fills. Ordinary sections remain unboxed. Do not
  combine a border, contrasting fill and shadow just to make a group visible.
- Hover and selection are fills: distinct steps on a row (`list_hover`, `accent`; on the rail's side bar surface its
  own pair, `sidebar_accent` and `rail_chosen`, as a fill is a step from the surface under it), one step on a ghost button (`control_fill`, between a
  row's hover and its selected step). A secondary button rests on a fill of its own and takes
  its pointed-at step under the pointer; the selected step is a chosen row's. Keyboard
  focus on a control is the ring. A field is marked by its fill and has no edge at rest, so its focus
  is an edge in the `ring` ink; the composer, the input surface,
  shows focus by its caret (*Components*).
- At most one primary in a view; a view needs none (the Settings page has none). Where layers
  stack, what waits on the person holds it while it shows (a floating issue, an open form, a
  pinned card, a run's approval), and what it covers steps down to `secondary`.
- Surfaces and inks are greys, so a hue on screen means something: a state, a link, a label's
  kind, focus, the primary action, or in the dark palette a selected item. A missing surface or
  ink is a VS Code key's value added to the ramp with its contrast asserted, never a value in
  one view; a step between two the ramp holds is mixed from them perceptually
  (`mix_oklab`), so both modes derive it by one rule.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, plus gpui's own `list` for the transcript. Controls go through
the UI crate's builders, which set the pointer cursor and the one corner (`radius`): buttons through
`action`; fields through `text_input`, `search_input` and `text_area`. **Every field has a fill
and no edge**: a field in a form, a dialog or a page's top and a search alike stand on
`field_fill`, a step past the well, so one shape reads as a field wherever it sits; a search adds
the search mark and the clear button. The rail's search keeps the rail's own fill
(`theme::inset`), and the composer's field is its card's. A button names its variant by role: `primary`
the main action, `secondary` an alternative beside it, a set picker's trigger (an unset one is a ghost, through
`picker`; a form's picker is set whatever it holds), every Settings button and the rail's *New*
(the one filled control in that chrome, as starting a session is what the rail is opened for),
`choice` in a pick-one row, `danger` as below, and ghost (`.quiet(cx)`, whose hover
is the control fill) the rest. **No button draws an edge** (no default variant, no `outline()`): a
field is marked by its fill, and its edge is focus alone. A control that refuses says so
and why (`.refuses(why)`, the reason its tooltip) and goes back to the arrow.

**A ghost never starts a line alone.** Without a fill, a ghost's padding sets its words in from
the edge the text above and below it starts on, and nothing drawn says where it begins. A ghost at
the start of its line has something before it on that line; one that would stand there alone
takes the secondary fill instead (a form's picker, a fold toggle). A ghost ending a line or
centred on it may stand alone.

**A ghost is known by where it stands**: in a strip, a toolbar, a footer, a row's action slot,
beside other controls. Beside a sentence (a preflight finding, a notice, a status line) it reads
as more of the text, so an action there takes the secondary fill (*Check the agent*, *Retry…*).

**An action with a menu of alternatives is one control.** The action and its caret half are joined
edge to edge, one variant and one height, the inner corners square, parted by a seam in the colour of the surface
under them (`split_action`); a caret never
stands as a button apart beside the action it qualifies. A caret otherwise ends a picker or a tab,
after its words.

**Danger is a word, and a fill only to end or confirm.** A destructive action is a word in the
danger tint. The solid `danger` fill marks two buttons: *Stop* in the composer while a turn runs,
and the button confirming a removal inside its modal. Nothing that destroys nothing takes the
danger ink.

**The composer is the input surface**, a role of its own rather than a box or a field: a card
with no edge, standing off the page on its own fill (`theme::raised`) and lift (`composer_lift`),
showing focus by the caret alone. Its pickers (the model, the effort, the mode, *Fast*, the
branch) are `.short()` ghosts with a caret whatever they hold (*Fast* is its bolt alone, lit in
the warning ink while fast, as a mode switched on is, and as a toggle opens no menu and carries none), where a picker elsewhere takes
the secondary fill once set: each always holds a choice, and a fill on every chip would crowd
the field they sit under.

**The library's own parts are the library's.** Inside a gpui-component control or overlay, what
it draws itself stays as it draws it: a refusing or loading button's alpha, a picker's caret, a
badge under the pointer, a switch's thumb and the motion of its overlays; the `sm` radius, half
the one corner, of its small parts drawn inside a control (a `Kbd`, a checkbox, its own tabs),
where a full corner would round them to a pill; and an open dropdown's trigger on its pressed
fill, in the ink the caller gave it, so a quiet icon stays muted there. The app corrects the
library only through a shared wrapper: where the library's mark vanishes on a fill the app
draws on (a radio's or a checkbox's ring on the selected step), `choice_mark` draws it in ink.

**A size names a role, and the role follows the room the control stands in.** The library's
sizes disagree across components (its default button letters at the body size beside a field
at `.text_sm()`), so a control takes one of two heights. On a page, where there is room and
the person reads and types what a control holds, it takes `.tall()`: a page's head and a page's
form, its fields, pickers and buttons alike, so a field's words stand at its title's size; a
dialog holding a form at `DIALOG_FORM` is that room too. In dense chrome (the rail, a dock, a
strip, a filter bar, a dialog that asks one thing, a row, a box's title row, the composer) it takes
`.short()`, so it never outweighs what it acts on. An icon alone is square at
its row's height: `.small()` beside `.short()` peers, `.tall()` beside tall ones. `.tall()`
(`FIELD_H`) is the ceiling: no control stands taller. Never the library's default.
**What a small object holds scales with the object.** Inside a fixed object smaller than a row of
controls (an attachment's plate, a thumbnail), an icon-only control takes `.xsmall()` and a caption
`MICRO_TEXT`, so neither crowds what it sits on; anywhere else `.short()`, `.small()` and the meta
size are the floor, and a worded control never takes `.xsmall()`, whose words run to its edges.
Every control in one row of actions stands at one height. A correction a component needs lives
in its shared wrapper (`Short`, `Tall`), never at a call site. `docs/ui-patterns.md` (*Control
sizes*) maps each role to its component and size.

The app owns only its own: the transcript renderers, the icon registry, the terminal panel,
per-panel zoom, and the composer popup (one shell for `@`, `/`, the pickers, the `+` and branch
menus and the attachment tray, with a pinned title and no key hints, at most ten rows, grouped, then
how many more; a menu opens just above the control that opened it).

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a mode with a PTY of
its own per project. Files lives inside Editor, not as a mode. Every mode is built in.

## Icons

Every icon is an SVG from gpui-component's `IconName`, the library's default subset of Lucide.
`crate::icons` holds only what that set
cannot draw: shapes it has no drawing of, picked from the kit's complete Lucide catalog, and brand
marks and forks of bundled shapes for stroke weight, synced from `assets/icons/manifest.toml` by
`scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
Equivalent icon roles use the same library size through a shared builder; optical corrections
belong there. Add an icon only when it helps recognition or names an action without visible text;
a word reads faster than a shape nobody has learned. An icon beside text is no taller than that
text's line and sits on its centre; icons down a column share one slot width, so the labels after
them start on one edge. No icon sits on a tile of its own; a fill behind one belongs to the
control or row it is part of.
