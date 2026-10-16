# Screens

What each region of the window holds and the order it holds it in, in the app and its built-in
plugins. It binds at the level written here, as `DESIGN.md` does; below it, the code and its tests
decide. `DESIGN.md` owns the principles, the window's frame, the transcript and the visual rules
these screens are drawn with; `docs/ui-patterns.md` the patterns they repeat; and a feature's own
document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) its screens' detail. Code
never cites this file.

## Rail

App chrome outside the dock, so a layout restore cannot lose it. Top to bottom: the workspace bar
(letter tile, name, switcher, *Hide the rail*); the search over session titles and project names
(`Ctrl+K` outside a terminal, showing a hidden rail) beside *New*, which starts in the keyboard's
project, else the one on screen, its caret picking *Start in* or *With agent*; *Overview*, *Tasks*
(a pill counting tasks that need attention, none at zero), *Issues* and *Workflows*, each filled
while its page shows; *Sessions* with a warning-ink *N need attention* chip (shown above zero or
while on, toggling that filter and back) and the filter menu (*By project*, *All sessions*, *Needs
attention*, *Recent activity*) over a hairline; the scrolling list, ending on *Add project…*; a foot
with *Settings*, a page row like the four above, filled while it shows. Every row at rest is in the
muted ink; the one where the person is takes its fill and the ink paired with it. A neutral badge
sinks a step into the surface it sits on, here the inset step.

*By project* lists pinned projects first, a gap between projects. A project row: the fold chevron,
folder and name, its git line (branch, uncommitted changes, commits ahead and behind, each only
above zero and named in full on hover), the pin, the unattended pill (*auto*, *auto · N*, *auto · N
waiting*; a run's pill opens its issue on the Issues page), and while folded its sessions' most
urgent state. Narrowed, the name keeps a minimum and the branch gives way first. Its chevron alone
folds it (not during a search); the rest of the row shows the project's last session, or its page,
which then fills the row. `+` and `⋯` show on hover and on the keyboard's row, right-click opens the
same menu. A session row, indented under its project: its state mark, title and `⋯` (on hover, on
the keyboard's row, always on the one shown), and under them `state · agent · age` (*waiting Nm*
while it needs input, the state's word in its ink while it waits on the person), over which the
pointer and the keyboard's row lay *Stop* (running), *Send the last prompt again* (failed) and
*Close*; the keyboard's row lays them on any session but the one shown, whose conversation already
holds *Stop* and the way back, so a row the keys only pass through stays still. The other filters
are flat, the project in place of the agent. A group lists six sessions, then *N more*; an open
project with none says *No open sessions*, a search or filter with none *No sessions match*. Each
state has its own ink, and a mark per thing the person can do, none animated (running is a filled
dot): failed and disconnected share one, both answered by trying again, their words telling them
apart. The inks: failed or disconnected danger, needs input warning, running the accent, done unread
success, idle muted. `↑`/`↓` move the list's row, ringed while the keys move it, `Enter` and `←`/`→`
act on it, `Alt+↑`/`↓` step through sessions in its order. The tree's order is the user's, set by
dragging; pinned projects stay on top and sessions never leave their project. `Ctrl+Shift+B` hides
the rail completely, never to an icon column. It resizes between 232 and 448px, on the side bar's
surface.

## Agent pane header

Left to right: the show-rail button (only while the rail is hidden); the conversation's name, medium
weight; a dots menu (*Rename…*, *Export as Markdown…*; *Resume in this session…*, refused mid-turn,
and *Restart the agent*; *Delete conversation* in the danger tint, refused until the first turn
ends); a spacer; past conversations and *Close session* (only while a session shows); the terminal,
an accent dot on it while its dock is closed over a shell running a command; last, the Workbench.
The two dock buttons are filled while their dock is open. On a page (the overview, Tasks, Issues,
Workflows, Settings) it reads the page's name, with no dots menu and no dock buttons. As the pane
narrows, the name gives way first, down to a minimum. Without a session it names the project, its
dots menu holding the project's actions but *New session* and *Open terminal*, which the page and
the header already offer.

## Step strip

Under the header, only while a run drives the session on screen: the workflow's name muted, each
step's label between chevrons (done ones with a muted check, the current one in full ink), then
*Review…*, *Revise…* and *Continue* (primary) while it waits for approval, and *Stop* always. Only
the step labels clip. *Review…* reads the answer from the run; *Revise…* refuses an empty note; a
press the run no longer waits at shows the new answer, in the warning ink, rather than approving it
unread.

## Composer

A card with no edge, set off by its raised fill and lift; the caret alone shows focus, and no line
is ever added to it (a refusal is Send's tooltip, a failure a toast). The field, then the `+` menu,
*Fast* (*Fast: On* or *Off*, a toggle with no caret, when its choices read as on and off; else
*Fast:* and the value in force), the model and effort chips, then *Send*, or *Stop* (solid danger,
worded) and *Queue* over a draft. Under it the branch and the mode, stacked when narrow. Each chip's
menu opens just above it, without its tooltip. Above the card: one pinned permission or question,
the oldest, with how many more wait; then the queue and a reconnect as plain lines; then the staged
files as one row of chips, faded where cut beside *Show all N*. *Run a workflow…* (the `+` menu, the
keymap, the Workflows page's *Run…*) opens the launcher, and an issue's *Run workflow…* its start
form; both are drawn as `docs/workflows.md` describes.

## Docks

Bare panels on the dock surface with strips of their own, no library tab bars; the terminal is a
block held off its neighbours by a gap, drawn no taller than leaves the conversation a readable
height (its dragged height kept).

- Strips as tall as the header. The Workbench's: *← Conversation* while it has the area, the modes
  (Editor, Markdown, Issues, Plugins, Neovim; a select when narrow), maximize, hide. The terminal's:
  shell tabs (each capped in width, scrolling, so none is left out), `+`, maximize, hide. Crowded
  file tabs: a select.
- A list beside its detail (files, documents, issues) only while the dock holds both; else one at a
  time, the detail under a link back naming the list. Picking shows it; going back keeps it. Files
  and Markdown draw one tree row (`tree_row`); the file or document open beside the tree is its
  picked row, a changed file's letter in its state's ink at the row's end.
- A failed open or save in the Editor or Markdown is a toast; a file changed on disk, a Neovim that
  would not start and a listing that failed stand as a status line under the mode.
- Plugins: the search and filters a filter bar; a row's actions on hover and on keyboard focus, its
  switch always; what runs and what waits on a restart under the list, so neither moves it.
- The Issues tab's issue: a trail (project › reference, linked to its forge, or *Draft*) with its
  actions (one primary, one other, ⋯); its tags (a neutral state badge, labels); the title, large;
  one line of facts; what it waits on (a conflict, where its work stands, the review); then two
  chips: *Description* (the body as a document, the files it names, what the work left, *Before*)
  and *Activity N* (the history). Sections are parted by space, not rules. ⋯ ▸ *Open in Issues* and
  *Review…* open it on the Issues page, the latter with its review open.
- Hiding a dock keeps its buffers and processes; the terminal's open state follows the project.

## Pages without a session

- A project with no session shows one column from the top: a head, the line counting its tasks that
  need attention, run or wait at the start (not drawn at zero, and opening the Tasks page narrowed
  to the project), *Run check* (spinning while a check runs) and *New session* (primary) at the end;
  its *Check command* field; and its past conversations as rows, capped, each with *Delete…* on
  hover. The page scrolls as one.
- *Workspace overview* shows, across all projects, the boxes *Waiting on you* (runs waiting for an
  answer or an approval, and sessions waiting, failed, finished or lost) and *Working*, then
  *Projects* as a grid of tiles (not drawn without projects), then the sections *Recent
  conversations* and *Open issues* (its project picker kept while the page is left), plain lists on
  the page's surface. The page scrolls as one.
- *Tasks*: a filter bar (the project picker), then four boxes two by two, *Needs attention*,
  *Running*, *Queued*, *Finished*, each with a count, an empty line and a cap. A row is the title
  over workflow, step or outcome and project, with ghost actions by state (*Open session*, *Stop*,
  *Resume*, *Retry…*, *Dismiss*) shown on hover, kept under *Needs attention*; its text opens the
  task's detail, a page of its own at the prose width: a trail, the title, then *Awaiting approval*,
  *Way out* and the runs as plain sections.
- *Issues*, a list page (`docs/ui-patterns.md`, *List pages*): a head (the search, then *Refresh*
  and *New issue*, its primary, at the end); a filter bar (*Open* and *Closed* as choices, each its
  count in a badge on its corner, then pickers for progress, project and label and *Clear filters*);
  a row is its state mark (open in the success ink, closed in the closed ink), reference, title,
  priority and note, then its labels, project and age in slots at the end, the labels and project
  dropped first; *Showing N of M open issues* at the foot. *Reading the issues…* until the first
  read lands; *Refresh* spins while it reads. *No issues yet* or *No issues match* when empty.
- The **review block** opens below where the work stands only on *Review…*, as `docs/unattended.md`
  draws it.
- *Workflows*: a head (*New workflow*, its primary, *Import…*, then how many), then the workflows as
  rows, capped, shipped ones *Built in* (read-only) in a slot of their own, *Run…* (projects, the
  rail's first) and the rest shown on hover. *New workflow* or *Edit* opens a form below, one box
  per step (its kind its first field, one ⋯ to move or remove it), a margin marking where a failure
  goes back to, problems (a name clash among them) above a *Save* that refuses with the first and
  spins while it writes.

## Settings

Settings is a page: a nav column with Appearance, Workspace, Agents, Connections and Shortcuts (rows
as the rail's page rows), and beside it the sub-page, untitled (its nav row names it), opening on
its scope as a badge and a hint, its groups split by hairlines with semibold headings, not boxes.
Every button there is secondary: Settings holds no primary.

## Persistence

The layout is saved as the Workbench's dragged width, the terminal and rail sizes and whether each
dock is open, never as the library's `DockAreaState`.
