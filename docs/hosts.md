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
  all wait on that one, so two never run at once. A person's ask that finds an attach made by
  itself under way (which does not take the host, below) waits on it too, and takes what it lands;
  if it fails, held or otherwise, the person's own attach, taking the host, follows. Only the
  latest attempt for a host says what is known of it: one a restart replaced is not heard. The attach runs on the ACP runtime and its
  answer crosses back on a oneshot.
- **What a host is to this run** (`HostState`, `crates/app/src/hosts.rs`): never tried, attaching,
  live, **lost** (an attach failed, or a live connection closed, without another machine taking the
  host), **taken** (another machine attached; the connection keeps it, `HostConn::taken`), or
  refused. Refused is a host that cannot work at all: one that is not a Linux x86_64 machine, or
  an `onehand-host` that is not beside onehand or cannot be read (`HostFailure::Refused`). It is
  never lost and never tried again by itself, because trying again cannot change it, and it
  carries no mark.
- **The newer attach wins.** A second laptop attaching takes the host over; this one's sessions
  end on *<host> was attached from another machine*. Nothing stops on the host, and nothing here
  takes the host back by itself: only a person's restart, show or *Try again* attaches again. A
  person's attach that fails on a taken host leaves it lost, and so attached again by itself
  from then on.
- **An attach made by itself never takes the host.** The hello says whether it takes (`take` on
  `Call::Hello`, true when absent, so an older laptop still takes). A person's attach (the form,
  a show, *Try again*, Restart, the restart offer) takes; the re-attach of a lost host and the
  busy wait's poll do not. The service (`crates/host/src/serve.rs`) settles an attach once its
  first line is read, not as it connects: a hello that does not take, while another laptop's
  attach there is still live, is answered `Reply::Held` and closed, and the attach that holds the
  host goes on. The hello also names this laptop's run (`laptop`), so an attach of its own that
  the service has yet to see go (a laptop that slept, a link that dropped silently) does not hold
  it. The client turns `Held` into `HostError::Held`, the app into `HostFailure::Taken`, and the
  host is taken from then on, as if another machine had kicked it: the wait for it to be back and
  any busy wait end on *<host> was attached from another machine*, and nothing tries it again.
- **A lost host is attached again by itself.** A lost connection ends every session watched over
  it, and the host is tried again after 2 s (`RECONNECT_FIRST`), the wait doubling to 60 s
  (`RECONNECT_MAX`, `next_delay`), for as long as something listens, and never for a taken or
  refused one. What listens is a session that was reading its host's copy because the host was
  away, and the project page showing what it kept of the host's sessions; each is a `Want::Back`
  in the window's `HostFlow` (`crates/app/src/shell/host_project.rs`), joined to one loop per host
  (`hosts::back`), like the wait for a busy host, and the loop stops when the last of them goes.
  When an attach lands, each such session is restarted, and so watches the host from where its
  copy ends, and the page lists again; nothing is toasted. A host found busy at that point hands
  over to the wait for its busy sessions. A host found taken or refused stops the loop, and each
  session ends as a lost agent with the reason (the person's Restart tries by hand), the page
  keeping what it kept with the reason. A host that is lost with nothing listening, such as a
  project with no session shown and no page, is not tried until something on it is shown; the
  Workbench's own re-attach (below) is separate.
- **What the window says of it** (the marks are in [screens.md](screens.md)): the rail's host
  project row's mark and the session row's mark say of a lost host *Not connected to <host>;
  trying again* while a back wait stands (`hosts::retrying`), else plain *Not connected to <host>*
  (`hosts::not_connected`), and of a taken host *<host> was attached from another machine*. The
  session's signal where it stands for the row (a page naming the session, a folded project's
  badge) says *Not connected to its host* alone, never that it is trying again. The Workbench's host work is
  separate: its line and *Try again* are as under *Files and git status*.
- **Detaching never stops anything on the host.** Closing a host session in the window, removing
  its project, or quitting onehand lets go of the watch; the session and its agent run on there,
  and the project's page lists it.
- **Ending one does.** *End on <host>…*, in a host session's `⋯` on the rail, in the
  conversation's dots menu, and on a running row of the project page's *Sessions on <host>* (its
  `⋯` and right-click), asks *End <session> on <host>?* (*Its agent stops there, and a running
  turn is lost. The conversation stays readable on the project page.*), then sends `End`: the
  host drops the task driving the agent, which kills the agent's process, journals *the session
  was ended from the laptop* as its `Disconnected`, and lists it ended (`Hub::end_session`,
  `crates/app/src/shell/host_end.rs`). A session in another laptop's mirror is refused, as a
  request to it is. Here, the session watching it closes and the page lists again; a failure is a
  toast, *Ending <session> on <host> failed: …*. A session the host has not named yet has
  nothing there to end, and says so.

## Builds and the restart offer

Every attach opens by exchanging builds (`client::attach`):

- **The same build:** attached.
- **Another build, nothing mid-turn and no terminal open:** the old service is shut down and the
  attach made again, onto this build.
- **Another build with sessions mid-turn or terminals open:** the host is left alone and the
  attach fails as busy, since a service that exits closes every terminal, hanging up each shell,
  an idle one included, and the command it runs (the hello's `terms`). **The laptop waits for
  those sessions to go idle and those terminals to close** (which, since it has no attach to the
  host meanwhile, it cannot do itself: a terminal closes from a laptop on the host's build, or by
  its shell exiting), once per host however many sessions, pages
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
  restarting its host service onto this build* (with terminals, *Waiting for N terminals on
  <host> to close…*, or *N sessions to finish and N terminals to close on <host>…*), with
  *Restart now*: a session as a line in its
  transcript (a notice, not a failure), the page and the form in their own places, each only
  while the host is waited on for some. A waiting session is not lost: the rail draws it calm,
  *Waiting for <host>*, never as needing attention, and Restart on it asks the restart question
  instead of attaching again. The line goes with the wait, never into the transcript a restart
  carries on.
- **Restart now** asks one question, *Restart the host service on <host>?*, saying that the host
  runs another build of the host service, naming up to five of the mid-turn sessions by when they
  started (*a session started 3m ago*) and how many more, and how many terminals are open, and
  that restarting stops the sessions and closes the terminals;
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
- **Picking the host:** beside *Host*, a quiet caret (*Pick a host*) opens a menu of
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
  The laptop waits up to 15 minutes for it (`CLONE_WAIT`). A clone that fails leaves no partial
  folder: git removes the one it made. A failure stands inline: *There is already <path> on <host>*, or
  *Could not clone on <host>: <why>*, git's words without their `fatal:`.
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
  machine's; what the page draws is in [screens.md](screens.md#pages-without-a-session). A host
  that cannot be attached shows the list kept from the last read instead (*The laptop's copy*),
  said as not connected, with the why and *Try again*.
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
- **A host session has no store of its own on this machine:** what is kept of it is the copy of
  its journal (*The laptop's copy*), not the chat store a local session writes, and the header's
  past conversations, which are this machine's, are not offered on a host session.
- **A host that cannot be attached is read from the copy.** A session shown or restarted while its
  attach fails for any reason but a busy host is built from its copy, through the same rule as a
  replay, without raising a turn's end, a card's notice or a run's input
  (`offline_start`, `crates/app/src/chat/session/host.rs`). It ends on a line of the Info kind,
  *Not connected to <host>. This is the copy kept on this machine, up to <when>; it carries on
  once the host is back* (*up to* is the last entry that kept a time, and goes when none did), and
  the link is lost, so Restart tries again. Nothing kept ends the session on the failure's words,
  as before; a busy host waits as above whatever is kept; a refused host reads the copy with no
  notice and ends on the reason. The line goes with the restart that carries on.
- **A connection lost under a shown session says the same.** The cut link is settled
  (`Chat::link_cut`: the thought finalised, pending permission cards withdrawn, the link lost)
  without a failure item, then the copy's line is said with *up to just now*, and the session
  joins the back wait, so it restarts by itself when the host answers. A connection taken by
  another machine is not that: it ends as before, on *<host> was attached from another machine*.
  Either is a session that is not lost to the person: the remote bridge's *agent lost* is not
  announced, and the rail draws it calm.
- **A fork is refused**, *Not on a host yet*, on the prompt's fork control.
- **Attachments are not sent.** A prompt goes to the agent without them, and the transcript says
  *Attachments are not sent to a host yet; the prompt went without them*.

## The permission bypass

A session left alone on a host does not stand still on a card: while a folder's switch is on, the
host service answers every card its sessions park, so the work goes on while nobody watches.

- **One switch per host project, on by default,** *Answer cards on the host*, in the *Settings*
  box of its project page (see [screens.md](screens.md#pages-without-a-session)). The host keeps
  it, per folder, in `bypass.json` in its data directory, as the folders switched off; a folder
  it never heard of is on (`Hub::bypass`, `crates/host/src/hub.rs`). A `bypass.json` that cannot
  be read is not taken for none kept: the host answers no card at all and refuses to change the
  switch, saying why in `host.log`, until a person mends or removes the file. So it holds while the laptop
  is away, through a restart of the service, and for whichever laptop attaches. The page reads it
  over the attach with the session list (the wire's `Bypass`, `HostConn::bypass`) and draws no box
  while it has not; a press shows the new state at once, and a host that does not take it puts
  the switch back with a toast, *Changing the switch on <host> failed: …*.
- **What it costs is said under it, always:** an agent can then do anything the host's user can
  on that host, `git push` and deleting files included. This is deliberate for hosts, a machine
  apart from the laptop, and changes nothing for unattended runs.
- **A permission is allowed** with its narrowest grant, *allow once* before *allow always*
  (`PermissionRequest::bypass_answer`, `crates/core/src/acp/types.rs`). One with no grant to give
  waits for a person.
- **A question is answered, never picked for:** each question's free text (a select's *Other*
  box, or a text field) gets *Nobody is watching this session to answer. Decide on your own
  judgement and carry on, and in your reply say what you chose and why.*
  (`Elicitation::bypass_answer`, `BYPASS_ANSWER`). A form with nowhere to write is declined,
  which tells the agent to carry on without an answer. The transcript keeps the question and that
  answer, so the person reads in the morning what was decided.
- **Answered as the card arrives, in one journal line with it** (`Entry::Answered`,
  `crates/core/src/host/wire.rs`), so a laptop takes the card and its answer together and draws
  the card already answered. A card that is answered by then raises no desktop notice and no
  Telegram message: a session says a card waits only once the events that came with it are in,
  and only if one still does (`crates/app/src/chat/session.rs`).
- **Switching it on answers the cards already waiting** in that folder's sessions, each journaled
  as its answer alone. A card has one answer: a person's that comes after the host's goes
  nowhere, and the host never answers one a person already has. A card stays waiting past its
  turn's end (a background subagent's may), so a person's answer to it still reaches the agent.

## The journal and replay

- **One JSON line per entry**, `{"at":<unix seconds>,"entry":…}`, appended and never rewritten
  (`crates/core/src/host/journal.rs`); everything that shapes the transcript, in the order the host
  saw it: what the agent sent and what the person asked. A line written before lines kept their
  time reads with `at` 0.
- **A watch replays the journal, then goes live** without a gap or a duplicate; with no copy on
  the laptop it replays from the start, and with one only from where the copy ends (*The laptop's
  copy*). The laptop rebuilds the conversation by putting the replay through `Chat::apply`, so a host session
  has the one rule for what a transcript is that a local one has.
- **Times come with the replay.** Before an entry whose second differs from the last one said, the
  watch yields `AcpEvent::At(secs)`; `Chat::apply(At(t))` stamps everything after it with `t`
  until the next one. Live entries carry their time too. A prompt typed here is stamped with this
  machine's clock.

## The laptop's copy

The laptop keeps a copy of each host session's journal, so a host that is away can still be read
and a watch need only fetch what the copy lacks (`crates/core/src/host/copy.rs`).

- **Where:** `hosts/<host>/<session>.jsonl` in the config directory (`copies_dir`), the names
  made safe as the chat store's are. Beside them, `hosts/<host>/sessions.json` keeps the last
  list of that host's sessions that was read, written whole through a temporary file.
- **What is in it:** the journal's own lines, one JSON line per entry, `{"seq":…,"at":…,"entry":…}`,
  with the seq the host gave it. It is not the chat store's format; it is read back through
  the same rule as a replay (`copy::events`), so the conversation is the one `Chat::apply` makes
  of any entry.
- **Appended as entries are taken:** every entry a watch takes, replayed or live, is appended
  before it is yielded, and one whose seq is not past the copy's last is not written twice. A line
  a crash cut short is cut off before the next is written, and a write that fails drops the
  whole copy, so no entry is ever missing below the last; the next watch fills it again.
- **A watch says where the copy ends.** `Watch` carries `after`, the seq of the copy's last
  whole line (`crates/core/src/host/wire.rs`), and the host's `Replay` answers with the `after` it
  kept. The host replays only later lines when its journal has the `after` line (`Hub::watch`,
  `crates/host/src/hub.rs`), then the laptop yields the copy's entries first and the host's after
  them. A host that answers with no `after` is replaying all, as it does for a copy longer than
  its journal; the laptop then drops the copy and takes the whole replay, since the host's copy
  is the truth. A copy with a line that does not parse is broken: it has no last, it is dropped
  before the watch, and the host is asked for all.
- **Fresh per watch.** Each watch opens its copy anew, and a session reads one on the background
  executor, never on the UI's.
- **The list is kept per host** and is what the project page shows while the host is away
  (*Sessions*), filtered to the folder as a live one is. It is saved after each list read, and
  read when the host cannot be.
- **Never pruned:** nothing removes a copy; see [known-gaps.md](known-gaps.md).

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
  is a warning toast, *<project> runs its sessions on a host; workflows do not run there yet*. The workspace
  overview keeps it: its tile reads *On <host>*, then its git status when it has one, counts its
  sessions only, and is not one of the overview's project filters.
- **onehand's tools** do not list it, and a call naming it is refused, *<project> is on a host;
  onehand's tools do not reach it yet* ([control.md](control.md#the-tools)). A host session's
  agent is given no token, so it cannot call them.
- **The Telegram bridge** does not offer it: the bridge reads the projects on this machine only.

## Offloaded projects

A folder on this machine whose sessions run on a host, against a copy of it the host keeps, its
**mirror**. Its tree, editor, terminal and git stay this machine's (`ProjectRoot::session_host`,
`crates/core/src/workspace.rs`, gives a session its host and the mirror; `local_path` still names
the folder here).

- **This laptop has an id of its own,** made once and kept in `laptop-id` in the config directory
  (`laptop`, `crates/core/src/host/client/attach.rs`), and named in every hello. A mirror belongs
  to the laptop that made it: the host refuses another laptop anything in it (opening or watching a
  session there, a request to one, a file call or a terminal), *<path> is offloaded from another
  laptop*. A path into the mirrors spelled through a link or with `..` is refused to every laptop. The same id is what tells the host an attach is this laptop's
  own (*An attach made by itself never takes the host*), now across runs too. A config directory
  synced between two machines gives them one id, and the host then takes them for one laptop.
- **Where the host keeps them:** each mirror is `mirrors/<laptop>/<name>` in the host's data
  directory, made on the laptop's first `Mirror` call (`crates/host/src/hub/mirrors.rs`), and
  `mirrors.json` beside it keeps each one's owner, who holds the key and its generation. A
  `mirrors.json` that cannot be read is not taken for none: every mirror call is refused, and so
  is every prompt in a mirror, saying why in `host.log`.
- **The host says who holds the key.** It starts with the laptop. A prompt to a session in a
  mirror whose key the laptop holds is refused, and the refusal is said in that session: *The
  laptop holds this project's code; send the prompt again to send it up first*. Every prompt that
  goes through moves the mirror's generation on. The key goes to the host whenever the laptop
  hands it over, and back to the laptop only while nothing works in the mirror (no session
  mid-turn, no background work or subagent still running, `aside_after` in
  `crates/core/src/host/wire.rs`) and the generation is the one the laptop saw before it copied the
  code back. So a prompt that lands while the code comes back keeps the key on the host, and the
  laptop copies again later.
- **What the copy carries** (`crates/core/src/host/offload.rs`): what `git ls-files --cached
  --others --exclude-standard` lists, then `.git`. An ignored file never travels on its own. A
  name that is not UTF-8 is refused, naming it. So is a folder that is not a repository's top with
  a `.git` folder of its own (a worktree's `.git` is a file naming a path on this machine), and one
  holding another repository (a submodule, or a checkout nobody added), which git lists as one
  folder that would travel whole, ignored files and all. The host lists a mirror the same way (the wire's
  `MirrorFiles`).
- **It goes with `rsync`** over the attach's ssh options (a plain folder for the host `local`),
  told apart by contents (`--checksum`), since rsync's own size-and-second check misses a file
  changed within the second it was copied. Up, what was sent before and is gone now is deleted in
  the mirror, and `.git` is made the laptop's exactly. Back, what the mirror no longer has is
  deleted here, and `.git` comes back without deleting anything, so a commit only this folder had
  keeps its objects. `rsync` missing is said by side: *rsync is not installed on this machine* /
  *on <host>*.
- **What went is remembered** as a manifest: each listed file's size and modification time,
  `HEAD`, every ref, what is staged and the repository's settings, read *before* the code goes
  up, so an edit made while it goes reads as a change later rather than as sent. Held against the
  folder, it names what changed: a file changed, added or gone, `HEAD` or a ref moved, `index`
  (something staged or unstaged) or `config`. Objects git adds or packs on its own are not looked
  at. **The copy back checks first** (`bring_back_blocking`) and copies nothing when anything
  changed, a file the mirror carries that is here and was never sent (one this machine's git
  ignores and the host's does not) counting as changed; `take_back_blocking` is the copy a person
  agreed to. **An edit landing while it copies is put back:** what rsync writes over waits in
  `coming-back` in the project's storage, and a file there that is not as the folder was read just
  before goes back in place over the host's version; the copy then stands stopped on those files
  (`Back::Raced`), held against the host's code, so the box lists only them. Nothing under `.git`
  is put back. A `coming-back` a copy that did not finish left files in is never emptied: the next
  copy refuses, naming it, until the person has looked through it and removed it. A backup (`backup_blocking`) copies the changed files as they are, `refs.txt`
  of `HEAD` and every ref when refs moved (the objects stay, since `.git` comes back without
  deleting), and `.git/index` or `.git/config` when what is staged or the settings changed. The laptop keeps it with who holds the key, when the code last moved and what stopped a
  copy back in `offload.json` in the project's storage (`OffloadState`).

**Branch mirrors** (`crates/host/src/hub/branch.rs`, `crates/core/src/host/offload/branch.rs`)
are what an offloaded project in branch mode runs in. They have no key and no lock: the host's
sessions work on a branch of their own, and the code moves by git alone over the person's ssh.

- **Made empty, with no key.** A `Mirror` call naming a branch (`onehand/<host>`, `branch_for`)
  makes a repository of its own (`git init`) with the key on the host's side, so its prompts go
  through; handing its key over is refused, *<path> has no key; it works on a branch*.
  `MirrorState.head` is the branch's commit, once it has started.
- **Up, only a commit.** The laptop pushes its `HEAD` into the mirror as `refs/onehand/laptop`
  (`push_head_blocking`), never the folder: what is not committed, and every other branch, stays
  on the laptop. A folder with no commit is refused. `MirrorUpdate` then starts the branch from
  that commit and checks it out, or, once the branch has started, commits what was left and
  merges the commit in. A merge that conflicts is undone and says so, naming up to ten files:
  *Merging this machine's commit into <branch> stopped on a conflict in: …; the host's branch is
  as it was*, or, when what was left was committed first, *…; the work left on the host's branch
  was committed first, and the branch is as it was after that*. A mirror whose HEAD is not on its
  branch, or that the agent left in the middle of a merge, a cherry-pick, a revert or a rebase, is
  left as it is and the update refused. What the
  laptop sends must be a commit id, never a name git could take for one of its options.
- **No agent works under a merge.** An update is refused while a session there works (mid-turn,
  or with work running aside), and while it runs a prompt there is refused, *The host's branch
  is being updated from the laptop; send the prompt again in a moment*.
- **A turn's end commits what it left** onto the branch (`git add -A`, then *Work left at a
  turn's end*), as onehand (`onehand <onehand@onehand.invalid>`, whatever the host's git
  identity), so nothing an agent did waits uncommitted for the laptop. The host's `.gitignore`
  keeps build output out. A mirror on another branch, or in the middle of a merge, a
  cherry-pick, a revert or a rebase, is not committed to, and `host.log` says so. Every git write to branch mirrors goes one at a time.
- **Back, only a remote-tracking ref.** The laptop fetches the branch into
  `refs/remotes/onehand/<host>` (`fetch_blocking`, forced, no tags), which no branch here is, so
  a fetch never moves a branch of the person's, nor refuses on the one checked out; it adds no
  remote to the repository's settings. `ahead_blocking` counts what that ref has that `HEAD`
  does not. Git reaches the mirror at `ssh://<host><mirror>` with the attach's ssh options (a
  path, for the host `local`), so the host needs git on its non-interactive `PATH`.

In the window (`crates/app/src/offload.rs`, `crates/app/src/shell/offload.rs`):

- **Offloading converts the project in place.** *Offload to a host…*, in a local project's menu on
  the rail and on its page, opens a form naming the host (its menu as *Add a project on a host*'s).
  *Offload* checks `rsync` on both ends, attaches, asks the host for the mirror and sends the code
  up, then closes the project's sessions here (refused while one is mid-turn; their conversations
  stay among its past ones) and keeps `offloaded` in the workspace file, the root valued
  `<host>:<mirror>`. The key stays here until the first prompt. A mirror whose code the host
  holds already is not sent over. New sessions open on the host, in the mirror.
- **The key on send.** A prompt to a session of an offloaded project while this machine holds the
  code waits: the code goes up, the key goes to the host with the generation it had, and then the
  prompt goes. A second press waits on the same send. A failure is a toast and leaves the prompt
  in the composer and the key here; a manifest of code that went up is kept whatever the key's
  hand-over said. With the key on the host the prompt goes at once.
- **The code comes back** when a turn of a session on it ends here, and every 30 s
  (`OFFLOAD_POLL`) while the host holds it, the poll attaching to the host by itself when it is
  not attached (an attach that never takes a host another machine holds; one the host refuses
  stands in the *On <host>* box and is not tried again until *Check again*): the host asked whether anything works in the
  mirror, then `bring_back_blocking`, then the key handed back with the generation seen before the
  copy. A stop keeps the changes and the key on the host; a refused hand-back keeps the new
  manifest and the key on the host, and the next check copies again. Nothing is said on success.
- **The lock** holds while the host holds the key, while the code goes up, and until what was kept
  has been read: the editor's buffers are read only and *Save* writes nothing, the project's shells
  and its Neovim drop what is typed (at the writer, `onehand_terminal_ui::PtyTab::set_locked`, so
  a paste and an input method's commit are dropped too), and the project's git actions that write
  (*New worktree…*, *Rename branch…*) are not offered, nor taken. It holds in every window
  holding the folder. An `offload.json` that cannot be read is taken for the host holding the
  code, so the next check asks the host and a copy back stops rather than writes.
- **What an offloaded project does not do** is what a host project does not (*What a host project
  does not do*), its Workbench aside: workflows, tasks, schedules, unattended runs, worktrees, the
  check command, onehand's tools and the remote bridge reach only a project whose sessions run here
  (`ProjectRoot::runs_here`).
- **Settling a copy back that stopped.** The project page's box *Bringing the code back stopped*
  lists what changed here. *Check again* attaches if need be and checks as a turn end does; a
  file added here and removed again, or one deleted and restored, lets the code come (an edit
  undone still reads as a change, the manifest holding sizes and times). *Take the host's code…* asks first, naming the backup
  folder, `backups/<date-time>` in the project's storage (`offload::backup_dir`); yes checks once
  more, backs up what changed then (a list that grew since the box was drawn is backed up whole),
  takes the host's code over it (`take_back_blocking`) and hands the key back as any copy back
  does. A toast says where the backup is; a failure stands in the *On <host>* box. A file
  changed here after the backup stays as it is and stops the copy again, *Files changed here
  while the code came back; they stay as this machine has them*.
- **Send to <host>**, on a row of the Files tree of an offloaded project (its right-click menu,
  `crates/files/src/view.rs`, raising `Request::SendToHost`), copies that file, or that folder
  with everything under it, ignored files included, to the same place in the mirror
  (`send_path_blocking`), whoever holds the key. A toast says how it went; nothing is remembered,
  so a file git ignores is not kept in step after that.
- **Run here again**, offered only while this machine holds the code, lets the project's host
  sessions go here (they run on there until they end) and clears `offloaded`. The mirror and
  `offload.json` stay, so a later offload sends only what changed.

On a branch (`crates/app/src/offload/branch.rs`), the window's side of a branch mirror:

- **Picked in the offload form**, *Into this folder* (the mirror with a key, the default) or *On
  a branch*, and kept beside `offloaded` as `offloaded_branches` (the branch, by root; a file
  from before reads as a mirror). It changes only by *Run here again* and offloading again; each
  way has a mirror of its own on the host (the branch's is named `<stem>.branch`), so neither
  touches the other's code.
- **Offloading** attaches, asks for the branch mirror, and, unless its branch started already
  (offloaded before), pushes this folder's last commit to start it; then fetches it. `rsync` is
  not needed. A folder with no commit is refused.
- **No key and no lock:** nothing here is locked, a prompt goes at once with nothing sent first,
  no copy comes back, and `offload.json` is neither read nor written.
- **Fetched when it moved:** at a turn's end of a session on it here, and with the 30 s check,
  the laptop asks an attached host for the branch's commit and fetches only when it differs from
  the ref here, then counts what it has to merge. A check never attaches; with the host away it
  counts what was fetched, so a merge made here clears the count by the next check. The count is
  the page box's line and the rail's merge mark.
- **Update host from laptop**, in the *On <host>* box: attaches, refuses while a session there
  works (*Wait for the sessions on <host> to finish*), pushes this folder's last commit and has
  the host merge it, then fetches and counts again. A failure, a conflict among them, stands in
  the box until an update lands, whatever the checks meanwhile; a check's own failure goes with
  the next check. Success says nothing but the count. While it runs, a prompt to a session on it
  is not held here; the host refuses it while it merges.
- **Run here again** is offered whenever the branch is not being fetched or updated, and leaves
  the mirror, the host's branch and the ref here as they are.

## Not built

Every step of the design is built; what each leaves out is in [known-gaps.md](known-gaps.md).
