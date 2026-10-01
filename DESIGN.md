# DESIGN.md — UI constraints

The whole-app visual contract for `onehand`, held to by the render layer. The
traffic runs one way: this file points at code, and code never points back — a
source comment states its reason in its own words rather than naming a section
here, and a test enforces it. The transcript's own design language is
[DESIGN-ANSWER.md](DESIGN-ANSWER.md).

> **This file no longer carries a palette.** Until the GPUI migration it mirrored
> a hand-built token set into `theme.rs`, and the two had to be kept in sync by
> hand. Decision **D1** (DECISIONS.md) ended that: onehand uses
> gpui-component's theme with one surface-ramp override. So the rule here is not
> "these are the values"
> but **"never write a value"** — every color, radius and font size is read from
> `cx.theme()` at the call site. A hex literal in the render layer is a bug even
> when it looks right, because it is the one thing a theme switch cannot reach.

Guiding principles:

1. **Separate by hairline, not shadow.** Panels split from their neighbours by a
   1px `cx.theme().border`. Shadows belong to genuinely *floating* surfaces —
   dialogs, popovers, the completion popup.
2. **Chat is the centre.** The conversation is the dock's centre panel and the
   only region that flexes. Workbench and terminal are docks: closed by default,
   opened on demand, never crowding the conversation.
3. **Accent restraint.** One accent, from the theme. Semantic color marks
   *state* — adaptive danger ink for failure, warning ink for in-flight, and
   success ink for done.
   If a color is not carrying meaning, it is `muted_foreground`.
4. **Mono for machines, sans for people.** Code, paths, terminal output and
   diffs use `cx.theme().mono_font_family`; everything a human wrote is the
   default family.
5. **Icons are registry SVGs, never glyphs.** No `＋`, `●`, `✓`, `×`, `❯`, `⚙` in
   rendered UI (§6).

---

## §1 — Layout

One window hosts exactly one workspace. The frame is a navigation **rail** plus a
**`DockArea`**:

```
┌────────────────┬──────────────────────────────┬───────────────┐
│ workspace      │ title ⋮            ⌕ ⍈ ▤ ▣   │               │
│ + New session  │                              │   Workbench   │
│                │        agent pane            │  (right dock, │
│ PROJECTS       │      (centre panel)          │   closed by   │
│  project       │                              │    default)   │
│   session      │      ┌── composer ──┐        │               │
│   session      │      └──────────────┘        │               │
│                │       ⑂ main    ▣ mode       │               │
│                ├──────────────────────────────┴───────────────┤
│ ⚙ settings     │        terminal (bottom dock, closed)        │
└────────────────┴──────────────────────────────────────────────┘
     rail                        DockArea
```

- The **agent pane's header** is the row above the transcript, and it is split by
  what a control is *about*. **The conversation's name is prose and the
  vertical-dots mark at its end is its menu**: the name is the one thing on the
  row drawn in full ink and weight, and pressing the dots opens everything done
  to the conversation — rename, the exports, resume another, restart, and,
  alone in the danger tint, delete. The menu opens *below* the mark, which is
  the whole of what the move bought: pressing the name opened a list over the
  name, so the one thing the row exists to say was covered by the answer to a
  question about it. What it costs is that the popup's edge now follows the
  title's length instead of standing at the name's start, and that is the
  right way round — a menu belongs under the thing that was pressed. With the
  menu off the name, the name is also free to give way: it truncates while
  the mark never shrinks, so narrowing the panel shortens the name and never
  takes the control. The mark also
  carries a tooltip, which the name-as-button never could: the library builds
  a button's accessible name from its label alone, and the name had to be a
  child to ellipsize at all. **No badge sits beside it.** A badge carrying
  what the session is doing did, and every word of it was said twice —
  connecting, working and awaiting approval all appear on the running line at
  the foot of the transcript, and the rail's row for that session carries the
  same mark for the same condition. A second copy in the one row that never
  scrolls is something permanently on screen restating what is already on
  screen, and it took its room from the name, which is the only thing in the row
  nothing else says. What it cost is the one state neither of those puts into
  words: a lost adapter is now the rail's mark and its tooltip, and not a
  sentence here.
  **No hairline under the row.** A hairline is an edge between two surfaces and
  there are not two here: the header and the transcript are one reading surface,
  and what tells them apart is that one is a row of controls and the other is
  prose — which the muted ink and the spacing already say.
  Every other control is a size up and a tone down — big enough to aim at,
  muted enough not to out-shout the name — and **sits on the side of what it
  acts on**. The way back to a hidden rail is the row's *left* edge, the side
  the rail returns to; filed in the right-hand cluster it had to be found
  rather than reached for. The right-hand end reads outward from the name by
  what each control is about: the past conversations and *Close session*,
  which act on the session the name names, then the terminal and **last the
  Workbench, always** — its dock is the window's right edge, so the outermost
  control moves the outermost panel. That is also why *Close session* stands
  before the docks rather than at the end: the far edge is where a pointer
  drifts, and it is the one control on the row that ends something. The
  vertical dots beside the name are the row's one menu mark, and everything
  behind them is done to the conversation the name beside them is.
  **Narrowing the panel takes the name first and the controls last, and stops
  at a floor.** The name ellipsizes while every control keeps its full size — a
  name half-read still names the conversation, and a reader can finish it in the
  rail or in the menu behind it, where a control pushed off the edge is gone
  with nothing on screen to say it was ever there. But the name keeps a
  minimum, below which nothing more is taken: a name cut to two characters names nothing, so
  past that point the row is simply narrower than its own furniture and the
  controls clip. That is a width at which the panel has stopped being a place a
  conversation is read.
  **The agent pane is mounted as a bare panel, not a tab group**, so this is the
  only chrome it has: one tab that can never gain a sibling is not a tab, it is
  the conversation's own name printed a second time directly above the header
  that says it. Every way back to something the window has put away is therefore
  offered from here. The rail's button appears only while the rail is gone,
  because a button that unhides what is already on screen does nothing; the two
  docks' buttons stay, and each opens or closes immediately, matching the
  visibility shortcuts. No panel in the window
  keeps a tab group; the Workbench's mode strip and the
  terminal's shell strip are each that panel's own chrome.
- **Standing state sits under the composer, outside its card.** A bare strip
  with no chrome of its own carries the project's branch on the left and the
  turn's permission mode on the right. The card is the message being written and
  everything inside it acts on that message; neither of these does — the branch
  holds across every session in the project, and the mode outlives the prompt in
  the field — so a strip resting *under* the card says "this is the standing
  state" where a fourth control inside the row would have said they were part of
  what is being typed. **Left is the project, right is the turn**, which is the
  whole of what says which kind a thing is. Both sides are pressable: the branch
  is a control and not a label, opening the same kind of menu the rail's project
  rows do — switch it, rename it, take it to a worktree — because a word that
  answers *which branch* while refusing *and now what* is the one thing in the
  row that stops short. Where there is neither — a project that is not a
  repository, an agent advertising no modes — the strip is not drawn at all,
  rather than ruled empty.
- **A project with no conversation open gets a page, not a sentence.** Selecting
  a project that has no session — every freshly added one, and any whose last
  session was closed — fills the centre with that project's name, a *New
  session* button, and the conversations already had in it, newest first and
  across every agent. Picking one starts a session on the agent that held it and
  resumes it. The list is capped and says how many older ones it left out, and
  it distinguishes *still looking* from *none yet*: a project of a hundred
  conversations must not be told it has none for the half-second a directory
  read takes. One line of grey text saying *Start a session in X* was the first
  thing a new user saw and the one screen in the app with nothing to press.
  **The list scrolls inside the page; the page never grows past the panel.** The
  column is centred while it fits and bounded by the panel when it does not, and
  only the conversations scroll — *New session* and the count of what was left
  out stay where they are, the first because it is why most people are on this
  page and the second because a page cannot say a bound bit from under the fold.
  Centring an overflowing column spends the overflow at *both* ends, so the rows
  that ran off did so with nothing to scroll them back. The same holds for the
  resume picker, where *Start a new conversation* is the way out and must never
  be the thing that scrolled away.
  Each row also carries the one way to **delete** a conversation, and it is a
  word in the danger tint rather than an icon: everything else this app offers
  can be done again, this cannot, and a destructive control should be read
  rather than recognized. It sits **inside the card**, at the end of the row the
  name is on, so what it acts on is the thing beside it rather than whatever the
  press happened to land nearest. Pressing it opens a **modal that names the
  conversation** and has to be answered — *Keep* plain, *Delete* in the danger
  tint — rather than arming the word and waiting for a second press: an armed
  control looks like one that did nothing, and a user who has looked away comes
  back to a row one accidental press from gone with no warning left on screen.
  The same modal is what the conversation's own title menu opens, so the one
  open in front of the user is not deleted on a lighter guard than the ones
  filed away behind it. The row's own delete is offered here and nowhere else —
  this page is what shows when a project has no session on it, so every row on
  it is a conversation nothing is writing to.
- **The workspace has a page too**, for the question no project page can
  answer: what needs me, and what is there to do, across every project at once.
  It is reached from a muted row in the rail's header, *Workspace overview*,
  between *Add project…* and *New session*, and that row takes the selected
  fill while the page shows. No project or session row is marked meanwhile,
  since the page is about none of them. Any other rail click leaves it, since each one is
  a choice of project or session. **The header stays**, for the way back to a
  hidden rail, but **without the terminal and Workbench buttons**: both docks
  hold one project's things and the page stands on none. Opening the page puts
  both away and their keys do nothing while it shows; leaving it brings each
  back as it was, the terminal as the project arrived at left it. It names the page with the fixed word
  *Workspace* and carries no dots menu, because nothing on the page is one
  thing that could be renamed, exported or removed. Below it the page is
  **cards, not one column**: wider than the project page's column, top-aligned,
  and scrolling as one, since cards that fill in as their reads land would move
  a centred page each time one arrived. Each card is a hairline box (borders,
  not fills) with its title in bold, a muted count beside it, and any control at
  the far end of the title row. Three bands, top to bottom: **Waiting on you**
  and **Working** side by side; then **Projects** as a wrapping grid of tiles;
  then **Recent conversations** and **Open issues** side by side. A pair sits
  side by side where the panel is wide enough and one under the other where it
  is not. The two activity cards are always drawn and say *nothing is waiting*
  or *nothing is running* when empty, so the page keeps one shape; inside a
  card, a row is the issue row's shape — a muted head, the title truncated, a
  muted line at the end — and pressing it goes to what it names.
  - **Waiting on you**: every unattended run standing on a card (head `#N`,
    then the question, then the project), and every session whose rail mark is
    *lost*, *waiting for you* or *finished* (head the rail's own mark, then the
    conversation's name, then the project and the state in the rail's word).
    Pressing a run or session opens it; a run in another window brings that
    window forward.
  - **Working**: every other run, with the issue's title in place of a
    question, and every session with a turn in flight, drawn the same way.
  - **Projects**: one tile per project, a bordered box taking the hover fill:
    a folder icon, the name in bold and, at the far end, the most urgent rail
    mark among its sessions; under it the branch and changed count (or *Not a
    git repository*), then how many sessions it holds and how many issues are
    open (or *Nothing open*, and *Looking for open issues…* while they are
    read). Pressing it selects the project.
  - **Recent conversations**: the newest few past conversations across every
    project, head how long ago, then the title, then the project and the agent.
    One already open in a session is left out, since it is on the rail and a
    second resume would put it in two sessions. Pressing one selects its project
    and starts a session resuming it.
  - **Open issues**: every project's own issues, open only, most recently
    changed first. A row is the picker's row (number muted, title truncated, up
    to three label pills) with the project's name muted at the end, followed by
    the forge's reference where the issue is kept in step with one. Pressing it
    selects that project and opens the Workbench on its Issues mode with that
    issue selected. The one card that grows long, so the one whose list
    scrolls inside it; the cards around it keep their place. The project's own files are the only source: an issue
    brought in from a forge lives in the same file, so it is listed once. A
    **project filter** sits beside the heading as a small menu control reading
    *All projects* or the project picked, and it narrows everything under the
    heading: the list, its cap and the count of closed ones. Under the list a
    muted line says how many closed issues are not listed, because an empty list
    and a list of closed issues are different answers.

  **Bounded and said.** The two activity groups, the projects and the recent
  conversations are each capped, and the issues at a longer list. Each cap, when it bites, adds a muted line saying how
  many were left out. **Three states say themselves.** While the files are read
  the issue group says it is looking, which is different from having none. A
  workspace bound to no storage keeps no issues, and the group says so in the
  Issues mode's words rather than offering an empty list. A file that cannot be
  read is named in the warning ink above the rows the others gave.
- The **rail** is app chrome, not a panel: it lives outside the dock, so the dock
  cannot swallow it and a layout restore cannot lose it. `Ctrl+Shift+B` **hides
  it entirely** — it is never narrowed to an icon column, because at that width
  every project is the same folder icon and the one thing the rail is for
  (which project, which session) is exactly what it can no longer say. The way
  back is the sidebar button in the agent panel's header, offered only while
  the rail is gone. It **is** drag-resizable, between 232 and 320px: narrower
  and its rows say nothing, wider and it is taking the conversation's space to
  show padding. The width is remembered per workspace; whether it is showing is
  not.
- **The order of the rail's tree is the user's, and it is dragged.** A project is
  dropped onto another project and a session onto another session of the same
  project — sessions never leave their project, because a session is an agent
  bound to that project's files. Pinned projects stay above unpinned ones: a drag
  that would cross that line stops at it rather than silently undoing itself,
  since the pin is the stronger statement. The row being aimed at takes the fill
  hover would give it, which is free during a drag because hover is suspended for
  the length of one, and no line is drawn above or below it — a drop lands *at*
  the row it was made on. The project order is remembered per workspace; the
  session order is not, because sessions are not persisted at all. The flat *All
  sessions* list is not draggable: it is in the order the sessions were started,
  which is a fact rather than an arrangement.
- **A project says whether its issues are worked unattended.** The switch is per
  project and off until turned on, because being open in the rail says what
  somebody is working on, not what an agent may push to. It is offered in two
  places that must agree: a checked *Work labelled issues* entry in the
  project's ••• menu (the rail's and the project page's), and a list of
  switches under Settings ▸ Workspace, where every project's answer can be read
  at once. While on, the row carries a pill in the change count's style reading
  **`auto`**, and **`auto · #N`** while a run is working issue N of that
  project. It is a word and not a glyph, because the pill already reads as a
  fact about the project and an icon would be one more shape to learn. It never
  gives way to width, because a permission to push is the worst thing the row
  could quietly hide. The hover says it in full: which label is looked for, or
  which issue is being worked.
- **A switch that is on while nothing can happen says so on the row.** That is
  the one state that looks exactly like working. So when something stops every
  run, or the last look at the project failed, the pill keeps its word and takes
  the **warning ink**, and the hover gives the reason. What stops every run is a
  config that cannot work: no label, an interval that does not parse, a mode the
  agent does not offer. What fails a look is a remote that is not on GitHub, or a
  `gh` that is missing, signed out or not answering. The colour says "look here"
  and the words say what is wrong, so neither carries the message alone. A
  project is looked at the moment it is switched on, when its window opens, and
  on every tick, a run in progress included. The switch is **not offered on a
  run's own worktree**: nobody chose that project, and no run ever searches it.
- **An issue can be picked by hand.** *Work an issue…* sits under *Work labelled
  issues* in both project menus, on repositories that are not a run's own
  worktree. It opens a dialog listing the project's open issues, one row each:
  the number muted, then the title (truncated), up to three labels as pills in
  the change count's style, then *by* its author muted at the end. The author is
  on every row because the issue's text goes to the agent as written. Rows take
  the pointer and the same half-accent hover the Settings nav uses. The list
  scrolls inside a bounded height and says in a muted line when it was cut. While
  the list is being read, the dialog says so where the list will be. Picking
  closes the dialog and puts the run's session on screen, where its transcript
  logs the run as short notices, one fact per line, since a notice is a single
  centred line cut at the column's edge. The session stays when the run ends,
  and its project is kept.
- **The connection runs are made through has a page of its own**, Settings ▸
  Connections: a row per connector reading *Signed in as …* in the success ink,
  or saying in the warning ink what is wrong and what to run about it, with
  *Check again* under the list. Settings ▸ Workspace keeps *Look for an issue
  now* above the switches, which runs the search at once and always answers with a
  notification — including when nothing is switched on, a run is already going,
  something blocks every run, or no labelled issue was found — because a button
  that sometimes does nothing visible reads as broken. When the config
  stops every run, a warning line under the explanation says so before any
  switch is read. Each switch takes the pointer across its own width only, not
  across the empty column beside its name. The servers page lists only the
  connectors the build carries — GitHub alone today — since a row for one
  nothing can use would read as a promise.
- **Everything else is a dock panel**, and the arrangement persists as **five
  values, not the library's `DockAreaState`**: Workbench width, terminal height,
  whether each is open, and the rail's width. `DockAreaState` is serde and would
  be the obvious thing to store, but *restoring* one rebuilds every panel through
  a process-global registry — which would leave the shell holding handles to
  orphans and could not tell two windows' panels apart. The arrangement here is
  fixed by design, so what a user actually changes is those five numbers.
- **Docks open on demand.** Both the Workbench and the terminal start closed.
  Workbench mode shortcuts (`Ctrl+Shift+E / M / N` by default) open and focus
  Editor / Markdown / Neovim without hiding them on a repeated press.
  `Ctrl+Shift+J` toggles the whole Workbench immediately regardless of focus,
  reopening its previous mode. The terminal shortcut and the docks' visibility
  buttons also open or close directly. Hiding preserves buffers and processes.
  Shortcuts are editable in Settings; the displayed keys follow the live map.
- **The terminal's open/closed state belongs to the project, not the window.**
  Its tabs, its shells and its working directory are all per root and none of
  them follow the selection, so a dock left open across a project switch showed
  the arriving project an empty panel where the previous one's shells had been —
  which reads as the terminal having lost them. Switching files the live state
  under the project being left and restores whatever the arriving one was left
  in; a project it has never been opened in gets it closed, because inheriting
  *open* just reproduces the empty panel one project further along.
  **The Workbench deliberately does not follow this rule.** Its state is per root
  too, but every root has a file tree, so an open Workbench after a switch is
  never empty — there is nothing there to misread.
- **A terminal that is not showing occupies nothing.** It is *mounted and
  unmounted*, not opened and closed: a closed bottom dock still draws a strip of
  title bar, because the library puts the button that reopens it there, and this
  terminal has no such button — leaving a bare band of chrome across the bottom
  of every window in every project, naming nothing and reopening nothing. The
  ways back are `` Ctrl+` `` and the terminal button in the agent pane's
  header — which carries a dot while a shell is alive, since a child process
  outliving a closed dock is the one thing the icon cannot say.
- **The terminal has no library tab bar either.** Its several tabs are its own,
  drawn inside the panel with the shell labels, their ✕ and the `+`; a tab group
  around it held one panel that could never gain a sibling and printed
  *Terminal* over the strip that already names every shell. Like the agent pane
  it is a bare `DockItem::panel`.
- **The Workbench has no library tab bar either**, and it is the one where the
  duplication was loudest: a title bar reading *Workbench* sat directly over the
  strip naming its modes, and the panel's own controls were stranded on the
  row that said the least. The strip carries them now — the modes at the left
  end, the maximize and the way out at the right — and the panel is a bare
  `DockItem::panel` like the other two. **A mode is the terminal's own tab chip
  with a word in it**: `accent` and the ink that goes on it for the one showing,
  nothing until the pointer arrives for the rest. It was a library button before,
  which spent `primary` — the fill reserved for the single most important action
  on a screen — on saying which of three views is up, and left two strips an inch
  apart disagreeing about what a selected tab looks like.
- **A dock draws no divider; whatever is inside it draws its own edge.** The
  library paints a permanent hairline down the seam of every resizable split,
  which is a second line beside a panel that already marks its own edge — a seam
  that cannot decide where it is. The resting colour is taken
  off (dragging still paints, which is the one moment the seam is what is being
  looked at). Nothing replaced it at the rail: that seam is a change of surface
  now, so `Sidebar`'s own right border is switched off too. The two docks are
  marked by their cards.
- **The rail is the one panel lifted off the reading surface**, drawn in the
  ramp's well and asking for it by name rather than through the sidebar token,
  which ships with a value of its own and would come up level with the
  conversation. It is the only panel that is not about the work at all — a
  workspace, its projects, its sessions — which is what the step now says. The
  two docks took the same one for a while, and sharing it made lifted mean
  nothing more precise than "not the conversation". **The hairline goes with the
  fill**: the fill is the edge, and a rule beside it is a line drawn along a
  boundary that was not in doubt — safe to say because those two surfaces are
  the ramp's asserted pair, 1.15 apart at worst. Its own **marked row is a ramp
  step of its own**, quieter than any surface pair and lifting rather than
  sinking: the faintest existing step is 1.04 against the rail's surface and the
  reading surface is 1.19, a hole punched through the panel rather than a row
  raised out of it.
- **Both docks are drawn as cards floating in their dock** — inset on every side
  but the seam, one border, one radius, and **the reading surface under them**,
  the same one the conversation is on. The border is the whole of what says
  where a panel begins. They were filled a step off the conversation, which in
  the dark palette made them the *lighter* regions on screen with the
  conversation as the dark gap between them: lighter reads as nearer, so two
  panels about the work were drawn in front of the work, and with both open the
  conversation was the one region nothing had raised. **A smaller step in the
  same direction is not the answer and a step the other way does not exist** —
  halving it measures 1.07 against the reading surface, under the 1.14 floor the
  ramp's tests hold every surface pair to, and going down instead would need a
  value below a near-black surface. What the flip gives back is the well
  *inside* a panel: a hover fill or a code block sunk into a dock is the well
  again, where on a filled card it had to borrow the reading surface to be seen.
  **The seam is flush and the rest are inset** — the Workbench's left edge, the
  terminal's top, each being the edge that dock is dragged by: the resize grip is
  a fixed band a few pixels either side of the dock's own edge, so a card held
  off there leaves the one line a user reads as draggable outside the only place
  a drag is taken, and the panel is resized from a strip of apparently empty
  surface. Flush, the border is the grip. The gap on the other three belongs to
  the panel — a click in it is a click on that panel, and it is what says a dock
  is something put down on the window rather than a piece of it. The terminal is
  the one this costs something: its grid measures its own bounds and resizes the
  PTY to match, so the inset is a column of cells and half a row. Paid once
  rather than growing with the panel.
- **A terminal grid is drawn in the surface of the panel holding it**, in the
  dock and in the Neovim mode alike, so a shell is that panel rather than a plate
  laid on it. It has to be *told* which surface that is — a grid fills every cell
  it was not told otherwise about with its palette's default background — so the
  panel passes it in rather than the palette reading the theme and hoping the two
  agree.
- **Maximize has one direction.** `Ctrl+Shift+K` fills the frame and hides the
  rail, and each dock's strip carries a button for the same thing on that panel,
  named rather than focused: a control sitting inside the terminal cannot blow
  up the conversation because that is where the caret happened to be. The
  dock-only zoom the library draws in a tab bar is gone with the tab bars, and
  nothing is lost by it — the conversation already fills everything right of the
  rail whenever both docks are closed.
- **No global top bar and no right toolbar.** Transient status is a toast;
  modals are `Dialog`s. **Settings is a roomy one**, up to 960 × 680 and never
  past the window, with its nav column on the left and a ✕ in its corner,
  beside which the last write made from the page says *Saved* or why not. Inside it is
  one surface: no border beside the nav, no header bar, no box around a group
  — a group is a heading with a hairline above it, and a setting stacks its
  name, a line about it and a full-width control.
- **No status bar either.** There was one — a row under the rail and the dock
  reading out the project, its branch, the running agent, unsaved buffers and any
  panel left off 100% — and it is gone. Every fact on it was either already said
  by something nearer to what it was about (the project and its branch by the
  rail row that names them, the agent's condition by the same mark on that row
  and in the conversation's own header) or was chrome reporting on chrome. What it
  cost was a permanent strip across the bottom of every window in every project.
  **The one thing that went with it** is the away switch, which had no other home
  at the keyboard; `/away` and `/here` over the remote bridge still set it.

---

## §2 — Typography

Two families, both from the theme: the default UI family, and
`cx.theme().mono_font_family` for anything a machine produced.

| Role | How to write it |
|------|-----------------|
| Body / prose | the inherited size — do not set one |
| Chrome — a panel's own rows, cards and controls | `.text_sm()` |
| Headings, titles | `.font_semibold()`, at the size of whatever they title |
| Meta, status, hints | `.text_xs()` + `.text_color(cx.theme().muted_foreground)` |
| Code, diffs, paths, terminal | `.font_family(cx.theme().mono_font_family.clone())` |

**A title is its body's size in bold, not a size of its own.** Weight is what
separates a name from the thing it names; a title that also steps up is two
signals for one distinction, and it is how a ladder grows a rung every time
someone needs a heading to feel slightly more important than the last one.

**Sizes are rems, never pixels.** This is what makes per-panel zoom work: zoom
overrides the *rem base* for one panel's subtree (`crate::zoom`), so everything
sized in rems scales together and a `px(13.)` written by hand does not. A fixed
pixel size is how a panel ends up with one label stranded at its original size
beside doubled body text.

**A borrowed component's pixel size is the same bug arriving from outside.**
gpui-component sizes some of what it draws from `Theme::mono_font_size`, which
is pixels, so those parts sit still while the panel around them zooms. Where
the component takes a style refinement, the fix is a rem size written at the
call site — the refinement is applied after the component's own. Where it takes
none, the size has to be handed in from the current rem size at render time.

Weight carries hierarchy before size does. Three sizes and two weights read as
one system; five sizes read as an accident.

---

## §3 — Dimensions & spacing

- **Spacing is gpui's base-4 scale** — `p_1` `p_2` `p_3` `p_4`, `gap_1` … Snap to
  it; a one-off `px(7.)` is noise no one will ever notice missing.
- **Radius is `cx.theme().radius`**, and its derivations for larger surfaces.
  Circles (`rounded_full`) are for avatars, status dots and true pills only.
- **Hairlines are `border_1` + `cx.theme().border`.** Not a shade of the
  background, not a shadow.
- Fixed chrome heights (tab strips, headers) stay outside the zoom wrapper, so
  they hold still while content scales.

---

## §4 — Color & state

Read the theme. The tokens this app leans on:

| Token | Use |
|-------|-----|
| `background` / `foreground` | the surface and its text |
| `muted` / `muted_foreground` | quiet fills; meta text, descriptors, hints |
| `border` | every hairline — the primary separator |
| `accent` / `accent_foreground` | the one item selected among several |
| `list_hover` | hover on a row or chip that is there to be picked |
| `primary` / `primary_hover` / `primary_foreground` | the single primary action in a view |
| `danger` / `warning` / `success` | status fills and borders |
| `status_ink().danger` | failure, destructive text, removed diff lines |
| `status_ink().warning` | in-flight text and "needs attention" |
| `status_ink().success` | completed text and added diff lines |
| `popover` | floating surfaces (menus, the completion popup) |
| `sidebar_accent` / `sidebar_accent_foreground` | the rail's selected row |

The surfaces above — `background`, `muted`, `secondary`, `accent`, `popover`,
`border` — and the greys drawn on them are the app's own, set once at boot as a
pair of overrides on the component library's configs. Every other value is the
library's. A call site never needs to know which is which: it reads the token.

Rules that outlive any particular theme:

- **State, not decoration.** A color must mean something. Three colors on screen
  that each mean nothing is worse than one that means "this failed".
- **One primary per view.** If two buttons are primary, neither is.
- **Cards are borders, not fills.** Depth comes from a hairline and padding. A
  lighter block inside a lighter block inside a lighter block is a hierarchy
  nobody can read.
- **Nothing is ringed — a state is a fill.** Neither hover nor selection draws a
  border, and the library's own list highlight is turned off to match. What
  separates them is which fill: hover is the faintest step in the ramp,
  selection a clear stage past it. Both are asserted against each other, because
  a row can be hovered *and* selected and the two must not read alike. A rule
  around a row costs the row width it has to reserve at rest, and a ring on
  hover makes the pointer resting somewhere look like a decision.
- **Never hard-code.** Not at a call site, not even for a colour the theme
  happens to lack. If a surface is genuinely missing, it belongs in the ramp
  that boot installs, with the contrast it owes its neighbours asserted — not
  written into the one view that noticed.

---

## §5 — Components

**Reuse gpui-component before building anything.** It is the reason the port was
worth doing, and every hand-rolled equivalent is a widget that will not follow
the theme, will not follow the focus rules, and will have to be maintained here:

| Need | Use |
|------|-----|
| Window frame, docks, panels | `Root`, `DockArea`, `Panel`, `DockItem` |
| The rail | `Sidebar` for the panel; its list rows are the app's own, because the row's name needs to be an element (the pixel fade, the full name on hover) and the library row holds it as a bare string |
| Buttons, ghost/primary variants | `Button` + `ButtonVariants` |
| Modals | `Dialog` |
| On/off settings | `Switch`, inside a box that shows the pointer, since the switch sets no cursor of its own |
| Single-line and multi-line input | `InputState` + `TextInput` / `Textarea` |
| The file editor | `EditorState` + `Editor` (tree-sitter, no LSP — D3) |
| Markdown | `TextView` + `TextViewState` |
| Long lists | `list` / `virtual_list` — never a `div` per row over an unbounded set |

**Anything that acts on a click shows the pointer.** The library draws every
button variant but `link` and `text` with the arrow, which is a form's
convention; this app's rows, chips, tabs and candidates are hand-made and show a
pointer, and half the actions on screen answering the cursor while the other
half do not leaves the cursor meaning nothing — the only way left to learn what
is clickable is to click it. So buttons are built through the app's own action
wrapper, which overrides that one property and nothing else, and a control that
is *disabled* gives the pointer back: it is a promise that a press will do
something. A guard fails the build on a button built straight from the library.

What the app *does* own, because it is onehand's and not a widget library's: the
transcript block renderers (DESIGN-ANSWER.md), the icon registry (§6), the
terminal panel over the vendored grid, per-panel zoom, and the composer's popup.

**The composer's popup is one shell for every overlay** — the `@` list, the `/`
list, the three settings pickers and the attachment tray. Same frame, same row
height, same grouping, same keyboard model, same empty state; only the contents
of a row differ. Two widgets here drift apart the first time either is touched.

Its shape, and the reason for each part:

- **A pinned title and a pinned footer**, outside the scroll, each ruled on the
  edge facing the list. Held among the rows, the title scrolled away exactly
  when the list was long enough to need it, and the footer sat under whatever
  part-row the scroll stopped on.
- **The whole-row bound is on the scrolling box**, not on the surface. The
  surface also carries that chrome, so flooring there left the fold wherever the
  chrome happened to put it — which is the part-row the flooring exists to
  prevent.
- **The scrollbar runs down the popup's own right edge**, in the surface's
  inset, with the rows held clear of it. Laid over the rows, the thumb sat on
  the highlight fill and on the right end of every row's border, and read as
  part of the row under it. A parked question card's choices put their thumb
  on the card's edge the same way, so two scrolling cards stacked one over the
  other agree about where a scrollbar goes.
- **Height is measured once against an empty query** and held while the popup is
  open. It belongs to the list, not to what is typed: taken from what was on
  screen it held while a query narrowed and grew when a character was deleted,
  and growth moves every row out from under the hand aiming at one.
- **Rows are grouped under small labels** at the quiet step, carried by the
  first row of their run rather than standing as rows themselves — so the index
  the arrows walk is made only of things that can be taken, and a group that
  matched nothing cannot leave a heading behind.
- **Three ink steps for three kinds of text**: the name at full strength, its
  detail a step below (`theme::meta_ink`), the run label at the quiet step. The
  run a query matched is carried by *weight*, because a name at full strength
  has nothing above it to climb to.
- **A popup drawn over a parked card is lifted off it**, leaving the card's
  bottom edge showing. Flush, the two share a width, a surface and an edge and
  read as one tall panel; the usual cue is a drop shadow and the library's is
  invisible on this palette (§4).
- **A surface covering the conversation claims the wheel.** gpui's handler for a
  scrolling box adjusts that box's offset and stops there — it never claims the
  event — so without this the wheel travelled on to the transcript underneath
  and moved the rows the surface is sitting on top of. The claim goes on the
  *outer* surface, not on the box that scrolls, so the chrome and the inset
  swallow it too; the inner box still scrolls, because bubble order runs the
  deeper listener first. This binds the popup and a parked card. It does **not**
  bind the composer, which is the one surface down here the transcript *clears*
  rather than hides behind — nothing is underneath it to be moved out of sight.

---

## §6 — Icons

**Every icon is an SVG.** No Unicode or emoji glyphs as icons. Shell-prompt
typography inside a code block (`❯`, `$`) is text, not an icon, and is exempt.

- **Nearly every UI glyph comes from gpui-component's `IconName`**, the enum
  generated from the SVGs it bundles. That set has to stay loaded anyway — its
  own components reference `icons/…` internally in ~97 places — and drawing the
  app's chrome from it is what keeps one stroke weight across the two.
- **`crate::icons` holds only what that set cannot draw**: a brand mark, which
  belongs to the product it stands for rather than to a general-purpose UI kit,
  and the occasional shape the bundled set has no drawing of at all — taken from
  Lucide, which it is packaged from, so the weight still matches. A name that
  merely reads oddly does not qualify; an absence does. To add one: update
  [assets/icons/manifest.toml](assets/icons/manifest.toml) with the reason
  beside the entry, run [scripts/sync-icons.sh](scripts/sync-icons.sh), register
  it in the `icons!` macro. A test fails if the manifest and the registry
  disagree. The two live in separate namespaces and the asset source serves
  both.
- The bundled set covers less than the app once carried, so a number of glyphs
  are approximations rather than the icon the design would pick. That tradeoff
  is accepted by decision D5.
- Tint by meaning: `muted_foreground` at rest, a semantic token when the icon is
  carrying state. An icon that tracks adjacent text (a rail row's folder, a
  selector's chevron) shares that text's color instead.

---

## §7 — Bounded rendering

Every code, diff and output renderer draws **one element per line**, so unbounded
content freezes the UI. Each cap is a named constant beside the renderer it
bounds: diff lines per card across all hunks, mono output lines per well, the
fold threshold beneath them, terminal lines, plan items, attachment rows and code
block height in the block files under `crates/app/src/chat/transcript/`; completion rows and tray
chips in the composer; mention candidates in the session; and `MAX_TERM_BYTES` at
parse time in core, which bounds the model rather than the view.

Keep any new content rendering bounded, and **say so on screen** when the bound
bites — a truncated view that does not admit it is a lie about the data.
