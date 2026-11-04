# Hosts

How a project lives on another machine: a host reached over SSH, whose host service runs the
project's sessions and their agents, and keeps them going while the laptop is off. The laptop
attaches, reads each session back from the host's journal and shows it in the chat pane as it
shows a local one.

**A host project is sessions, files, git status and terminals, for now.** Its folder lives on the
host and the laptop holds no copy of it: the file tree, the editor and git status read it there and
its terminals run there, but nothing of onehand's own work (tools, workflows, tasks, issues, unattended runs, worktrees, the check command) runs on
it. What is refused, and how, is under *What a host project does not do*.

## A host

- **Named the way `ssh` names it:** an alias from `~/.ssh/config`, or `user@machine`, as the person
  types it. `HostName` (`crates/core/src/workspace.rs`) carries it; `ssh` is run with `BatchMode`
  (a password prompt fails at once rather than wait on a person who cannot see it) and keepalives
  that notice a host that went away (`crates/core/src/host/client.rs`).
- **`local`, exactly, is this machine without ssh:** the same commands run through `sh -c`
  (`HostAddr.local`). It is how the owner tries host projects with no machine to reach, and how
  the smoke run and the tests reach one.
- **Only Linux x86_64.** The install asks `uname -sm` first and refuses anything else, naming what
  the host said: *<host> is not a machine this build runs on: …*.

## The host service

- **The binary is the one built beside this one:** `onehand-host` in `current_exe()`'s directory
  (`bundled_binary`, `crates/app/src/hosts.rs`). The release tarball ships the two together;
  `make dev` and `make showcase` build `-p onehand-host` before the app. Missing, an attach fails
  with *onehand-host is not beside onehand; build it with `cargo build -p onehand-host`*.
- **Installed by build:** the attach copies it to `~/.onehand-host/<build>/onehand-host` unless
  that build is already there, `<build>` a hash of the binary itself
  (`crates/core/src/host/client/install.rs`), so a rebuilt binary is never taken for the old one.
- **One per user on a host**, behind a lock in its data directory, `~/.onehand-host/data`
  (`ONEHAND_HOST_DIR` moves it), listening on a Unix socket there only its user can open, and
  logging to `host.log` (`crates/host/src/serve.rs`). The laptop runs `onehand-host attach` over
  ssh, which joins its stdin and stdout to that socket and starts the service first when none
  runs (`crates/host/src/attach.rs`).

## Attach and detach

- **One attach per host, for the whole process**, held on `Shared` (`crates/app/src/hosts.rs`):
  every window, project and session on a host shares it. It is made on first use (a host
  project's page shown, a host session shown, a host project shown in the Workbench, the *Add* of
  the form) and made again only once its stream has ended. Asks that arrive while one attach runs
  all wait on that one, so two never install at once. The attach runs on the ACP runtime and its
  answer crosses back on a oneshot.
- **The newer attach wins.** A second laptop attaching takes the host over; this one's sessions
  end on *<host> was attached from another machine*. Nothing stops on the host, and nothing here
  takes the host back by itself: the connection keeps why it closed (`HostConn::taken`), and only
  a person's restart, show or *Try again* attaches again.
- **A lost connection** ends every session watched over it on *lost the connection to <host>*.
  A session comes back when restarted. The Workbench attaches again by itself when the connection
  its files were shown over has closed (see *Files and git status*), never after another machine
  took the host.
- **Detaching never stops anything on the host.** Closing a host session in the window, removing
  its project, or quitting onehand lets go of the watch; the session and its agent run on there,
  and the project's page lists it.

## Builds and the restart offer

Every attach opens by exchanging builds (`client::attach`):

- **The same build:** attached.
- **Another build, nothing mid-turn:** the old service is shut down and the attach made again, onto
  this build. **The host's terminals end with it, unasked:** the build check counts only sessions
  mid-turn, and a service that exits closes every terminal, so each shell is hung up and takes the
  command it runs with it.
- **Another build with sessions mid-turn:** the host is left alone and the attach fails as busy.
  **The laptop waits for those sessions to go idle**, once per host however many sessions, pages
  and forms wait on it (`Waits` in `crates/app/src/hosts.rs`): it attaches again every 15 s
  (`HOST_IDLE_POLL`), and the first attach that lands, which restarts the idle service onto this
  build by itself, ends the wait. An attach that cannot reach the host ends it too, on that
  failure (from the poll only after two misses in a row, `HOST_UNREACHABLE_POLLS`, so a blip
  does not; from anything else at once): a waiting session ends as a lost one does (*Disconnected: …*, which Restart tries
  again), the page shows its failed line with *Try again*, and the form its inline error. The
  wait lasts only while something listens to it: once every session, page and form that waited
  has gone (closed, left, put away, or its window closed), the wait and its poll stop, and the
  next listener to a host found busy starts them again; a host found busy with nobody listening
  (a restart's first look) is not waited on. Each window keeps what waits there (`HostFlow`,
  `crates/app/src/shell/host_project.rs`) and, once the wait ends, restarts each session that
  waited (as after a service restart), lists the page again and carries on with the form's *Add*,
  unless that form was put away or added its project since (a form opened again is never added by
  an older wait). Meanwhile each says *Waiting for N sessions on <host> to finish before
  restarting its host service onto this build*, with *Restart now*: a session as a line in its
  transcript (a notice, not a failure), the page and the form in their own places, each only
  while the host is waited on for some. A waiting session is not lost: the rail draws it calm,
  *Waiting for <host>*, never as needing attention, and Restart on it asks the restart question
  instead of attaching again. The line goes with the wait, never into the transcript a restart
  carries on.
- **Restart now** asks one question, *Restart the host service on <host>?*, saying that the host
  runs another build of the host service, naming up to five of the mid-turn sessions by when they
  started (*a session started 3m ago*) and how many more, and that restarting stops them;
  *Restart* or keep. A yes attaches again and restarts the service whatever is mid-turn only if
  the host is still busy (`client::restart`); that attach landing ends the wait as above. A failed
  restart is said once. A host out of reach ends the wait with its reason, *Could not attach to
  <host>: …*, said at each place that waited and nowhere else. A host still busy goes on waiting,
  and the failure is said inline on a form waiting on that host, else in a toast, *Restarting the
  host service failed: …*, as is a failure with nothing waiting. On a waiting session the rail's
  menu reads *Restart the host service…*, and it asks this question. Kept, nothing changes, and
  the wait goes on.
- A service that restarts reads its sessions back from their journals, listed as ended; a session
  it had running ends on *The host service stopped while this session was running*.

## Adding a host project

- **From the rail:** *Add a project on a host…*, under *Add project…*, opens *Add a project on a
  host* (`crates/app/src/dialogs/host_project.rs`): *Host* (*An alias from your SSH config, or
  user@machine*) and *Folder* (*The project's folder on that host, from /*). *Add* refuses with
  *Name the host*, *Name the folder* or *The folder starts at /*, and while the attach runs.
- **Picking the host:** beside *Host*, a quiet caret (*Hosts from your SSH config*) opens a menu of
  the window's own host projects' hosts, in rail order, then the plain aliases of `~/.ssh/config`
  and what it `Include`s (`sshconf::aliases`, read once on the background executor as the form
  opens), each once. A `Host` pattern with `*`, `?` or `!` is left out, as is a `Host *`. The menu
  holds at most 50 (`HOST_CHOICES_CAP`) and ends on *<n> more hosts not shown* when it cuts. A pick
  fills *Host* and drops any list and failure shown. With nothing to offer the caret is not drawn.
- **Picking the folder:** *Browse* beside *Folder* (refused with *Name the host*, and while *Add*
  attaches or waits) attaches as *Add* does, the same waiting line and *Restart now* standing for
  a busy host. It starts at the folder *Folder* names when it is absolute and the host lists it,
  else at the host's home (the wire's `Home`, `HostConn::home`; a host user with no home folder
  fails the listing). It lists under the field *Folders in <path> on <host>*: an *..* row unless at
  `/`, then one row per folder, files left out, at most 200 (`BROWSE_CAP`; *<n> more folders not
  shown* counts the cap and the host's own bound together), *No folders here* when it has none,
  and *Reading the folders on <host>…* until the first listing lands. A row pressed becomes
  *Folder* and is listed in turn; a listing that fails (the folder gone, unreadable, the host out
  of reach) is the list's own line in the danger ink and keeps the list before it. A row is
  refused, as *Browse* is, while *Add* attaches or waits. Typing in *Host* drops the list, and a
  newer listing drops the one under way (`browse_host_folders`,
  `crates/app/src/shell/host_project.rs`).
- **Two ways:** a switch at the top of the form, *Open a folder* / *Clone a repository*
  (`Way`), picked while no attach runs. Switching keeps *Host* and the folder field, and drops a
  failure shown.
- **Clone a repository:** *Host*, *Repository* (*A git URL the host can clone with its own
  credentials*) and *Into* (*The folder on that host to clone into, from /*), the *Folder* field
  relabelled, which *Browse* fills as it fills *Folder*. *Clone* refuses with *Name the host*,
  *Name the repository*, *Cannot tell the repository's name from this URL* (`wire::clone_name`),
  *Name the folder to clone into* or *The folder starts at /* (`clone_refusal`), and reads
  *Cloning…* while it runs. It attaches as *Add* does, the busy-host wait included, then probes
  the clone's folder (`clone_target`, *Into* joined with the repository's name) with `Probe`
  *before* cloning, so one already there reads *There is already <path> on <host>* and is never
  touched. Otherwise the host clones (the wire's `Clone`, `HostConn::clone_repo`,
  `crates/host/src/files.rs`) and the form adds that folder. The clone is `git clone` run by the
  host service as the host user, so it uses the host's own git and ssh credentials, never this
  machine's; it has no terminal and no stdin (`GIT_TERMINAL_PROMPT=0`, `GIT_ASKPASS` empty,
  `SSH_ASKPASS_REQUIRE=never`), so a repository that wants a password fails instead of waiting.
  The laptop waits up to 15 minutes for it (`CLONE_WAIT`). A clone that fails has its partial
  folder removed on the host. A failure stands inline: *There is already <path> on <host>*, or
  *Could not clone on <host>: <why>*, git's last line without its `fatal:`.
- **Add** trims both fields, attaches (installing the service if needed), asks the host whether
  the folder is a directory there (`hosts::is_dir`, the wire's `Probe`), then adds the project and
  selects it. A failure stands once, inline under the fields: the attach's own words, *Could not
  ask <host> about the folder: …*, or *There is no folder <path> on <host>*. A host on another
  build with sessions mid-turn keeps the form waiting, *Add* refused, with the waiting line and
  *Restart now* in place of a failure; the wait's end carries the *Add* on.
- **Adding one that is already there selects it.** A root is told apart by host and path together
  (`Workspace::add_host_root`); a host's path is kept as typed, never resolved on this machine.
- **Kept in `onehand-workspace.toml` as `<host>:<path>`**, one string in `roots` and, keyed the
  same way, in `pinned`, `unattended` and `checks` (`root_key`, `parse_root_key`,
  `crates/core/src/workspace.rs`). An entry that is not absolute and has a host before its first
  `:/` is a host root, so a file written before hosts still reads, a host named by an IPv6 address
  (`fe80::1`, `user@[fe80::1]`) keeps its colons, and a local and a host folder on one path are two
  projects that both survive a reload. Restarting onehand shows the project again;
  it attaches when it is shown, as a session connects lazily.

## Sessions

- **A new session on a host project opens on the host.** The laptop sends the agent's spec with
  its `env` cleared, since a laptop's variable names a laptop path or a secret; the host resolves
  the agent against its own PATH and the account signed in there, and the laptop's
  `AgentSpec::launch` never runs. Whatever an open runs into there is prefixed *On <host>:*, an
  agent the host cannot start among it.
  The warm adapter is not started for a host project.
- **The project's page lists the host's sessions on that folder** (`HostConn::list`, filtered by
  folder; `crates/app/src/chat/pane/project_page/host.rs`), newest first, capped at 50
  (`HOST_SESSIONS_CAP`), read each time the page shows and on its *Refresh*. A press attaches the
  session in the chat pane, or shows the session already watching it. The page's head names the
  folder `<host>:<path>`, and it has no *Tasks*, *Schedules* or *Settings* box, which are this
  machine's; what the page draws is in [screens.md](screens.md#pages-without-a-session).
- **Shown in the chat pane through the same code as a local session**
  (`crates/app/src/chat/session/host.rs`): the host's events stand in for a local adapter's.
  Prompts, cancels, card answers and picks go to the host and into its journal.
- **Restart** re-watches the session if the host still runs it, its journal being the whole
  transcript. A session the host has ended opens a new host session resuming the agent's own
  conversation (`Opening::Resume` with the agent's session id from the transcript), or a fresh one
  when there is none, with the old transcript kept on screen until the agent's replay lands. After
  the host service's own restart, one the host has ended with nothing on screen to carry (a
  session pressed on the page whose attach that restart ended first) is read from its journal
  instead, so it shows rather than a blank new one; its next Restart resumes it. Any other
  Restart of an ended session with nothing carried (its agent was not found, or died before
  naming a conversation) opens a fresh one. A host that cannot say what it runs ends the restart on *Could not read the sessions on <host>: …*
  rather than risk a second session beside one still running.
- **Nothing of a host session is written on this machine:** its chat has no store, so the host's
  journal is the only copy, and the header's past conversations, which are this machine's, are not
  offered on a host session.
- **A fork is refused**, *Not on a host yet*, on the prompt's fork control.
- **Attachments are not sent.** A prompt goes to the agent without them, and the transcript says
  *Attachments are not sent to a host yet; the prompt went without them*.

## The journal and replay

- **One JSON line per entry**, `{"at":<unix seconds>,"entry":…}`, appended and never rewritten
  (`crates/core/src/host/journal.rs`); everything that shapes the transcript, in the order the host
  saw it: what the agent sent and what the person asked. A line written before lines kept their
  time reads with `at` 0.
- **A watch replays the journal from the start, then goes live** without a gap or a duplicate; the
  laptop rebuilds the conversation by putting the replay through `Chat::apply`, so a host session
  has the one rule for what a transcript is that a local one has.
- **Times come with the replay.** Before an entry whose second differs from the last one said, the
  watch yields `AcpEvent::At(secs)`; `Chat::apply(At(t))` stamps everything after it with `t`
  until the next one. Live entries carry their time too. A prompt typed here is stamped with this
  machine's clock.

## Files and git status

- **One seam, `Disk`** (`crates/core/src/disk.rs`): this machine's disk or a host's. The Files
  view, the editor and the git sweep call its four blocking methods (list a folder, read, save,
  git status) and do not know which they have. A host's call goes over the attach
  (`crates/core/src/host/client/files.rs`), where the host runs the same tree, editor and git
  functions on its own disk (`crates/host/src/files.rs`), answered off its stream loop so a slow
  one holds up no other call.
- **Only the Editor, with Files, is offered on a host project.** Markdown, Neovim, Issues and
  Plugins are not in the Workbench's strip (`on_host` on `WorkbenchModeSpec`,
  `crates/workbench/src/lib.rs`; `offered` in `crates/app/src/workbench/panel.rs`); an active mode
  that is not offered gives way to the first that is.
- **The Workbench attaches when a host root is shown** (`show_host_files`,
  `crates/app/src/shell/roots.rs`). Meanwhile it says *Attaching to <host>…*; on a host on another
  build with sessions mid-turn it says the waiting line and carries on when the wait ends; a
  failure stands as a line with *Try again*, which attaches again. With an attach already live the
  files show at once, with no waiting line.
- **A dropped attach is made again only when it is needed:** on window activation and at a turn's
  end, and only when the connection the Workbench shows has closed (`Workbench::attach_again`). One
  still attaching, or whose attach failed, is left to that attach or to *Try again*. One the host
  closed because another machine attached is not made again: the Workbench says *<host> was
  attached from another machine* with *Try again*, which takes the host back.
- **Git status is read for a host root only while its attach is live** (`hosts::live`,
  `crates/app/src/shell/git_sweep.rs`), in the same pass as the local roots and kept by path, so
  opening the window never attaches to every host. A host that is not attached shows no git
  status until it is shown. Once read it shows where a local root's does: the rail's line, the
  project page's header, the workspace overview's tile, the status bar and the tree's badges. A
  host root on a path a local root in the workspace also has gets none, so the path-keyed status
  never mixes the two. Having a status offers nothing more on a host project (see below).
- **A folder that cannot be read** says *Could not read <folder> on <host>: …* under the rows
  (`crates/files/src/view.rs`), and the line clears when that folder lists or is folded. The host
  answers a folder that is missing or that it cannot open with an error, not an empty listing
  (`crates/host/src/files.rs`), and a host that dropped says so the same way, so neither draws an
  empty tree that looks real.
- **A buffer saves only to the machine it was read from:** each Editor buffer keeps the `Disk` it
  was read through (`crates/editor/src/buffers.rs`), so a save never lands on the other side of a
  local and a host project on one path. The mtime guard runs on the host, against its clock, and a
  stale save is a conflict as a local one is. A failure reads *On <host>: …*.
- **A file link pressed in a host session opens in the editor**, as a local one does.
- **Refresh Git status** is offered on a host project, in the rail's menu and the project page's.
  **Open terminal** is offered in the rail's menu, as for every project (the project page's menu
  leaves it out for every project); it opens the dock on the host's terminals (see below).

## Terminals

- **The host runs the shell** (`crates/host/src/terms.rs`): `$SHELL -l`, or `/bin/sh -l`, in the
  project's folder, with `TERM=xterm-256color`, `COLORTERM=truecolor` and the service's login
  `PATH`. The laptop's dock draws it (`crates/app/src/terminal/host.rs`) and sends keys, resizes
  and closes over the attach (`crates/core/src/host/client/terms.rs`).
- **Each terminal keeps its last 1 MiB of raw output.** A watch replays what is kept, then goes
  live. A replay of a buffer that dropped older output starts with *[Earlier output was not
  kept]*.
- **A program printing faster than the attach carries its output is slowed down,** as in a
  terminal of this machine: past 256 KiB queued for the attach and not yet written to it
  (`TERM_IN_FLIGHT`), the host stops reading the terminal until the attach catches up. A terminal
  nobody watches is never held up; it keeps its last 1 MiB as above.
- **Terminals run on while the laptop is away.** Showing the dock on a host project, on arrival
  or by opening it, lists that folder's terminals on the host and opens a tab for each; with none
  and the dock open, it starts one. Meanwhile the dock says *Attaching to <host>…*, and a failure
  or a busy host shows as it does in the Workbench (`attach_waiting`, `crates/app/src/hosts.rs`).
- **A newer watch takes a terminal over.** With two windows on one host project, the other
  window's tab ends, and it comes back when that window shows the project or opens the dock again.
- **Close against exit.** Closing a host tab always asks *Close this terminal?*, because the dock
  cannot tell whether a command is running. Closing ends the shell on the host (SIGHUP, a grace
  period, then SIGKILL) and the terminal leaves the list at once. A tab whose stream ended (the
  shell exited, the connection was lost, a newer watch took over) leaves the dock, a tab not on
  screen when it is next selected, and closes nothing on the host. Quitting, removing the project and a lost connection never close a host
  terminal.
- **No removal warning.** Host tabs report no running processes to the removal warnings.

## What a host project does not do

Refused or left out, each in one place:

- **Markdown, Neovim, Issues and Plugins** are not offered in the Workbench on a host project.
- **Worktrees, the check command and unattended runs** are not offered: *New worktree…*, *Work an
  issue…*, the unattended switch, *Run check* and the *Check command* field are absent, and the
  Settings list of unattended projects leaves it out.
- **The Tasks, Issues, Workflows and Schedules pages** leave it out, and starting a workflow on it
  is a warning toast, *<project> is on a host; workflows do not run there yet*. The workspace
  overview keeps it: its tile reads *On <host>*, then its git status when it has one, counts its
  sessions only, and is not one of the overview's project filters.
- **onehand's tools** do not list it, and a call naming it is refused, *<project> is on a host;
  onehand's tools do not reach it yet* ([control.md](control.md#the-tools)). A host session's
  agent is given no token, so it cannot call them.
- **The Telegram bridge** does not offer it: the bridge reads the projects on this machine only.

## Not built

The steps of the design still to come, each its own change; the gaps they leave meanwhile are in
[known-gaps.md](known-gaps.md):

- **The host's own UI, what is left:** re-attaching by itself with a backoff, and reading a host's
  sessions while it is not attached, from a copy the laptop keeps. Picking a host, browsing its
  folders and cloning are built (*Adding a host project*).
- **The permission bypass:** a switch per host project under which the host answers every
  permission itself, so a session never waits on a card overnight.
- **Offloaded projects:** a folder on the laptop whose sessions run on a host against a mirror of
  it, with a key saying which side may change the code, and then a branch mode with no key.
