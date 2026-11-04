# Hosts

How a project lives on another machine: a host reached over SSH, whose host service runs the
project's sessions and their agents, and keeps them going while the laptop is off. The laptop
attaches, reads each session back from the host's journal and shows it in the chat pane as it
shows a local one.

**A host project is sessions only, for now.** Its folder lives on the host and the laptop holds no
copy of it: no file tree, editor, git status or terminal reaches it yet, and nothing of onehand's
own work (tools, workflows, tasks, issues, unattended runs, worktrees, the check command) runs on
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
  project's page shown, a host session shown, the *Add* of the form) and made again only once its
  stream has ended. Asks that arrive while one attach runs all wait on that one, so two never
  install at once. The attach runs on the ACP runtime and its answer crosses back on a oneshot.
- **The newer attach wins.** A second laptop attaching takes the host over; this one's sessions
  end on *<host> was attached from another machine*. Nothing stops on the host.
- **A lost connection** ends every session watched over it on *lost the connection to <host>*.
  Nothing tries again by itself: restarting the session attaches again.
- **Detaching never stops anything on the host.** Closing a host session in the window, removing
  its project, or quitting onehand lets go of the watch; the session and its agent run on there,
  and the project's page lists it.

## Builds and the restart offer

Every attach opens by exchanging builds (`client::attach`):

- **The same build:** attached.
- **Another build, nothing mid-turn:** the old service is shut down and the attach made again, onto
  this build.
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

## What a host project does not do

Refused or left out, each in one place:

- **The Workbench and the terminal** show *Files and terminals on a host come later* in place of
  their content, and the Workbench offers no mode; a file link pressed in a host session is an
  info toast with the same words. *Open terminal* and *Refresh Git status* leave the project's
  menus. No git status is read for it.
- **Worktrees, the check command and unattended runs** are not offered: *New worktree…*, *Work an
  issue…*, the unattended switch, *Run check* and the *Check command* field are absent, and the
  Settings list of unattended projects leaves it out.
- **The Tasks, Issues, Workflows and Schedules pages** leave it out, and starting a workflow on it
  is a warning toast, *<project> is on a host; workflows do not run there yet*. The workspace
  overview keeps it: its tile reads *On <host>* and counts its sessions only, and it is not one of
  the overview's project filters.
- **onehand's tools** do not list it, and a call naming it is refused, *<project> is on a host;
  onehand's tools do not reach it yet* ([control.md](control.md#the-tools)). A host session's
  agent is given no token, so it cannot call them.
- **The Telegram bridge** does not offer it: the bridge reads the projects on this machine only.

## Not built

The steps of the design still to come, each its own change; the gaps they leave meanwhile are in
[known-gaps.md](known-gaps.md):

- **The rest of a host project:** the file tree, the editor, git status and the terminal on the
  host.
- **The host's own UI:** picking a host from `~/.ssh/config`, browsing the host's folders, cloning,
  re-attaching by itself with a backoff, and reading a host's sessions while it is not attached,
  from a copy the laptop keeps.
- **The permission bypass:** a switch per host project under which the host answers every
  permission itself, so a session never waits on a card overnight.
- **Offloaded projects:** a folder on the laptop whose sessions run on a host against a mirror of
  it, with a key saying which side may change the code, and then a branch mode with no key.
