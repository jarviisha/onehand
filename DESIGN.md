# DESIGN.md: UI overview

The whole-app UI contract for `onehand`: how the window is laid out and the rules every view
follows. It describes structure and behaviour, not values. It is binding at the level written here;
below it, the code and its tests decide. A feature's own document (`docs/tasks.md`,
`docs/workflows.md`, `docs/unattended.md`) holds the detail of its screens. Code never cites this file; a comment gives its reason in its own words.

**No palette lives here.** onehand uses gpui-component's theme, plus one surface-ramp override
installed at boot (`crate::theme::install`, which carries contrast tests). Every colour, radius and
size is read from `cx.theme()` at the call site. A hex literal in the render layer is a bug even
when it looks right, because a theme switch cannot reach it.

## Principles

1. **The conversation is the centre while a session shows.** It is then the only region that
   flexes. The Workbench and terminal are docks that start closed, open on demand, and never crowd
   the conversation. Pages take the agent pane and put the docks away. A page never opens over a
   session by itself; only a person picking it does.
2. **Separate by hairline, not shadow.** A 1px `border` separates panels. Shadows are only for
   surfaces that really float: dialogs, popovers, the composer popup, and what floats over the
   transcript (the composer card, the cards pinned above it, the attachment tray and the
   to-bottom button).
3. **Colour means state.** One accent, from the theme. Danger, warning and success ink mark
   failure, in-flight and done. Anything else is `muted_foreground`, or the transcript's
   `meta_ink`, one step nearer full ink.
4. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
5. **Icons are registry SVGs, never glyphs.**
6. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
7. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.

## Layout

One window hosts one workspace. The Workbench runs the full height; the terminal sits under the
agent pane only. The diagram uses ASCII and box-drawing characters only, so it stays aligned in any
monospace font.

```
┌──────────────────┬────────────────────────────────────┬──────────────────┐
│ Workspace name   │ Title (...)    past close term wb  │ modes   max hide │
│ Add project...   │                                    │                  │
│ Overview         │ workflow > step > step   Stop      │                  │
│ Tasks         2  │                                    │                  │
│ Issues           │                                    │                  │
│ [+ New session v]│                                    │    Workbench     │
│ ──────────────── │          agent pane                │  (right dock,    │
│ Projects | All   │        (centre, flexes)            │   full height)   │
│ > project  main  │                                    │                  │
│     session      │  ┌──────────── composer ──────┐    │                  │
│     session      │  │ +  Fast  model        Send │    │                  │
│                  │  └────────────────────────────┘    │                  │
│                  │    branch                mode      │                  │
│                  ├────────────────────────────────────┤                  │
│                  │ shell tabs  +                      │                  │
│ Settings         │ terminal (bottom dock)             │                  │
└──────────────────┴────────────────────────────────────┴──────────────────┘
        rail                     DockArea
```

- **Rail.** App chrome outside the dock, so a layout restore cannot lose it. Top to bottom:
  - the workspace row, which opens the workspace switcher;
  - *Add project…*;
  - *Workspace overview*, highlighted while that page shows;
  - *Tasks*, highlighted the same way, with a count pill of the tasks that need attention (no pill
    at zero);
  - *Issues*, highlighted the same way, with no count (*Tasks* already counts what needs a
    person);
  - *New session*, a filled split button whose caret picks the project and agent (shown only when
    there is a choice);
  - a hairline;
  - a *Projects | All sessions* switch;
  - the scrolling tree, or under *All sessions* every session in the order it was made: rows
    that cannot be dragged, each with its project as a muted footnote;
  - *Settings* in the footer.

  A project row carries its folder icon and name, then any of: a pin, the branch (selected row
  only), the change count, an unattended-run pill (*auto*, *auto · N*, *auto · N waiting*), the
  most urgent session mark (a run's pill opens its issue on the Issues page), the ellipsis menu (selected row only; every row has it on right-click)
  and the fold chevron, which alone folds. A session row carries its name, its mark only when it
  has one, and a muted agent footnote only when the project's sessions use different agents
  and the conversation has a title.
  The order of the tree is the user's, set by dragging. Pinned projects stay on top, and
  sessions never leave their project. `Ctrl+Shift+B` hides the rail completely; it never
  collapses to an icon column. It resizes between 232 and 320px. It is the one panel drawn on
  the ramp's lifted surface.
- **Agent pane header**, left to right:
  - the show-rail button, only while the rail is hidden;
  - the conversation's name, semibold, full ink;
  - a dots menu: *Rename…* and *Export as Markdown…*; then *Resume in this session…* (refused
    mid-turn) and *Restart the agent*; then *Delete conversation* in the danger tint (refused
    until the first turn has ended);
  - a spacer;
  - past conversations and *Close session*, only while a session shows;
  - the terminal, with a dot while a shell is alive;
  - always last, the Workbench.

  On the workspace overview, the Tasks page and the Issues page the header reads *Workspace*,
  *Tasks* or *Issues*, with no dots menu and no dock buttons.

  When the pane narrows, the name gives way first, down to a minimum width; the controls keep
  their size. Without a session, the row names the project, and its dots menu holds the project's
  actions except *New session* and *Open terminal*, which the page and the header already offer.
- **Step strip.** Under the header, only while a run drives the connected session on screen: the workflow's
  name muted, then each step's label with a chevron between them (done steps carry a muted check,
  the current one is in full ink and weight), then at the far end *Review…*, *Revise…* and
  *Continue* (the one primary) while the run waits for approval, and *Stop* always. The step
  labels are clipped by width; the workflow's name and the controls never are. *Review…* reads the answer from the run, not the
  transcript. *Revise…* asks for a note and refuses an empty one. Each press carries the visit it
  was drawn from; one the run no longer waits at shows the new answer, saying it changed, in the
  warning ink, rather than approving it unread.
- **Composer.** A card at the foot of the transcript. Inside it, one row: the `+` menu, *Fast*,
  the model chip (those two only when the agent offers them), and *Send* or *Stop*, with *Queue*
  beside *Stop* while a turn runs and there is a draft. Under the card, outside it, a strip shows standing state:
  the project's branch on the left and the turn's permission mode on the right; both open a
  menu. If neither exists, the strip is not drawn. *Run a workflow…* is the `+` menu's last entry,
  below a separator; it and the keymap command open one launcher: a workflow menu, what the
  workflow does and where it works, a collapsed *Preview* (steps, limits and the first prompt,
  bounded), then *Title*, *Details* and *Instructions*. What the preflight found is listed under
  them, blocks in the danger ink and the rest muted, and *Run* is spent while a block remains, saying
  how many beside it; the form scrolls so the footer never leaves the screen.
  *Run workflow…* on an issue opens a start form of its own on that issue, with no row to pick:
  the workflow menu and what it does, where it works (branch, base and agent), *Before it starts*
  (the preflight's findings, as in the launcher), *Instructions for this run*, the limits on one
  line, then the same collapsed *Preview* without them; *Run* is the footer's primary action. Once
  started the dialog closes and the person stays on the issue.
- **Docks.** Bare panels with strips of their own and no library tab bars.
  - The Workbench strip has the mode chips (Editor, Markdown, Neovim, Issues, Plugins), then
    maximize, then hide.
  - The terminal strip has its shell tabs and `+`, then the same maximize and hide.
  - Each dock is a card, inset on three sides and flush on the side it is dragged by, on the same
    reading surface as the conversation.
  - The Issues tab's issue is drawn in one fixed order whatever the state, so what it waits on
    comes before what it says: its title with its state (*Open*, *Closed*) beside it and nowhere
    else, and its facts; then where its work stands (the newest task's progress with *step N of
    M*, the next action in one sentence, at most one primary action, the secondary ones in a
    place that does not move, then ⋯); then its body; then what the work left (the branch and,
    on a project a forge serves, the pull request's state with *read 2m ago* and *Refresh*);
    then *Before*, its earlier runs and tasks (capped); then its history. The work's three lines
    keep their height while a step ends, so the body never moves under a reader. The tab is the
    glance beside a session; ⋯ ▸ *Open in Issues* opens the same issue on the Issues page, its
    filters left as they are. The tab draws no review: its *Review…* opens the issue on the page
    with the review block open.
  - Hiding a dock keeps its buffers and processes.
  - A hidden terminal is unmounted and takes no room.
  - The terminal's open state follows the selected project.
- **Pages without a session.**
  - A project with no session shows *New session*, *Run check* when the project has a check
    command, one line counting its tasks that need attention, run or wait (not drawn at zero, and
    opening the Tasks page narrowed to the project), and its past conversations: capped,
    scrolling, each with a *Delete* word.
  - *Workspace overview* shows, across all projects, the cards *Waiting on you* (runs waiting
    for an answer or an approval, and sessions waiting, finished or lost) and *Working*, then
    *Projects* as a grid of project tiles (not drawn without projects), then the cards *Recent
    conversations* and *Open issues*. While it shows, the header reads
    *Workspace*, and both docks are put away.
  - *Tasks* is the third page, left like the overview and drawn in its column: one column of four
    cards, *Needs attention* (with the project filter), *Running*, *Queued* and *Finished*, each
    with a count, an empty line and a cap. A row is the task's title over a muted line (workflow,
    step or outcome, project) with ghost actions at its end: *Open session*, *Stop*, *Resume*,
    *Retry*, *Dismiss*, by state. Pressing a row's text opens the task's detail in the same column.
  - *Issues* lists the issues of every project of the workspace, with the one picked beside the
    list, or alone under *Back* when the page is too narrow for both. Above the list: the search and
    *New issue* (in the project filtered to, else the one picked, its form saying which; while its
    body is empty a *Template* row offers *Bug*, *Feature* and *Refactor*, or the project's own
    templates when it keeps any),
    an *Open N | Closed N* switch, then the progress (*All*, *Needs attention*, *Running*,
    *Queued*, *Pull request open*, *No run recorded*), project and label filters, how old the
    pull request reading is and *Refresh*. A row is the title over a muted line (project,
    reference), its line of work in the Tasks page's words (the warning ink only for what needs
    the person, none for *No run recorded*) and the labels that fit with a count. The list never
    moves under a person: a row keeps its place while its run changes, one that stops matching
    stays saying *now …* until another issue is picked, new matches go below, and an issue
    opened from elsewhere that the filters leave out is pinned on top, *Outside current
    filters*, with *Clear filters*. The issue is the tab's order with the steps still to come
    under its progress, and in *What the work left* the check, the files this run and the branch
    changed (each opening its diff in place) and the commits past where the task started. The
    page keeps its filters, search, selection and scroll while a session or the task detail is
    looked at.
  - The **review block** opens below where the work stands only when a person presses *Review…*,
    pushing the body down; a run reaching an approval never opens it, and it closes only by its
    *Close* or by picking another issue. In a hairline box that scrolls past its cap: *Review:
    <step>*, the answer (its last 60 lines with *Show all N lines*), what the step under review
    changed with each file opening its diff (drawn only when it changed something), the check
    when a command ran since (passed or failed, with its last 20 lines in a mono well), the
    issue's *Acceptance* collapsed, then
    *Revise…* and *Continue* (the one primary), each beside what it starts: *Plan runs again
    with your note*, *Continue starts Implement: the agent changes the code*. A cut answer says
    *Showing the last 60 of N lines.* above them in the warning ink. *Revise…* writes its note in
    the block. A press shows at once that it was sent, then what came of it (*The run moved on
    to …*), without moving the reader's scroll; one the run no longer waits for reloads the
    block, saying *The answer changed since you opened it.*
  - The task detail's *Awaiting approval* draws the same answer, what each answer starts, and
    *Review…*, *Revise…* and *Continue* through the same guarded call as the strip.
    An ended task's detail draws *Way out*: why it ended in the issue's words, and *Retry with
    current settings…* when that fits, primary for a configuration failure.
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast. Modals are
  `Dialog`s. Settings is a large dialog: a nav column with Appearance, Workspace, Agents,
  Connections, Workflows and Shortcuts. Groups are separated by hairlines, not boxes.
  Workflows lists the workflows, capped and saying how many it left out (shipped ones tagged
  *Built in* and read-only). *New workflow* or *Edit* opens a form below in the agent form's
  shape, one hairline box per step; its problems are listed above *Save*, which
  stays spent while any remain, and the project check commands close the page.
- **Persistence.** The layout is saved as the Workbench width, terminal height and rail width,
  plus whether each dock is open. It is never saved as the library's `DockAreaState`.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, sized so 100 mono columns of a diff fit inside a card, and
  narrowed on small panels. The composer, and anything pinned above it (permission, question,
  queued prompt, an adapter still connecting), are capped narrower and read as one stack.
- **One turn, many blocks**: prose, thoughts, plans, activity rows and clusters, commands,
  diffs, permission and question cards, notices and errors. A workflow's steps reach the
  transcript as notices. Long output folds and is capped. Content
  wider than its well scrolls inside the well.
- **Destructive actions are words in the danger tint**, confirmed through a modal that names the
  thing being removed, never through a button that arms on first press.

## Typography and spacing

| Role | How to write it |
|---|---|
| Body | the inherited size, never set; the transcript reads one step under it |
| Chrome (a panel's rows, cards, controls) | `.text_sm()` |
| Titles | `.font_semibold()` (or `.font_medium()` for a page title) at the size of what they title |
| Meta, hints | `.text_xs()` + `muted_foreground` |
| Machine text | `mono_font_family` |

- **Sizes are rems, never pixels**, because per-panel zoom overrides the rem base. Fixed chrome
  heights stay outside the zoom wrapper. Two exceptions are pixels on purpose: the settings
  dialog's bounds, which are measured against the window, and a menu row's inset, which cancels
  one the library draws in pixels.
- Spacing uses gpui's base-4 scale. Radius comes from `cx.theme().radius` (`radius_lg` for
  cards); `rounded_full` is only for dots and pills.
- Weight carries hierarchy before size does.

## Colour and state

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `muted` / `muted_foreground` | quiet fills, meta text |
| `theme::meta_ink` | the transcript's meta text, contrast-tested |
| `border` | every hairline |
| `ring` | a border marking where the keyboard is: the composer while typing there, a question card's row under the arrow keys |
| `secondary` | the user's prompt bubble |
| `accent` | the one item selected among several |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text |
| `popover` | floating surfaces |

- Cards are borders, not fills.
- No control gets a focus ring: hover and selection are fills, at distinct steps of the ramp.
- One primary per view.
- If a surface is missing, add it to the ramp with its contrast asserted. Never add it in the one
  view that needed it.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, plus gpui's own `list` for the transcript. Buttons go through the app's action
wrapper, which sets the pointer cursor; a control that refuses says so (`resting()` or
`.refuses()`) and goes back to the arrow. The app owns
only what is onehand's own:
- the transcript renderers;
- the icon registry;
- the terminal panel;
- per-panel zoom;
- the composer popup. This is one shell for `@`, `/`, the pickers and the attachment tray, with a
  pinned title and footer, a height capped at twelve rows, grouped rows, and its own scrollbar on its edge.

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a Workbench mode with a
PTY of its own per project, through the shared terminal crate. Files lives inside Editor, not as a
mode. Plugins are built in, because Rust has no stable ABI to load them at run time.

## Icons

Every icon is an SVG from gpui-component's `IconName`. `crate::icons` holds only what that set
cannot draw (shapes it has no drawing of, brand marks, and forks of bundled shapes for stroke
weight), synced from `assets/icons/manifest.toml` by `scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
