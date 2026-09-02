# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`onehand` is a Rust desktop GUI (**GPUI** + [gpui-component](https://github.com/longbridge/gpui-component))
that hosts AI coding agents. The window is a left **navigation rail** (workspace, New session,
projects/sessions and settings), a central **agent pane**, a right-hand **Workbench** (quick editor
and file tree), a bottom **terminal** — the last two closed until asked for — and a **status bar**
along the bottom of the frame. Work is organized as
a tree: a *workspace* groups one or more *project roots*, and each root runs one or more *sessions* —
a session is one agent bound to that root. A root can hold several concurrent sessions.

**Every session is an ACP agent.** The agent is driven over the
[Agent Client Protocol](https://agentclientprotocol.com) and rendered as a native GPUI chat. There is
no terminal-session kind — the chat is the pane's content well. Agents can still *run commands*;
those come back over ACP's terminal extension and render inline in the transcript.

> **[DECISIONS.md](DECISIONS.md)** holds the choices that reading the code will not explain: the
> locked decisions (D1–D6), the pinned revisions and *why* `gpui` must carry no `rev`, what the
> vendored terminal is and what onehand added to it, the icon rules, and what is deliberately not
> built yet. Read it before changing anything it covers.

> **UI contracts.** [DESIGN.md](DESIGN.md) is the whole-app visual contract and
> [DESIGN-ANSWER.md](DESIGN-ANSWER.md) the transcript's design language. Neither carries a palette
> any more: gpui-component's theme is the look (decision D1), so both describe *structure and
> behaviour* while every colour, radius and size is read from `cx.theme()` at the call site. They are
> binding, and source files still cite neither of them — see *Code describes; it never cites* under
> **Rules**.

## Commands

```bash
cargo run                       # the app; the positional arg seeds the workspace's project root
cargo run -- /path/to/project
cargo build --release           # LTO on; binary at target/release/onehand
cargo check                     # fast type-check
cargo test                      # all tests (app + core + the vendored terminal)
cargo test changed_line_only    # run a single test by name substring

# Headless ACP smoke test (no GUI): connect, send a prompt, print the streamed reply.
cargo run -p onehand-core --example acp_smoke
# Drive a different adapter / the terminal extension / the question (elicitation) flow:
ACP_CMD="node crates/core/examples/mock_terminal_agent.js" cargo run -p onehand-core --example acp_smoke go
ACP_CMD="node crates/core/examples/mock_ask_agent.js" cargo run -p onehand-core --example acp_smoke go
```

There is no CI. **Use the Makefile targets for `fmt` and `clippy`**, not bare cargo: `vendor/`
is a workspace member, so `cargo fmt` reformats it and `clippy --fix` rewrites it — hundreds of lines
of churn on upstream code, destroying the one property that vendor has (its diff against upstream is
exactly our patches). `make fmt` / `make lint` scope to `-p onehand -p onehand-core`.

Tests are inline `#[cfg(test)]` modules — there is no `tests/` directory.

## Architecture

### Repo layout

| Path | Crate | What |
|---|---|---|
| `crates/app` | `onehand` | the GPUI front end + the binary |
| `crates/core` | `onehand-core` | GUI-free logic: config, the workspace tree, ACP, the chat model, the remote bridge, editor rules, completion, git status, worktree rules, the directory flatten |
| `vendor/gpui-terminal` | `gpui-terminal` | a vendored terminal grid + the interaction layer upstream never had |

The workspace root is a **virtual manifest** — it owns nothing but the member list and the release
profile.

**Two invariants hold the split together:**

- `cargo tree -p onehand-core -i gpui` must keep **erroring with "did not match any packages"**.
  Core is the half that survived one front-end rewrite; keeping it framework-free is what would let
  it survive another. (Use `-i`, not `| grep gpui` — the checkout directory is still named
  `onehand-gpui`, so a plain grep matches the path and always "finds" something.)
- **Core must not dictate an async runtime.** Every blocking operation is a plain blocking function;
  anything async is a thin wrapper over it. GPUI runs on smol and has no tokio reactor, so a core
  that awaited tokio I/O directly would panic inside the UI process.

Shared rules live in core, not restated per call site: `GitStatus::label`, `AppConfig::update_in_place`,
`gitstat::read_blocking`, `RootEditors::open`, `Chat::apply`, `Away::headline`, `remote::press::option_at`.

### Library + thin binary

[crates/app/src/main.rs](crates/app/src/main.rs) is fifteen lines: install the asset source, call
`gpui_component::init` (**before anything else touches the library**), then `shell::boot`. Everything
real is in the lib so it stays reachable from tests without opening a window. Keep new logic out of
`main`.

**Only `assets` and `shell` are `pub`; every other module is private, and that is load-bearing.**
rustc's `dead_code` analysis stops at a `pub` item in a library, because something outside the crate
might use it — and nothing outside this one ever will. While the modules were all `pub`, dead code
was invisible to the compiler; making them private surfaced five dead items immediately. Tests live
inside the crate, so privacy costs them nothing. **Keep new modules private.**

[guards.rs](crates/app/src/guards.rs) holds the rules rustc has no opinion about: no glyph used as an
icon, our own event enums matched exhaustively (not `matches!`), no field assigned and never read.
**Every one was added after a repo-wide sweep found the same mistake in several places at once** —
a guard here is evidence that the rule cannot be held by intention alone, so removing one because it
is inconvenient re-opens something that has already gone wrong more than once.

### The GPUI model (what replaced MVU)

There is no central `update(Message)`. State lives in **entities** (`Entity<T>`), each rendering
itself and mutating through `cx.update`/`cx.listener`; a change is published with `cx.notify()`.
Entities talk to their owners by **emitting events** (`EventEmitter<E>` + `cx.subscribe`), which is
how a panel asks for something it has no business doing itself — the chat emits
`ChatPaneEvent::OpenFile` and the shell decides the Workbench is where that goes.

Consequences worth internalizing:

- **An entity renders where it is mounted, once.** Rendering the same entity in two places in one
  frame is not a layout trick; it is a bug.
- **Focus is a real tree.** `focus_handle.contains_focused(window, cx)` answers "is focus inside this
  panel", which is what makes panel-scoped commands possible without hand-tracked focus hints.
- **Async is `cx.spawn`.** Background work goes to `cx.background_executor()`; the result comes back
  through `entity.update(cx, …)`, which fails cleanly if the entity is gone.

### The workspace tree (the central data model)

```
Shared (global)                                  crates/app/src/state.rs
  agents · config_path · next_uid · windows · recents · acp runtime
Shell (one per window)                           crates/app/src/shell.rs
  WorkspaceWindow { workspace, git }             crates/app/src/state.rs
    Workspace { name, roots, active_root, storage_dir }   core/workspace.rs
      ProjectRoot { path, label, sessions, active_session }
        Session { spec, uid }                             core/agent.rs
  dock · chat · workbench · terminal (entities)
```

- Agent *definitions* are global: `Shared.agents` is the menu a new session spawns from, edited in
  the agent-manager dialog; each session keeps a clone of the spec it was spawned with.
- `Session.uid` is a process-wide id salt (`Shared::next_uid`), which is how a session's chat state
  survives switching roots and sessions, and how an event finds its window.
- Sessions connect **lazily**: `Shell::show_active_session` spawns an adapter the first time a
  session is actually shown. A workspace with a dozen roots must not launch a dozen agents at boot.

### Sessions are ACP only

`AgentSpec` is `{ name, command, args }` — no `kind` field. Legacy `onehand.toml` files carrying
`kind = "acp"|"terminal"` still parse: the key is unknown and serde ignores it.

### ACP client (core) and the executor bridge

[crates/core/src/acp/](crates/core/src/acp/) is a minimal JSON-RPC (newline-delimited JSON over
stdio) client behind a facade:

- `types.rs` — the data model only (`AcpRequest` in, `AcpEvent` + `ToolCall`/`Mode`/`SlashCommand`/
  `PermissionRequest`/… out); **serde-free**, so it stays a pure model.
- `client.rs` — spawns the adapter (default `npx -y @agentclientprotocol/claude-agent-acp@<pinned>`,
  the pin being `config::DEFAULT_ACP_ADAPTER` — never `@latest`, so a build is reproducible),
  runs the request/RPC loop, does `initialize` → (`session/load` resume, else `session/new`) →
  `session/prompt`, answers the agent's reverse requests (`fs/read_text_file`, `fs/write_text_file`,
  `session/request_permission`, `elicitation/create` — both parked until the user answers), and runs
  each prompt turn in its own task so the loop stays free to service permissions that arrive *during*
  the turn. Exposed as `impl Stream<Item = AcpEvent>`.
- `parse.rs` — `session/update` notifications → `AcpEvent`s.
- `terminal.rs` — the client side of the ACP terminal extension: `terminal/create` spawns a real PTY
  via `portable-pty`, output is drained on its own thread into a byte-capped buffer *and* streamed to
  the UI. This is **agent-run commands**, not a user terminal.

**Liveness:** handshake calls race `child.wait()` and the serve loop `select!`s against it, so a dead
or stuck adapter surfaces as `Disconnected` instead of hanging.

**Questions (`elicitation/create`).** Claude Code's `AskUserQuestion` reaches the client as a form
elicitation, and the adapter puts that tool in `disallowedTools` unless the client advertises
`clientCapabilities.elicitation.form` — so that capability is what makes multiple-choice prompts
appear *at all*; drop it and the model silently stops asking and guesses. Anything unrenderable (url
mode, an empty schema) is declined on the spot rather than parked, or the turn hangs on a card nobody
can fill in.

**The bridge** ([crates/app/src/acp.rs](crates/app/src/acp.rs)) is the one place tokio and smol meet.
`connect` folds its serve loop into the stream it returns — the adapter only advances while something
polls it, and dropping the stream kills the child — but that loop is `tokio::process` + `tokio::io`,
which needs a tokio reactor GPUI does not have. So the stream is driven on a tokio runtime this
module owns, and events cross to GPUI on a plain `futures` channel belonging to neither executor. The
*request* side needs no bridge: an unbounded tokio send never touches the reactor.

### The remote bridge

A second channel into the app, for the times nobody is at the machine. Same shape as the ACP bridge
above, deliberately: [crates/core/src/remote/](crates/core/src/remote/) is GUI-free and exposes the
channel as `impl Stream`, and [crates/app/src/remote.rs](crates/app/src/remote.rs) drives it on a
tokio runtime of its own with events crossing on a `futures` channel.

**The layer is general; Telegram is the first adapter.** `remote::types` is the neutral model — chat
ids are strings, a message carries text and rows of `Button`s, and `RemoteChannel::connect` folds its
serve loop into the stream it returns. `remote::telegram` is the only implementation, a long poll
plus `sendMessage` and `answerCallbackQuery`. Everything that is not the wire is pure and tested:
`access` (who may reach the app), `command` (the little language a chat drives it with), `press`
(what a button means), `secret` (where the credential comes from).

- **The token is read and never written, and it is not in `onehand.toml`.** That file is rewritten
  whole by the settings dialog and the agent manager, it is what people paste into a bug report, and
  it is world-readable because everything else in it is a preference — so a bearer credential in it
  would be printed back out on a schedule nobody chose. Two sources instead: `$ONEHAND_TELEGRAM_TOKEN`
  (or whatever `token_env` names), then `<config_dir>/onehand/telegram.token`, a file whose only
  content is the secret. The second exists because a desktop app is launched by clicking an icon and
  there is no shell in that path to have set a variable in; its permissions are checked and a
  group-or-world-readable one is complained about on stderr rather than refused, since refusing means
  a feature that silently does not work. It is still plaintext on disk, and this is not a keyring.
- **A chat not on `allowed_chats` is answered with nothing at all** — not a refusal, because a refusal
  confirms that the bot is real, that it is running right now, and that there is a list to get onto.
  **The empty list allows nobody**, so forgetting to fill it in fails closed. That same list is the
  audience for everything the app says out: a notification exists to reach somebody who has not asked
  for anything yet, so narrowing it to whoever spoke last would silence the bridge exactly when it has
  been quiet.
- **One process, one bot**, so the bridge lives on `Shared` rather than on a window — a second poll
  against one token is two clients splitting one queue. What follows is the routing problem: an
  incoming message belongs to no window, so `OpenWindow` carries a weak `Entity<Shell>` and the bridge
  asks every window in turn. The window holding the session answers; a map of uid to window kept on
  the bridge would need correcting on every open, close and restart and would be wrong in between.
- **Out.** The three moments a session stops being self-explanatory to somebody not looking at it, as
  `onehand_core::chat::Away` — a turn that finished, an ask that parked, an adapter that stopped
  answering. The sentences are core's so the desktop notification and the chat cannot drift, and
  `UserAsk::headline` still names permission and question apart underneath. **A finished turn carries
  the end of the answer** (`Chat::answer_tail`) and a parked ask carries the question, both through
  `Announcement::detail` — the line a reader on the far side needs and a reader at the window does
  not, since the desktop notification is one keystroke from the transcript and a phone is not. The end
  and not the beginning: an answer opens by restating the problem and closes by saying what was done
  about it. **The silence rules are the
  desktop's, unchanged**: a finished turn says nothing while any part of its window is in front of the
  user, while a parked ask and a lost adapter speak unless the user is looking at *that* conversation
  — an agent standing still stands still until somebody notices, and reading one conversation is when
  a dot on another row goes unseen. The moment an ask parks is still `ApplyOutcome::asked_user`, not
  `Chat::awaiting_permission`, which stays true for as long as the card is up.
- **`Shared.away` is the one thing those rules cannot work out.** All of them ask whether the user is
  looking, and answer it from the focused window and the conversation on screen — both of which stay
  true in front of an empty chair, so a window nobody is at reports every turn as read. The user says
  otherwise, and while they have said so every announcement goes out as though nothing were on screen.
  It is read in exactly one place, `ChatPane::here`, which `watching` is then built on, so the switch
  cannot reach one rule and miss another. Global rather than per window, because walking away from one
  window is walking away from all of them, and **not persisted** — a launch that came up believing the
  user was elsewhere would message somebody sitting in front of it about every turn. Thrown from the
  status bar (`Shell::toggle_away`) or from the chat (`/away`, `/here`), both through
  `remote::set_away`, since a mode with two setters is a mode that means two things. The switch is
  drawn only where a channel is live, is an eye and its absence because that is literally the question
  it answers, and is silent when off and named in the standing-condition colour when on.
- **In.** `/away` and `/here` set the presence fact above from wherever the user actually is —
  the point of having them, since the switch at the keyboard is no use to somebody who has already
  left. `/sessions` numbers every session across every window and says what each is doing, in the
  rail's own `signal_word` so one condition keeps one name. `/use <n>` points a chat at one, and
  **the number is the session's uid, not its place in the list** — a place shifts when a session
  closes, so a number read and then typed back would land on a different conversation. Anything not
  starting with `/` is a prompt for the bound session, submitted straight into it rather than through
  the composer (one composer serves the pane and it holds what the person at the keyboard was typing),
  and queued rather than refused mid-turn, since the sender cannot see that a turn is in flight.
  **A chat is bound by being told to and never by being guessed at**: one root runs as many sessions as
  it is asked to, so "the active one" moves every time somebody clicks a rail row, and a message sent
  from a train would land wherever the window happened to be pointing.
- **Answering.** A parked ask goes out with the question and inline buttons. A press carries `uid`,
  the card's position in the transcript, and the option's position in that card — positions and never
  identifiers, because the payload is capped (64 bytes) and an option id is the agent's to choose.
  **It names the exact card**, so a second permission parked before the button is pressed cannot take
  the answer, and a card already settled says so rather than sliding onto the next one. Which option a
  press means is `press::option_at`, and an index that names nothing **falls to a refusal** found by
  `PermissionOption::weight` — an answer nobody can read must not be able to grant something. The same
  `weight` decides the layout: grants on one row, refusals on the next, because on a phone those two
  are a thumb-width apart. Only a one-field single-select question becomes choice buttons; every other
  form gets *Skip* alone, which is always safe and is what stops a session standing still.
- **A dropped long poll is the normal condition, not the failure.** Cuts, timeouts and rate limits are
  retried with a widening gap inside the channel and nothing is reported upward; only a refusal
  retrying cannot fix — a token the far side rejects — ends the stream with `Disconnected`. Same
  spirit as the ACP client racing `child.wait()`: what cannot recover surfaces, what can does not.

### The chat pane

[crates/app/src/chat/](crates/app/src/chat/):

- `session.rs` — one ACP session: the `Chat` model from core, the event pump, and the front-end-shaped
  caches (parsed markdown per block, decoded images). The pump is **held**, not detached: dropping the
  session drops the task, which drops the receiver, which kills the adapter. Nothing else has to
  remember to shut an agent down.
- `transcript.rs` — one element per `ChatItem`, following DESIGN-ANSWER.md §5. Bounded (§8).
- `composer.rs` — the card the pane mounts: the input, `@`/`/` completion, attachments,
  agent-advertised selectors and Send. It draws itself and reports the send press as an event,
  because which of Send and Stop was pressed is a question about the turn, not about the click.
- `pane.rs` — what the shell mounts: session switching, the resume picker, the project page, the find bar, unseen
  badges, and the run plan the virtualized list reads.
  Its **header is the panel's only chrome** (the dock draws the conversation bare), and it is split by
  what a control is *about*. **The conversation's name is itself the menu** — full-strength ink and
  semibold against an otherwise muted row, with the hover background and a chevron whose space is held
  whether or not it is drawn, so the name does not shift under the pointer. Behind it: *Rename…*,
  *Export as Markdown…*, *Export as JSON…* (named and disabled — it is planned, and leaving it out
  would say otherwise), *Resume another conversation…*, *Restart the agent*, and then, alone in the
  danger tint, *Delete conversation* — the only entry there that ends something for good. It is a
  **The header is drawn on the project page too**, and the page no longer prints the project's name
  itself: the row names the project there and its menu is the project's (`chat::pane::project_menu`)
  — *Pin to top*/*Unpin*, *New worktree…* on repositories, *Copy project path*, *Refresh Git status*,
  then *Remove from workspace* in the danger tint. **Not a copy of the rail's**: *New session* is the
  primary button in the middle of that page and *Open terminal* is a button at the end of the same
  row, and offering either again would be the page saying one thing twice within an inch of itself.
  The two facts that menu needs — pinned, and whether it is a git repository — are pushed by
  `Shell::sync_project_facts` from the three moments either changes (arriving at a project, pinning
  one, a git sweep landing), and the arrival push must happen **after** `clear_active`, which builds
  the page's state fresh and would throw an earlier one away. Find and *Close session* are the two
  controls that go on that page, since neither has anything to act on; the terminal, the Workbench
  and the way back to a hidden rail stay, because all three are about the project and dropping the
  row took them away at the one moment there is no conversation to reach them from.
  Beside the name is a **status badge**: a pill carrying the rail's own `signal_mark` — one condition,
  one shape everywhere — and either `Chat::activity_status` (the specific sentence: which agent is
  being connected to, that approval is what is awaited) or, where there is none, the signal's short
  name from `rail::signal_word`. Colour lives in the mark and the words stay muted, so a routine
  *Working…* is not as loud as a dead agent. Busy with no activity status stays silent, because that
  means the transcript's own last block is already saying what is running — but a **lost adapter now
  says so here**, where the header used to be blank and only the rail's small triangle knew.
  The right-hand end carries the row's controls (`ChatPane::header_control`, one builder so the call
  sites cannot drift): find, the terminal, the Workbench, the way back to a hidden rail, and last
  *Close session*, offered only while there is one. The terminal button carries a **dot in success ink
  at its corner while a shell is alive** — a child process outliving a closed dock is the one thing
  the icon cannot say, and closing the window is what would end it. The fact is pushed down from the
  shell (`ChatPane::set_terminal_live`, from `Shell::sync_panel_facts` and from every project switch,
  since a shell belongs to a project); the push is guarded on both sides because a terminal notifies
  once per chunk a build prints. They are **a size up and a tone down** — big enough to aim at,
  muted so four icons in a row do not out-shout the conversation's name beside them; the library's
  hover fill brings the ink back on the one about to be pressed. **Closing is a control and deleting is
  not**, and that is the split: closing keeps every word — the transcript is written at the end of
  every turn — while deleting is the one thing the app cannot undo, so it stays behind the name, two
  presses and a warning away, and the two are never adjacent. There is no ••• — a menu button beside
  the name it acts on says nothing the name could not say itself.

The **model** is core's (`onehand_core::chat`): `Chat` + `apply(AcpEvent)`, the conversation store, the
find pass, and the activity-run rules. `ChatSession` derefs to it, which is what lets the whole
renderer read `chat.items` / `chat.busy` without knowing where the model lives.

### Workbench

[crates/app/src/workbench/](crates/app/src/workbench/) — one dock panel, two modes:

- **Editor**: a quick editor, not an IDE. Buffers here, rules in core (`onehand_core::editor`): the
  size bound, the tab set, the **mtime guard**, labels, blocking read/save. Highlighting is
  gpui-component's tree-sitter over a deliberately small grammar set (decision D3) — no LSP.
  Reopening an already-open file **never reloads it**: a second click on a path must not discard what
  the user just typed.
- **Files**: the active root's tree, `tree::visible_rows` from core, bounded per directory and in
  total, `.git` skipped. Rows carry git state as one-letter badges; a directory holding changes gets a
  dot. Indentation is padding by depth, not nested containers — hundreds of nested rows are hundreds
  of wasted elements.

State is per project root, so switching roots swaps the whole thing.

### Terminal panel

[crates/app/src/terminal.rs](crates/app/src/terminal.rs) over `vendor/gpui-terminal`. A tab per root,
spawned lazily; dropping a tab drops its PTY, so the child dies with it and there is no separate
shutdown to forget.

**Whether the dock is open is per root too** (`Shell::terminal_open` / `terminal_root`), because
everything below it already is: switching projects files the dock's live state under the project
being left and restores whatever the arriving one was left in, and a project it has never been opened
in gets it closed. An open dock that stayed open across a switch showed the new project an empty
panel where the old project's shells had been, which reads as the terminal having lost them rather
than as their having been left behind — and inheriting *open* into an unvisited root would reproduce
exactly that. The handover happens in `Shell::follow_terminal_dock`, called from
`show_active_session` and nowhere else: four controls can toggle this dock (the key, the conversation
header, the project menu, the dock's own chrome), so the state is **read off the dock at the switch** rather
than mirrored at each of them. A session switch inside one project is not a handover — it files the
live state and moves nothing, or it would fight a user who had just opened it. The Workbench keeps
one state for the window: its state is per root as well, but every root has a file tree, so an open
Workbench after a switch is never the empty panel this exists to prevent.

`TERM`/`COLORTERM` are set app-side — `alacritty_terminal` ships a
`tty::setup_env` it never calls, and an inherited foreign `TERM` breaks key and colour detection in
anything curses-based.

The grid installs an `EntityInputHandler`, which is what makes a composing input method (Vietnamese
telex, pinyin, kana) work: without one the platform never opens an input context and the raw keys
fall through to the shell. Its counterpart is that `on_key_down` stops propagation on every key it
encodes — the platform hands an unclaimed key's character to the input handler, so not stopping there
types everything twice.

**Neovim mode does not exist in this build** (decision D4; it is P8), and neither does
`Ctrl+Shift+N`.

### Window shell

[crates/app/src/shell.rs](crates/app/src/shell.rs) owns the window: the rail plus a `DockArea` whose
centre is the chat, right dock the Workbench, bottom dock the terminal — and, under both of them, the
status bar.

- The **rail** ([rail.rs](crates/app/src/rail.rs), gpui-component's `Sidebar`) is app chrome and
  lives *outside* the dock, so a layout restore cannot lose it. It is **session-first**: every folder
  row lists its sessions, each row selecting root *and* session in one click. A session row is named
  by its **conversation** (`Chat::conversation_title` — the first prompt, or a rename), falling back
  to the agent's name until it has been prompted; the agent's name rides in the suffix only where
  more than one is configured. A trailing mark appears only while that session carries a signal, and
  each of the four has a **shape** of its own rather than a tint of one shared dot: a spinner for
  busy, a warning icon for a lost adapter, an accent dot for a parked question, a success dot for a
  turn finished unseen. Every mark names itself in a tooltip (`rail::signal_hint`) — colour alone is
  a code that has to be learned first and cannot be read at all by someone who does not separate red
  from green.
  *Add project…* closes the Projects group. **The selected project row is marked whether or not it
  holds the session on screen**, only the selected project starts expanded, and a project with no
  sessions expands into a *Start a session* row rather than into nothing. Branch and
  change count ride in the suffix — the count as a badge, not a coloured number — with the full
  branch, the count in words and the root's path in a tooltip. The primary *New session* button names
  the project it would start in, in its tooltip.
- **The rail's two header rows are a block one step above the list** (`rail::lead_row`): the workspace
  name and *New session*, taller, at a larger text size and a weight up, with the identity's icon in
  full ink rather than muted. At the list's own scale they read as its first two entries, which is
  what they are not. **The 16px icon column does not move** — only the row around it grows, or the
  header's labels would sit a few pixels off every label below them.
- **The workspace identity row *is* the switcher** (`rail::workspace_menu`) — the whole row opens the
  menu, and nothing marks it but the hover, the pointer and the tooltip: no chevron, because a caret
  on the rail's topmost row competed with the primary action directly below it. The menu is the
  recents list — each row named by
  its folder with the parent path beside it (shortened from the *front*, since a path is read from
  its tail), the one on screen checked and unpickable — then *Open workspace…* and *New workspace…*.
  **Nothing is replaced in place**: every entry funnels through `Shell::open_recent` /
  `open_or_focus`, so a pick opens another window or focuses the one already showing that folder.
  The same list is still in Settings, where the storage binding it depends on lives; what changed is
  that reaching it no longer means opening a dialog two surfaces away from the name it changes.
  A row can be a menu trigger at all because of `controls::MenuTrigger`: the library opens a menu
  from anything `Selectable`, `Stateful<Div>` is not, and both of those are other crates' — so the
  newtype that answers `Selectable` for a row is what stops the target being the icon at its end.
- **Both row kinds carry one ••• menu on the active row**, never a ✕. A project's holds *Pin to top*
  / *Unpin*, *New session*, *New worktree…* (git repositories only), *Open terminal*,
  *Copy project path*, *Refresh Git status*, then,
  separated and in the danger tint, *Remove from workspace* (still guarded by a second click while
  the root has live sessions or unsaved buffers). A session's holds *Rename…*, *Restart the agent*,
  *Export as Markdown…* and, in the danger tint, *Close session* (guarded only mid-turn, since the
  transcript is written at the end of every turn — `Shell::close_session`, also `Ctrl+Shift+W`). The
  session menu is **also** the row's right-click menu, on every row and not just the active one;
  Restart and Export select the session first, so they always act on what is on screen.
- **Two `Dialog`s have no trigger: renaming a conversation, and splitting a project into a
  worktree.** Every other dialog is opened by a control that carries `Dialog::trigger`; these are
  opened from a menu entry that is gone by the time they appear, so `Shell::renaming` /
  `Shell::worktree_draft` being `Some` is what puts each on screen and Esc/Cancel/close must all
  clear it. A rename archives immediately rather than at the end of the next turn.
- **A worktree becomes a project root of its own**, added to the same workspace and selected
  (`Shell::commit_worktree`). It is a whole second checkout, so its file tree, terminal, git status
  and sessions all differ from the original's — and every one of those is already keyed by path, so
  the workspace tree holds it with nothing added. The rules are core's
  (`onehand_core::worktree`): the branch-name check that answers before anything is created, the
  slug, and the folder — **beside** the project rather than inside it, because a second checkout
  under the first shows up in that project's own file tree and `git status`. The dialog derives the
  folder from the branch name and only lets the *parent* be picked; an existing branch is checked
  out and a new name is created off HEAD, one call deciding which.
- **Pinning is explicit and changes only the drawing order.** `Workspace::display_order` is a stable
  partition, `roots` never moves, and pins are stored by path so a root added elsewhere in the file
  cannot slide a pin onto another project. Nothing reorders the list on the app's own initiative.
- **A project row rolls up its sessions' signals** (`SessionSignal::most_urgent`, same rank as a
  single session's `pick`), so a collapsed project is not silent about an agent waiting or dead
  inside it.
- **A *Recent* group sits above Projects** once the workspace holds at least four sessions: up to
  five rows, most recently viewed first, each naming its project. It is a separate section precisely
  so the tree itself never reorders.
- **`Ctrl+Shift+B` hides the rail; it never narrows it.** An icon-width rail is ten identical folder
  icons, which is the one thing a session-first rail must not become. The way back is a button in the
  agent panel's header, shown only while the rail is hidden (`ChatPaneEvent::ShowRail`).
- **The agent pane and the terminal are bare `DockItem::panel`s, not tab groups.** `DockItem::tab`
  wraps its panel in a `TabPanel` whose title bar draws a tab carrying the panel's title — for the
  conversation that is the conversation's own name, printed directly above the header that already
  says it, and one tab that can never gain a sibling is not a tab. **The terminal's several tabs are
  its own**, drawn inside the panel with the shell labels, their ✕ and the `+`; the library tab group
  around it held exactly one panel and added a second strip saying "Terminal" over the strip that
  already names every shell. Only the Workbench keeps a tab group, because its modes really are
  sibling tabs. Two consequences for both bare panels: `zoomable` returns `None` (there is no tab bar
  to put the content-direction maximize on — `Ctrl+Shift+K` still works), and each must call
  `track_focus` itself (see the focus gotcha below). What the agent pane's tab bar used to carry moved
  into `ChatPane::header`.
- **`Root`'s overlay layers are the app's to mount.** `Root` stores dialogs, sheets and notifications
  but draws none of them — `Shell::render` calls `Root::render_{sheet,dialog,notification}_layer`.
  Forget that and `Dialog::trigger` opens into a list nobody reads, which is exactly what happened
  between P2 and P7: every dialog was dead and nothing pointed at why.
- **Transient status is a notification**, pushed with `window.push_notification`. The one exception is
  the Workbench's save-conflict line, which is a standing condition rather than news: a toast that
  fades leaves the user believing the save went through. It is cleared by whatever answers it.
- **Two things are said on the *desktop*, outside the window** (`chat::session::notify_desktop`, over
  `notify-rust`, fire-and-forget on its own thread because `show()` blocks on the bus): a turn that
  finished, and an agent that has parked a permission or a question and stopped. Both are announced by
  the *pane*, because it is the half that knows what is on screen — but under **different rules, and
  that is the point**. A finished turn says nothing while any part of its window is in front of the
  user, since the rail badge is already there and the work is done. A parked ask says something unless
  the user is looking at *the conversation that asked*: an agent waiting is an agent standing still
  for as long as it takes to notice, and reading one conversation is exactly when a dot on another
  row goes unseen. It is sent at critical urgency so most desktops will not fade it while the agent is
  still blocked. The *moment* an ask parks is `ApplyOutcome::asked_user` — the reducer's answer, not
  `Chat::awaiting_permission`, which stays true for as long as the card is up and would re-announce a
  blocked session on every chunk that followed. The sentence is `UserAsk::headline` in core, so
  permission and question are named apart wherever either is announced.
- **The panel arrangement persists** into the workspace's `onehand-workspace.toml` — Workbench width,
  terminal height, whether each is open, and the rail's width
  (`onehand_core::config::PanelLayout`). Five values, not gpui-component's whole `DockAreaState`:
  restoring one of those rebuilds every panel through a *process-global* `PanelRegistry`, which would
  leave the shell holding handles to orphans and could not tell two windows' panels apart. Writes are
  debounced, because both the dock and the rail split emit on every frame of a drag. An unbound
  workspace persists nothing, as with everything else.
- **The rail is a panel in an `h_resizable` split**, drag-resizable between `PanelLayout::RAIL_MIN`
  and `RAIL_MAX` (232–320px) — its own range, not the docks', because it is sized by what its rows
  have to fit rather than by preference. The `ResizableState` is the shell's, not the element's: the
  width outlives frames the rail is not drawn in (hidden, or a panel maximized). Whether the rail is
  *showing* is deliberately not persisted — a workspace that reopened with no rail reads as broken.
- **The status bar** ([statusbar.rs](crates/app/src/statusbar.rs)) is the frame's other piece of
  chrome: one row under both the rail and the dock, so the frame is a column whose first child is
  that pair. It goes with the rail when a panel is maximized in the app direction. It carries **only
  what nothing else on screen carries** — the conversation's name and what it is doing belong to the
  agent pane's header and are not repeated. Left: the active project (click copies its path), its
  branch and change count from `GitStatus::label` (click re-reads status), and the running agent
  behind the rail's own `signal_mark`, so one condition keeps one shape. Right: how many open buffers
  are unsaved (click opens the editor, and it is the one cell drawn in a colour, because unsaved work
  is a standing condition rather than news) and one cell per panel left off 100%. **The terminal is
  not here** — it moved into the conversation header beside the Workbench button, because the two
  docks the conversation sits between are one decision and the panel they take their space from is
  where both belong.
  **The pointer is the contract**: a cell lights on hover iff pressing it does something; the agent
  cell is a reading and is drawn flat. The **away switch** sits at the right-hand end and is the one
  cell that is a control before it is a reading — it appears only while a remote channel is live,
  draws its icon alone while off, and takes a word and the standing-condition colour when on, because
  that is the state worth saying out loud and the other is a switch waiting to be thrown.
  Two things it must not do. **Zoom is read from the panels, not from focus** (`zoomed_panels`):
  focus moves without telling the window, so a focus-derived factor would sit on screen stale with
  nothing to admit it. And the two dock facts it draws (`PanelFacts`) reach it through observers
  **guarded by comparison**, the same way the rail's rows are — the terminal notifies once per chunk
  of output, so an unguarded observer would put a full window repaint in the output path of every
  build.
- **Dialogs** ([dialogs.rs](crates/app/src/dialogs.rs)): settings (the light/dark/system picker, then
  the workspace's name, storage folder and recents), the agent manager, and Help. The appearance is
  app-wide while everything below it in that dialog is one workspace's — the theme it selects is a
  global, so two windows cannot be drawn in two modes.
- **Multi-window**: one window hosts exactly one workspace. Opening a workspace whose storage dir is
  already on screen focuses that window instead of duplicating it; storage dirs are canonicalized on
  the way in so symlink and `..` aliases dedupe.

### Keyboard, zoom, maximize

App commands occupy an exact `Ctrl+Shift` namespace so plain Ctrl keys stay usable inside a PTY:
`B` rail · `E` Files · `O` Editor · `A` composer · `F` find · `R` guarded restart ·
`W` guarded close · `K` maximize. Plus `` Ctrl+` `` terminal, `Ctrl+S` save, `Ctrl+1…9` session by position, `Ctrl+Tab` session by recency,
`Ctrl+=`/`Ctrl+-`/`Ctrl+0` zoom, and inside the composer `Up`/`Down` (its completion list) and
`Ctrl+V` (an image or a file on the clipboard becomes an attachment; text is handed back to the input).

**GPUI resolves these itself.** Key bindings are matched against the focus context stack *before* the
key is delivered to whatever is focused, so an app binding reaches the app even while a PTY holds
focus, and the terminal never sees that keystroke. Three kinds of deliberate exception to the
namespace: `Ctrl+S`, bound `Shell && !Terminal` because the PTY has a real claim on it; the
composer's `Up`/`Down` and `Ctrl+V`, bound `ChatComposer > Input` and `ChatComposerCard > Input`
because they have to be taken from the input that already binds them; and the terminal toggle, which
is plain `` Ctrl+` `` because the shifted form **cannot be typed** — gpui names a key by the keysym
the layout produces with the modifiers applied, so shift over the backtick yields `~` and shift is
then dropped from the keystroke, leaving `ctrl-~`. `ctrl-shift-\`` matched nothing for as long as it
was bound; the tilde is not bound in its place because it is shifted on some layouts and unshifted on
others. A binding wins on the *depth* at which its predicate holds and only then on being
registered later, and `A > B` scores at `B`'s depth — so that predicate ties with the input's own
and the tie goes to the app, which binds after the library. The composer claims `ChatComposer` only
while a list is open, so the keys otherwise still move the caret.

Panel shortcuts are three-state: closed opens and focuses, open-but-unfocused focuses,
open-and-focused closes.

**Zoom is per panel** ([zoom.rs](crates/app/src/zoom.rs)) and overrides the *rem base* for that
panel's subtree, so everything sized in rems scales together — which is why sizes must be rems and
not pixels. The terminal is the exception: it is a measured glyph grid, so its zoom is a font size
that re-measures the cell and resizes the PTY.

**Maximize has two directions**: `Ctrl+Shift+K` fills the frame and hides the rail; the button in a
panel's tab bar fills only the dock area and keeps it. Only the Workbench offers the second — the
agent pane and the terminal are mounted bare and have no tab bar to put it on.

The Help dialog's table is the whole keymap, and a test (`dialogs::tests::keymap_and_help_agree`)
fails if a binding is added without a row — a shortcut nobody can find is a shortcut nobody has.

### Persistence

- **Transcripts** ([crates/core/src/chat/store.rs](crates/core/src/chat/store.rs)). Every conversation
  is a **directory** under `<config_dir>/onehand/conversations/`, named by the agent's ACP `sessionId`:
  `meta.json` (rewritten whole), `items.jsonl` (**only ever appended to**), `blobs/` (image results, by
  content hash). A fresh `session/new` means a new id and a new directory. Nothing is written while the
  chat is empty, so a fresh session never creates a directory beside conversations that were had.
  **Written at the end of every turn**, not only when the session is dropped: the write is prepared on
  the UI thread and carried out on the background executor, so a crash costs the turn in flight rather
  than the whole conversation.

  The split earns three things at once. A turn writes *its own turn*, so the cost of a turn stops
  growing with the conversation it belongs to. Listing reads one small file per conversation instead of
  parsing every transcript in full — `meta.json` carries the first prompt for exactly that reason.
  And appending removes a class of failure rather than guarding against it: while every save replaced
  the file, any moment the transcript in memory was *short* — a resume halfway through re-delivering
  itself, a session being taken apart — was a moment saving destroyed what was on disk.

  What it costs is that a line already written is not revisited, so an item that changes after its turn
  ended keeps the shape it was written in. That is why a rename writes **metadata only**
  (`Chat::flush_meta`): a rename can land mid-turn, and a line written then describes a tool call that
  never finished. `<config_dir>/onehand/sessions/` is the previous store — left where it is, never read.
- **The mark, and the replay.** `Chat` carries how much of the transcript is already on disk, so a
  restart hands it to its replacement (`take_snapshot`) rather than writing the conversation into its
  own file twice. A `session/load` re-delivers the conversation as ordinary content events, and there
  is **no event anywhere that says a replay has finished** — so the adopted copy is kept until
  something settles the question, and nothing is written while it is open. Settling puts the copy back
  whenever the replay came up shorter, which is not only a failed resume: a *successful* load replays
  the conversation as the agent holds it, without the tool cards, plans and reasoning the file holds.
  A replay that delivered more rewrites the file instead of being added to it, because a re-delivery is
  chunked as the agent chose rather than as the file was.
- **Resumed selector state.** `meta.json` also records the session's last mode and config-option
  picks, because the adapter rebuilds those from static `settings.json` on every `session/load` — a
  reopened conversation would otherwise silently drop them. **Model is deliberately skipped**: the SDK
  re-reads it from the transcript, and re-pushing a picker alias could switch the context lane rather
  than describe it.
- **The resume picker is asked for, never volunteered.** A new session connects straight away — it
  was minted by an explicit *New session*, and a picker there asks the user to choose a conversation
  immediately after they chose not to resume one. It is reached from a live session's header menu
  (*Resume another conversation…*), and the choice still happens *before* anything reconnects:
  connecting first would start a fresh conversation and archive it.
- **Deleting is offered in two places, and both ask the same way: a modal naming the conversation.**
  A live conversation deleted underneath its own session would not even stay deleted — the next turn
  writes the file again holding only what came after, because the session's mark says the rest is
  already on disk. That one fact is what the placement rules below are about.
  On the **project page** the placement *is* a guard: that page shows when the selected project has
  no session on it, so every row on it names a conversation nothing is writing to. A session in another
  window is the case the page's shape does not cover, so `ChatPane::delete_conversation` checks for one
  and refuses. The control is a **word, not a glyph**, and it lives **inside the card** — a control
  that acts on one conversation belongs in the card naming it, which is why `conversation_card` is a
  row with the text as one column and whatever the caller hangs on afterwards at its end. That puts one
  clickable inside another, so the delete's handler calls `cx.stop_propagation()`: without it the press
  that asks to delete a conversation also opens it.
  From the **conversation's own title menu** there *is* a session, so `Shell::delete_conversation`
  closes it first and unconditionally — dropping the session ends the agent and settles the mark, and
  the mid-turn question `close_session` would normally ask is skipped because the stronger question has
  already been answered and a second one about a settled decision teaches the user to click through
  both.
  **Both ask in `window.open_alert_dialog`** (`ChatPane::confirm_delete`, `Shell::confirm_delete_conversation`),
  not by arming a control and waiting for a second press: an armed control looks like one that did
  nothing, and the warning it raised is gone by the time the next press lands. Two things these dialogs
  do that the library's defaults would not. The name of the conversation is read *before* the dialog
  opens and carried into it, since the page or the session it came from can be replaced while the
  question is on screen. And the footer is the app's own pair — *Keep* through `DialogClose`, *Delete*
  in the danger tint — because the library builds its default OK/Cancel out of plain library buttons,
  which draw the arrow cursor, and the one dialog that asks before destroying something is the last
  place for a control to say "this does nothing" with the pointer. Unlike a dialog opened from a
  `Dialog::trigger`, everything here survives: the builder handed to `open_alert_dialog` is what the
  window keeps, so title, description and footer are rebuilt with it on every frame.
  Both exist because everything else the app offers can be done again and this cannot.
  `store::delete` removes the whole directory, so a conversation's images go with it.
  **Nothing is ever deleted automatically** — there is no retention sweep, by decision, because that
  would be the app throwing away work nobody asked it to.
- **The project page** is the store's other reader: with no session on the selected project, the
  pane draws that project's past conversations (`list_conversations` with no agent named — every agent,
  not just the session's, since there is no session yet) above a *New session* button. Picking one emits
  `ChatPaneEvent::StartSession`, and the shell mints the session, hands the pane the archive
  (`ChatPane::resume_next`) and *then* shows it — a resume that arrives after the adapter is up has
  already lost. An agent named by an archive but no longer configured falls back to the default
  rather than refusing to open.
- **Workspace + global state.** A workspace can be bound to a storage directory holding
  `onehand-workspace.toml`; it is written on rename and on binding. Sessions are not persisted (they
  respawn). Storage dirs are remembered in
  `<config_dir>/onehand/state.toml` as a recents list, and the next launch reopens the most recent,
  taking precedence over the CLI root. Binding a directory that already holds another workspace's
  config **never overwrites it** — that workspace is opened instead.

### Config

[crates/core/src/config.rs](crates/core/src/config.rs) loads `onehand.toml` (or
`<config_dir>/onehand/config.toml`, else built-in defaults) into `AppConfig`. `#[serde(default)]`
means a partial file overrides only the keys it sets. The default agent is Claude Code over ACP.
`load_resolved` returns the config *and the path to write back to*, so in-app agent edits land in the
file the next launch reads.

`appearance` is the one key the settings dialog writes: `system` (the default) · `light` · `dark`.
There are two palettes and the app only chooses which one is loaded — `shell::apply_appearance`
is the single place that does it, at boot and on every change. Each is the library's own config with
the app's surface ramp written over it (`crate::theme::install`, run once before the first mode is
chosen); see the theme module for what is ours and what is inherited. `system` **keeps following** the desktop
(each window observes its own appearance), which is also what settles the Linux startup race where the
platform answers with its default until the desktop portal replies. An unrecognized value reads as
`system` rather than failing the file, because the agent list is in that same file. Two things the
switch has to repair: the resolved monospace family, since loading a mode re-applies a whole theme
config over it, and every *other* window, since the mode is global while a refresh is per window. The
embedded terminal has its own ANSI palette and does not follow the mode. Declaration order matters —
`appearance` is a bare TOML key, so it must be declared before the sections or saving the config fails
outright.

`[remote.telegram]` is off unless asked for — a bridge that came up by default would put a process on
the network on the strength of a file nobody edited. It carries `enabled`, `allowed_chats` and an
optional `token_env`, and **it deliberately has no key for the token**; see the remote bridge above
for where that is read from and why it is not here. Declaration order does not bite for this one,
since it is a table like `[font]` and `[icons]` and only the bare `appearance` key has to lead.

`AppConfig` still carries `[font]` and `[icons]` sections the front end **mostly does not read**:
decision D1 makes gpui-component's theme the look. They parse (so existing config files keep working)
and are the obvious hook if per-role icon tinting comes back. The one exception is
`[font].monospace`, which `shell::use_installed_mono` takes as the first preference when it picks a
mono family the machine actually has (see the font gotcha).

## Known gaps in this build

Listed because a missing feature nobody wrote down reads as a bug in the ones that exist:

- **No command palette** (`Ctrl+Shift+P`). It is a feature — a command registry plus a filtered
  popup — not a keymap entry.
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
- **`path:line:col` tokens in agent prose are not clickable.** The transcript renders prose through
  `TextView::markdown` and does not scan it for path tokens. Only a tool card's path header opens a
  file, and it carries no line — ACP's diff payload has no hunk offsets. Core's
  `parse::parse_path_line` is the parser that feature needs and currently **has no caller**.
- **No Neovim mode** (`Ctrl+Shift+N`) — decision D4, together with the hardest terminal
  parity work (mouse reporting, `Shift` bypass, OSC 52, DECSCUSR). IME inside the PTY was on that
  list and is now done.
- **`[font]` and `[icons]` config sections are parsed and ignored** (see Config).
- Transcript blocks the design contract asks for that are not drawn are marked *(not rendered)* in
  DESIGN-ANSWER.md, each with the reason.

## Rules

- **Every icon is an SVG, and nearly every UI glyph comes from `gpui_component::IconName`.** No Unicode or
  emoji glyphs as icons. `IconName` is generated from the SVGs `gpui-component-assets` ships, which
  is also what the library's own components reach for in ~97 places — so that set has to stay loaded
  regardless. Two things this costs, both silent: the library **renames icons when it packages
  them** (its `close` is Lucide's `x`, its `delete` is the backspace key), and an icon that fails to
  resolve draws *nothing* rather than failing the build. Bumping the `gpui-component` rev means
  looking at the app's chrome afterwards.
  `crate::icons` holds **only what that enum cannot draw**: brand marks, plus the occasional shape
  the bundled set has no drawing of at all (today two — a pencil, for the transcript's *Changed*
  group, and a branch, for splitting a project into a worktree).
  An `IconName` whose *name* reads oddly does not qualify; a missing drawing does.
  To add one: update [assets/icons/manifest.toml](assets/icons/manifest.toml) with the reason beside
  the entry, run [scripts/sync-icons.sh](scripts/sync-icons.sh) (it knows Simple Icons for marks and
  Lucide for shapes), register it in the `icons!` macro. A test fails if manifest and registry
  disagree.
- **Code describes; it never cites.** No comment, doc comment or runtime string may name another
  document — not `CLAUDE.md`, not `DESIGN.md` / `DESIGN-ANSWER.md`, not `DECISIONS.md`, and no
  section number, anchor or item code belonging to one. The guard's list of forbidden names is
  deliberately longer than the set of documents that exist, because a name that was retired is
  exactly the one a stale comment would still be holding. Say the reason **in the comment's own
  words**, in full, so the comment stands alone.

  Two reasons. A citation *decays*: reorganize a document and every pointer at it silently starts
  aiming at the wrong place, and a confidently wrong pointer is worse than none. And a citation
  *tempts* — it lets a comment gesture at an explanation instead of giving one, so the reader ends up
  holding two files to understand one line. If a rule is worth a comment, the comment is worth
  writing out.

  **Still fine:** pointing at *code* — `[`crate::icons`]`, `gpui_component::dock`,
  `dock/tab_panel.rs:775`, an upstream rev. Those are checkable, and rustdoc links break the build
  when they rot. The rule is about prose that lives in a document.

  The traffic runs one way: **documents point at code, code does not point back.** Enforced by
  `guards::tests::code_never_cites_a_document`.
- **Every `.md` file in this repo is written in English.** Not a style preference: these documents are
  the binding contracts, they are read alongside source that is entirely in English, and half of what
  they explain is quoted identifiers, compiler messages and upstream prose that has no translation.
  A file split across two languages is one that gets read in neither — the reader has to switch, and
  the terms stop matching the code they name. This covers prose, headings, tables and comments inside
  fenced blocks; a quoted string that is itself Vietnamese (a test fixture, a bug report being cited)
  is data and stays as it is. Enforced by `guards::tests::documents_are_written_in_english`.
- **DESIGN.md and DESIGN-ANSWER.md are binding.** Read the theme; never hard-code a colour, radius or
  size. Sizes are rems.
- **Reuse gpui-component before building.** A hand-rolled equivalent will not follow the theme, will
  not follow the focus rules, and becomes ours to maintain.
- **Keep rendering bounded**, and say on screen when a bound bit.
- **Don't self-verify UI by launching or screenshotting.** Make the change, make sure it builds and
  tests pass, then stop — the user inspects the result visually.

## Gotchas

- **Key bindings beat `on_key_down`.** GPUI matches bindings against the focus context stack first and
  only delivers the key to focused elements if nothing matched. That is why the app keymap reaches
  over a PTY with no cooperation from the terminal widget — and why binding a key the terminal needs
  silently takes it away. A `!Context` predicate means "that context appears nowhere in the stack".
- **`with_rem_size` must be set in all three element phases.** `request_layout` is where rem sizes
  become numbers, but `prepaint` and `paint` re-resolve some of them; overriding in one phase gives a
  subtree measured at one size and painted at another.
- **A font family is a request, and a missing one fails silently.** gpui-component's default
  `mono_font_family` is one hard-coded name per platform, and on Linux it is DejaVu Sans Mono — which
  plenty of distributions do not ship. Every well in the transcript then drew in the body face while
  the code drawing it was, correctly, asking for mono, with nothing on screen or in the log to say
  the request went nowhere. `shell::use_installed_mono` picks a family from
  `cx.text_system().all_font_names()` once at boot; the choosing rule is
  `onehand_core::config::resolve_monospace`, which is pure and tested. **Never assume a family name
  resolves** — check it against the enumeration. The terminal is the sharpest case: its grid is
  *measured* from a shaped glyph, so a family that does not resolve does not merely change the
  typeface — the cell is sized from one font while the row is drawn in another and every column lands
  past its glyph. `terminal::spawn_shell` hands the grid the resolved family for exactly that reason;
  the vendored default is the string `monospace`, which is a CSS generic and not a family anything
  enumerates.
- **`mx_auto` does nothing inside a `gpui::list` row.** The list lays every row out as its own
  *layout root*, and a root has no containing block for an auto margin to take its share of, so the
  margin resolves to zero — silently, with no warning and nothing wrong-looking in the row itself.
  Centre a list row with a flex parent (`h_flex().justify_center()`) around a `max_w` child instead.
  This cost a round trip once: the transcript sat against the left edge while the composer, centred
  inside an ordinary flex column, sat in the middle of the panel, and the two halves disagreeing was
  the only symptom.
- **A panel's focus handle is tracked by the dock, not by the panel — unless it has no tab group.**
  gpui-component's `TabPanel` calls `track_focus` on the active panel's handle, so `contains_focused`
  works without the panel adding it, and a panel that adds `track_focus` on top of that becomes
  doubly click-focusable. A `DockItem::Panel` renders bare, so nothing tracks it and
  `contains_focused` answers "no" however deep inside the pane the caret is — which silently points
  the whole three-state panel keymap at the wrong panel. That is why `ChatPane::render` and
  `TerminalPanel::render` — the two bare panels — track their own handles, and the Workbench, which
  keeps its tab group, does not. Focus-on-click stays correct either way: gpui's handler runs in the
  bubble phase and an inner focusable takes the click first and calls `prevent_default`.
- **A panel closed while it holds focus takes the whole keymap with it.** GPUI resolves a key along
  the path from the dispatch tree's root down to the *focused* node; with nothing focused that path is
  the root alone, and every `on_action` the shell hangs on its own frame sits below it, unreachable.
  So unmounting the terminal — or closing the Workbench dock — with the caret inside leaves a window
  where no shortcut works at all, including the one that would reopen the panel. It reads as "the key
  only closes it, never opens it", which is nothing like a focus bug and sends you looking at the
  binding. Both close paths call `ChatPane::reclaim_focus`, which asks *before* the panel leaves the
  frame, since a handle that is not drawn cannot answer `contains_focused`. Any new panel that can be
  taken off screen owes the same call.
- **Zoom factors must snap to the step.** Binary floating point does not round-trip `1.0 - 0.1 + 0.1`,
  so an unsnapped factor drifts and `Ctrl+0` becomes the only way back to 100%.
- **`vendor/gpui-terminal` is a vendored render core plus the interaction layer upstream never had.**
  Scrollback, selection, copy/paste (`Ctrl+Shift+C/V` — plain Ctrl+C is SIGINT and Ctrl+V is
  literal-next), bracketed paste, copy-on-select and typing-snaps-to-bottom are all onehand's, marked
  `onehand patch`. Upstream is `zortax/gpui-terminal@51f0292`; the verbatim import is one commit and
  the patches the next, so the delta stays readable. `gpui` there is a **revless** git dependency:
  cargo keys a git source by URL plus rev, so any rev (or crates.io) yields a second `gpui` in the
  graph and "expected gpui::App, found App".
- **`gpui` carries no rev anywhere**, for the same reason; `gpui-component` *is* pinned by rev, and
  `Cargo.lock` is the pin for both.
- **Native dialogs and file scans run off the UI loop** — pickers, the `@`-mention scan and directory
  scans go through `cx.background_executor()`, never inline in a render or an action handler.
- **IME can swallow a typed `/` on Linux.** With a Vietnamese IME enabled, the composer may never
  receive the character, so the slash-command popup cannot be opened by typing. The workaround is the
  composer's `@` and `/` toolbar buttons, which insert the trigger *from code*
  (`Composer::insert_trigger`) and bypass the IME. Keep them: they are not a convenience.
