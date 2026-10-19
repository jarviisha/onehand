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
The two dock buttons are filled while their dock is open. On Issues and Settings it reads the
page's name; on the overview, Tasks and Workflows, which name themselves in their head, it carries
no name; none of these has a dots menu or dock buttons. As the pane narrows, the name gives way
first, down to a minimum. Without a session it carries no name, the page naming the project, but
keeps the project's dots menu, holding the project's actions but *New session* and *Open
terminal*, which the page and the header already offer.

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
- The Issues tab: one filter bar over the list and the issue both, a faint line under it: the
  search, the label picker, while the search or a label narrows the list *Clear filters*, and `+`
  (*New issue*, primary, secondary while the form holds that) at the end, the filters wrapping
  under the search when narrow; a filter picked while the issue shows alone shows the list. Over
  the rows, *Open* and *Closed* with their counts; *Showing N of M open issues* at the list's end
  while it is narrowed, *No issues match* when nothing is left. `↑`/`↓` step the list as on the
  Issues page (where the list shows alone, the first press opens the row picked); Esc closes the
  form, else goes back from the issue to the list where the two show one at a time.
- The Issues tab's issue: a trail (project › reference, linked to its forge, or *Draft*) with its
  actions (the next one secondary, one other a ghost, ⋯; here and on the Issues page, *New
  issue* holds the view's primary);
  its tags (a neutral state badge, labels); the title, large; one line of facts; what it waits on
  (a conflict, where its work stands, the review); then two chips: *Description* (the body as a
  document, the files it names, what the work left, *Before*)
  and *Activity N* (the history). Sections are parted by space, not rules. ⋯ ▸ *Open in Issues* and
  *Review…* open it on the Issues page, the latter with its review open.
- Hiding a dock keeps its buffers and processes; the terminal's open state follows the project.

## Pages without a session

- A project with no session shows, from the top: a head (the project's name, its path and git line
  under it, *Run check* (spinning while a check runs) and *New session* (primary, with a caret
  picking another agent while more than one is configured) at the end); then the box *Past
  conversations* with its count, its rows the overview's (age, title, then items and agent at the
  edge, *Delete…* laid over them on hover), capped, its wait and empty lines inside it, and beside
  it a column of the box *Tasks* (how many need attention, run or wait, each opening the Tasks page
  narrowed to the project; *Nothing running here* at zero) over the box *Settings* (its *Check
  command* field). The column wraps under the history where both do not fit. The page scrolls as
  one.
- *Workspace overview* shows, under its head (*Overview*), across all projects, the boxes *Waiting
  on you* (runs waiting for an answer or an approval, and sessions waiting, failed, finished or
  lost) and *Working*, then *Projects* as a grid of tiles (not drawn without projects), then the
  boxes *Recent conversations* and *Open issues* (its project picker kept while the page is left).
  The page scrolls as one.
- *Tasks*: a head (*Tasks*), a filter bar (the project picker), then four boxes two by two,
  *Needs attention*, *Running*, *Queued*, *Finished*, each with a count, an empty line and a cap,
  its rows parted by hairlines. A row is the title
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
- *Workflows*: a list beside its editor. The list, a column of its own, opens on a head cut down
  to it (*Workflows*, how many under it, *New*, its primary, and a ⋯ holding *Import…*), then the
  workflows as rows in no box, the split's hairline running the pane's whole height beside them, the person's own under *Yours* and the shipped under *Built in*,
  a row its name alone (*Cannot be read* under it in the warning ink when it is so), capped, a
  new workflow not saved yet an *Untitled* row at the top of *Yours*. Nothing is picked on
  arrival: beside the list, *Pick a workflow to see and change it, or make a new one*. Picking a
  row opens it beside the list: a head (its name, how many steps and where it works under it,
  *Run…*, which refuses until it is saved, and a ⋯ with *Duplicate*, *Export…* and *Delete…*), then
  three groups under headings, *About* (name and description), *How it runs* (where it works,
  misses allowed, timeout) and *Steps*, the one group in a box, as it decides what a run
  does (its count and *Add step* with its menu of kinds on its title row, what a prompt may use,
  the steps down a rail), then the problems (a name clash among them) above *Save*,
  which refuses with the first and spins while it writes, and *Cancel*. A step reads as a
  numbered dot on a rail that joins it to the next (the open one's dot filled), then its name,
  its kind, where its failure goes back to (*on failure ↑ N*), and its first problem; one step
  opens at a time, its line marked as picked, in a drawer docked under the form: a sheet inset from the editor's sides, its top
  corners rounded and its top edge in the primary ink, lifted by a shadow cast upward, on the
  page's own surface, that takes the lower part of its height, the form above scrolled so the open
  step's line stands just clear of the drawer's edge. The drawer's head names the step (*Step N ·
  its name*, its kind) and steps to the one before or after or closes it (Esc closes it before the
  form); then its kind, id and label on one line, then its kind's fields (an agent step's prompt
  growing with what is written, *Write larger…* under it opening it in a dialog the width of a
  page, with what a prompt may use, *Cancel* and *Done*). Its line's ⋯ moves or removes it. A shipped workflow opens read-only, *Duplicate to
  edit* its primary, with no *Save*; an unreadable file of the person's own opens why, with
  *Delete…*.

## Settings

Settings is a page: a nav column with Appearance, Workspace, Agents, Connections and Shortcuts (rows
as the rail's page rows), and beside it the sub-page, untitled (its nav row names it), opening on
its scope as a badge and a hint, its groups split by hairlines with semibold headings, not boxes.
Every button there is secondary: Settings holds no primary.

## Persistence

The layout is saved as the Workbench's dragged width, the terminal and rail sizes and whether each
dock is open, never as the library's `DockAreaState`.
