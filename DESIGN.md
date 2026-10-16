# DESIGN.md: onehand UI contract

The whole-app UI contract for `onehand`: how the window is laid out and the rules every view
follows, as structure and behaviour, not values, save the palette's (*Colour and state*). It
binds at the level written here; below it, the code and its tests decide. A feature's own
document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) holds its screens' detail,
and `docs/ui-patterns.md` the patterns every screen repeats and how they behave. Code never cites
this file.

This contract defines onehand's own visual language: quiet navigation, clear content hierarchy,
compact controls and predictable alignment. It keeps no standing exceptions: a treatment that
differs is a rule of the role it belongs to, written where that role is. A departure is a bug,
put right or recorded in `docs/known-gaps.md` with its reason within the change that makes it.

**The palette is VS Code's Dark Modern and Light Modern** (*Colour and state*), installed over
gpui-component's theme at boot (`crate::theme::install`, with its contrast tests). Every colour,
radius and size is a token read at the call site (see `.claude/skills/design-tokens/reference.md`);
a hex literal outside the ramp is a bug even when it looks right, as no theme switch reaches it.

## Principles

1. **The conversation is the centre while a session shows**, the only region that flexes. Docks
   start closed and open on demand. Beside the Workbench it keeps a minimum, times its zoom; too
   narrow for both, the Workbench takes the area under *← Conversation*, which steps it aside. Pages
   take the agent pane and put the docks away; a page opens over a session only when picked.
2. **Give each boundary a purpose.** Separate with space first, then a line, then a fill, then a
   shadow, taking each step only for a reason the one before cannot give. Docks use a distinct
   surface; independent boxes and tables use a 1px `border`; a subsection is a heading and space,
   not a frame, and a hairline parts groups only where space alone runs many peers together (a
   settings sub-page). Shadows only for what floats, the app's own in
   `lift`: dialogs, popovers, menus, over the transcript the composer card, the cards pinned above
   it, the to-bottom button, and on a list page the item floating over the list.
3. **Content leads; navigation recedes.** Neutral surfaces carry the layout. Hue identifies a
   state, a link, a label's kind or code in prose, never a decorative section background.
   The palette's blue also marks keyboard focus and the primary action, and in the dark
   palette the selected item's fill. Attention is earned: the signals that call for the person
   (a state's hue, a count or badge of what waits, a dot, weight past what the role takes) mark
   only what needs them or has just changed, so a view at rest is calm. Hierarchy and identity are not attention: a heading keeps
   its weight, a list its muted count, a label its kind's hue. All colours are theme tokens with
   legible contrast on their actual surfaces in both modes.
4. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
5. **Same role, same treatment.** Across app and plugins, equivalent rows, headers and controls
   share size, spacing, corners and states. A valid token alone does not make a consistent view.
6. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
7. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.
8. **Calm and immediate.** What a person changes shows at once, and the work behind it follows.
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

- **Rail.** App chrome outside the dock, so a layout restore cannot lose it. Top to bottom: the
  workspace bar (letter tile, name, switcher, *Hide the rail*); the search over session titles and
  project names (`Ctrl+K` outside a terminal, showing a hidden rail) beside *New*, which starts in
  the keyboard's project, else the one on screen, its caret picking *Start in* or *With agent*;
  *Overview*, *Tasks* (a pill counting tasks that need attention, none at zero), *Issues* and
  *Workflows*, each filled while its page shows; *Sessions* with a warning-ink *N need attention*
  chip (shown above zero or while on, toggling that filter and back) and the filter menu (*By
  project*, *All sessions*, *Needs attention*, *Recent activity*) over a hairline; the scrolling
  list, ending on *Add project…*; a foot with *Settings*, a page row like the four above, filled
  while it shows. Every row at rest is in the muted ink; the one where the person is takes its
  fill and the ink paired with it. A neutral badge sinks a step into the surface it sits on, here
  the inset step.

  *By project* lists pinned projects first, a gap between projects. A project row: the fold
  chevron, folder and name, its git line (branch, uncommitted changes, commits ahead and behind,
  each only above zero and named in full on hover), the pin, the unattended pill (*auto*, *auto ·
  N*, *auto · N waiting*; a run's pill opens its issue on the Issues page), and while folded its
  sessions' most urgent state. Narrowed, the name keeps a minimum and the branch gives way first.
  Its chevron alone folds it (not during a search); the rest of the
  row shows the project's last session, or its page, which then fills the row. `+` and `⋯` show
  on hover and on the keyboard's row, right-click opens the same menu. A session row, indented
  under its project: its state mark, title and `⋯` (on hover, on the keyboard's row, always on
  the one shown), and under them `state · agent · age` (*waiting Nm* while it needs input, the
  state's word in its ink while it waits on the person), over which the pointer and the
  keyboard's row lay *Stop* (running), *Send the last prompt again* (failed) and *Close*; the
  keyboard's row lays them on any session but the one shown, whose conversation already holds
  *Stop* and the way back, so a row the keys only pass through stays still. The
  other filters are flat, the project in place of the agent. A group lists six sessions, then *N
  more*; an open project with none says *No open sessions*, a search or filter with none *No
  sessions match*. Each state has its own ink, and a mark per thing the person can do, none
  animated (running is a filled dot): failed and disconnected share one, both answered by trying
  again, their words telling them apart. The inks: failed or disconnected danger, needs input
  warning, running the accent, done unread success, idle muted. `↑`/`↓` move the list's row,
  ringed while the keys move it, `Enter` and `←`/`→` act on it, `Alt+↑`/`↓` step through
  sessions in its order. The tree's order is the user's, set by dragging; pinned projects stay on
  top and sessions never leave their project. `Ctrl+Shift+B` hides the rail completely, never to an
  icon column. It resizes between 232 and 448px, on the side bar's surface.
- **Agent pane header**, left to right: the show-rail button (only while the rail is hidden); the
  conversation's name, medium weight; a dots menu (*Rename…*, *Export as Markdown…*; *Resume in
  this session…*, refused mid-turn, and *Restart the agent*; *Delete conversation* in the danger
  tint, refused until the first turn ends); a spacer; past conversations and *Close session*
  (only while a session shows); the terminal, an accent dot on it while its dock is closed over a
  shell running a command; last, the Workbench. The two dock buttons are filled while their dock is open. On
  a page (the overview, Tasks, Issues, Workflows, Settings) it reads the page's name, with no dots
  menu and no dock buttons. As the pane narrows, the name gives way first, down to a minimum.
  Without a session it names the project, its dots menu holding the project's
  actions but *New session* and *Open terminal*, which the page and the header already offer.
- **Step strip.** Under the header, only while a run drives the session on screen: the workflow's
  name muted, each step's label between chevrons (done ones with a muted check, the current one in
  full ink), then *Review…*, *Revise…* and *Continue* (primary) while it waits for approval, and
  *Stop* always. Only the step labels clip. *Review…* reads the answer from the run; *Revise…*
  refuses an empty note; a press the run no longer waits at shows the new answer, in the warning
  ink, rather than approving it unread.
- **Composer.** A card with no edge, set off by its raised fill and lift; the caret alone shows
  focus, and no line is ever added to it (a refusal is Send's tooltip, a failure a toast). The
  field, then the `+` menu, *Fast* (*Fast: On* or *Off*, a toggle with no caret, when its choices
  read as on and off; else *Fast:* and the value in force), the model and
  effort chips, then *Send*, or *Stop* (solid danger, worded) and *Queue* over a draft. Under it
  the branch and the mode, stacked when narrow. Each chip's menu opens just above it, without
  its tooltip. Above the card: one pinned permission or question, the oldest, with how many more
  wait; then the queue and a reconnect as plain lines; then the staged files as one row of chips,
  faded where cut beside *Show all N*. *Run a workflow…* (the `+` menu, the keymap, the Workflows
  page's *Run…*) opens the launcher, and an issue's *Run workflow…* its start form; both are
  drawn as `docs/workflows.md` describes.
- **Docks.** Bare panels on the dock surface with strips of their own, no library tab bars; the
  terminal is a block held off its neighbours by a gap, drawn no taller than leaves the
  conversation a readable height (its dragged height kept).
  - Strips as tall as the header. The Workbench's: *← Conversation* while it has the area, the
    modes (Editor, Markdown, Issues, Plugins, Neovim; a select when narrow), maximize, hide. The
    terminal's: shell tabs (each capped in width, scrolling, so none is left out), `+`, maximize,
    hide. Crowded file tabs: a select.
  - A list beside its detail (files, documents, issues) only while the dock holds both; else one at
    a time, the detail under a link back naming the list. Picking shows it; going back keeps it.
    Files and Markdown draw one tree row (`tree_row`); the file or document open beside the tree
    is its picked row, a changed file's letter in its state's ink at the row's end.
  - A failed open or save in the Editor or Markdown is a toast; a file changed on disk, a
    Neovim that would not start and a listing that failed stand as a status line under the mode.
  - Plugins: the search and filters a filter bar; a row's actions on hover and on keyboard focus, its switch always;
    what runs and what waits on a restart under the list, so neither moves it.
  - The Issues tab's issue: a trail (project › reference, linked to its forge, or *Draft*) with
    its actions (one primary, one other, ⋯); its tags (a neutral state badge, labels); the title,
    large; one line of facts; what it waits on (a conflict, where its work stands, the review);
    then two chips: *Description* (the body as a document, the files it names, what the work
    left, *Before*) and *Activity N* (the history). Sections are parted by space, not rules.
    ⋯ ▸ *Open in Issues* and *Review…* open it on the Issues page, the latter with its review open.
  - Hiding a dock keeps its buffers and processes; the terminal's open state follows the project.
- **Pages without a session.**
  - A project with no session shows one column from the top: a head, the line counting its
    tasks that need attention, run or wait at the start (not drawn at zero, and opening the
    Tasks page narrowed to the project), *Run check* (spinning while a check runs) and *New
    session* (primary) at the end; its *Check command* field; and its past conversations as
    rows, capped, each with *Delete…* on hover. The page scrolls as one.
  - *Workspace overview* shows, across all projects, the boxes *Waiting on you* (runs waiting
    for an answer or an approval, and sessions waiting, failed, finished or lost) and *Working*, then
    *Projects* as a grid of tiles (not drawn without projects), then the sections *Recent
    conversations* and *Open issues* (its project picker kept while the page is left), plain
    lists on the page's surface. The page scrolls as one.
  - *Tasks*: a filter bar (the project picker), then four boxes two by two, *Needs attention*,
    *Running*, *Queued*, *Finished*, each with a count, an empty line and a cap. A row is the
    title over workflow, step or outcome and project, with ghost actions by state (*Open
    session*, *Stop*, *Resume*, *Retry…*, *Dismiss*) shown on hover, kept under *Needs attention*;
    its text opens the task's detail, a page of its own at the prose width: a trail, the title,
    then *Awaiting approval*, *Way out* and the runs as plain sections.
  - *Issues*, a list page (`docs/ui-patterns.md`, *List pages*): a head (the search, then
    *Refresh* and *New issue*, its primary, at the end); a filter bar (*Open* and
    *Closed* as choices, each its count in a badge on its corner, then pickers for progress,
    project and label and *Clear filters*); a row is its state mark (open in the success ink,
    closed in the closed ink), reference, title, priority and note, then its labels, project
    and age in slots at the end, the labels and project dropped first; *Showing N of M open
    issues* at the foot. *Reading the issues…* until the first read lands; *Refresh* spins while
    it reads. *No issues yet* or *No issues match* when empty.
  - The **review block** opens below where the work stands only on *Review…*, as
    `docs/unattended.md` draws it.
  - *Workflows*: a head (*New workflow*, its primary, *Import…*, then how many), then
    the workflows as rows, capped, shipped ones *Built in* (read-only) in a slot of their own,
    *Run…* (projects, the rail's first) and the rest shown on hover. *New workflow* or *Edit*
    opens a form below, one box per step (its kind its first field, one ⋯ to move or remove it),
    a margin marking where a failure goes back to, problems (a name clash among them) above a
    *Save* that refuses with the first and spins while it writes.
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast; modals are
  `Dialog`s. Settings is a page: a nav column with Appearance, Workspace, Agents, Connections and
  Shortcuts (rows as the rail's page rows), and beside it the sub-page, untitled (its nav row
  names it), opening on its scope as a badge and a hint, its groups split by hairlines with
  semibold headings, not boxes. Every button there is secondary: Settings holds no primary.
- **Persistence.** The layout is saved as the Workbench's dragged width, the terminal and rail
  sizes and whether each dock is open, never as the library's `DockAreaState`.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read a step over the chrome; what is
  wider scrolls inside its well. A scrollbar runs on the panel's edge down to the composer.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters (tools only), commands,
  diffs, answered permissions and questions, notices and errors, the files the turn changed,
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
  (`.xsmall()`, quiet, named in its tooltip, in a column of its own; a permission's on the line
  under its command, beside where it runs; a prompt's under its bubble, shown while the prompt
  is pointed at or the control holds focus) and one show-more control serve every block. Text
  that scrolls keeps a gutter for its scrollbar.
- **Calm once it is past**: a failure a later prompt has moved past keeps its words in the
  muted ink, with no fill; a settled *N failed* drops the danger ink; a refusal's badge is
  neutral. A live failure is a solid danger wash with no edge, in the agent's words (*The turn
  failed: …*), what it attached folded under *Show details* in mono; a lost agent's names the key
  that restarts it. A failed turn holds until a prompt goes out; a restart does not answer it. An
  error that failed no turn (one answering a *Stop*, a resume that fell back) is past at once.
- **The pinned cards** (a permission, a question) share one shell: a head, the body, the
  answers at the end at `.xsmall()` (a hairline under the head and over the answers, padded as a box
  whose body scrolls), the dismissing one a quiet ghost. A choice is a row on a well
  of its own at rest and on the selected step when taken, a radio or checkbox drawn in ink at
  its head (the library's rings vanish on the selected step) and its number as a `Kbd`, at least
  a field's height; a typed answer is a plain field (`.short()`), ringed while the keys are on it. Their keys are named in tooltips, never in a label.
  The card holds the one primary while it shows.

## Type, space and shape

| Role | Size |
|---|---|
| Meta, hints, counts, a small label | `.text_xs()`, in `muted_foreground` when it is meta |
| Machine text | `CODE_TEXT`, in `mono_font_family` |
| Chrome: a panel's rows, boxes and forms | `.text_sm()` |
| A control's words (`.short()`, `.xsmall()`) | `.text_xs()` |
| Reading text: the transcript, the composer's field, a document | the inherited size, `.text_base()` |
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
  `onehand_plugin_host::tokens`, others beside their use), its reason in its doc comment.
- **Two shapes.** What a person presses or what holds content takes `radius`, 4px
  (`tokens::CORNER`): a control, a field, a row, a tab, a chip, a well, the bubble, a box, the
  composer, the terminal, a floating panel, a dialog, a toast. The theme sets `radius_lg` to the
  same, so the library's own surfaces follow. What is only read beside content is
  `rounded_full`: a badge (a `Tag`), so it reads as a tag rather than a
  button, its words `py_1` off its edges, its edge halfway from its fill to its ink (`badge_edge`), and a dot or a status mark is `rounded_full` too. The library's small parts keep its half
  corner (*Components*). A part joined to its container's edge takes its corner there.
- **Lines** part regions and are a 1px `border`. A **mark** says where the person is, not where
  a region ends: the one under an open tab is 2px, as a stroke a step heavier than a line. A list page's lines across
  the pane are a step fainter (`faint_rule`), so the edge-to-edge list reads as one surface.

## Colour and state

The palette is VS Code's **Dark Modern** and **Light Modern**, each value the one VS Code gives
the key named beside it: resolved through the theme's include chain (`dark_modern.json` over
`dark_plus.json` over `dark_vs.json`, and the light trio likewise), and for a key no theme file
sets, the default VS Code registers in its source. The values live in `crate::theme`'s ramp and,
for the terminal, `onehand_terminal_ui::ansi`; this table is their contract and changes with them.
A new colour is the value of a VS Code key, and a colour read anywhere but through a token is a bug.

| Role | VS Code key | Dark | Light |
|---|---|---|---|
| reading surface (`background`), an open tab | `editor.background`, `tab.activeBackground` | `#1F1F1F` | `#FFFFFF` |
| prose (`foreground`), the rail's slot ink | `foreground`, `sideBar.foreground` | `#CCCCCC` | `#3B3B3B` |
| docks, the rail, title, status and tab bars, list and table heads | `panel.background`, `sideBar.background`, `titleBar.activeBackground`, `statusBar.background`, `editorGroupHeader.tabsBackground`, `sideBarSectionHeader.background` | `#181818` | `#F8F8F8` |
| every well, the prompt bubble (`muted`), a skeleton | `textCodeBlock.background` | `#2B2B2B` | `#F8F8F8` |
| meta text (`muted_foreground`), an off switch, a tab's word | `descriptionForeground` (dark), `tab.inactiveForeground` | `#9D9D9D` | `#616161` |
| a step past the well (`secondary` and its ink) | `textPreformat.background`, `textPreformat.foreground` | `#3C3C3C`, `#D0D0D0` | black at 12% (`#E0E0E0`), `#3B3B3B` |
| hover (`list_hover`) | `list.hoverBackground` | `#2A2D2E` | `#F2F2F2` |
| the selected item (`accent` and its ink) | `list.activeSelectionBackground`, `list.activeSelectionForeground` | `#04395E`, `#FFFFFF` | `#E8E8E8`, `#000000` |
| the rail's hovered row (`sidebar_accent`) | `list.inactiveSelectionBackground` | `#37373D` | `#E4E6F1` |
| every hairline (`border`), the window's edge | `panel.border`, `titleBar.border` | `#2B2B2B` | `#E5E5E5` |
| floating surfaces (`popover`) | `editorWidget.background` | `#202020` | `#F8F8F8` |
| the composer card, an edgeless field (`theme::raised`, `theme::inset`) | `input.background` | `#313131` | `#FFFFFF` |
| a control's edge (`input`) | `input.border` | `#3C3C3C` | `#CECECE` |
| a secondary button at rest, pointed at, its word | `button.secondaryBackground` (dark: `input.background`, as Dark Modern's is transparent), `button.secondaryHoverBackground`, `button.secondaryForeground` | `#313131`, `#2B2B2B`, `#CCCCCC` | `#E5E5E5`, `#CCCCCC`, `#3B3B3B` |
| the open tab's word | `tab.activeForeground` | `#FFFFFF` | `#3B3B3B` |
| what runs, links (`link`, `info`), at rest, pointed at, pressed | `textLink.foreground`, `textLink.activeForeground` | `#4DAAFC` | `#005FB8` |
| keyboard focus (`ring`), a dragged panel's edge | `focusBorder`, `sash.hoverBorder` | `#0078D4` | `#005FB8` |
| where a dragged panel lands | `editorGroup.dropBackground` | `#53595D` at 50% | `#2677CB` at 18% |
| a dialog's backdrop | `widget.shadow` | black at 36% | black at 16% |
| the scrollbar thumb, pointed at | `scrollbarSlider.background`, `scrollbarSlider.hoverBackground` | `#797979` at 40%, `#646464` at 70% | `#646464` at 40%, `#646464` at 70% |
| the caret | `editorCursor.foreground` | `#AEAFAD` | `#000000` |
| selected text (`selection`) | `editor.selectionBackground` | `#264F78` | `#ADD6FF` |
| the primary action (`primary` and its ink), pointed at and pressed, a progress bar | `button.background`, `button.hoverBackground`, `button.foreground`, `progressBar.background` | `#0078D4`, `#026EC1`, `#FFFFFF` | `#005FB8`, `#0258A8`, `#FFFFFF` |
| danger as a fill (`danger`) | `statusBarItem.errorBackground` | `#B90F07` (`errorForeground` darkened by 0.4) | `#C72E0F` |
| danger pointed at and pressed | none: the fill mixed 15% and 25% toward Light Modern's ink | | |
| the status hues `status_ink()` is derived from | `errorForeground`, `editorWarning.foreground`, `testing.iconPassed` | `#F85149`, `#CCA700`, `#73C991` | `#F85149`, `#BF8803`, `#73C991` |
| a label's kind (`magenta`, `cyan`) | `terminal.ansiMagenta`, `terminal.ansiCyan` | `#BC3FBC`, `#11A8CD` | `#BC05BC`, `#0598BC` |
| the terminal's sixteen colours | `terminal.ansi*` | `onehand_terminal_ui::ansi::DARK` | `onehand_terminal_ui::ansi::LIGHT` |
| a chart's series | `charts.blue`, `charts.green`, `charts.yellow`, `charts.orange`, `charts.purple`, `charts.red` | `#59A4F9`, `#89D185`, `#CCA700`, `#D18616`, `#B180D7`, `#F14C4C` | `#0063D3`, `#388A34`, `#BF8803`, `#D18616`, `#652D90`, `#E51400` |

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
| `border` | every hairline |
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
  focus on a control is the ring. A field is marked by its edge, so its focus is that edge turned
  the `ring` ink, and an edgeless one gains it while focused; the composer, the input surface,
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
the plugin host's builders, which set the pointer cursor and the one corner (`radius`): buttons through
`action`; fields through `text_input`, `search_input` and `text_area`. A field comes in two kinds:
an **edged field**, in a form or a dialog, on no fill of its own; and a **search field** (the
search mark, the clear button), with no edge, sunk into the fill of the place it sits (the
rail's on `theme::inset`, a list page's on the well's `muted`), since it heads what it searches
rather than standing among fields. A button names its variant by role: `primary`
the main action, `secondary` an alternative beside it, a set picker's trigger (an unset one is a ghost, through
`picker`) and every Settings button,
`choice` in a pick-one row, `danger` as below, and ghost (`.quiet(cx)`, whose hover
is the control fill) the rest. **No button draws an edge** (no default variant, no `outline()`): a
field is the one control marked by its edge, or, searching, by its sunk fill. A control that refuses says so
and why (`.refuses(why)`, the reason its tooltip) and goes back to the arrow.

**Danger is a word, and a fill only to end or confirm.** A destructive action is a word in the
danger tint. The solid `danger` fill marks two buttons: *Stop* in the composer while a turn runs,
and the button confirming a removal inside its modal. Nothing that destroys nothing takes the
danger ink.

**The composer is the input surface**, a role of its own rather than a box or a field: a card
with no edge, standing off the page on its own fill (`theme::raised`) and lift (`composer_lift`),
showing focus by the caret alone. Its pickers (the model, the effort, the mode, *Fast*, the
branch) are `.xsmall()` ghosts with a caret whatever they hold (*Fast* as a toggle opens no menu
and carries none), where a picker elsewhere takes
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

**A size names a role, and means one height and one text size in every control.** The library's
sizes disagree across components (its default button letters at the reading size beside a field
at the chrome size), so the app uses three: `.xsmall()`, the compact default, in a strip, a box, a
column page's head, for a row's actions and for a secondary action inside dense content (a line of
meta text, a transcript block, a chip); `.short()` (`CONTROL_H`) in a form, a page's filter bar
and a dialog's footer; `.tall()` (`FIELD_H`, chrome-size words) in a list page's head, its
search and actions. Never the library's default. **`.xsmall()` and `.short()` letter their words
at `.text_xs()`**, so a `.short()` control's sit in its box with room above and below. A
correction a component needs lives in its shared wrapper (`Short`, `Tall`), never at a call site. An icon
keeps its role's size whatever the hit area round it, and a badge, a checkbox or a switch keeps
the library's own geometry, centred in its row. `docs/ui-patterns.md` (*Control sizes*) maps each
role to its component and size.

The app owns only its own: the transcript renderers, the icon registry, the terminal panel,
per-panel zoom, and the composer popup (one shell for `@`, `/`, the pickers, the `+` and branch
menus and the attachment tray, with a pinned title and no key hints, at most six rows, grouped, then
how many more; a menu opens just above the control that opened it).

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a mode with a PTY of
its own per project. Files lives inside Editor, not as a mode. Plugins are built in.

## Icons

Every icon is an SVG from gpui-component's `IconName`. `crate::icons` holds only what that set
cannot draw (shapes it has no drawing of, brand marks, and forks of bundled shapes for stroke
weight), synced from `assets/icons/manifest.toml` by `scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
Equivalent icon roles use the same library size through a shared builder; optical corrections
belong there. Add an icon only when it helps recognition or names an action without visible text;
a word reads faster than a shape nobody has learned. An icon beside text is no taller than that
text's line and sits on its centre; icons down a column share one slot width, so the labels after
them start on one edge. No icon sits on a tile of its own; a fill behind one belongs to the
control or row it is part of.
