# Known gaps in this build

What this build does not do yet, and why: a missing feature nobody wrote down reads as a bug in
the ones that exist. What workflows, tasks and unattended runs do not do is listed at the end of
[workflows.md](workflows.md#not-built), [tasks.md](tasks.md#not-built) and
[unattended.md](unattended.md#not-built-on-purpose).

- **Meta text on a well, some status words and the focus ring fall under WCAG AA.** The palette is
  gpui-kit's own and is not changed here. For text: the library's `muted_foreground` on its
  `muted` (about 4.4 on the light palette), and status ink, a tint's words and a diff line's sign
  on some surfaces they land on, held at `HUE_INK` in `crates/app/src/theme.rs`. For a control:
  the focus ring, the library's grey, at about 2.5 on the light page. Fixing them means changing
  the library's values, which this palette does not do.
- **Some window behaviour has no automated test**: Settings' drafts kept across leaving the page,
  a failed write said once, and the keyboard handed back when a dialog closes. The app runs
  window tests through gpui's harness (the terminal dock, the Workbench panel), but none covers
  these; they are checked by hand.
- **A submenu's row is lettered a step larger.** The library draws it itself, past `menu_row`,
  at its own size, so *Change scope* in the Plugins mode's ⋯ stands a step over the rows around
  it. Fixing it means submenus of the app's own. For the same reason a group's heading in a menu
  stands as tall as a row: the library sets that height on every row drawn as an element.
- **A refused menu row cannot say why.** The library's menu row takes no tooltip, so a row that
  refuses (*Move up* on a workflow's first step) is drawn refusing without its reason, which its
  place has to say.
- **A tooltip of a row or a mark comes back only once the pointer leaves it.** After a press it
  stays away until the pointer leaves the control, not as soon as it moves from where it was
  pressed: gpui's own tooltip, which these use, counts the one kept away as shown and offers no
  way to ask again from outside. The library's controls follow the rule as written.
- **A fork splits the conversation, not the files.** What the agent changed after the point stays
  on disk. Only a prompt forks: not an answer, a tool step or a subagent card, and there is no
  switching between branches inside one conversation. A prompt of attachments only has no
  controls under it, so no fork; a conversation saved before message ids were kept offers none.
  Copilot does not fork.
- **The Workbench's file tree has no search.** A file in a closed folder is reached by opening the
  folders above it.
- **Dragging a rail row has no edge autoscroll**, and the keyboard's row in the rail is not
  scrolled into view. A project past the bottom of a full rail has to be scrolled to first.
- **A button whose words are its children is announced as nothing.** The library names a button by
  its `.label` alone, so the composer's model, effort, mode and branch chips, `tab_select`'s tabs,
  an ask card's *Skip* and the rail's auto-run pill read on screen and say nothing to a screen
  reader. Icon-only buttons are named through `named`; these need an `accessibility_label` with
  the words they draw, or a label in place of their children.
- **The rail's workspace strip has no name of its own for a screen reader.** It is the library's
  sidebar header, which takes the hover fill a rail row takes but becomes an element with an id
  only inside its own render, so no role or accessible name can be set on it from outside; it is
  read by the workspace's name drawn in it. A `Button` would take a name but draws the ghost's
  hover, not the rail's; fixing it means a header of the app's own, or a name setter in the library.
- **A session's age on the rail starts again at every launch.** It counts from when the session
  took its present state, and that moment is kept in memory only: an archive records when a
  conversation last changed, not when it last changed state.
- **A rail session row says nothing about the lines it changed.** No source reports a diff per
  session: the git status is the project's, shared by every session working in it.
- **The keyboard barely reaches into the transcript.** Its fold lines and activity rows take no
  focus, so a block opens by pointer only. Its buttons (Copy, the show-more controls, a diff's
  path) take Tab, but only while drawn: the transcript is a `gpui::list` that draws the rows on
  screen, so a tab order through it skips what is scrolled away. The pinned cards and the
  composer take the keyboard whole.
- **There is no search in the transcript**, and the removal was deliberate rather than pending. It
  matched whole *items* and never occurrences, so a word said ten times in one answer was one hit
  with no mark on the word itself, and a hit in text a block had truncated was counted, scrolled to
  and still not on screen. Worse, the two halves disagreed about what the transcript *is*: the pass
  read the whole model while the render plan drops a parked permission or question, which are drawn
  above the composer — so a query matching one was counted in "3 of 7" and Next moved the number and
  nothing else. Rebuilding it means per-occurrence offsets through the markdown renderer, which is
  the same span machinery a drag-selection across blocks would need; the two should be built
  together or not at all.
- **A lost adapter is reported by the rail's mark and its own transcript alone.** The transcript
  ends on a *Disconnected* notice with the Restart key, and the remote chat hears of it where one
  is set up; the conversation header used to carry the same condition in words, and the badge
  that did went with everything else that row was saying twice. It is deliberately kept off the
  desktop too — an agent that has stopped answering is a standing condition rather than news, and
  a notification for one would fire again on every reconnection attempt. So with the rail hidden
  (`Ctrl+Shift+B`) or a panel maximized, a dead agent in a session not on screen is announced
  nowhere in the window. The two ways out, neither taken yet, are to put
  the mark back on the header for that one condition — not the whole badge, since what the badge
  otherwise said is on the running line already — or to let this one kind of news reach the desktop
  after all, which means deciding how often it may repeat.
- **No command palette** (`Ctrl+Shift+P`). It is a feature — a command registry plus a filtered
  popup — not a keymap entry.
- **The completion popup has no argument step.** A command that takes one is accepted like any
  other and leaves the caret after it; there is no chip for the chosen command, no trailing chevron
  saying an argument is coming, and no Backspace-returns-to-the-list. `Tab` accepts the highlighted
  row rather than completing the common prefix. The keys that do work — `Up`/`Down` wrapping,
  `Enter`, `Esc`, and a click, which go through one router so neither can act where the other
  cannot — are bound `ChatComposer > Input` and claimed only while a list is open, since gpui's
  dispatch reaches the focused input before any outer wrapper.
- **An accepted mention is plain text, not a token.** It inserts the whole path, so a long one is
  as wide as it reads; there is no single-unit deletion and no hover carrying the full path. That
  needs the input to own a span it treats atomically, which `Input` does not offer.
- **An exit status only exists for a command run through ACP's terminal extension.** The protocol
  carries one nowhere else, so an adapter reporting a failure as a plain `tool_call` has no code to
  give and the row says `failed` rather than `exit N`. Recovering it from the output was considered
  and refused: the code is in the footer this app itself appends, so parsing it back is parsing our
  own wording, and a number got that way is wrong the first time the wording moves. `ToolItem`
  carries it instead, lifted off the terminal at the one moment both are in hand — the turn-end
  flatten, after which the terminal is gone. `mock_terminal_agent.js` exits 101 on purpose so the
  path is reachable without breaking a real build.
- **What a turn's closing summary cannot say, and where the data would have to
  come from.** The summary is the block a finished turn ends on: the files it wrote, its `+N −M`
  and how long it took. *Renames* are absent because ACP's diff section is `{path, old, new}`
  and carries no second path — an adapter reports one as a delete and an add,
  so a fourth verdict would be one `turn_changes` could never return. *Test and
  lint results* are absent because nothing in the protocol is structured: a run
  is a `tool_call` whose output is text, so counting passes means parsing
  `cargo`/`jest`/`pytest` prose, which is a rule that is wrong the first time a
  tool changes its wording. The place for it is the ACP layer — a structured
  result on `ToolCall`, filled either by an adapter that knows what it ran or by
  a declared per-tool parser — not a scan of the transcript. *Undo* is absent
  because nothing in the app writes files back: the first `old` of each path in
  a turn is the snapshot it would need, so the missing half is a write path plus
  a second snapshot for undoing the undo, and both belong in core beside
  `editor::save_blocking` rather than in a renderer. A *suggested commit
  message* is absent for a different reason — writing one means asking the
  model, and the mechanical sentence a renderer could manage ("Update 3 files")
  is worse than none; there is no commit path for a turn's work either, only `gitstat`'s read
  (a host's branch mirror commits what a turn left, but on the host, not here).
  And `Deleted` is a **guess**: a file emptied and a file removed arrive as the
  same thing, a diff section whose new text is empty, so the letter on the row
  reads the commoner of the two while the counts and the bar stay right either
  way.
- **The remote bridge does not stream the transcript.** A finished turn carries the *end* of the
  agent's last answer (`Chat::answer_tail`) and nothing else: no tool cards, no diffs, no reasoning,
  nothing mid-turn. That excerpt is there because "finished a turn" alone is a notification whose only
  content is that there is content — it costs a walk back to the machine to find out whether anything
  needs doing — and the close of an answer is where it says what it did. Carrying the whole
  conversation is a different feature with its own questions (what a tool card becomes there, what a
  diff looks like, what happens to an answer longer than a message), and half of it would be worse
  than none.
- **Only Telegram.** The layer underneath is general and `RemoteChannel` is what a second one would
  implement, but nothing else does. There is no Discord adapter and no HTTP endpoint.
- **Only the agent's prose can be selected with a drag.** The one selectable thing in the
  transcript is what goes through `TextView`, which is a *markdown* renderer — so a command, an
  output, a diff and the **user's own prompt** are all plain elements a drag slides straight past.
  Neither of the two could simply be routed through it. A diff's three columns are layout and
  markdown has no notion of them; and a prompt is drawn *as typed* on purpose, so rendering it would
  turn `**/*.rs` into bold and a backtick into a code span — the transcript misquoting the person
  who wrote it. Each carries a Copy for the whole of what it holds instead, the prompt's sitting
  outside its bubble rather than over the one short sentence it is offering.

  **Two ways out exist, and both were weighed and declined for now.** The component library does
  ship a selectable plain-text control — `Editor` (and `Textarea`) with `.readonly(true)`, which
  *"keeps the normal appearance and still can be focused, selected and copied, it only rejects the
  changes made by the user"* — and with `.appearance(false)` and no explicit height it would sit in
  a bubble and grow with its text. What it costs is an `Entity<EditorState>` per prompt cached on
  the session (beside the `TextViewState` it already keeps for each block of prose), a click on a
  prompt taking focus off the composer, and selection that still stops
  at each block's edge — a diff stays unselectable either way.

  The thorough one is `gpui_base::TextSelectionHandle` with `TextSelectionRegistration` /
  `TextSelectionRun`: a document-wide selection spanning arbitrary elements, which is what
  `TextView` itself is built on, on top of `TextLayout::index_for_position`. It keeps the diff's
  columns, needs no per-message state and steals no focus, and costs one custom element: a hitbox,
  runs projected from a `TextLayout`, and the highlight painted behind the glyphs.
- **`path:line:col` tokens in agent prose are not clickable.** The transcript renders prose through
  a cached markdown `TextViewState` and does not scan it for path tokens. Only a tool card's path
  header opens a file, and it carries no line — ACP's diff payload has no hunk offsets, and core
  does not read a tool call's `locations`. Core holds no parser for
  these tokens either: the feature is the detection pass, and a parser written ahead of it is a
  guess at an interface nobody has designed.
- **A fenced code block inside prose has no header, and cannot fold independently.** What the
  renderer opens to a caller is one `StyleRefinement` for the container and one closure for a box it
  pins to the top-right corner itself — so the surface (edge, corner, padding, size, leading) is
  ours, and a header *row* carrying a file path, a language and a copy is not: there is no slot
  above the code to put one in, the copy's position is written by the library, and
  `TextViewStyle::code_block` is one style for every block with nowhere to keep per-block fold
  state. The language is said in the corner box instead, since that is the only slot there is.
  A block past its height cap says there how many lines it holds and opens them whole in a
  dialog, because a block cannot scroll in place inside the transcript's list without taking the
  wheel from the conversation.

  **Owning the block is reachable and costs selection.** `TextView::markdown_block_parser` runs
  *before* the built-in conversion and can intercept `mdast::Node::Code`, and
  `markdown_block_renderer` then draws it — that is the supported hook, and `SyntaxHighlighter` is
  public so highlighting survives. What does not is selection: the element that carries it,
  `gpui_base`'s text `Inline`, is private, so a hand-rolled block keeps the drag only through the
  custom selection element described above. Trading a header for the ability to select code is
  the wrong way round.

  **The comment scope cannot be retinted either.** `TextViewStyle::highlight_theme` is a public
  field, but `ThemeStyle::color` inside it is private with no setter and no constructor — reachable
  only by round-tripping through its `Deserialize`, which means writing a colour literal back in,
  and a literal is the one thing the theme exists to stop.
- **The terminal has no `APP_KEYPAD`.** The numeric keypad's application mode is unimplemented,
  because gpui does not report a keypad key differently from the digit above it. The keys work; they
  always send the ordinary form. The rest of the required full-screen terminal behaviour is present.
- **The terminal's cursor does not blink**, by decision — it would mean a repaint on a timer for the
  life of every tab, in a view that otherwise draws only when bytes arrive.
- **The Plugins mode stops where the command line does.** It manages Claude Code's plugins only —
  no other agent's, so there is no agent picker and no per-agent enabling. There is no auto-update
  switch and no roll-back, since the command line offers neither and the mode writes none of
  Claude Code's files; for the same reason a scope once set on or off cannot be set back to saying
  nothing short of editing its `settings.json`. A plugin installed by managed settings is not
  listed, because the listing names a scope this build does not read. *Change scope* is an install
  and a removal, not one step. The pending banner counts only changes made in this mode, so one
  made from a terminal is not announced. The scope *Install* remembers lasts for the session. The
  catalog carries no dates and no component list before install, so there is no sort but
  popularity and no filter by kind. And the title row of a submenu (*Change scope ▸*, *Turn on for
  ▸*) draws the arrow cursor: the menu row that answers the pointer can only reach what goes inside
  a row, and a submenu's row is the library's own.
- **Settings has no interface size**, and no editor or terminal font size of its own; no
  Comfortable / Compact density; no recorder that takes a pressed key combination (a shortcut is
  typed as text); no page of its own for what unattended runs need (a section of Workspace holds
  their switches); and no export or import of the whole configuration. Connections lists
  connectors, their sign-in and outside control only; managing MCP servers
  would be a group of its own. There is no search across Settings, and none is planned.
- **A forge's own issue says nothing of its work in the Issues tab or on the Issues page.** Only
  issues a project keeps
  (local, or kept in step with a forge) get the work summary, the next action and the pull request
  line: an issue's work is matched by the issues file it is kept in and its number, and a forge's
  issue has no such file. Its task is on the Tasks page. The tab leaves the files changed, the
  commits past the base and the review block to the page.
- **The task detail's review leaves the files to the timeline.** Its *Awaiting approval* draws the
  answer and the two answers, not what the step under review changed or the check: those are the
  visits just below it, each opening onto its files.
- **A workflow being written is lost to a signal or a logout.** The app ends when its last window
  closes, and closing a window over an unsaved workflow asks first; a process ended from outside
  (a signal, the session logging out) asks nothing, since GPUI gives no hook that can refuse it.
- **The Plugins mode comes with the window, not with the agent list.** A window offers Plugins
  when a Claude Code agent was set up as it opened; one added later brings it to the next window.
  The Workbench builds its modes once per window, and nothing in it removes or adds one after.
- **Agent activity is partial.** Copilot shows no context, limit or background work, and Codex no
  limit meter, because their adapters do not report them. A subagent shows as a card in the
  transcript and in the rail's count, not in the background list, and one cannot be stopped or
  prompted on its own. Copilot shows no subagent cards.
- **An agent waiting on a timer reads idle.** An agent that ends its turn to wake later, or to
  wait on an event, reports nothing while it waits, so the rail says idle until it writes again.
  An item of background work a step starts that never ends, a server left running, holds that step
  until the run's timeout, since the step is judged only once the agent's work is done; one an
  earlier step started does not.
- **A background subagent that never ends holds later turns out of the saved transcript.** A card
  is written only once it has ended, and the file is only ever added to, so every turn after a
  card still working waits for it. They are written when the session closes, the card as
  interrupted; a crash before that loses them.
- **A lead's workers do not outlive the run of the app.** The handle, the relation and the
  worker's project are kept in memory only, so after a restart the lead opens new workers and each
  rebuilds its context from its worktree; the old conversation stays on disk, attached to nothing.
  Keeping them would mean the relation on disk, keyed by the conversation, the worker's project
  kept rather than transient, and the lead switched on again taking its workers back.
- **A lead cannot close a worker, nor pick a model per turn.** A finished worker holds its place
  under the cap until the person closes it; the refusal lists the four open. A worker runs on the
  model it was opened with; a model per turn needs agents that change model inside a session.
- **Only a session's agent leads.** An outside program may open sessions but never workers, since
  it has no session to own them by; owning what it opened for as long as its connection lasts is
  not built.
- **A worker is seen only in the lead's strip and the workers' region** (the lead's rail row
  counts those that need the person), never inside the lead's conversation, and the region's
  state (which worker is picked, a composer open) is not saved. A
  worker's compact composer has no `@` or `/` completion and no `+` menu.
- **No tool answers a card, a permission or an approval, and none cuts a worktree.** Both are on
  purpose: only a person answers what an agent parked, and the lead cuts its worktrees with git.
  The cap of four sessions a session's agent may open is not raised for leads either.
- **A host terminal's tab gone with a lost connection comes back only when the project is shown
  or the dock opened again.**
- **An open host terminal holds a restart onto a new build, an idle shell too, and only *Restart
  now* ends that wait here.** The host counts its terminals, not what runs in them, and a laptop
  waiting on a host has no attach to list or close them by: they close from a laptop on the
  host's build, or by exiting on their own. A shell whose background job keeps its terminal open
  counts after the shell has exited.
- **Two windows on one host project take the terminal from each other:** a newer watch wins and
  the other window's tab ends.
- **After a host project is removed, its tabs' reader threads stay blocked** until the terminal
  prints, exits or the attach ends.
- **A background job that keeps a closed terminal's PTY open keeps its reader thread on the
  host.**
- **Markdown, Neovim, Issues and Plugins are not offered on a host project.** Markdown would need
  its document index walked on the host; Neovim, Issues and Plugins are this machine's.
- **A host call times out after 30 s**, so a git status slower than that reads as no status.
- **A lost host is attached again by itself only while a session read from the copy or the
  project page's kept list is shown for it,** and by the Workbench for the files it shows. A host
  lost with nothing of that on screen is tried again when something on it is shown, and the rail's
  mark then reads *Not connected to <host>* without *trying again*.
- **A host that is not supported, or whose `onehand-host` is missing, is not tried again,** and
  its project row shows no mark. Trying again cannot change it; a person's Restart or *Try again*
  does after the binary is there.
- **The laptop's copies are never pruned.** A copy of a session the host has long since dropped,
  or of a project removed here, stays under `hosts/` until a person deletes it.
- **The permission bypass is changed only while the host is attached:** the page draws no switch
  until the host has said where it stands. A permission card offering no grant waits for a person
  even with the switch on.
- **A host whose data directory was wiped can reuse a session id.** A session's copy is matched to
  the host's journal by length alone, so the new one's lines after the old copy's last seq would
  be taken for the old one's. The ids are `<seconds>-<n>`, so a collision is unlikely.
- **A local and a host project on one path share the rail's state for that path.** Their fold, their
  place in the order the rail keeps, the keyboard's row and the terminal dock's open state are kept
  by path alone, so folding one folds the other. Pins, unattended runs and check commands are kept
  per host and path. The Workbench's per-path state is shared too: removing either project
  forgets, and counts in its unsaved warning, the other's Editor buffers and Files tree, and a
  file reopened from the other side's tab shows that machine's text (a save still goes only to the
  machine the buffer was read from). The window's git status is keyed by path as well, so the host
  project gets no git status while the local one is in the workspace. Fixing it means keying that
  state by the pair, as the workspace file is.
- **A replayed session's durations read short, and one turn can mix two clocks.** A thought's or a
  tool's elapsed time is measured as it arrives, so a replay that delivers a whole turn at once
  reads it as taking no time, and a tool's settle time is when the replay reached it. A prompt
  typed here is stamped with this machine's clock and the entries after it with the host's. The
  journal keeps each entry's second, so the fix is measuring those from the times the replay says
  rather than from when they arrive.
- **onehand's own work does not reach a host project or an offloaded one**
  (`ProjectRoot::runs_here`). No tool lists or reaches it, and no
  workflow, task, issue, schedule, unattended run, worktree or check command runs on it; the
  Telegram bridge does not offer it either. Each reads files, runs git or starts a process on this
  machine, and needs the host to do that work instead.
- **Attachments are not sent to a host.** A prompt goes without them, and the transcript says so:
  an attachment is a path on this machine, which the host cannot read. Sending them means
  copying them to the host first.
- **A host session cannot be forked**, *Not on a host yet*: a fork opens a new agent session from
  the old one's history, which on a host means a fork the host service carries out on its own
  journal and agent.
- **A host session is never ended from the window.** Closing it, removing its project or quitting
  only stops watching it; it runs on with its agent and stays on its project's page. The host
  service takes no call that stops a session.
- **A host session restarting onto a session the host still runs is blank for a moment.** The
  transcript is drawn from the journal alone, and the copy is yielded only once the host's replay
  begins, so it is empty until then. Keeping the old one on screen meanwhile would draw every item
  twice once the replay lands.
- **A new host session given up while it opens still starts.** The open is sent at once and the host
  starts the agent anyway, so a session closed before it was up leaves one idle agent on the host,
  listed on the project's page and attachable again. Cancelling an open needs a call the host
  service does not take yet.
- **The rows of the host's sessions on a host or offloaded project's page take no keyboard
  focus**, as the rows of a local project page's Tasks and Schedules boxes do not; they are
  reached by pointer only. Making each box row a tab stop with `Enter` opening it would fix all three.
- **A clone running past 15 minutes reads as failed on the laptop while git may go on.** The
  laptop gives up waiting after `CLONE_WAIT`, but the host service sets no deadline of its own on
  `git clone`, so the clone can finish there, leaving the folder, which the form's next *Clone*
  then refuses as already there. A host-side deadline, or a call to cancel one, would close it.
- **The folder rows in *Add a project on a host* take no keyboard focus.** They are reached by
  pointer only, as the Files tree's rows are; *Browse* and the caret are tab stops, the rows are
  not. Typing the path in the field is the keyboard's way.
- **The host menu reads a plain subset of `~/.ssh/config`.** An `Include` globs only `*` in the
  file name, not `?` or `[]`, and the hosts of a `Match` block are not offered; an alias ssh would
  resolve that way is typed instead.
- ***Send to <host>* takes one row.** The Files tree has no multi-selection; a folder carries
  everything under it. What it sent is not remembered, so a file git ignores is not kept in step.
- **The manifest is each file's size and modification time.** A file touched without being
  changed stops a copy back as a change would; hashing the files would close that.
- **A commit made here while the code comes back loses its ref.** A file saved meanwhile is put
  back and stops the copy, but nothing under `.git` is: the commit keeps its objects, and the ref
  takes the host's.
- **Every copy reads every file on both ends** (`rsync --checksum`), `.git/objects` included, so a
  big repository's copy up and back takes a while each time; skipping the objects from the
  checksum is the way out if it is felt.
- **`.git` objects are never pruned on the way back.** A commit only this machine had keeps its
  objects, and so does one the host dropped; `git gc` here clears them.
- **A backup holds this machine's versions, not a way back.** Restoring one after *Take the host's
  code* is copying its files over by hand; backups are never cleared. A backup skips a link to a
  folder, and does not keep `.git/logs`.
- **A past conversation from before a project was offloaded opens readable, but its agent cannot
  carry it on:** it resumes on the host, which has no record of it, and fails there. A new session
  is the way to go on.
- **An offloaded project's prompt queued behind a turn can be refused by the host** when the code
  came back at that turn's end before the queued prompt went: the transcript says to send it
  again, and sending it again sends the code up first.
- **A locked shell drops what is typed, and what a program asks of the terminal too** (a cursor
  query, say), which a program waiting on its answer notices. The lock is at the writer, which
  both pass through.
- **Removing an offloaded project from the workspace stops its code coming back.** What its host
  holds stays in the mirror, the key with it; offloading the folder again brings it back as any
  copy back does.
- **Offloading is per window.** A folder offloaded in one window is locked in every window holding
  it, but another window's project on it still runs its sessions, workflows and runs here. One
  folder open in two windows is rare; refusing the offload then is not built.
- **A key handed to the host whose answer was lost** leaves this machine holding it as far as it
  knows: the next prompt finds the host holding it and goes without sending, so edits made here
  meanwhile reach the host only once the code has come back (it stops on them, safely).
- **An agent that leaves its host's branch gets no turn-end commit.** A branch mirror whose HEAD
  is on another branch, or mid-merge, cherry-pick or rebase, is left as the agent left it, and
  *Update host from laptop* refuses until it is back on its branch.
- **The host's own git settings can fail *Update host from laptop*.** The merge runs with the
  host account's global git config, so a `merge.ff=only` or a hooks path set there can refuse it.
- **A branch mirror holds only the history of this folder's last commit:** the laptop's other
  branches, its tags and its `origin` are not there, so an agent cannot fetch or push from it.
- **The host's branch's ref here is never pruned,** nor the branch mirror: *Run here again*
  leaves both.
- **A sync mode changes only through *Run here again*** and offloading again.
- **The branch count on a project's page waits for the next check** (a turn's end, or the 30 s
  check); showing the page does not fetch.
- **A host another machine attached is never attached again by the offload check,** for this
  run, even after that machine lets it go: a project whose code it holds stays locked until
  *Check again* or anything else attaches to it.
- **Offloaded projects are checked one after another,** so one copy, or one attach to a host out
  of reach, that hangs holds back the checks of the others. The code comes back at a turn end or
  the 30 s check, not at an attach.
