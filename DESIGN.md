# DESIGN.md: UI overview

The whole-app UI contract for `onehand`: how the window is laid out and the rules every view
follows. It describes structure and behaviour, not values. It is binding at the level written here;
below it, the code and its tests decide. A feature's own document (`docs/tasks.md`,
`docs/workflows.md`, `docs/unattended.md`) holds the detail of its screens. Code never cites this file; a comment gives its reason in its own words.

**No palette lives here.** onehand uses gpui-component's theme, overridden at boot by the app's
own palette (`crate::theme::install`, which carries contrast tests): neutral greys for every
surface and ink, a hue only for state. Every colour, radius and
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
   failure, in-flight and done; in the rail, warning is what waits on the person and the accent
   what runs. Anything else is `muted_foreground`, or the transcript's
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
- **Agent pane header**, left to right:
  - the show-rail button, only while the rail is hidden;
  - the conversation's name, medium weight, full ink;
  - a dots menu: *Rename…* and *Export as Markdown…*; then *Resume in this session…* (refused
    mid-turn) and *Restart the agent*; then *Delete conversation* in the danger tint (refused
    until the first turn has ended);
  - a spacer;
  - past conversations and *Close session*, only while a session shows;
  - the terminal, with an accent dot while its dock is closed over a live shell;
  - always last, the Workbench. Each of the two is filled while its dock is open.

  On the overview, the Tasks, Issues and Workflows pages it reads the page's name, with no dots
  menu and no dock buttons. When the pane narrows, the name gives way first, down to a minimum
  width. Without a session, the row names the project, and its dots menu holds the project's
  actions except *New session* and *Open terminal*, which the page and the header already offer.
- **Step strip.** Under the header, only while a run drives the session on screen: the workflow's
  name muted, each step's label between chevrons (done ones with a muted check, the current one in
  full ink), then *Review…*, *Revise…* and *Continue* (primary) while it waits for approval, and
  *Stop* always. Only the step labels clip. *Review…* reads the answer from the run; *Revise…*
  refuses an empty note; a press the run no longer waits at shows the new answer, in the warning
  ink, rather than approving it unread.
- **Composer.** A card at the foot of the transcript with no edge, set off by its raised fill and
  its lift; the caret alone shows focus. The field, then one row: the `+` menu, *Fast* (a toggle
  when the agent offers two choices, else a picker) and the model chip with its effort (when
  offered), then *Send*; while a turn runs, *Stop* (solid danger, with its word) and, over a
  draft, *Queue*. Under it, a strip: the branch left, the permission mode right, each opening a
  menu (not drawn when neither exists), the branch over the mode when narrow. Pinned cards share
  one shape: what is asked as the title, who asks, the body, then the keys left and the answers
  right. One is pinned at a time, the oldest, over a line saying how many more wait; the next
  takes its place, and the caret, once it is answered. A question's description is said once. The queue and a reconnect are plain lines. *Run a workflow…*, the `+` menu's last entry, the keymap command and the
  Workflows page's *Run…* open one launcher: a workflow menu, what it does and where, a collapsed
  *Preview* (steps, limits, the first prompt), *Title*, *Details*, *Instructions*, then what the
  preflight found (blocks in the danger ink), *Run* spent while a block remains; it scrolls so the
  footer stays. *Run workflow…* on an issue opens its own start form, no row to pick: the workflow
  and where it works, *Before it starts* (the findings, limits included), *Instructions for this
  run*, the *Preview*; once started the person stays on the issue.
- **Docks.** Bare panels with strips of their own and no library tab bars.
  - The Workbench strip has the mode chips (Editor, Markdown, Neovim, Issues, Plugins), then
    maximize, then hide.
  - The terminal strip has its shell tabs and `+`, then the same maximize and hide.
  - Each dock is a card, inset on three sides and flush on the side it is dragged by, on the same
    reading surface as the conversation.
  - The Issues tab's issue is drawn in one fixed order, what it waits on before what it says: the
    title with its state (*Open*, *Closed*) and its facts; where its work stands (progress with
    *step N of M*, the next action in a sentence, at most one primary, the rest in a place that
    does not move, then ⋯); the body; what the work left (the branch, and the pull request's state
    with *read 2m ago* and *Refresh*); *Before* (capped); its history. Those lines keep their
    height while a step ends. The tab is a glance: ⋯ ▸ *Open in Issues* and its *Review…* open the
    issue on the Issues page, the latter with the review block open.
  - Hiding a dock keeps its buffers and processes.
  - A hidden terminal is unmounted and takes no room.
  - The terminal's open state follows the selected project.
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
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast. Modals are
  `Dialog`s. Settings is a large dialog: a nav column with Appearance, Workspace, Agents,
  Connections and Shortcuts. Groups are separated by hairlines, not boxes.
- **Persistence.** The layout is saved as the Workbench width, terminal height and rail width,
  plus whether each dock is open. It is never saved as the library's `DockAreaState`.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read a step over the chrome. Content
  wider than the column scrolls inside its well. The composer, and anything pinned above it (permission, question,
  queued prompt, an adapter still connecting), are capped narrower and read as one stack.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters, commands, diffs,
  answered permissions and questions, notices and errors. A cluster holds tools only; a thought
  and an answer are lines of their own. A workflow's steps reach the transcript as notices.
- **Folding blocks open from one line**, the chevron first. What runs starts open and closes
  when done, the plan stays open, and a reader's fold wins. Long output folds and is capped.
- **Wells** (code, output, diffs) are filled, with no edge. A finished turn ends on a footer:
  copy its closing paragraph, and how long it took.
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

- **The families ship with the app:** Inter for the interface (the theme's `font_family`) and
  JetBrains Mono for machine text (`mono_font_family`), so text looks and centres the same on every
  machine. `[font].monospace` can still name another installed mono family.

- **Sizes are rems, never pixels**, because per-panel zoom overrides the rem base. Fixed chrome
  heights stay outside the zoom wrapper. Two exceptions are pixels on purpose: the settings
  dialog's bounds, which are measured against the window, and a menu row's inset, which cancels
  one the library draws in pixels.
- Spacing uses gpui's base-4 scale. Radius comes from `cx.theme().radius` (`radius_lg` for
  cards); `rounded_full` is only for dots and pills. A size neither gives is a named constant
  beside the code using it, its reason in its doc comment, never a number at the call site.
- Weight carries hierarchy before size does.

## Colour and state

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `muted` / `muted_foreground` | quiet fills (the prompt bubble, every well), meta text |
| `theme::meta_ink` | the transcript's meta text, contrast-tested |
| `border` | every hairline |
| `ring` | the composer while a file is dragged over it |
| `accent` | the one item selected among several |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text |
| `popover` | floating surfaces |

- Cards are borders; wells and the bubble are fills.
- No control gets a focus ring: hover and selection are fills, at distinct steps of the ramp.
- One primary per view.
- If a surface is missing, add it to the ramp with its contrast asserted. Never add it in the one
  view that needed it.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, plus gpui's own `list` for the transcript. Buttons go through
the app's action wrapper, which sets the pointer cursor; a control that refuses says so
(`resting()` or `.refuses()`) and goes back to the arrow. The app owns only what is its own: the
transcript renderers, the icon registry, the terminal panel, per-panel zoom, and the composer
popup (one shell for `@`, `/`, the pickers, the `+` and branch menus and the attachment tray, with
a pinned title and a footer of key caps, at most six rows, grouped, then how many more; a menu
opens under its chip).

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a Workbench mode with a
PTY of its own per project, through the shared terminal crate. Files lives inside Editor, not as a
mode. Plugins are built in, because Rust has no stable ABI to load them at run time.

## Icons

Every icon is an SVG from gpui-component's `IconName`. `crate::icons` holds only what that set
cannot draw (shapes it has no drawing of, brand marks, and forks of bundled shapes for stroke
weight), synced from `assets/icons/manifest.toml` by `scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
