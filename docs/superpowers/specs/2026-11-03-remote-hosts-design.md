# Remote hosts: sessions that live on a machine reached over SSH

Status: design, decided with the owner on 2026-11-03; step 1 built (see *Order of work*).

## Why

The owner works on many projects that build and test constantly, .NET repositories above all, and
those builds eat the laptop. They also want to shut the laptop down while a session is mid-way
through work, and today that kills the agent: closing the last window quits onehand, the adapter
dies with it, and the owner fears losing the session's context.

What they want: the agent, the repository and the builds run on another machine that stays on (a
VPS, a desktop at home, anything reachable with `ssh`). onehand on the laptop becomes the screen
onto that work: it attaches, shows the project as if it were local, and detaches whenever the
laptop closes. The work carries on without it.

Prior art, as the owner and I discussed it: tmux on a VPS keeps the session but gives only a
terminal. VS Code Remote-SSH gives the full UI but the session ends with the laptop. Codespaces and
the agents' own cloud modes keep running but on someone else's machine, where a heavy .NET setup is
hard to bring. Nothing joins all four: your own machine, a session that survives the laptop, the
full UI, and code that comes back to the local repository by itself.

Success, in the owner's words: the morning after, the agent finished the work it was given and did
not stop half-way or wander off. The commonest reason an agent "stops" overnight is a card nobody
answers, so not parking while nobody watches is part of the goal (see *Permission bypass*).

## Terms

New words this feature brings. They go into `CONTEXT.md` when the feature is built.

- **Host**: a machine onehand reaches over SSH and runs sessions on. Named by what `ssh` accepts
  (an alias from `~/.ssh/config`, or `user@host`).
- **Host service**: the onehand process that runs on a host with no window. It owns that host's
  sessions, their agents, their transcripts and terminals, and runs on while no laptop is attached.
- **Attach / detach**: the laptop connecting to a host service and showing its work; and letting go
  of it. Detaching never stops anything on the host.
- **Host project**: a project whose folder lives on the host. The laptop holds no copy of it.
- **Offloaded project**: a project whose folder lives on the laptop and whose sessions run on a
  host, against a copy of the folder the host keeps (its **mirror**).
- **The key**: for an offloaded project in mirror mode, which side may change the code right now,
  the laptop or the host. Exactly one holds it.

## Goals

1. Both kinds of project go through the same host service. An offloaded project is a host project
   plus code that travels both ways; it adds a layer on top and is not a second path.
2. For a host project the laptop shows the same things as for a local one: sessions and their
   transcripts, the file tree, the editor, the terminal dock, the git status. Everything runs and
   reads on the host.
3. Sessions on a host run on while the laptop is detached, asleep or off. On attaching again the
   laptop shows the whole transcript, including everything that happened while it was away.
4. Setup works like VS Code Remote-SSH. The person picks a host from `~/.ssh/config` or types
   `user@host`. On first attach onehand copies its host service to the host, starts it and keeps it
   at the laptop's version. Nothing is installed by hand except the agents (see non-goals).
5. A window can hold local projects, host projects and offloaded projects side by side, and one
   host can run many projects at once.
6. **An offloaded project is a local project whose sessions run elsewhere.** Its file tree, editor,
   terminal dock and git status show the laptop's folder, as for any local project, and only its
   sessions run on the host. (Decided with the owner.) Each sync brings the host's code back into
   that folder, so the tree always shows the newest code. A person who wants to run commands on the
   host by hand uses a host project. A terminal on the mirror can come later, if it is missed.
7. An offloaded project brings its code back by one of two **sync modes**, chosen per project:
   - **Mirror (1:1).** Both sides end up identical, and the new code lands in the laptop's working
     folder with no review. The key decides who may change the code (see *Mirror and the key*).
   - **Branch.** The host works on a branch of its own, and the laptop fetches that branch. The
     person merges it when they choose.
8. The person can send chosen files from the laptop to the host by hand. This is how files that
   `.gitignore` keeps out of the copy reach the host when a build needs them
   (`appsettings.Development.json`, `.env`, secrets).
9. A **permission bypass** switch per host or offloaded project, on by default, so that a session
   left alone does not stop on a card.

## Non-goals (this design)

- Installing agents on the host, or signing them in. The owner installs and signs in every agent on
  the host themselves; onehand has no safe way to carry credentials there yet. A host where the
  chosen agent is not found says so plainly, naming the command it looked for.
- Running agents in containers. For now the host itself is the sandbox. Containers come later, as an
  option per project.
- Watching or prompting from a phone. The Telegram bridge stays in the laptop app and does not see
  host sessions.
- These onehand features stay local-only in the first version: workflows and runs, Tasks,
  schedules, unattended runs, leads and workers, the Neovim mode, Issues, and worktrees. A host or
  offloaded project offers only plain sessions. The onehand tools an agent gets through the control
  socket are not offered to host sessions either.
- Surviving a reboot of the host. If the host service dies, its live sessions end. Their
  transcripts stay on the host, and they resume the way a local session resumes after onehand
  restarts.
- Hosts that are not Linux.
- Two laptops attached to one host at once (see *One laptop at a time*).

## Anti-goals (a failure even if it works)

- **Silently losing work in the code that travels back and forth.** No sync may overwrite a change
  the person or the agent made without saying so. When in doubt it stops and asks.
- **Opening a port.** Nothing on the host listens on the network. The only way in is the person's
  own SSH.
- **Secrets travelling on their own.** A file that `.gitignore` excludes goes to the host only when
  the person sends it.
- **A detach that stops anything.** Closing the laptop, losing Wi-Fi or quitting onehand never ends
  a host session or a turn.

## Constraints

These come from the repository's invariants (AGENTS.md).

- `onehand-core` stays GUI-free and dictates no async runtime. The host service is built from core.
  It must never link GPUI: a VPS has no display, and the GUI libraries may not even load there.
- A rule both front ends share lives in core, not per call site. That covers how a transcript is
  built from agent events (`Chat::apply`), how it is stored (`chat/store.rs`), and how a card is
  answered. On the laptop, a host session's transcript is built by `Chat::apply` too, from the
  host's journal.
- `items.jsonl` stays append-only and is written at every turn end. On a host, the journal (below)
  is append-only too, and written as each entry arrives.
- Every UI change follows the `ui` skill and the contracts in `DESIGN.md`, `docs/screens.md` and
  `docs/ui-patterns.md`.

## How it works

### The host service

- **A separate binary built only from core.** My call: a new crate (for example `crates/host`,
  binary `onehand-host`) depending on `onehand-core` and never on GPUI. The release tarball ships
  it beside `onehand`. That makes "the host service never links a GUI" something the build
  enforces, not a promise.
- **Installed side by side, by build.** onehand copies the binary over SSH to
  `~/.onehand-host/<build>/onehand-host` on the host (like VS Code's `~/.vscode-server`), where
  `<build>` is a hash of the binary itself, so a rebuilt binary is never mistaken for the old one.
  A copy goes into place only once it runs and names its own build. The service keeps its data
  under `~/.onehand-host/data/` (mode 0700): each session's journal, mirrors, and its own state.
- **One per host user.** It runs detached from SSH (it outlives the session that started it),
  holds a lock file, and listens only on a Unix socket in its data directory, with mode 0600, as
  the laptop's control socket does.
- **Reached through SSH.** The laptop runs the system `ssh` binary (so `~/.ssh/config`, keys,
  agents and jump hosts all just work) with a command that relays stdio to the socket:
  `onehand-host attach`, the same pattern as today's `--mcp-proxy`. If no service is running, the
  attach command starts one first.
- **Build handshake.** The first message exchanges builds. When they differ, the laptop
  installs its own build beside the old one. If the old service has no busy session it restarts
  on the new one. If it has, the laptop says so, waits for those sessions to go idle, and offers
  a restart that names the sessions it would stop. My call: a mismatched service is never killed
  silently. As built, the wait is one per host: the laptop attaches again every 15 s, and the
  first attach that lands (the idle service restarted onto the new build) ends it, carrying on
  whatever waited. Meanwhile each waiting session, page and form says how many sessions it waits
  for, with *Restart now*, which asks the one question that may stop them, naming each by when
  it started.
- **What it owns:** the ACP connections (core's `acp::connect`, spawned on the host, so the agent's
  `fs/*` and `terminal/create` requests are served on the host's disk); each session's **journal**;
  the PTYs for the terminal dock; and file reads, writes, directory listings and git status for its
  projects.
- **The journal is the host's transcript.** It is one append-only JSON line per entry, holding
  everything that shapes the transcript in the order the host saw it: what the agent sent (its
  events) and what the person asked (prompts, card answers, cancels, picks). The host does not build
  a `Chat` of its own. A laptop rebuilds the transcript by replaying the journal through
  `Chat::apply`, the person's requests included. So the code that draws a local session draws a
  host session unchanged, and there is one rule for what a transcript is, not two that can drift.
  (Decided with the owner on 2026-11-03, after step 1a was built this way.)
- **A restarted service keeps its sessions.** After a crash, or after a restart onto a new build,
  the service reads its sessions back from their journals. They are listed as ended and can still be
  read in full. Each one's journal ends with one line saying the service stopped, unless the agent
  had already ended it. Resuming the agent is the laptop's to do, as it does for a local session.

### The protocol between laptop and host

- **NDJSON, one stream per attach:** one serde-tagged JSON message per line, with an id to pair
  an answer with its question. Both ends are ours and ship together, so a JSON-RPC envelope would
  buy nothing. The builder designs the messages. They must cover:
  - listing projects and sessions;
  - opening a session;
  - sending a prompt and cancelling a turn;
  - answering a card;
  - streaming a session's events;
  - on attach, replaying a session's transcript up to now, then going live without gap or
    duplicate;
  - the file and git operations the tree and the editor use;
  - opening, resizing, writing to and reading from a terminal;
  - the sync operations below.
- **The laptop is a second front end.** It renders a host session's transcript with the same
  chat-pane code it uses locally, fed by events from the host instead of a local adapter. The
  existing seams make this natural: the terminal view takes any reader and writer plus a resize
  callback; the tree, the editor and git status each go through a few blocking functions in core
  (`tree::read_dir_sorted`, `editor::read_blocking`/`save_blocking`, `gitstat::read_blocking`),
  which become "local or this host".
- **Detached is a state, not an error.** A host the laptop cannot reach shows its projects with
  what it last knew, marked as not connected, and the laptop tries again by itself. Nothing on the
  laptop is lost by a dropped connection. My call: reconnecting uses the same SSH command, with a
  backoff.

### One laptop at a time

Decided with the owner.

- **Only one laptop attaches to a host at a time, and the newer attach wins.** The laptop that was
  attached is detached and says it was attached from elsewhere, like `tmux attach -d`. Nothing on
  the host stops.
- **Every window of the one laptop shares the one attach.** It is held process-wide, on `Shared`.
- **A host project opens from any laptop,** one at a time.
- **An offloaded project belongs to the laptop that made it,** because its folder and its key are
  there. The host records which laptop that is (my call: an id the laptop generates once and keeps
  in its config directory). Another laptop sees the project listed but cannot open it.

### Agents on a host

My call, from the owner's rule that they install and sign in agents on the host themselves.

- **The laptop sends the agent's `AgentSpec` with its `env` cleared** (changed with the owner,
  2026-11-04): for a built-in agent the host resolves the preset by its id; a declared agent runs
  by the entry's command and args.
- **The host resolves the agent on its own.** It turns the preset id into a launch the way the
  laptop does (`AgentSpec::launch`), against the host's PATH, using the host user's own
  environment. Fetching the pinned adapter through `npx` happens on the host as it does locally,
  so `node` must be on the host; that falls under "the owner installs the agents".
- **The laptop's `env` is never sent.** A variable such as `CLAUDE_CONFIG_DIR` names a laptop path
  and would silently pick the wrong account on the host. The host's agent uses whatever account the
  owner signed in on the host.
- **"Agent not found"** names the command the host looked for (`npx`, `node`, `codex`…) and the
  host.

### Transcripts on the laptop

My call, from the owner's wish that the whole transcript of the time away comes back to the laptop.

- **The laptop keeps a copy of each host session's transcript in the same store format.** New
  items are appended to it as they arrive.
- **On attach the host replays from where the laptop's copy ends,** then goes live.
- **A detached host's sessions can still be read on the laptop,** up to where its copy ends, marked
  as not connected.
- **The host's copy is the truth.** If the two disagree (the laptop's copy is longer, or broken),
  the laptop drops its copy and takes the host's whole.

### Terminals on a host

My call. A host project's terminals keep running while the laptop is detached.

- **The host service keeps each terminal's raw output in a bounded buffer,** with a named cap.
- **On attach the laptop feeds that output into a fresh grid,** so the screen and recent scrollback
  come back. Output older than the cap is gone, and the terminal says so.
- **The host needs no grid of its own.** The GPUI-bound terminal crates stay on the laptop
  unchanged; only their reader, writer and resize source change.

### Adding a host project

- **Open a folder on the host:** onehand shows the host's directory tree to pick from, like "Open
  Folder" in VS Code Remote.
- **Clone:** the person pastes a git URL, and the host clones it with its own git credentials.
- The workspace remembers the project as host plus path. Restarting onehand on the laptop shows it
  again and attaches when it is shown, the way sessions connect lazily today.

### Offloaded projects

- **First copy.** onehand copies the folder to the host's mirror, `.git` included, so the copy
  works for a repository with no remote and the host needs no access to the person's forge. It
  leaves out what `.gitignore` excludes (`bin/`, `obj/`, `node_modules/`…); the host restores and
  builds those itself. My call: the file list is
  `git ls-files --cached --others --exclude-standard` plus `.git`, and the copy goes over the same
  SSH connection with `rsync`. onehand checks that `rsync` is present on both ends and says which
  side lacks it.
- **Sending files by hand.** In an offloaded project's file tree, a *Send to host* action takes one
  or more selected files, ignored ones included, and copies each to the same relative path in the
  mirror. My call: a one-off copy. onehand does not remember the file or sync it again later, so a
  secret never travels without the person doing it. It works whoever holds the key, because the
  person asked for it.

#### Mirror and the key

Only one side may change the code at a time.

- **The laptop holds the key:** the person edits freely on the laptop, and nothing on the host
  touches the mirror.
- **Sending a prompt to a session on the host** first syncs the laptop's code up to the mirror (the
  changes since the last sync, deletions included). Only then does the key pass to the host and the
  prompt go out.
- **While the host holds the key** the laptop's copy of the project is locked in onehand: the editor
  is read-only, the project's terminals take no input, and file actions that change the tree are
  off. The project says plainly that the host has the key and why.
- **When the agent finishes**, onehand syncs the mirror's code back into the laptop's folder and
  returns the key to the laptop. "Finishes" means every session of the project on that host has
  ended its turn and has no background work running. My call: background work could still write
  files after the copy.
- **If the laptop is away when the agent finishes**, the key stays with the host until the laptop
  attaches. Then the sync back runs first, before the project unlocks.
- **Changes made outside onehand.** onehand cannot stop another editor from writing to the laptop's
  folder while the host holds the key. Before syncing back it compares the laptop's folder with
  what it sent up. If something changed there, it does not overwrite: it stops, lists the files,
  and asks the person what to keep. My call, from the anti-goal on lost work.
- **How `.git` is compared** (my call): not byte for byte, since fetches and `git gc` touch it
  constantly. What is compared is the laptop's HEAD, its branch refs and its working-tree files
  against what was sent up. A changed HEAD or ref, an outside commit or a branch switch, stops
  the sync and asks, like a changed file. Object-store churn alone never stops it.

#### Branch mode

- There is no key: the laptop stays free, and the mirror's working tree is the host's alone.
- The host's sessions work on a branch of their own in the mirror. My call: `onehand/<host>`,
  created from the laptop's current commit at the first copy.
- **The laptop's code goes up again only when the person asks** (my call), with an *Update host
  from laptop* action. It copies the laptop's current commit to the mirror and merges it into the
  host's branch with git. A conflict stops, leaves the host's branch as it was, and says so. Changes
  not yet committed on the laptop never go up in branch mode, the first copy included: the branch
  starts from the laptop's commit.
- At each turn end the host commits whatever changed onto that branch, so nothing the agent left
  uncommitted is lost. The laptop fetches the branch over SSH with plain `git fetch` from the
  mirror, when it attaches and after each turn end while attached.
- Merging the branch is the person's job. onehand shows that it has new commits.

### Permission bypass

- A switch per host project and per offloaded project, **on by default**, because the point of the
  feature is work that runs while nobody watches. The person can turn it off at any time; while it
  is off, cards park and wait as they do today.
- **While on, the host service answers every permission request by allowing it**, for every agent
  alike, so no session waits on one. The answer is applied on the host, so it holds while the
  laptop is away.
- **A question cannot be "allowed".** While the switch is on, the host answers a question card by
  telling the agent to decide on its own judgement and to write down what it chose and why. The
  transcript keeps both the question and that answer, so the person reads in the morning what was
  decided. The builder picks the exact wording and how it is sent for each kind of card.
- **The cost is stated on screen where the switch is:** while on, the agent can do anything the
  host user can, `git push` and deleting files included. This goes against the unattended runs'
  rule of never running unsupervised with full rights by default. The owner chose it deliberately
  for hosts, where the machine is separate from the laptop. It does not change unattended runs.

## Order of work

These form a sequence, and each step is useful on its own:

1. The host service and the protocol for sessions only: install, start, attach, version handshake,
   sessions and transcripts that survive a detach. A host project appears with its sessions.
   Built in two parts: 1a, the host service and the laptop's client (built); 1b, host projects in
   the window (built). Calls changed on the way: the laptop keeps no copy of a host session's
   transcript yet (that waits for step 3); a host root is kept as `<host>:<path>` in the
   workspace file; and typing a host and a folder came forward into this step, from step 3. As
   built: `docs/hosts.md`.
2. The rest of a host project: file tree, editor, git status, terminal dock.
3. The host UI: picking or typing a host, opening a folder on it, cloning, showing a host that is
   not connected, and the laptop's own copy of host transcripts for reading while not attached.
4. The permission bypass switch.
5. Offloaded projects in mirror mode, with the key, plus *Send to host*.
6. Branch mode.

Where each step's PR boundaries fall is the plan's job.

## Left to the builder

- The method names and message shapes of the protocol, and how events are multiplexed on the one
  stream.
- How the chat pane, the tree, the editor and the terminal are fed from a host: where the
  local-or-host seam sits in core and in the app. Rules both sides share go in core.
- Where exactly on screen a host, its connection state, the key and the bypass switch appear,
  following the `ui` skill and the screen contracts.
- How the laptop finds an `onehand-host` binary for the host's architecture. Ship x86_64 first;
  a host on another architecture gets a plain refusal naming its architecture.
- The reconnect backoff, and how the laptop learns a turn ended while it was away.
- Updating `CONTEXT.md`, `docs/screens.md`, `docs/known-gaps.md` and a new feature document as
  each step lands.
