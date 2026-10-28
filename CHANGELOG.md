# Changelog

Notable changes to onehand, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Versions are `0.x`, and `0.x` here means what it says: the config file, the on-disk transcript
format, the window layout and the keymap have all changed without migration and will again. A
minor bump may break any of them.

## [Unreleased]

### Breaking

- `pipelines/` in the config directory moves to `workflows/`, and `pipeline-runs/` to `tasks/`, at
  the first start of this build. The move can be cut short and run again, but there is no way back:
  an older build does not read what it leaves.
- Only one onehand runs on a config directory. A second launch hands the folder it was given to
  the running one, which opens it as a project in its front window, and exits; more windows still
  open in the one process.
- In a Telegram group on `remote.telegram.allowed_chats`, only members whose own ids are listed
  too reach the app. Before, every member of a listed group could prompt and answer permissions.

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

### Unattended runs

- A project switched on works issues carrying a trigger label, or one picked by hand, as a task in
  a worktree and session of its own, off screen. Only issues the user opened are taken.
- Issues come from the forge (GitHub, through `gh`) or are kept in onehand, in a new Issues mode of
  the Workbench, optionally in step with the forge both ways.
- A parked question waits for a person and gives up its slot meanwhile; a prompt from a person
  takes the run over. The report on the issue is kept until it is delivered, and putting the label
  back answers a review on the pull request.

### The window

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
- The palette is VS Code's Dark Modern and Light Modern, the rail on its side bar's grey.
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
- The overview, Tasks, Issues, Workflows and Schedules start alike: the page's name in large type
  at the top, then its search and filters, the two staying in place over a line while the rest
  scrolls. Issues still spans the whole pane; the others share one column width, Schedules
  widened to it.
- Every text field and search sits on a fill of its own with no edge until it takes focus; the
  composer is unchanged.
- Menus are lettered at the size of the chrome around them, a step smaller than before.
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

### Fixed

- A session shown for the first time takes the keyboard in its composer once it connects,
  instead of leaving typing to go nowhere until the composer is clicked.
- A draft put back in the composer, on switching back to a session or in a fork, keeps the caret
  after its last character.
- A subagent's steps are no longer mixed into the agent's own: each is kept in the subagent's card.
- A session whose agent still runs work in the background reads *Running* on the rail, not idle.
- What an agent writes after its turn has ended, when background work reports back, is saved to
  disk; before, it waited for the next turn's end and was lost if onehand closed first.
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
- Stop waits for a push or a pull request in flight, and no pull request is opened after it.
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
