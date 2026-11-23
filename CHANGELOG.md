# Changelog

Notable changes to onehand, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions are `0.x`, and `0.x` here means what it says: the config file, the on-disk transcript
format, the window layout and the keymap have all changed without migration and will again. A
minor bump may break any of them.

## [Unreleased]

### Breaking

- A conversation records the project it belongs to. At the first start of this build each kept
  conversation is given its project, found by its folder, through git for a worktree, or through
  the task that ran there for one since removed; an older build still reads it, and a save made
  by one drops the project, which the next start of this build gives back while its folder can be
  found. A project's own page lists the conversations of a worktree since removed, and each
  conversation is listed on one row; resuming one of those runs where it ran, and is refused when
  that folder is gone.
- A project keeps one issues file, `projects/<id>/issues.json` in the config directory, which
  every folder of it and every workspace holding it reads. At the first start of this build the
  issues each workspace kept for the project and for its worktrees are brought in: an issue from a
  forge once, the copy changed last, with the notes of both; any other at its number, or after
  the highest number when its own was taken. Each file taken in stays where it was, renamed
  `.json.merged`, which an older build no longer reads.
- `pipelines/` in the config directory moves to `workflows/`, and `pipeline-runs/` to `tasks/`, at
  the first start of this build. The move can be cut short and run again, but there is no way back:
  an older build does not read what it leaves.
- Only one onehand runs on a config directory. A second launch hands the folder it was given to
  the running one, which opens it as a project in its front window, and exits; more windows still
  open in the one process.
- In a Telegram group on `remote.telegram.allowed_chats`, only members whose own ids are listed
  too reach the app. Before, every member of a listed group could prompt and answer permissions.
- A schedule whose tools let its agent open workers with *Open session* has to switch on *Open
  worker* instead: `open_session` now opens a session of the person's own, and a schedule's saved
  tools are not rewritten.
- onehand keeps every workspace itself, each in a folder of its own under `workspaces/` in the
  config directory: *Choose folder…*, *Unbind* and *Open workspace…* are gone, and the rail's
  workspace menu lists every workspace kept. At the first start of this build a workspace an
  earlier build kept in a folder of its own is copied in, its issues with it, and the folder is
  left as it was; tasks reporting into those issues follow the copy. A project's offload data moves
  to `projects/`. An older build reads none of this.
- The workspace file keeps one `[[projects]]` record per project, holding its pin, its check and
  its offload. A file in the old shape still reads, and is rewritten in the
  new one at its next save.
- onehand keeps every project itself, in `projects/<id>/project.toml` in the config directory,
  and a workspace's file names each folder's project. At the first start of this build every
  workspace's projects are brought in: the folders of one repository become one project, the
  others its worktrees, and one folder in two workspaces is one project. Each workspace file is
  copied to `onehand-workspace.toml.before-projects` before its first rewrite. A project's check
  command and build hosts move into its project file, and an offloaded folder's data moves from
  `projects/` to `offload/`. An older build still opens the workspaces, but sees none of this.

### Workflows and tasks

- A workflow takes a brief through named steps: agent steps judged by gates onehand checks itself,
  command steps, approvals, and forge steps that push the commit the check passed on, open a draft
  pull request and repair it until its status checks pass. Three ship read-only; the rest are
  edited, imported and exported in Settings ▸ Workflows, and carry an id and a version.
- Every run belongs to a task, kept as history (the newest 200 finished per project), with each
  step visit pinned as a commit under `refs/onehand/`. One task works in a checkout at a time; the
  rest queue.
- A Tasks page lists what needs attention, runs, queues and has finished, opens a task's detail
  with each visit's output and diffs, and offers Resume, Retry (from an earlier step too) and
  Dismiss. A project's check command runs as a task of its own.
- An agent step can name the agent's mode and config options (model, effort, fast…), set before
  each visit's first prompt and recorded on the visit. A value the agent does not offer blocks the
  start, or fails the run at that step; it never runs on something else.

### Issues

- Issues come from the forge (GitHub, through `gh`) or are kept in onehand, in a new Issues mode of
  the Workbench, optionally in step with the forge both ways.
- Unattended runs are gone: no trigger label is looked for, no issue is claimed or told how a run
  went, and the Issues views no longer show the work, pull request or review of an issue. A
  `[unattended]` table or a project's `unattended` key left by an earlier build is ignored and
  dropped at the next save, and a task an earlier build kept for an issue reads as a workflow task.

### Hosts

- The release tarball carries `onehand-host`, a service without a window that runs sessions on
  another machine reached over SSH. It keeps them going while nobody is attached, and keeps each
  one's transcript as a journal it replays when a laptop attaches again. onehand installs and
  updates it on the host itself.
- A project can live on a host: *Add a project on a host…* on the rail names the host and the
  folder there. Its sessions run on the host and show in the chat pane, read back from the
  host's journal with their times; its page lists the host's sessions on that folder. A host
  on another build with sessions mid-turn is waited on until they finish, then restarted onto
  this build; *Restart now* asks before stopping them. Files, the editor, git and the
  terminal on a host, and onehand's tools, workflows, tasks, issues and unattended runs for a
  host project, are not built yet.
- Hosts are declared: Settings gains a *Hosts* page that adds a host by name, asks it over ssh
  what it is and what it has (`git`, `tar`, `cargo`, `rustc`, `dotnet`), and keeps it in
  `onehand.toml` as `[[hosts]]`. A host that is not Linux on x86_64, a Windows one included, can
  be declared but runs no sessions: the host-project and offload forms never offer it.
- A project can build on a Linux host instead of this machine. Its page names its build hosts
  and what each brings back; `onehand --build <host> -- <command>` sends the folder as it
  stands, runs the command there with its output coming back as it prints, and brings the named
  paths back when it exits 0. A session's agent has the same as the tool `build_on_host`. It
  needs only `git`, `tar` and the toolchain on the host, and nothing of onehand's.
- A project builds on a Windows host too, through its `cmd.exe`: the same flag, tool and page,
  with the command a `cmd.exe` line.
- Nothing attaches to a host declared as one that only builds, however it is asked: a project
  that named it before, or a workspace file written by hand, is told why.
- Putting the offload form away no longer stops a send of the code under way: it runs to its end,
  and what went is kept for the next offload.

### The window

- A project's page is where its sessions start. A project's row on the rail, and its tile on the
  overview, show its page even while it has sessions open, and every *New session* lands there
  with its place and agent picked; no session is made until a prompt is sent. The page carries a
  composer under *What are we building?*: the agent, its model, effort, fast switch and mode as a
  session of it last offered them (kept across launches in `agents-seen.toml`), files to attach,
  `@` and `/`, and under it *Start in* (the project's folder, a worktree, or a new worktree cut on
  send) and *Cards*. A session starts off screen and shows once its agent is up; one that never
  comes up leaves nothing on the rail, and the page says why, its words kept.
- *Cards*: a session answers its agent's cards at its level, *Manual*, *Allow edits* (a read or an
  edit inside its folder, its git and agent settings aside) or *Bypass*. A project keeps a default
  for the sessions sent from its page; every other session starts on *Manual*. On a host's page the
  chip is the host's own switch, in place of its *Settings* box.
- A past conversation continues in a new session, with its agent or another: *Continue in a new
  session…* on its row; the conversation is handed over as a Markdown file in
  `.onehand/continued/`, which git ignores, optionally asking for a summary first.
- A project's settings (check command, build hosts, worktree folder) move to Settings ▸ Projects,
  reached from *Project settings…* in its menu; the page no longer lists tasks or schedules.

- A project's page lists its worktrees: each it records with its sessions, *New session* and
  *Remove…*, which deletes the checkout only when nothing would be lost and keeps a branch that is
  not merged; checkouts git lists that the project does not record, to add; recorded ones git no
  longer lists, to forget; *New worktree…*; and the folder its new worktrees go in, a workflow
  run's included. A worktree a task removes is forgotten by its project. Its *New
  session* caret starts one in a worktree.
- A project is one row in the rail: its worktrees are no longer rows of their own, and a session
  running in one carries the branch mark and the branch's name on its line. *New session* starts
  beside the session on screen in its folder, its menu offers *In <branch>* for each worktree, a
  worktree split off a project opens with a session in it, and the Workbench, the terminal and the
  git badges follow the folder of the session on screen. A workspace file written by an earlier
  build opens with each project's entries folded into one row, pinned when any was.
- A lead's worker opened on a worktree runs on its project's row, nested under its lead, and a
  worktree the project did not hold is recorded in it as the worker's. A project brought in for
  workers alone leaves with its last one.
- onehand's tools name a project by its name, its folder or one of its worktrees' folders:
  `open_session` and `open_worker` named by a worktree start there, and `list_projects` lists a
  project's worktrees.
- A pick-one setting (the theme, an agent dialog's kinds, a schedule's days, a Plugins or Issues
  switch) is one filled track with no edge, the chosen value raised off it.
- A project has a name of its own: *Rename project…* on its row's menu and its page's dots menu
  renames it in every window holding it, and its folder keeps its own name.
- A subagent is one line in the conversation: its name, what it is doing or how it ended, its
  steps and its time. Pressed, its own conversation opens in a panel floating over the pane, with
  the previous and next subagent a press away and Escape to close it. While subagents work, a chip
  in the composer's row counts them by how they stand.
- What a step opens into is one box that never grows past sixteen lines: the rest scrolls inside
  it, with its scrollbar on the box's edge, and a line under it says how many lines there are and
  opens them whole. A command reads whole above its output, which opens at its end. A new file is
  shown plain, without a green fill on every line. A turn's changes name one file in one line,
  and several with their counts lined up at the right; *Show every diff* is gone.
- A turn that was stopped says so: one *Stopped* line, its running steps marked *stopped* in quiet
  ink rather than *failed* in red, and *Stopped after …* under the answer. A turn the agent ended
  at its output or request limit, or by refusing, is a calm card saying which, with *Continue*
  after a limit to have the agent carry on; it still counts as a failed turn in the rail. A failed
  step's exit code stops being red once its turn is over.
- While a turn works, the conversation says each thing once. What runs is told by its own words,
  in the present tense (*Running*, *Reading*, *Editing*), with a band of brighter ink crossing
  them; the turning marks are gone, and every row keeps the mark of its kind of work. A step
  that runs past ten seconds shows its own clock. The line under a live turn keeps the clock and
  the count (*2 running*) and no longer repeats the running command. A thought that took under a second reads *Thought*, and a file
  outside the project is named from `~`.
- *Delete workspace…*, in Settings ▸ Workspace, closes the window and removes the workspace and
  the issues it keeps, after asking; its projects stay on disk. Refused while a task works one of
  its issues.
- The agent pane, the Workbench and the terminal are each a box with a hairline edge and 6px
  corners on the window's fill, parted by a gap. The Workbench now stands beside the conversation
  rather than in the right dock, so the terminal runs under both. On the light palette the boxes
  are white on a grey window.
- The title bar opens on the workspace (its initial in a tile, then its name, which opens the
  workspace menu) and the rail's toggle; the search no longer has a fill. Gone: onehand's mark
  and name, the in-window File, View, Go and Session menus (every item was a key already), the
  line under the bar, and the status bar along the foot.
- The rail opens on a *New session* row with its key; its workspace bar and *Hide the rail* button
  are gone, as is the conversation header's rail button.
- An empty session greets with *What are we building?* over the composer, centred in the pane.
- The Editor's bar shows the open file alone; its name opens the project's tree in a dropdown,
  with *Close file* at its head. The Files and Markdown trees stand on the right of what they
  open, their toggles at the bar's end; lists beside their detail are parted by a line.
- The Issues tab's filter bar is as tall as the mode tabs, its search a bare field fading into the
  filters, *Clear filters* filled and *New issue* worded (its `+` alone when the tab is narrow).
- The terminal lists its shells down its right side when it holds more than one, each with its own
  live dot; its top bar names the shell open.
- Controls take 4px corners and containers 6px; badges are slimmer, still edged in their hue.
- The conversation now sits on the deepest surface, and the rail, the Workbench, the terminal and the
  bars a step over it, the other way round from before: on the dark palette the conversation is
  `#181818` under `#1f1f1f` chrome, on the light one a pale grey under white.
- The whole interface is redrawn after gpui-kit's components: its own Default Light and Default
  Dark palette with a blue primary (the terminal keeps its own sixteen ANSI colours), its corners,
  its edged buttons and fields, and its tags for states and labels, at an editor's density: 13px
  text (12px for code and the transcript's record), 26 and 22px controls, 24px rows, 35px bars.
- The window draws its own frame: a title bar holding onehand's mark and name, File, View, Go and
  Session menus (each item a command the keymap already binds and showing its key), the search
  over sessions and projects, moved up from the rail, and the terminal's and the Workbench's
  toggles, lit while their dock is open (a dot on the terminal's while a command runs behind its
  closed dock); and a status bar with the branch. The conversation's header no longer carries the
  dock buttons.
- The rail's workspace strip opens the workspace menu wherever it is pressed, and *New* fills its
  row.
- The rail takes the library's sidebar look. The transcript is flat: the prompt in a grey bubble,
  folding blocks bare with no box or rule, a run of answered cards under one hairline, a remark
  or an error as a divider across the column. A waiting permission or question is a neutral
  card, its tone in its mark and title, as wide as the composer and tucked under it, the ones
  behind peeking over it; a failed turn and a task's approval take the same neutral shape.
- A prompt longer than ten lines is cut under a fade, with *Show all N lines* beneath it to read it whole.
- Pages open under breadcrumbs, a row's actions sit behind a `⋯` that a right-click opens too,
  Settings groups stand in boxes, the Issues list has a table head, and a task's runs and a
  workflow's steps are steppers. The Workbench's modes, the terminal's shells and the editor's
  files sit on tab bars, the terminal flush with its dock as the Workbench is.
- Schedules start work by the clock while onehand is open: a session with a prompt, or a workflow
  on a brief, on a project, with its own agent, mode, model, tools and issue labels, at the times a
  cron expression names. A time missed while onehand was closed runs once when it opens, or is
  skipped; one that finds the last run still going is skipped or queued; each as the schedule says.
  Every start is a task that says it came from its schedule, and its end is a desktop
  notification. Kept in `schedules.toml`, listed and switched on or off on the *Schedules* page
  (and a box on each project's page), and made, edited and deleted there in a form.
- onehand is an MCP server on its socket. With *Outside control* on in Settings ▸ Connections (off
  by default), a program run as you lists projects, agents, workflows and tasks, opens a session
  on a project with a first prompt, starts a workflow on a brief or on one of a project's issues
  (off screen, waiting its turn as any task does), stops a task and opens a file in the Workbench.
  `list_issues` and `create_issue` read and write a project's own issues, never writing an open
  title twice. `read_workflow` and `save_workflow` hand a workflow over as TOML and take it back,
  checked as the editor's *Save* checks it; a shipped workflow is only copied. The same tools reach a session's own agent when *Agent controls onehand* is on in the
  composer's `+` menu, off by default; the status row says so while it is. No tool
  answers a card, a permission or an approval.
- A workspace overview of sessions, projects and recent conversations as cards.
- A Plugins mode in the Workbench manages Claude Code's plugins per project and globally.
- Settings is one paged modal: Appearance, Workspace, Agents (with a default agent and a test),
  Connections, Workflows and Shortcuts. Shortcuts are edited there and persisted under `[keymap]`.
- An agent is added and edited in Settings ▸ Agents through a dialog, opened by *Add agent…* or a
  row's edit mark, rather than in a form under the list.
- The transcript reads the agent's work as one line per stretch, and a turn ends on the files it
  changed. The composer is one surface above the conversation.
- The rail keeps the user's order, splits into two lists, and fades a name that runs out of room.
- `Ctrl+Shift+O` starts a new session on the current project with the default agent, and
  `Shift+Tab` in the composer steps through the session's modes.
- Terminal tabs size to their names and scroll; the terminal caches rows and coalesces redraws.
- The interface font is Be Vietnam Pro, shipped inside the binary in place of Inter.
- Copilot is a built-in agent: `copilot --acp`, found on first launch and offered in Settings. Its
  unattended runs ask before each step unless `unattended.mode` names *Autopilot*.
- The UI library moved to gpui-component 0.8. Code is highlighted in its new dark colours, and
  Markdown in the reader, an issue and a review is spaced its new way: paragraphs further apart,
  more room above a heading, links underlined. Code blocks have no edge and name no language.
- The Workbench editor sets code at the code size and zooms with its panel; `[font].monospace`
  now reaches code blocks in rendered Markdown too.
- Every button drawn as an icon alone tells a screen reader what it does, in the words of its
  tooltip: the header's, the rail's, the strips', the Workbench modes' and the dialogs' close.
- An image or file pasted into the composer is attached however the paste arrives, its field's
  right-click *Paste* included. Shortcuts no longer lists *Paste text / attach image or file*:
  the composer takes the field's own `Ctrl+V`, and a key saved for it under `[keymap]` is dropped.
- A line runs under the Workbench's strip, parting its modes from the strip a mode opens on.
- The Workbench is lighter: Files and Markdown rows stand at a 24px pitch with the name centred,
  rows and the Workbench's bars sit further in from the dock's edges, a line parts each region's
  head from its content, an issue row writes its label in the label's ink instead of a badge (a
  long label now shortens instead of running out of the row), and the editor's line numbers rest
  in the muted ink.
- A line runs down the rail's edge and the Workbench's, and the terminal's block is edged in one.
- The Issues tab and Plugins read less cramped: an issue's activity, its conflicts and what came
  before are set at the panel's text size and spaced further apart, as is a plugin's description
  and the lists in its details.
- The Editor's tab bar and the Markdown reader's bar are a step lower than the Workbench's strip.
  Editor tabs no longer shrink when crowded: the newest that fit stay, the current file among
  them, and the older ones are in an *N more* menu after them.
- The overview, Tasks, Issues, Workflows, Schedules and Settings start alike: the page's name at the
  size of the chrome around it with a line under it saying what the page is for, a hairline, then
  its search and filters, staying in place while the rest scrolls. The buttons, search and filters
  there are the small size, so they never outweigh the page's name. Issues still spans the whole
  pane; the others share one column width, Schedules widened to it.
- Every text field and search sits on a fill of its own with no edge until it takes focus; the
  composer is unchanged.
- Menus are lettered at the size of the chrome around them, a step smaller than before.
- The composer's menus (`+`, model, effort, mode, branch) look like the window's other menus: the
  same surface and edge, no title over them, and a row's detail at its end. The `@` and `/` lists
  keep their title.
- The waiting permission and question cards stand on the composer's own fill under a hairline, so
  the row under the keys, the row under the pointer and the taken one read apart in both themes.
  A permission's answers say *Allow once*, *Always allow* or *Deny*, the agent's own wording in
  their tooltips, so they stay on one line.
- Claude Code and Codex sessions show the work their agent left running in the background: an
  *N in background* button in the agent pane's header lists each item with how long it has run, and
  pressing one stops it without stopping the turn. Under the composer, a meter says how full the
  context is, and for Claude Code another says how close the account is to its 5-hour or weekly
  limit. Copilot sends none of this and shows none of it.
- Claude Code and Codex sessions draw each subagent as a card in the transcript, where the agent
  handed work off: its name and state, the first line of its task, and, while it works, its latest
  3 steps. Once it ends it folds to its state and step count, opened by a press. Its permission
  requests and questions pin above the composer under its name, and the rail counts the subagents
  a session has working. A session closed mid-subagent keeps the card as interrupted.
- Claude Code and Codex sessions offer *Fork from here* under a prompt: a new session on the same
  project, its history the conversation's up to just before that prompt and the prompt in its
  composer to send again or rewrite. The original conversation is left as it is, and files are not
  rolled back.
- A session whose agent opens workers is their lead: the rail lists its workers under its row,
  whatever project they run in, and the lead's row keeps its own state and counts *N workers need
  you*. A worker opens in the agent pane as any session, its header naming the lead that
  coordinates it, with *Back to lead*. A chip in the lead's composer row counts its workers by how
  they stand; pressed, it lists them, and a worker picked there shows. Writing to a worker takes it
  from the lead; *Give back to the lead* in its header returns it. Closing a lead
  asks first whether its workers close with it; closed alone, it puts them back under their own
  project until its conversation is opened again, which takes them back as they stand.
- A permission card shows its command whole up to about ten lines, then *Show all N lines*,
  rather than in a box that scrolls.
- A worker is named for what it works on when its lead gives a name, else for its folder, never
  for the first words of its lead's prompt.
- A session's agent opens workers with `open_worker`. `open_session` now opens a session the
  person holds as their own, in the rail and beside any session already on the project, for a
  session's agent as for an outside program, so asking an agent for a new session no longer
  gives a worker refused on its own folder.

### Fixed

- Enter in a dialog's field does what its main button does: renaming a conversation, a branch or
  a project, a new worktree, the workflow launcher, a project on a host, an offload, a build host
  and an agent. It used to close the dialog and throw away what was typed.
- A question's typed answer counts while an input method is still composing it, so *Next* and
  *Submit* no longer stay refusing over words written in Vietnamese.
- A step still running while its agent writes on below it (work handed to a subagent, a command
  left running) no longer has the blocks after it drawn over its growing output.
- A run waiting on its pull request's status checks keeps waiting when a forge answer it did not
  ask for lands; before, the run could stop looking at its checks for good.
- A worktree removed while the window it was asked from closed is kept with its task, and its
  button no longer waits for good (#319). A removal's judgement, or a review's read, whose window
  closed meanwhile lets its button go at once.
- *Look for an issue now* refused at once (no project switched on, the cap reached, a run already
  starting) says why in its window; before, the warning was dropped.
- *Answer the pull request review* asks about the task and the project's check as they are once
  the review is read, and two quick presses open one question, not two. A Retry on a check that
  never ran does nothing.
- A run whose progress could not be saved says so in its window, once until a save succeeds;
  before, it was only logged.
- Work on an offloaded project that crashes says *Something went wrong here; try again* instead of
  leaving the folder locked.
- A session restarted starts with no unread badge.
- A worker opened by a lead's tools has its 120 seconds counted from the call, and the cap of four
  counts the sessions a lead's tools opened in every window, not only the lead's own.
- A program outside onehand that asks to open a worker is told at once that only a session's agent
  can, rather than first hearing that no window is open or that the project is on a host.
- Going back to the header's history keeps the live session running out of sight until a past
  conversation is chosen, and picking one takes the picker down at once; a history still loading
  for a session restarted or closed meanwhile shows nothing.
- A session resuming a kept conversation shows as opening while the file is read, rather than
  holding the window.
- A session waiting on a busy host's sessions is no longer announced as a lost agent when its
  stream ends.
- A removed project takes its worktree and branch forms with it, and a git status still being read
  no longer brings its status back; an order put back after a failed write tells a local and a
  host project on one path apart.
- A failed note on an issue a session took up is said in its window, once; before, it was only
  logged.
- A fork whose project left the workspace while its conversation was read says so.
- *End on <host>*, once confirmed, reaches the host even when its window is closed before onehand
  has reached the host.
- A host session opened from a project page with no agent set up says a session needs an agent.
- A saved conversation whose agent is no longer set up is refused and said, and its folder is not
  added back; one live in another window is shown there instead of resumed a second time here.
- *Open session* of a task with no session, *Run workflow…* on the Issues page for a project no
  longer in the workspace or on a host, and *Run check* with a host project or no project selected
  each say why nothing happened.
- A folder handed over by a second launch, or a tool's call, that arrives while onehand is still
  starting is taken up as soon as its first window is up, rather than up to 50 ms later.
- What *Test* found beside an agent shows in every window's Settings, and a second *Test* while one
  runs starts nothing.
- A tool's new issue, and an unattended run's or a session's note that it took an issue up, show at
  once in every Issues view of the file, the Issues page included, and reach the forge of a file
  kept in step at once; a tool's issue whose title is already open writes and sends nothing.
- An issues file kept in step syncs five minutes after its last sync, once however many views are
  on it, and a sync pressed by hand puts the next one back. An unattended run's search or claim
  that synced the file counts as that last sync, and what it brought shows in the views at once.
- Working an issue whose project left the workspace, or in a workspace keeping no issues, says why.
- A save in the Editor that wrote nothing, because the file changed on disk or is gone, is said
  under that project's files only; before, its line stood over whichever project was on screen.
- A file still being read when its project was removed no longer brings that project's tabs back.
- The Telegram bridge no longer lists or reaches a session whose project runs on a host or is
  offloaded to one. Before, a prompt sent to an offloaded session went without the project's code
  and was refused, and sending it again from Telegram never helped (#308).
- Restarting a session while its last turn is still being saved no longer loses that turn when
  the save then fails: the restarted session writes the whole conversation at its first save.
- A conversation's title taken from its first prompt no longer ends on half of a backtick pair.
- The rail between a workflow's step dots draws again.
- A session shown for the first time takes the keyboard in its composer once it connects,
  instead of leaving typing to go nowhere until the composer is clicked.
- A draft put back in the composer, on switching back to a session or in a fork, keeps the caret
  after its last character.
- A subagent's steps are no longer mixed into the agent's own: each is kept in the subagent's card.
- A session whose agent still runs work in the background reads *Running* on the rail, not idle.
- What an agent writes after its turn has ended, when background work reports back, is saved to
  disk; before, it waited for the next turn's end and was lost if onehand closed first.
- A session reads *Running* while its agent answers, by itself, the background work it waited on,
  and from the moment that work ends; before, it read idle and stayed so while the agent wrote.
- A workflow step whose turn leaves background work or a subagent running is judged once that work
  and the agent's answer to it are done, not at the turn's end.
- A password on a running command's line no longer shows on the line under a live turn; the
  block above it already masked it.
- A question card asking one pick-one question carries *Skip* (and Alt+S, Esc), so passing on it
  no longer takes *Stop*, which ended the whole turn.
- A turn that failed closes on *Failed after Ns* instead of *Processed in Ns*.
- Commands that ran side by side are timed once: a stretch of them no longer reads longer than
  the turn it ran in.
- A tooltip no longer appears over a row or a mark just pressed while the pointer rests there, on
  the rail, in the Issues list and the transcript.
- A built-in workflow's Kind and Target pickers and its switches refuse a press, as its fields
  already did, instead of looking live and doing nothing.
- A workflow's description and an issue's title cut to two lines end on an ellipsis instead of
  stopping mid-word.
- The Issues, Tasks and Schedules pages' filters stand at the search's height, and a count of zero
  on *Open* or *Closed* no longer takes its state's colour.
- A conversation's header names its agent before the first prompt, as the rail does; *Recent
  conversations* counts what it holds; boxes stand evenly apart whether two to a row or stacked.
- Settings ▸ Shortcuts names each command in a plain sentence (*Show or hide the rail*), and says
  where a group's commands work once, on the group, rather than on every row.
- Config keys and a project's path are set in the monospace face, and the new-issue form says what
  it is for once.
- Settings ▸ Shortcuts groups the commands by where they work, the rail's list, each page, each
  card and the composer, instead of listing the pages' and cards' commands under *Composer*.
- The rail joining a workflow's steps is drawn in a box edge's ink, so it shows; in the hairline
  ink it was all but lost on the page.
- The *More* and *Refresh* buttons on the Workflows, Issues and schedule pages, and a step
  setting's remove, are drawn whole; they had shrunk to a dot or a sliver.
- A question card's long choice wraps clear of its Alt key instead of running under it.
- A wide Markdown table in an answer keeps its columns readable and scrolls sideways inside the
  column instead of squeezing every column to fit. Under every table, Copy takes it as Markdown and
  *Open* shows it whole in a dialog as wide as the window allows.
- A long failure from the agent (a connection error, say) wraps inside its banner instead of running
  out past the conversation's column.
- A text drag stays in the place it began: one begun in the conversation no longer runs on into the
  Workbench's Markdown, and one begun there no longer runs back into the conversation.
- A Retry pressed on an issue's task while an unattended search or a pick was looking at that issue
  no longer has the issue claimed under it, spending its review request or opening a second branch.
  The issue keeps its label for the next look, and a pick or *Look for an issue now* says it is
  being worked. An issue an older task of which is still working is never claimed either.
- An agent's file write that names no content is refused instead of emptying the file.
- An adapter that answers a turn with an error and exits says why, instead of only that it exited.
- Releasing an agent's terminal stops a command that ignores the hangup, and what it started.
- A transcript line torn by a crash no longer swallows the next save's first message; a save that
  fails is carried by the next one; a line this build cannot read is never rewritten away.
- A save keeps the config's permissions and writes through a symlink.
- An unattended claim whose comment fails still runs, instead of leaving the issue taken and idle.
- A run's report says it could not start only when it never worked, not when it failed after a
  resume.
- Close or Restart pressed mid-turn and cancelled works again on the next press.
- A lost adapter settles the steps it left running, and a step saved running comes back stopped.
- A turn the agent ended on a refusal or a limit is a failed turn, and a run fails on any failed
  turn instead of judging work that was never done.
- A fork's pull request on the run's branch name is never taken for the run's own.
- An adapter runs in a process group of its own and goes with everything it started.
- Reading the git status takes no lock, so an agent's commit does not trip over it.
- A folder cut at the file tree's cap is counted in its "more not shown".
- An agent left by its adapter gets a moment to finish before what is left of the group is killed.
- A picture goes inline only to an agent that says it reads pictures, and as a link otherwise.
- A file read asked for a range of lines gets that range.
- A method onehand does not serve is answered as missing, and a session the agent did not name is a
  failed start.
- A free unattended slot is filled at once rather than at the next tick, and labelled issues are
  read oldest first so the oldest are never the ones past the list's limit.
- A cancelled turn the agent never ends lets go of its slot after 30 seconds.
- Stop waits for a push, a pull request or a draft being taken out of draft in flight, and no pull
  request is opened after it.
- Publishing an issue that timed out says it may have been created, so it is not published twice.
- Each project's issues file has its own lock, so one slow sync holds no other project.
- The maximize key never blows up a panel that is off screen; a removed project's check field goes
  with it; a file opened from a maximized conversation is shown; and a session that cannot start
  leaves the panels on the project the rail selected.
- A large edit's diff is bounded by its size on the UI thread, and a card's +N/−M always matches
  the rows it draws.
- A closed step no longer reads its whole output on every frame; Copy on a read or a search takes
  every line, not the sixty shown; and ACP events already waiting are applied in one update.
- Settings writes only the agent or shortcut it changed, so an entry added to `onehand.toml` by hand
  while the app runs survives the next save.
- A `state.toml` that will not parse is kept as `state.toml.bad` before recents start over.
- Closing a terminal tab no longer holds the window while its shell is stopped, and an agent's
  terminal is never hung up after its process was reaped.
- Telegram keeps at most 20 messages waiting on a slow network; older ones are dropped and counted
  in one message per chat.
- An allowed chat written with spaces around its id hears announcements too.
- Tasks read at start come before any added meanwhile, and none is listed twice.
- Stopping a queued task is recorded in the run's history like any other stop.
- A step the agent announces twice is one card.
- A folded step stays folded on the right card after a resumed conversation's replay is dropped.
- A label holding a comma, a quote or a leading dash syncs as one label.
- A run's branch skips a name whose worktree folder is still on disk, and a branch past `-99` is an
  error rather than a guess.
- A file whose name is not UTF-8 gets its git badge and counts in a run's work.
- On macOS, removing a worktree sees a shell or Neovim working in it.
- One tool output keeps at most 1 MiB in the transcript and says how much more there was.
- A declared agent that is not logged in is told how in the agent's own words, when it gives any.
- *Continue* on a workflow's approval strip no longer crashes the app.
- An agent that marks its config groups with ACP's `category` (`model`, `thought_level`) gets the
  model and effort chips whatever it names them.
- A config naming an agent this build does not know loads: the entry is kept with its id, said to
  be unsupported, and refuses to start. Before, the whole file was set aside for defaults.
- A settings save writes only what it changed: comments, formatting and keys from a newer build
  stay in `onehand.toml`.

### Project

- Built-in plugins are composed at compile time from three ordered lists; the startup registry is
  gone. GitHub moved out of core into a connector plugin.

## [0.1.0] - 2026-09-07

First release. Everything below is new, so it is grouped by what it is rather than listed as one
wall of additions.

### Sessions and agents

- Every session is an [ACP](https://agentclientprotocol.com) agent, rendered as a native chat.
  Claude Code over `npx` is the default; the adapter command is configuration, so anything that
  speaks the protocol can take its place.
- A workspace groups project roots, and a root runs as many concurrent sessions as it is asked to.
  Sessions connect lazily — a workspace with a dozen roots does not launch a dozen agents at boot.
- Agent-run commands come back over ACP's terminal extension and render inline in the transcript.
- Permission requests and multiple-choice questions are answered from cards in the transcript.
- Agent-advertised pickers — mode, model, effort — are drawn in the composer.
- `@` mentions files and `/` opens the agent's own slash commands; images and files pasted onto the
  composer become attachments.

### The window

- A session-first left rail: every project row lists its sessions, each named by its conversation,
  each carrying a mark of its own shape while it is busy, waiting, finished unseen or disconnected.
- A Workbench dock with four modes — a quick editor with tree-sitter highlighting, the project's
  file tree with git status, a live Markdown reader, and Neovim in a real PTY.
- A terminal dock, one login shell per tab, whose open state is remembered per project.
- A status bar carrying only what nothing else on screen carries: the project, its branch and change
  count, the running agent, unsaved buffers and each panel's zoom.
- Per-panel zoom, two directions of maximize, and a keymap in an exact `Ctrl+Shift` namespace so
  plain `Ctrl` keys stay usable inside a PTY. The Help dialog is the whole keymap.
- Light, dark and system appearance, following the desktop while set to system.
- Multi-window: one window hosts one workspace, and opening a workspace already on screen focuses
  the window showing it.

### Persistence

- Every conversation is a directory: metadata rewritten whole, items appended, images stored by
  content hash. Written at the end of every turn, so a crash costs the turn in flight.
- Conversations can be resumed, renamed, exported as Markdown and deleted. Nothing is ever deleted
  on the app's own initiative.
- Panel arrangement, the recents list and per-workspace settings persist; sessions respawn.

### Remote bridge

- An optional Telegram bridge for when nobody is at the machine, off unless the config asks for it.
  The token is read from an environment variable or a file of its own, never from the config file.
- A chat is told about a session only after it follows one, and a chat not on `allowed_chats` is
  answered with nothing at all.
- `/sessions`, `/use`, `/follow`, `/unfollow`, `/status`, `/stop`, `/options`, `/archive`, `/open`,
  `/away` and `/here`; anything else is a prompt for the bound session.
- Parked permissions and questions go out with inline buttons and can be answered from the chat.

### Terminal

- A vendored grid plus the interaction layer upstream never had: scrollback, selection, copy and
  paste, bracketed paste, mouse reporting with a `Shift` bypass, cursor shapes, focus reporting,
  terminal replies going back to the PTY, and `OSC 52` writes.

### Project

- Built-in features are plugins registered once at startup behind a versioned Rust contract.
- `onehand-core` carries the GUI-free half and depends on no UI framework, enforced by a test.
- CI runs formatting, core tests, app tests and clippy with warnings denied.

[Unreleased]: https://github.com/jarviisha/onehand/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jarviisha/onehand/releases/tag/v0.1.0
