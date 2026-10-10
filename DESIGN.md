# DESIGN.md: UI overview

The whole-app UI contract for `onehand`: how the window is laid out and the rules every view
follows, as structure and behaviour, not values. It binds at the level written here; below it,
the code and its tests decide. A feature's own document (`docs/tasks.md`, `docs/workflows.md`,
`docs/unattended.md`) holds its screens' detail. Code never cites this file.

**No palette lives here.** gpui-component's theme, overridden at boot by the app's palette
(`crate::theme::install`, with its contrast tests): neutral greys for every surface and ink, a
hue only for state. Every colour, radius and size is read from `cx.theme()` at the call site; a
hex literal in the render layer is a bug even when it looks right, as no theme switch reaches it.

## Principles

1. **The conversation is the centre while a session shows**, the only region that flexes. Docks
   start closed and open on demand. Beside the Workbench it keeps a minimum, times its zoom; too
   narrow for both, the Workbench takes the area under *← Conversation*, which steps it aside. Pages
   take the agent pane and put the docks away; a page opens over a session only when picked.
2. **Separate by surface and hairline, not shadow.** Docks part from the conversation by their
   surface, rows and cards by a 1px `border`. Shadows only for what floats: dialogs, popovers,
   menus, and over the transcript the composer card, the cards pinned above it, the to-bottom pill.
3. **Colour means state.** One accent, from the theme. Danger, warning and success ink mark
   failure, in-flight and done; in the rail, warning waits on the person and the accent runs.
   Anything else is `muted_foreground`, or the transcript's `meta_ink`, a step nearer full ink.
4. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
5. **Icons are registry SVGs, never glyphs.**
6. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
7. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.

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
│         Settings │ terminal (bottom dock)             │                  │
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
  list, ending on *Add project…*; a foot with *Settings*, marked while it shows.

  *By project* lists pinned projects first, a gap between projects. A project row: the fold
  chevron, folder and name, its git line (branch, uncommitted changes, commits ahead and behind,
  each only above zero and named in full on hover), the pin, the unattended pill (*auto*, *auto ·
  N*, *auto · N waiting*; a run's pill opens its issue on the Issues page), and while folded its
  sessions' most urgent state. Its chevron alone folds it (not during a search); the rest of the
  row shows the project's last session, or its page. `+` and `⋯` show on hover, right-click opens
  the same menu. A session row, indented under its project: its state mark, title and `⋯` (on
  hover, always on the one shown), and under them `state · agent · age` (*waiting Nm* while it
  needs input), over which hovering lays *Stop* (running), *Send the last prompt again* (failed)
  and *Close*. The other filters are flat, the project in place of the agent. A group lists six
  sessions, then *N more*; an open project with none says *Empty*, a search or filter with none *No
  sessions match*. Each state has its own ink and, but for failed and disconnected, its own
  mark, none animated (running is a filled dot): failed or disconnected danger, needs input warning, running the accent, done unread success, idle
  muted. `↑`/`↓` move the list's row, `Enter` and `←`/`→` act on it, `Alt+↑`/`↓` step through
  sessions in its order. The tree's order is the user's, set by dragging; pinned projects stay on
  top and sessions never leave their project. `Ctrl+Shift+B` hides the rail completely, never to an
  icon column. It resizes between 232 and 448px, on the ramp's lifted surface.
- **Agent pane header**, left to right: the show-rail button (only while the rail is hidden); the
  conversation's name, medium weight; a dots menu (*Rename…*, *Export as Markdown…*; *Resume in
  this session…*, refused mid-turn, and *Restart the agent*; *Delete conversation* in the danger
  tint, refused until the first turn ends); a spacer; past conversations and *Close session*
  (only while a session shows); the terminal, an accent dot on it while its dock is closed over a
  live shell; last, the Workbench. The two dock buttons are filled while their dock is open. On
  the overview, Tasks, Issues and Workflows it reads the page's name, with no dots menu and no
  dock buttons. As the pane narrows, the name gives way first, down to a minimum. Without a
  session it names the project, its dots menu holding the project's actions but *New session*
  and *Open terminal*, which the page and the header already offer.
- **Step strip.** Under the header, only while a run drives the session on screen: the workflow's
  name muted, each step's label between chevrons (done ones with a muted check, the current one in
  full ink), then *Review…*, *Revise…* and *Continue* (primary) while it waits for approval, and
  *Stop* always. Only the step labels clip. *Review…* reads the answer from the run; *Revise…*
  refuses an empty note; a press the run no longer waits at shows the new answer, in the warning
  ink, rather than approving it unread.
- **Composer.** A card with no edge, set off by its raised fill and lift; the caret alone shows
  focus, and no line is ever added to it (a refusal is Send's tooltip, a failure a toast). The
  field, then the `+` menu, *Fast* (a toggle when its choices read as on and off), the model and
  effort chips, then *Send*, or *Stop* (solid danger, worded) and *Queue* over a draft. Under it
  the branch and the mode, stacked when narrow. Each chip's menu opens just above it, without
  its tooltip. Above the card: one pinned permission or question, the oldest, with how many more
  wait; then the queue and a reconnect as plain lines; then the staged files as one row of chips,
  faded where cut beside *Show all N*. *Run a workflow…* (the `+` menu, the keymap, the Workflows
  page's *Run…*) opens the launcher, and an issue's *Run workflow…* its start form; both are
  drawn as `docs/workflows.md` describes.
- **Docks.** Bare panels on the dock surface with strips of their own, no library tab bars; the
  terminal is a block held off its neighbours by a gap.
  - Strips as tall as the header. The Workbench's: *← Conversation* while it has the area, the
    modes (Editor, Markdown, Issues, Plugins, Neovim; a select when narrow), maximize, hide. The
    terminal's: shell tabs (capped, scrolling), `+`, maximize, hide. Crowded file tabs: a select.
  - A list beside its detail (files, documents, issues) only while the dock holds both; else one at
    a time, the detail under a link back naming the list. Picking shows it; going back keeps it.
  - The Issues tab's issue is drawn in one fixed order, what it waits on before what it says: the
    title over its facts, *Open* or *Closed* first; where its work stands (progress with
    *step N of M*, the next action in a sentence, at most one primary, the rest in a place that
    does not move, then ⋯); the body; what the work left (the branch, and the pull request's state
    with *read 2m ago* and *Refresh*); *Before* (capped); its history, each line keeping its height
    while a step ends. ⋯ ▸ *Open in Issues* and *Review…* open it on the Issues page, the latter
    with its review block open.
  - Hiding a dock keeps its buffers and processes; the terminal's open state follows the project.
- **Pages without a session.**
  - A project with no session shows *New session*, *Run check* when the project has a check
    command, its *Check command* field, one line counting its tasks that need attention, run or
    wait (not drawn at zero, and opening the Tasks page narrowed to the project), and its past
    conversations: capped, scrolling, each with a *Delete* word.
  - *Workspace overview* shows, across all projects, the cards *Waiting on you* (runs waiting
    for an answer or an approval, and sessions waiting, finished or lost) and *Working*, then
    *Projects* as a grid of tiles (not drawn without projects), then *Recent conversations* and
    *Open issues*.
  - *Tasks*: four cards, *Needs attention* (with the project filter), *Running*, *Queued*,
    *Finished*, each with a count, an empty line and a cap. A row is the title over workflow, step
    or outcome and project, with ghost actions by state (*Open session*, *Stop*, *Resume*, *Retry*,
    *Dismiss*); its text opens the task's detail in the same column.
  - *Issues* lists every project's issues, the one picked beside the list (alone under *Back* when
    narrow). Above the list: the search, *New issue* (its *Template* row, while the body is empty:
    *Bug*, *Feature*, *Refactor*, or the project's own), *Open N | Closed N*, the progress filter
    (*All*, *Needs attention*, *Running*, *Queued*, *Pull request open*, *No run recorded*),
    project and label filters, the pull request reading's age and *Refresh*. A row: the title over
    project and reference, its line of work (warning ink only for what needs the person), its
    labels. The list never moves under a person: a row keeps its place, one that stops matching
    says *now …* until another is picked, new matches go below, and an issue opened from elsewhere
    outside the filters is pinned on top with *Clear filters*. The issue is the tab's order plus
    the steps to come, and *What the work left* with the check, the files changed (diffs in place)
    and the commits. Filters, search, selection and scroll survive a look elsewhere.
  - The **review block** opens below where the work stands only on *Review…*, never by itself,
    and closes only by *Close* or another issue. A hairline box scrolling past its cap: *Review:
    <step>*, the answer (last 60 lines, *Show all N lines*), the files the step changed (each a
    diff), the check when a command ran since (last 20 lines in a mono well), *Acceptance*
    collapsed, then *Revise…* and *Continue* (primary), each beside what it starts. A cut answer
    says so above them in the warning ink. A press says it was sent, then what came of it,
    keeping the scroll; one the run no longer waits for reloads, saying the answer changed.
  - The task detail's *Awaiting approval* draws the same answer and actions through the strip's
    guarded call; an ended task's *Way out* says why in the issue's words, with *Retry with
    current settings…* when it fits. A merged worktree's *Remove worktree…* is a danger word.
  - *Workflows* lists the workflows, capped (shipped ones *Built in*, read-only), each with
    *Run…* (projects, the rail's first). *New workflow* or *Edit* opens a form below, one hairline
    box per step, a margin marking where a failure goes back to, problems above a spent *Save*.
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast; modals are
  `Dialog`s. Settings is a large dialog: a nav column with Appearance, Workspace, Agents,
  Connections and Shortcuts, its groups split by hairlines, not boxes.
- **Persistence.** The layout is saved as the Workbench's dragged width, the terminal and rail
  sizes and whether each dock is open, never as the library's `DockAreaState`.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read a step over the chrome; what is
  wider scrolls inside its well. A scrollbar runs on the panel's edge down to the composer.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters (tools only), commands,
  diffs, answered permissions and questions, notices and errors. A workflow's steps and an
  interrupted turn are notices, never prompts.
- **Folding blocks open from one line**, the chevron first: what runs is open until done, the
  plan stays open, and a reader's fold wins. Long output folds and is capped. Wells are filled
  with no edge. A finished turn ends on Copy (its closing paragraph) and how long it took.
- **Destructive actions are words in the danger tint**, confirmed through a modal that names the
  thing being removed, never through a button that arms on first press.

## Typography and spacing

| Role | How to write it |
|---|---|
| Body | the inherited size, never set; the transcript and the composer's field read a step over it |
| Chrome (a panel's rows, cards, controls) | `.text_sm()` |
| Titles | `.font_semibold()` (or `.font_medium()` for a page title) at the size of what they title |
| Meta, hints | `.text_xs()` + `muted_foreground` |
| Machine text | `mono_font_family` |

- **The families ship with the app:** Inter for the interface (the theme's `font_family`) and
  JetBrains Mono for machine text (`mono_font_family`), so text looks and centres the same on every
  machine. `[font].monospace` can still name another installed mono family.
- **Sizes are rems, never pixels**, because per-panel zoom overrides the rem base. Fixed chrome
  heights stay outside the zoom wrapper. Three exceptions are pixels on purpose: the settings
  dialog's bounds, which are measured against the window, a menu row's inset, which cancels
  one the library draws in pixels, and the composer's drop ring, which is a line and not a size.
- Spacing uses gpui's base-4 scale. Radius comes from `cx.theme().radius` (`radius_lg` for
  cards); `rounded_full` is only for dots and pills. A size neither gives is a named constant
  beside the code using it, its reason in its doc comment, never a number at the call site.
- Weight carries hierarchy before size does.

## Colour and state

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `theme::dock_surface` | the Workbench and the terminal, a step off the reading surface |
| `muted` / `muted_foreground` | quiet fills (the prompt bubble, every well), meta text |
| `theme::meta_ink` | the transcript's meta text, contrast-tested |
| `border` | every hairline |
| `ring` | a border marking where the keyboard is: the composer while a file is dragged over it, a question card's row under the arrow keys |
| `accent` | the one item selected among several |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text |
| `popover` | floating surfaces |
| `theme::raised` | the composer card, which stands on its fill and lift instead of an edge |
| `secondary` | inline code in the prompt bubble, a step off its fill; a document's code on a dock |

- Cards are borders, the composer aside; wells and the bubble are fills.
- No control gets a focus ring: hover and selection are fills, at distinct steps of the ramp.
- One primary per view.
- A missing surface goes into the ramp with its contrast asserted, never into one view.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, plus gpui's own `list` for the transcript. Buttons go through
the app's action wrapper, which sets the pointer cursor; a control that refuses says so
(`resting()` or `.refuses()`) and goes back to the arrow. The app owns only what is its own: the
transcript renderers, the icon registry, the terminal panel, per-panel zoom, and the composer
popup (one shell for `@`, `/`, the pickers, the `+` and branch menus and the attachment tray, with
a pinned title and no key hints, at most six rows, grouped, then how many more; a menu
opens just above the control that opened it).

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a mode with a PTY of
its own per project. Files lives inside Editor, not as a mode. Plugins are built in.

## Icons

Every icon is an SVG from gpui-component's `IconName`. `crate::icons` holds only what that set
cannot draw (shapes it has no drawing of, brand marks, and forks of bundled shapes for stroke
weight), synced from `assets/icons/manifest.toml` by `scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
