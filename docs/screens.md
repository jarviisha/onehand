# Screens

What each region of the window holds and the order it holds it in, in the app and its feature
crates. It binds at the level written here, as `DESIGN.md` does; below it, the code and its tests
decide. `DESIGN.md` owns the principles, the window's frame, the transcript and the visual rules
these screens are drawn with; `docs/ui-patterns.md` the patterns they repeat; and a feature's own
document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) its screens' detail. Code
never cites this file.

## Title bar

The library's `TitleBar`, its whole width a handle to move the window. At its start, the app's
menus, *File*, *View*, *Go* and *Session*, each item a command the keymap holds, under the name
the menu gives it and with the key the keymap binds: *File* holds *New session*, *Run a workflow on
this project…*, *Save the open file* and *Open Settings*; *View* the rail's, the Workbench's and
the terminal's toggles, *Open Editor*, *Open Markdown*, *Open Neovim*, maximizing the focused panel
and its zoom; *Go* the rail's search, the composer, Tasks, Schedules and stepping through sessions;
*Session* *Show the workers*, *Restart the agent* and *Close the session*. A menu offers nothing no
key reaches. At its end, the window's minimize, maximize and close, drawn by the library where the
app decorates the window; its close asks first what the compositor's close asks (a workflow
being written). Nothing else.

## Status bar

The library's `StatusBar` along the window's foot. At its start the rail's toggle and the branch of
the project on screen; at its end the terminal's and the Workbench's toggles, each an icon alone,
marked while what it shows is open (a closed one's glyph in the muted ink), and dispatching the
command its key does. The terminal's carries an `info` dot while its dock is closed over a shell
running a command. Nothing else: a
transient state is a toast.

## Rail

App chrome outside the dock, so a layout restore cannot lose it, a hairline down its edge. Top to bottom: the workspace bar
(letter tile, name, switcher, *Hide the rail*); the search over session titles and project names
(`Ctrl+K` outside a terminal, showing a hidden rail) beside *New* (secondary, the rail's only filled button), which starts in the keyboard's
project, else the one on screen, its caret picking *Start in* or *With agent*; *Overview*, *Tasks*
(a pill counting tasks that need attention, none at zero), *Issues*, *Workflows* and *Schedules*,
each filled while its page shows; *Sessions* with a warning-ink *N need attention* chip (shown above zero or
while on, toggling that filter and back) and the filter menu (*By project*, *All sessions*, *Needs
attention*, *Recent activity*) over a hairline; the scrolling list, ending on *Add project…*; a foot
with *Settings*, a page row like the five above, filled while it shows. The rail is drawn as the
library's sidebar draws its items: a row's name in the sidebar's ink and its second line muted, the
row under the pointer on the sidebar's accent fill, and the one where the person is on that fill
too, in the ink paired with it and at medium weight. The *Sessions* label is the small muted
heading the library gives a group. The search is the library's edged field. A neutral badge
stands on the page's fill.

*By project* lists pinned projects first, a gap between projects. A project row: the fold chevron,
folder and name, its git line (branch, uncommitted changes, commits ahead and behind, each only
above zero and named in full on hover), the pin, the unattended pill (*auto*, *auto · N*, *auto · N
waiting*; a run's pill opens its issue on the Issues page), and while folded its sessions' most
urgent state. Narrowed, the name keeps a minimum and the branch gives way first. Its chevron alone
folds it (not during a search); the rest of the row shows the project's last session, or its page,
which then fills the row. `+` and `⋯` show on hover and on the keyboard's row, right-click opens the
same menu. A session row, indented under its project: its state mark, title and `⋯` (on hover, on
the keyboard's row, always on the one shown), and under them `state · agent · age` (*waiting Nm*
while it needs input, a card or its run's approval, the state's word in its ink while it waits on the
person; *Running · N in background · M subagents · agent · age* while background work outlives the
turn or a subagent works, which keeps the session running, as does the agent writing with no turn
open: it answers what it waited for by itself, and is running until it says that cycle ended or
falls silent, 3s for an agent that never says, 30s for one that does or after the last of its
background work ended, since its answer may take a while to start), over which the
pointer and the keyboard's row lay *Stop* (only while a turn is in flight, not for background work alone), *Send the last prompt again* (failed) and
*Close*; the keyboard's row lays them on any session but the one shown, whose conversation already
holds *Stop* and the way back, so a row the keys only pass through stays still. The other filters
are flat, the project in place of the agent. A group lists six sessions, then *N more*; an open
project with none says *No open sessions*, a search or filter with none *No sessions match*. Each
state has its own ink, and a mark per thing the person can do, none animated (running is a filled
dot): failed and disconnected share one, both answered by trying again, their words telling them
apart. The inks: failed or disconnected danger, needs input warning, running `info`, done unread
success, idle muted. `↑`/`↓` move the list's row, ringed while the keys move it, `Enter` and `←`/`→`
act on it, `Alt+↑`/`↓` step through sessions in its order. The tree's order is the user's, set by
dragging; pinned projects stay on top and sessions never leave their project. `Ctrl+Shift+B` hides
the rail completely, never to an icon column. It resizes between 232 and 448px, on the side bar's
surface.

A worker of a lead in the window is not listed. Its lead's row keeps its own state and age (and
its place in *Recent activity*), so a lead never reads as failed for a worker that did, and after
its state its line counts *N workers need you* (a worker needing input, failed or disconnected)
in the warning ink. The *Needs attention* filter and a folded project's badge read the most urgent
state of the lead and its workers (by that order: lost, failed, needs input, running, done
unread), and *N need attention* counts a worker waiting on the person. A worktree project holding
nothing but such workers is left out with them. When the lead closes or is lost, its workers are
listed again under their project.

## Agent pane header

Left to right: the show-rail button (only while the rail is hidden); the conversation's name, medium
weight; a dots menu (*Rename…*, *Export as Markdown…*; *Resume in this session…*, refused mid-turn,
and *Restart the agent*; *Delete conversation* in the danger tint, refused until the first turn
ends); a spacer; *N in background* (only while the session's agent has work running in the
background: a menu of each item's name, its kind and how long it has run, each row stopping that
one item and never the turn, 12 listed and then *N more*; when an item ends, the transcript keeps a
notice of how and after how long: *Background work finished after 2m 14s: npm test*, an error when it
failed, *interrupted* when the agent went away first); past conversations and *Close session*
(only while a session shows); last, while the session on screen leads workers, its workers, a
warning dot on it while one waits on the person and the region is closed. The docks' toggles are
the status bar's. On the overview, Tasks, Issues,
Workflows, Schedules and Settings, which name themselves in their head, it carries no name; none of these has a dots menu. As the pane narrows, the name gives way
first, down to a minimum. Without a session it carries no name, the page naming the project, but
keeps the project's dots menu, holding the project's actions but *New session* and *Open
terminal*, which the page and the header already offer.

## A prompt's controls

Under each of the person's prompts, shown while it is pointed at or one of them holds focus: copy,
then *Fork from here* for an agent that forks (Claude Code, Codex). Fork is absent on the first
prompt, after a turn that did not end on an answer whose id is kept (a tool step, an interruption,
a failure, a conversation saved before this build), and while the agent has not said it forks.
Pressing it opens a new session on the same project and agent, beside the original on the rail and
on screen at once, named *Fork: <title>*, its transcript the original's up to just before that
prompt and the prompt's text and attachments in its composer. The original stays as it was; files
are not rolled back. A fork that cannot open is a toast saying why, and its session goes.

## Subagent card

Where the agent hands work to a subagent, the transcript draws a fold block, boxed as every
folding block is.
Its head is a fold line with a state mark, the subagent's name and its state word (*done*,
*failed*, *cancelled*, *interrupted*; a spinner while it works), and under it the first line of its
task in the meta ink, the body inset. While it works the card shows its latest 3 steps behind
*N earlier steps*; once it ends it folds to `<state> · N steps`, opened by a press, capped at 200
steps with *N more*. Steps inside are compact rows that do not open. A subagent of a subagent nests
one inset deeper. *interrupted* (an end never seen, or the adapter lost) and *cancelled* read in the
muted ink, *failed* in the danger ink. A card reaches the saved transcript whole, once it has ended;
one left working when the session closes is kept as *interrupted*.

## Step strip

Under the header, only while a run drives the session on screen: the workflow's name muted, each
step's label between chevrons (done ones with a muted check, the current one in full ink; a
schedule's prompt names no step), then
*Review…*, *Revise…* and *Continue* (primary) while it waits for approval, and *Stop* always. Only
the step labels clip. *Review…* reads the answer from the run; *Revise…* refuses an empty note; a
press the run no longer waits at shows the new answer, in the warning ink, rather than approving it
unread.

## Composer

A card edged in the field's edge, set off by its raised fill and lift; the caret alone shows
focus, and no line is ever added to it (a refusal is Send's tooltip, a failure a toast). The field, then the `+` menu,
*Fast* (its bolt alone, in the warning ink while fast: a toggle with no caret when its choices
read as on and off; else a picker with a caret, the value in force on its tooltip), the model and
effort chips, then *Send*, or *Stop* (solid danger,
worded) and *Queue* over a draft. Under it the branch and the mode, stacked when narrow; just
left of the mode the context and limit meters, each a ring filled to its percent and the percent,
with no name (the tip says `Context: 84k of 200k tokens`, or each limit and when it resets; warning
ink from 80%, danger when refused, a full ring when a refused limit has no figure), absent for an
agent that reports neither (the limit's also while it is reported with no figure and nothing
wrong); while the
session's agent may use onehand's tools, *Controls onehand* stands before the mode, its bot mark
in the warning ink, and pressing it switches that off. The `+` menu ends on *Agent controls
onehand* under a heading of its own, *This session*, checked while on, and not offered in an
unattended run's session. Each chip's
menu opens just above it, without its tooltip. Above the card, top down: one surface on the card's
fill, edge and lift holding a reconnect and the queue as lines, then the staged files as one row
of plates (below); then the slivers of the cards waiting behind, at most two; then one pinned
permission or question, the oldest, as wide as the card and its foot tucked under it (a
subagent's is titled `<name> asks permission` or `<name> asks: <question>`, its head ending on
*1 of N waiting* when more wait); a permission's command shows whole up to about ten lines, and
past them ends on *Show all N lines*, which opens it in place. The staged files' plates (a picture's preview,
pressed to open it whole in a dialog; a text file opening in the Workbench) fade where cut beside
*Show all N*. A picture attached to a sent prompt opens the same way. While the session on screen
leads workers, the lead's strip stands on the card, narrower than it, one line above the card's: *N
workers*, a count per state that has any, most urgent first, in its mark and ink, and at the end,
in the warning ink, *<name> needs you* for the most urgent worker waiting on the person, when one
does. It stays while the workers' region shows. Pressed, the region shows with the most urgent
worker picked; the pointer leaves the caret where it was, and the keys reach the workers through
the region's list.
*Run a workflow…* (the `+` menu, the
keymap, the Workflows page's *Run…*) opens the launcher, and an issue's *Run workflow…* its start
form; both are drawn as `docs/workflows.md` describes.

## Docks

Bare panels on the dock surface, flush with their docks, each opening on its own tab bar; a
hairline runs down the Workbench's edge beside the conversation and along the terminal's top
under it. The terminal is drawn no taller than leaves the conversation a readable height (its
dragged height kept).

- Strips as tall as the header, a mode's own bar under the Workbench's a step lower. The
  Workbench's, the terminal's and the Editor's files are the library's tab panel bars (`tab_bar`):
  on the tab bar's fill with a line under, the tabs flush and parted by lines, the open one on the
  tab's fill under a stroke of the primary along its top, the controls at the end. The
  Workbench's: *← Conversation* while it has the area, the modes as tabs
  (Editor, Markdown, Issues, Plugins only where a Claude Code agent is set up when the window
  opens, Neovim; a select under 26rem), maximize, hide. The terminal's:
  shell tabs (each capped in width, scrolling, so none is left out), `+`, maximize, hide. File
  tabs never shrink: the newest that fit stay, the current one among them, and the older ones
  go into an *N more* menu after them.
- A list beside its detail (files, documents, issues) only while the dock holds both; else one at a
  time, the detail under a link back naming the list. Picking shows it; going back keeps it. Files
  and Markdown draw one tree row (`tree_row`); the file or document open beside the tree is its
  picked row, a changed file's letter in its state's ink at the row's end.
- A failed open or save in the Editor or Markdown is a toast; a file changed on disk, a Neovim that
  would not start and a listing that failed stand as a status line under the mode.
- Plugins: the search and filters a filter bar; a row's actions on hover and on keyboard focus, its
  ⋯ menu also on a right-click (refused while another change runs), its switch always; what runs and what waits on a restart under the list, so neither moves it.
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
- **The workers' region**, in the Workbench's place and never beside it: opening one puts the
  other away, a caret in the one leaving going to the lead's composer and the one arriving taking
  none; hiding the region gives the Workbench back as it was, and while it shows the layout saves
  the Workbench's state, not the region's. It shows the workers of the lead on screen, follows the lead on screen, and closes on a
  session without workers or a page. Its strip: *Workers · N*, maximize, hide.
  - **The list**, pinned under the strip and never scrolled away: a row per worker, at most four
    (the cap): its mark, name, state, age, the lead's last turn and its background count, and
    under them what it waits on in the warning ink, else what it is doing, *Stop* on one whose
    turn runs; the picked one in the selected step. Ordered by urgency
    (failed or disconnected, needs input, running or connecting, done, idle; ties as the lead
    opened them) when the region opens and when a worker is picked by a press or from elsewhere;
    in between, and while `↑`/`↓` walk it, a row that changes state changes in place and does not
    move. A worker starting to wait moves nothing, picks
    nothing and opens nothing: its row turns to *Needs input* with the command under it.
  - **The picked worker's detail** fills the rest, the most urgent picked when the region opens.
    Its head: its state mark (a spinner while it connects, with nothing under it yet), name, the
    state's word in its ink, its folder's last component (the whole path on hover), *held by you*
    while the person holds it, *Write to <name>…* (the name up to its first ` · `), *Stop* while
    a turn runs, and `⋯` (*Give back to the lead* while offered, *Send the last prompt again* after
    a failure, *Close*). Under it the oldest card it waits on, the ones behind peeking over it and its head saying how
    many wait, bounded by
    the detail's own height, then its whole transcript,
    scrolling, following the newest output. No composer until *Write to …*: then one at the foot,
    with *Send*, *Stop* and *Queue* and nothing else; sending or `Esc` closes it, the draft kept.
  - **The keys.** Opened by the pointer the region takes no caret. *Show the workers*
    (`Ctrl+Shift+Y`) opens it with the caret on the list: `↑`/`↓` pick the row above or below,
    `Enter` opens *Write to …*, `Esc` gives the caret to the lead's composer. While the caret is
    in the region, `Alt+Y`/`Alt+A`/`Alt+N` answer the picked worker's card, and nothing when it
    waits on none.
  - A caret in a composer that leaves (closed, its lead gone) goes to the lead's composer first. A
    worker asked for anywhere else (the keys, a toast, the remote bridge) shows its lead with the
    region at that worker picked; `Ctrl+Tab` passes over workers. Not saved.

## Pages without a session

- With no agent set up, the pane shows neither the project page nor the no-project line, but one
  centred column in meta text: *No agent is set up*, then per built-in agent either *<agent> is on
  this machine* with *Add* (secondary) when it is installed, or how to install it (its command in
  the code face), *Then restart onehand, or add another agent in the settings*, and
  *Open the agent settings* (secondary), which opens Settings on Agents.
- A project with no session shows, from the top: a head (the project's name, its path and git line
  under it, *Run check* (spinning while a check runs) and *New session* (primary, with a caret
  picking another agent while more than one is configured) at the end); then, in one column at every width, the box *Past
  conversations* with its count, its rows the overview's (age, title, then items and agent at the
  edge, then a `⋯` on hover holding *Resume* and *Delete…*, the same on a right-click), capped, its wait and empty lines inside it; under
  it the box *Tasks* (how many need attention, run or wait, each opening the Tasks page
  narrowed to the project; *Nothing running here* at zero); then the box *Schedules* (the
  project's schedules by name with when each runs next, or *off*, each opening the Schedules page
  narrowed to the project; *No schedules here* at none; capped at five; reading and a failed read
  said as on the page); then the box *Settings*
  (its *Check command* field). The page scrolls as one.
- *Workspace overview* shows, under its head (*Overview*), across all projects, the boxes *Waiting
  on you* (runs waiting for an answer or an approval, and sessions waiting, failed, finished or
  lost) and *Working*, then *Projects* as a grid of tiles (not drawn without projects), then the
  boxes *Recent conversations* and *Open issues* (its project picker kept while the page is left).
  The head stays in place; the boxes scroll under it as one.
- *Schedules*: a head (*Schedules*, with how many are on and off), the project picker at the end
  of the row under it, as Tasks', the two kept in place; then one box, *Schedules*, with its count: a row per schedule as a task's
  row, parted by hairlines, its name over when it runs (in words where a quick choice says it,
  else the expression; one that does not parse in the warning ink), its project and when it runs
  next (or *off*), then what its last look said in the warning ink; its switch at the row's end,
  its tooltip saying what a press does. Capped, saying how many more. *Reading the schedules…*
  before the first read; a read that failed says why in the warning ink; *No schedules yet, kept
  in* the path in the code face, or *No schedules match* while a project is picked. Read again
  every minute, its next times with it. The page takes the keys while it shows. *New schedule* is
  its primary (refusing with no project). A row's text opens the schedule in the form.
- *A schedule's form*, in place of the list, under a trail (*Schedules* to go back, Esc too, asking first
  while it holds changes, › the schedule's name): a head (the name, *New schedule* until named; when it runs and its
  project; ⋯ with *Delete…* for one saved, *Cancel*, *Save* the primary, refusing with the first
  problem); then *Name*, *Project* (a picker) and *On*; *What it starts* (a switch: *A session with
  a prompt*, its *Prompt*, or *A workflow on a brief*, its *Workflow* picker, *Title* and *Brief*,
  each kind keeping what was typed); *The agent* (*Agent*, *The first agent* by default; *Mode* and
  each setting the agent offers as a row of choices, *Agent's own* first, *Check the agent* while
  nothing is known); *Tools* (a switch per tool, *None given* at none; *Labels*); *When* (the quick
  choices in one track, *Every N hours*, *Every day*, *On weekdays*, *Every week*, *Expression…*,
  the fields each takes, the expression in the code face and the next three times, or why it does
  not parse in the warning ink; *When it was missed* and *When the last run is still going*, each
  a switch). Problems in a bar along the page's foot, none on a new form nobody has changed.
- *Tasks*: a head (*Tasks*), the project picker at the end of the row under it, the two kept in
  place; then four boxes two by two,
  *Needs attention*, *Running*, *Queued*, *Finished*, each with a count, an empty line and a cap,
  its rows parted by hairlines. A row is the title
  over workflow, step or outcome and project, its actions by state (*Open session*, *Stop*,
  *Resume*, *Retry…*, *Dismiss*) in a `⋯` on hover and on a right-click, under *Needs attention*
  also drawn as buttons beside it; its text opens the task's detail, a page of its own at the
  prose width: a trail, the title, then *Awaiting approval* in a page's box, its mark and title in the warning ink,
  *Way out*, and the runs on a vertical stepper.
- *Issues*, a list page (`docs/ui-patterns.md`, *List pages*): a head (*Issues*, then *Refresh*
  and *New issue*, its primary, at the end); the row under it (the search, then *Open* and
  *Closed* as choices, each its count in a badge after its word, then pickers for progress, project
  and label and *Clear filters* at the row's end), the two kept in place;
  the rows a table under a head naming each column (*Issue*, *Title*, *Labels*, *Project*,
  *Updated*) in the meta ink, drawn only while there are rows;
  a row is its state mark (open in the success ink, closed in the closed ink), reference, title,
  priority and note, then its labels, project and age in slots at the end, the labels and project
  dropped first; *Showing N of M open issues* at the foot. *Reading the issues…* until the first
  read lands; *Refresh* spins while it reads. *No issues yet* or *No issues match* when empty.
- The **review block** opens below where the work stands only on *Review…*, as `docs/unattended.md`
  draws it.
- *Workflows*: a column page as wide as boxes side by side, holding the workflows as cards. Its
  head reads *Workflows*, how many under it, *New*, its one primary, and a ⋯ holding *Import…*;
  under it a search, matching a workflow's name and description, kept while the page is left; the
  two kept in place, the cards scrolling under them.
  The cards stand under *Yours* (the person's own) and *Built in* (the shipped), four to a row
  while the page holds them and fewer as it narrows, down to one. A card is a box: the
  workflow's name, its description cut at two lines, a short mark per step in its kind's ink (an
  agent's turn the primary ink, a command muted, an approval warning, what goes to the forge success),
  and a foot with where it works and how many steps, *Run…* and a ⋯ (*Duplicate* and *Export…*
  on any that reads, *Delete…* on the person's own). An unreadable one names its file and says
  *Cannot be read: …* in the warning ink, its ⋯ holding *Delete…* alone. Hover raises a card's
  edge and never fills it; the keyboard rings it, and Enter opens it, as pressing its body does.
  *Run…*, *Export…* and *Delete…* act from the card; *Duplicate* opens the copy. The grid is
  capped and says so; while the search narrows it, *Showing N of M workflows*, or *No workflows
  match*.

  A card opens its workflow on a page of its own: a trail (*Workflows*, back, › its name) over its head (its name, how
  many steps and where it works under it, *Run…*, which refuses until it is saved, a ⋯ with
  *Duplicate*, *Export…* and *Delete…*, then *Cancel* and *Save*, the head's primary, which
  refuses with the first problem and spins while it writes), then three columns parted by hairlines, each scrolling
  apart. The first holds the workflow's own fields: *About* (name and description), *How it runs*
  (where it works, a picker naming the choice over a menu of both; misses allowed; timeout), each
  pair of fields stacking where the column cannot hold both, then what a prompt may use. The second holds *Steps*, in a box, as it decides what a run does: its count and
  *Add step* with its menu of kinds on its title row, then the steps down a rail. A step reads
  as a numbered dot on a rail that joins it to the next (the open one's dot filled), then its
  name, its kind, where its failure goes back to (*on failure ↑ N*), and its first problem; its
  line's ⋯ moves or removes it. The third holds the one step open, its line marked as picked:
  its head names it (*Step N · its name*, its kind) and steps to the one before or after (↑ and
  ↓ too, never while a field holds the caret) or closes it (Esc closes it before the editor);
  then its kind, id and label on one line, then its kind's fields (an agent step's prompt
  growing with what is written, *Write larger…* under it opening it in a dialog the width of a
  page, with what a prompt may use, *Cancel* and *Done*). With none open it says *Pick a step to
  change it*. Where the three do not fit, the first two stack into one column; where two do not
  either, the steps stand alone and an open step takes their place under *Steps* (back). Going
  back to the cards over unsaved changes asks first. Along the page's foot, the whole width,
  stands what the editor has to tell: the problems (a name clash among them), each a line, on the
  danger tint led by the warning mark, from the first change on; or, on a shipped workflow, *Built
  in, read-only: duplicate it to change it* on the primary's tint led by the info mark. A shipped
  workflow opens read-only, *Duplicate to edit* its primary, with no *Save* or *Cancel*; an unreadable file of the person's own opens
  why, with *Delete…*.

## Settings

Settings is a page named in its own head, as the workspace pages are, over a nav column with
Appearance, Workspace, Agents, Connections and Shortcuts (rows as the rail's page rows), and beside
it the sub-page, untitled (its nav row names it), opening on its scope as a badge and a hint, its
groups in the library's setting-page shape: each a box with an edge, its semibold heading and the
line about it at the top inside. Every button there is secondary:
Settings holds no primary. Under the Agents page's rows, each built-in agent installed on the
machine with no entry of its own gets a line, *<agent> is on this machine*, with *Add* at its end,
then *Add agent…*; the first row carries the *Default* badge. *Add agent…* and a row's edit mark
open the agent form as a dialog (*Kind*, *Name*, *Command* and *Arguments* for another agent,
*Environment*; *Cancel*, then *Add* or *Save*), which a close drops. An entry naming a preset
this build does not know shows its `preset` line, *Not supported by this build of onehand* under it,
and only *Delete…*. The Connections page lists each connector and who it acts as, then an *Outside
control* group: what a program may and may never do, the socket's path in mono, and one switch.
The Shortcuts page lists the window's commands first, then a group for each place the rest work
in (the rail's list, a page, a card, the composer), in the keymap's order, and last the terminal's
fixed keys; a group says where its commands work, and a row says so only where it differs.

## Persistence

The layout is saved as the Workbench's dragged width, the terminal and rail sizes and whether each
dock is open, never as the library's `DockAreaState`.
