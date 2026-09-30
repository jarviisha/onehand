# How onehand is put together

The long account of the app's structure and the reasons behind it, moved out of
`CLAUDE.md` so that file stays short enough to be read in full every session.
`CLAUDE.md` keeps the commands, the invariants, the rules and a one-line form of
each gotcha; this file keeps everything else. Read the section for the area you
are changing before changing it.

## Development tooling

**To look at the app's chrome without an API key**, point an agent at
`crates/core/examples/mock_ui_agent.js` — Settings ▸ Agents, or in `onehand.toml`:

```toml
[[agents]]
name = "Mock UI"
command = "node"
args = ["crates/core/examples/mock_ui_agent.js"]
```

It advertises modes, config options (model with per-choice descriptions, effort, fast) and slash
commands, and **one prompt draws a whole transcript**: reasoning, prose carrying every markdown
block the renderer has, an activity block per shape a run can take — clean, one that stumbled and
recovered, one that ended on a failure, one longer than the child cap with its failures scattered
through the middle — a diff, a plan, an image result, two steps left mid-flight, then the parked
permission and the two question cards, which settle into the record rows they leave behind. **One
turn and not one shape per prompt**, because what goes wrong in a transcript goes wrong *between*
blocks: a run reading the same as the run above it, a child row repeating its parent, two blocks
failing to share one frame. None of those is visible one block at a time.

Three things it holds on purpose. The commands in one run are pointed at a single host with a
password in every one of them, so the rule that lifts a shared target up to the parent row and the
rule that masks a credential can both be *seen* rather than trusted. A failed command is followed by
the same command again, which is what earns the retry mark. And the three-question form mixes a
single-select, a multi-select and a free-text field, because each draws differently and a card of
three identical questions would only ever exercise one of them.

**The tour is played rather than printed**, over about twelve seconds. Three of the states the
app spends most of its time in have no finished form to look at — an answer arriving a chunk at a
time, a thought still being had, a command still running — and those are the frames where a spinner
has to hold its column, where a cluster's line has to change in place without moving anything, and
where the composer has to stay answerable. A `fast` anywhere in the prompt lands the whole thing at
once; cancelling stops it where it is, which is what a cancel is for and what a mock emitting into
an already-closed turn was not doing.

**Every other turn ends on a JSON-RPC error rather than a stop reason.** The error banner is the one
block an agent cannot ask for — it is what the app draws when a turn *breaks* — so the only way to
look at it is to break one, and the only way to still see everything else is to not break every one.

It answers `session/set_config_option` and `session/set_mode` by republishing the new state, so the
chips move when they are used.

**A mock *agent* and not a mock mode inside the app**, deliberately: the window is driven over the
real transport, by the real parser, through the real session lifecycle, so what is on screen is what
a real adapter would produce. A rendering path reachable only from a dev mode is one nobody is
looking at when it breaks — and it would be a second way to reach every view, kept in step by hand.

**Use the Makefile targets for `fmt` and `clippy`**, not bare cargo: `vendor/`
is a workspace member, so `cargo fmt` reformats it and `clippy --fix` rewrites it — hundreds of lines
of churn on upstream code, destroying the one property that vendor has (its diff against upstream is
exactly our patches). `make fmt` / `make lint` scope to first-party crates and exclude `vendor/`.

CI runs on every push to `main` and every pull request, split into four jobs by what each costs —
formatting (no compilation at all), core tests (120 crates, where a real failure usually shows first),
app tests, and clippy with warnings denied. Every cargo invocation is `--locked`, because `gpui` is a
git dependency carrying no rev and the lockfile is the only thing pinning it. The two heavy jobs go
through the Makefile for the reason above.

A release is cut by pushing a `v*` tag: that builds `--locked` on an older runner image, so the binary's
glibc requirement is one more distributions meet, and packages the binary with the icon, the desktop
installer and the licences into a tarball attached to a GitHub pre-release. Nothing publishes to
crates.io and nothing can — a git dependency with no rev is not publishable there, so tagged tarballs
are the only channel. `onehand --version` and the foot of Settings' nav both name the build.

Tests are inline `#[cfg(test)]` modules — there is no `tests/` directory.

## Architecture

### Repo layout

| Path | Crate | What |
|---|---|---|
| `crates/app` | `onehand` | the GPUI front end + the binary |
| `crates/core` | `onehand-core` | GUI-free logic: config, the workspace tree, ACP, the chat model, the remote bridge, the connector contract, a project's own issues, editor rules, completion, git status, worktree rules, the directory flatten |
| `crates/plugin-api` | `onehand-plugin-api` | GUI-free plugin IDs, descriptors, capabilities and registration contract |
| `crates/plugin-host` | `onehand-plugin-host` | the Workbench mode contract, the remote-channel factory type, and the three things a plugin cannot reach into the binary for: the button wrapper, status ink and the surface a dock card draws on |
| `crates/terminal-ui` | `onehand-terminal-ui` | shared PTY/grid ownership used by the terminal dock and Neovim |
| `plugins/builtin/*` | built-in plugins | Editor, Files, Markdown, Neovim, Issues, Plugins, Telegram and GitHub contributions compiled into the binary |
| `vendor/gpui-terminal` | `gpui-terminal` | a vendored terminal grid + the interaction layer upstream never had |

The workspace root is a **virtual manifest** — it owns nothing but the member list and the release
profile.

**Three invariants hold the split together:**

- `cargo tree -p onehand-core -i gpui` must keep **erroring with "did not match any packages"**.
  Core is the half that survived one front-end rewrite; keeping it framework-free is what would let
  it survive another. (Use `-i`, not `| grep gpui` — the checkout directory is still named
  `onehand-gpui`, so a plain grep matches the path and always "finds" something.)
- **Core must not dictate an async runtime.** Every blocking operation is a plain blocking function;
  anything async is a thin wrapper over it. GPUI runs on smol and has no tokio reactor, so a core
  that awaited tokio I/O directly would panic inside the UI process.
- **Nothing in core is `pub` unless something outside the crate names it.** This is the same rule
  `crates/app` keeps by making its modules private, arrived at from the other side: core cannot hide
  its modules, because the app and the built-in plugins import them. So the visibility is per item,
  and widening one is a decision rather than the default. It is load-bearing for the same reason: `dead_code` stops at
  a `pub` item in a library, so while every function here was `pub`, one that had lost its last
  caller looked exactly like a working feature to the compiler — seventeen accumulated that way, plus
  a fold-state chain and a write-only field that a repo-wide grep could not see and one compile did.
  `#![warn(unreachable_pub)]` is there too and catches nothing today, since no module is private; it
  is the guard for the first one that is. Every other first-party library carries the same line for
  the same reason — `plugin-api`, `plugin-host`, `terminal-ui` and each built-in plugin — while
  `crates/app` gets the effect from private modules instead.

Shared rules live in core, not restated per call site: `GitStatus::label`, `AppConfig::update_in_place`,
`gitstat::read_blocking`, `RootEditors::open`, `Chat::apply`, `Away::headline`, `remote::press::option_at`,
`Chats::reconcile`.

### Library + thin binary

[crates/app/src/main.rs](../crates/app/src/main.rs) is fifteen lines: install the asset source, call
`gpui_component::init` (**before anything else touches the library**), then `shell::boot`. Everything
real is in the lib so it stays reachable from tests without opening a window. Keep new logic out of
`main`.

**Only `assets` and `shell` are `pub`; every other module is private, and that is load-bearing.**
rustc's `dead_code` analysis stops at a `pub` item in a library, because something outside the crate
might use it — and nothing outside this one ever will. While the modules were all `pub`, dead code
was invisible to the compiler; making them private surfaced five dead items immediately. Tests live
inside the crate, so privacy costs them nothing. **Keep new modules private.**

[guards.rs](../crates/app/src/guards.rs) holds the rules rustc has no opinion about: no glyph used as an
icon, our own event enums matched exhaustively (not `matches!`), no field assigned and never read.
**Every one was added after a repo-wide sweep found the same mistake in several places at once** —
a guard here is evidence that the rule cannot be held by intention alone, so removing one because it
is inconvenient re-opens something that has already gone wrong more than once.

**A guard that overlaps a compiler lint is removed by measurement, not by argument.** The
field-never-read guard was proposed for deletion on the reasoning that narrowing every crate's `pub`
surface would let `dead_code` cover the same ground. It does not, and a probe said so in a minute
where the reasoning had been persuasive for a page. The guard stayed; what exactly rustc does and
does not cover is recorded on the guard itself, where the next person to argue this will be standing.
**Do the probe before the deletion.**

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
  Settings' Agents page; each session keeps a clone of the spec it was spawned with.
- `Session.uid` is a process-wide id salt (`Shared::next_uid`), which is how a session's chat state
  survives switching roots and sessions, and how an event finds its window.
- Sessions connect **lazily**: `Shell::show_active_session` spawns an adapter the first time a
  session is actually shown. A workspace with a dozen roots must not launch a dozen agents at boot.

### Sessions are ACP only

`AgentSpec` is `{ name, command, args }` — no `kind` field. Legacy `onehand.toml` files carrying
`kind = "acp"|"terminal"` still parse: the key is unknown and serde ignores it.

### ACP client (core) and the executor bridge

[crates/core/src/acp/](../crates/core/src/acp/) is a minimal JSON-RPC (newline-delimited JSON over
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

**The bridge** ([crates/app/src/acp.rs](../crates/app/src/acp.rs)) is the one place tokio and smol meet.
`connect` folds its serve loop into the stream it returns — the adapter only advances while something
polls it, and dropping the stream kills the child — but that loop is `tokio::process` + `tokio::io`,
which needs a tokio reactor GPUI does not have. So the stream is driven on a tokio runtime this
module owns, and events cross to GPUI on a plain `futures` channel belonging to neither executor. The
*request* side needs no bridge: an unbounded tokio send never touches the reactor.

### Built-in plugins

`crates/app/src/plugins.rs` is the composition root, and it is three ordered lists:
the Workbench modes (Editor, Markdown, Neovim, Issues, Plugins, which is the order on the strip —
Files is a mode too, composed inside the Editor rather than listed beside it),
the connectors (`plugins::connectors`, GitHub alone today) and how a named
remote channel is opened. Nothing registers, nothing is
sealed, and there is no capability declaration or API version — each was checking
something the compiler checks harder. `impl WorkbenchMode` *is* the capability
declaration and a mode that does not compile does not ship; this is one binary
compiled together, so cargo is the version check; and a list built and returned
in one call has no window in which anything could register late. What the
registry was genuinely buying is the order, declared here rather than inherited
from filesystem or linker order, and that is now the literal order of the list.

**A mode owns its state and its view; the panel owns neither.** The trait is
`onehand_plugin_host::WorkbenchMode`: declare yourself, hand back a view, answer
the requests you recognise. Everything a mode works on — open buffers, the file
tree, a document index, a live PTY — is inside that mode, and the panel keeps the
list, the active ID and the strip. This is *not* the arrangement an earlier
attempt removed: that one put a mode object **beside** the panel's own copy of
the same state and kept the two in step by hand. The rule that tells them apart
is testable — the panel holds no per-mode state at all, so there is nothing for a
mode object to be a second copy of.

**`Request` is what keeps the trait from growing a method per shell feature**,
and it is one vocabulary in both directions. Downward it is broadcast in the
panel's order and a mode says whether it took it, because the shell asks *the
Workbench* to save, to rescan, to reap and has no business knowing which mode
owns a buffer or a child. Upward it is what a mode raises through its `Ask` when
a click inside it means something another mode owns — a row in the file tree, the
Markdown header's *Edit source*, both of which mean "open this file" and neither
of which can reach the quick editor.

Three deliberate exceptions to that shape, each for a stated reason.
`Request::Shown` goes to the **arriving mode alone**, since every other mode's
answer would be a lie; it is what tells a mode whose listing costs a walk of the
whole project that the walk is now worth paying for. `focus` and `open_file` are
**methods rather than requests**, because they are the two things a mode is asked
that need a `Window` — and requests arrive from paths that have none, such as a
`git status` sweep landing or a turn ending. And **reaping splits across the
seam**: the mode collects its own exited children, but the panel decides the
caret, because only the panel knows whether focus was inside it, and it has to
ask before the drop since a handle no longer drawn cannot answer.

The Rust traits are versioned `0.x` and are an internal composition seam, not a
stable third-party ABI. A future external-plugin system is expected to use a
process protocol rather than Rust dynamic libraries.

**Status ink lives in the plugin host** for the reason the button wrapper below
does. A mode draws its own status line — a save conflict, a document that has
outgrown the read's size bound, a Neovim that would not start — and a second copy
of the derivation is a second place for a raw status fill to be used as ink,
which is the mistake `crate::theme::status_ink` exists to prevent.

**The dock card's surface is there for the same reason**, and is the sharpest case of
it: the Neovim mode hands a terminal grid the surface it is sitting on, because a
grid fills every cell it has not been told otherwise about with its default
background. A second copy of the answer is a panel and the shell inside it
disagreeing about what colour the panel is, which shows up as a rectangle of the
wrong shade behind a running program. `onehand_plugin_host::dock_surface` is the one
definition and `crate::theme::dock_surface` is the app's name for it. It was called `chrome` while
it was a step off the reading surface; it is that surface now, so the word had come to name the
opposite of what the function returns.

**The button wrapper lives here, not in the app.** A built-in plugin draws
buttons and cannot reach into the binary hosting it, so a copy in each half is
two places for the component library's arrow-cursor default to be let through.
`onehand_plugin_host::action` is the one definition and `crate::controls::action`
is the app's name for it; the guard that counts call sites exempts exactly one
file for that reason.

**Popup menu rows are the other half of that, and stay in the app** — no plugin
draws a menu. A `PopupMenu` row sets no cursor at all, so every menu drew the
arrow over entries that act while the buttons an inch away drew the pointer;
`controls::menu_item` / `controls::menu_row` are the one place that answers it.
The library gives no hook on the row itself, only on what goes inside it, so the
cursor is carried by the row's *content* stretched back out over the inset the
row puts around it — otherwise a strip at each end of every row still draws the
arrow, which is the same half-rule one step smaller. **A disabled entry keeps the
library's default and should**, for the reason `controls::resting` exists: a
pointer over something that refuses is a promise the control cannot keep. So the
handful of conditionally-refusing entries branch on the two builders rather than
setting one and disabling it.

### The remote bridge

A second channel into the app, for the times nobody is at the machine. Same shape as the ACP bridge
above, deliberately: [crates/core/src/remote/](../crates/core/src/remote/) owns the GUI-free neutral
model and channel contract, while the Telegram wire implementation and secret loading live in
`plugins/builtin/remote-telegram`. [crates/app/src/remote.rs](../crates/app/src/remote.rs) drives it on a
tokio runtime of its own with events crossing on a `futures` channel.

**The layer is general; Telegram is the first adapter.** `remote::types` is the neutral model — chat
ids are strings, a message carries text and rows of `Button`s, and `RemoteChannel::connect` folds its
serve loop into the stream it returns. The built-in Telegram plugin is the only implementation, a long poll
plus `sendMessage` and `answerCallbackQuery`. Everything that is not the wire is pure and tested:
`command` (the little language a chat drives it with), `press` (what a button means) and `chats`
(who may reach the app, where each types, and what each has asked to hear). Telegram's plugin owns
`secret` (where its credential comes from).

- **The token is read and never written, and it is not in `onehand.toml`.** That file is rewritten
  whole by the settings dialog, it is what people paste into a bug report, and
  it is world-readable because everything else in it is a preference — so a bearer credential in it
  would be printed back out on a schedule nobody chose. Two sources instead: `$ONEHAND_TELEGRAM_TOKEN`
  (or whatever `token_env` names), then `<config_dir>/onehand/telegram.token`, a file whose only
  content is the secret. The second exists because a desktop app is launched by clicking an icon and
  there is no shell in that path to have set a variable in; its permissions are checked and a
  group-or-world-readable one is complained about on stderr rather than refused, since refusing means
  a feature that silently does not work. It is still plaintext on disk, and this is not a keyring.
- **A chat not on `allowed_chats` is answered with nothing at all** — not a refusal, because a refusal
  confirms that the bot is real, that it is running right now, and that there is a list to get onto.
  **The empty list allows nobody**, so forgetting to fill it in fails closed. The rule is
  `Chats::allows`, on the type holding the list rather than beside it — the negative case has no
  function of its own, because the caller that would have had to name it does not exist and a
  negation nobody calls cannot enforce anything. It is **permission and
  not audience**: being on it is what lets a chat say anything and be told anything, while what a chat
  actually hears about a *session* is the narrower list it subscribed to itself (see **Following**
  below). The two coincide only for what is about the bridge rather than about a session — the away
  switch thrown at the keyboard — which has no session to be subscribed to and needs to reach somebody
  who has asked for nothing yet.
- **One process, one bot**, so the bridge lives on `Shared` rather than on a window — a second poll
  against one token is two clients splitting one queue. What follows is the routing problem: an
  incoming message belongs to no window, so `OpenWindow` carries a weak `Entity<Shell>` and the bridge
  asks every window in turn. The window holding the session answers; a map of uid to window kept on
  the bridge would need correcting on every open, close and restart and would be wrong in between.
- **Out.** The three moments a session stops being self-explanatory to somebody not looking at it, as
  `onehand_core::chat::Away` — a turn that finished, an ask that parked, an adapter that stopped
  answering. All three reach a chat only if it follows that session (see **Following** below); what
  follows here is about *whether* they are worth saying at all, which is a separate question decided
  by what is on screen. The sentences are core's so the desktop notification and the chat cannot drift, and
  `UserAsk::headline` still names permission and question apart underneath. **A finished turn carries
  the end of the answer** (`Chat::answer_tail`) and a parked ask carries the question, both through
  `Announcement::detail` — the line a reader on the far side needs and a reader at the window does
  not, since the desktop notification is one keystroke from the transcript and a phone is not. The end
  and not the beginning: an answer opens by restating the problem and closes by saying what was done
  about it — and **bounded to the turn that just ended**, since reading back past the prompt that
  started it would announce the previous turn's closing paragraph as this one's result, which is a
  wrong answer in the shape of a right one. Which card the ask carries is decided by the `UserAsk` the
  event handed over and never by looking for one kind before the other: both can be parked at once,
  and a headline saying "has a question" over a permission's Allow and Deny is answerable, so
  answering it does something nobody asked for. **The silence rules are the
  desktop's, unchanged**: a finished turn says nothing while any part of its window is in front of the
  user, while a parked ask and a lost adapter speak unless the user is looking at *that* conversation
  — an agent standing still stands still until somebody notices, and reading one conversation is when
  a dot on another row goes unseen. The moment an ask parks is still `ApplyOutcome::asked_user`, not
  `Chat::awaiting_permission`, which stays true for as long as the card is up.
- **`Shared.away` is the one thing those rules cannot work out.** All of them ask whether the user is
  looking, and answer it from the focused window and the conversation on screen — both of which stay
  true in front of an empty chair, so a window nobody is at reports every turn as read. The user says
  otherwise, and while they have said so every announcement goes out as though nothing were on screen.
  It is one field of `chat::Presence`, which is the only input the three rules take, so the switch
  cannot reach one of them and miss another. Global rather than per window, because walking away from one
  window is walking away from all of them, and **not persisted** — a launch that came up believing the
  user was elsewhere would message somebody sitting in front of it about every turn. Thrown from the
  chat (`/away`, `/here`), both through
  `remote::set_away`, since a mode with two setters is a mode that means two things. **There is no
  switch at the keyboard any more** — it lived in the status bar, which is gone — so the chat
  commands are the whole of the way in. Two things that setter owes. **A dead channel clears it**,
  because `/here` would arrive over the channel that just died and leaving it set is a mode with no
  exit short of a restart; that mattered while a switch could still be thrown at the window and
  matters more now that one cannot. And **coming back clears the badge
  on what is on screen**, on the active window only — every turn is unwatched while away, including
  one that ended in the conversation being read, and `unseen` is otherwise cleared when a window
  *becomes* active, which never happens to one that was focused the whole time. That clearing is
  **deferred**, and stays so: the clearing reaches into a shell a caller may already be holding, and
  updating an entity that is already being updated is a panic rather than an error.
- **In.** `/away` and `/here` set the presence fact above from wherever the user actually is —
  the point of having them, since the switch at the keyboard is no use to somebody who has already
  left. `/sessions` numbers every session across every window and says what each is doing, in the
  rail's own `signal_word` so one condition keeps one name. `/use <n>` points a chat at one *and
  follows it*, and **the number is the session's uid, not its place in the list** — a place shifts
  when a session closes, so a number read and then typed back would land on a different
  conversation. Anything not
  starting with `/` is a prompt for the bound session, submitted straight into it rather than through
  the composer (one composer serves the pane and it holds what the person at the keyboard was typing),
  and queued rather than refused mid-turn, since the sender cannot see that a turn is in flight — but
  **an occupied queue is a refusal**, because that queue is one slot and `Chat::queue` replaces what is
  in it: a message the sender knows did not go can be sent again, and one they believe went cannot be
  recovered. **`//x` sends `/x` on to the agent**, which is the only way its own slash commands are
  reachable at all — every one of them collides with this language. A single slash is never forwarded:
  guessing that an unrecognised word was meant for the agent would turn each mistyped bridge command
  into a prompt nobody sent. `/stop` cancels the bound session's turn, and is the other half of being
  able to start one — everything else here sets work going, and an agent heading the wrong way is what
  somebody away from the machine can do least about. **It takes back anything queued first**, because
  cancelling ends the turn and the end of a turn is exactly what flushes the queue: a plain cancel
  would stop the work and launch the next piece in the same breath. At the keyboard that is survivable
  since the queued prompt is on screen as a chip; from a chat there is nothing to see. The words are
  quoted back rather than dropped, and the emptying happens *before* the cancel goes out so no ordering
  of the adapter's replies can flush it on the way past.
- **Following, and the silence underneath it.** **Nothing about a session reaches a chat that did not
  ask for it.** `Live.followed` is a set of uids *per chat*, and `announce` sends only to the chats
  holding the uid it is about — so the audience for a session's news is narrower than `allowed_chats`,
  and a bridge nobody has subscribed from says nothing at all. The arrangement this replaced spoke
  about everything and was quietened one session at a time, which makes a chat's contents a
  consequence of whatever happens to be open at the far end: something its reader neither chose nor
  can see, where a machine running eight agents had to be told about seven of them before it was
  bearable and every session opened afterwards reopened the argument. **Per chat and not global**,
  unlike anything else about announcing, because a subscription is by construction a fact about a
  reader — and because `/use` subscribes, so a shared set would let one phone's pointing decide
  another phone's notifications.
  **`/use` follows what it points at**, and that is what keeps a channel silent by default from being
  a channel that looks broken: pointing a chat somewhere is the gesture that says "this is the one I
  am attending to", it is what somebody does before walking away, and requiring a second command
  after it would end the ordinary path in silence — the one outcome a reader cannot tell from a
  crash. It is said in the reply rather than left to be discovered, since `/use` reads as "send my
  typing here" and a chat that then started announcing turns unasked would be the bridge acting on
  its own. Unpointing does **not** unsubscribe (a chat follows many and types into one), but
  `Chats::reconcile` drops both wherever a session has closed — an entry naming a session neither
  `/sessions` nor `/status` can print is one nobody could afterwards remove.
  `/follow [n]` and `/unfollow [n]` are the explicit pair, bare meaning the pointed-at session. Both
  move **all three moments together**, the parked ask included, which is the honest reading of being
  told to say nothing: half a subscription is a mode whose rule nobody can state. A word where a
  number was meant (`Aim::Unreadable`) is refused rather than read as the bound one — on `/unfollow`
  that would silence a conversation nobody named, and what it costs is every message that session
  would have sent, until somebody thinks to wonder why it went quiet. `/sessions` marks both facts in
  one margin column: `→` where typing goes, `•` for followed-but-not-pointed-at, which is the row
  that is otherwise indistinguishable from a silent one.
  **The whole thing is decided in `Chats::announcement` and never by the pane**, because it is a fact
  about the channel alone — the pane's rules are about what is on screen and hold for the desktop
  notification too, so pushing a subscription up there would quiet the desktop over instructions that
  never mentioned it. `announce` is what is left in the app once that moves: read the messages, send
  them.
- **`/status` is the answer to "why is it quiet", and silence being the default is what makes it
  necessary.** Every fact that decides whether anything arrives is invisible from the far side by
  construction: following nothing shows itself as messages that do not come, and so does being at the
  keyboard, and so does a bot whose process died an hour ago. So it prints the two facts no session
  carries — away on or off, where this chat is pointed — and then **names** what it follows rather
  than counting it, since the point is to check the list against what you believe you asked for.
  **Not a second `/sessions`**, which answers what onehand is running and marks these rows in passing.
  The word is `status` and not `watching` although that is the question being asked:
  `Attention::Reading` already names the user's eyes being on a conversation, which is what decides
  whether a turn is announced at all, and one word answering two questions inside one feature is how
  the two answers end up swapped. It reads and never writes, which it can only do because
  **every message reconciles first**: `answer` walks the open sessions once and hands them to
  `Chats::reconcile` before it dispatches, so a binding or a subscription onto a session that has
  since closed is already gone by the time anything reports it. That is one policy in one place, and
  it replaced four — `/status` and `/follow` dropped both facts, a prompt dropped only the binding,
  and `/use` dropped neither, so which one a chat got depended on what it happened to type. A press
  is the deliberate exception and does not come through `answer` at all: it carries its own session,
  never went through a binding, and the message it was pressed on is simply older than the session it
  was about.
- **Selectors.** `/options` draws the agent's own pickers — mode, model, effort — with a button per
  value and a dot on the one in force, which is what stops somebody pressing to find out what is
  running and changing it by accident. `Chat::selectors` flattens the two shapes the protocol keeps
  apart (mode is a field of `session/new`, the rest a config group) because from outside the app they
  are one question, and `Chat::choose` is the single place that routes a pick back to the right
  request. **A picker press carries its group by name and its value by position** — the opposite way
  round from a card, and for a stated reason: a card is frozen once raised so a place in it cannot
  move, while what the agent offers is live and its groups come and go. A group or a choice that has
  since moved is refused rather than settled for the nearest, which is affordable here in a way it is
  not for a permission: a picker set wrongly is one more press, a grant is not. `Press::fits` is what
  keeps a group name the agent chose from silently overrunning the payload cap and making the far side
  refuse the whole message; a choice that cannot be carried is dropped, counted, and said.
- **Every remote path finds its window the same way**, through `remote::ask_windows`: each window is
  asked in turn and the one holding the session answers. A map of session to window kept on the bridge
  would need correcting on every open, close and restart and would be wrong in between; a window cannot
  be wrong about what it holds.
- **A message that cannot be read is answered, not dropped.** A photo or a voice note comes back as
  `RemoteEvent::Unreadable` and earns a sentence saying so. Silence is the answer reserved for a chat
  that is not on the list, and giving the same answer to somebody who *is* makes a working bridge
  indistinguishable, from the far side, from a crashed one. A caption is not read as the message
  either: handing the agent "fix this" with no picture is a worse answer than saying the picture did
  not travel.
  **A chat is bound by being told to and never by being guessed at**: one root runs as many sessions as
  it is asked to, so "the active one" moves every time somebody clicks a rail row, and a message sent
  from a train would land wherever the window happened to be pointing.
- **Reopening.** `/archive` lists saved conversations flat across every project, newest first, capped
  at ten — the question asked from a phone is "put back the one I was in", not "let me browse a
  month". `/open <n>` mints a session on that conversation's *own* root and resumes it, then points the
  chat at what it just opened (not a guess: naming the conversation is naming where the next prompt
  goes). Two things this needs that nothing else on the bridge does. The scan reads a file per
  conversation, so it goes to `cx.background_executor()` and **sends its own reply when it lands**
  rather than returning one — the only command that answers late. And it needs a `Window`, because
  showing the new session is what spawns its adapter, so it reaches the shell through
  `OpenWindow::handle` instead of the entity alone. **The number is a place in the listing, not an
  identity** — a saved conversation is named on disk by an agent-chosen session id, too long to type
  and too long for a button to carry. What makes a place safe here is that the bridge keeps the listing
  exactly as it went out and `/open` counts into *that*; re-scanning would reintroduce the drift that
  made session numbers uids in the first place.
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
  retrying cannot fix ends the stream with `Disconnected`. Same spirit as the ACP client racing
  `child.wait()`: what cannot recover surfaces, what can does not. Three qualify — a rejected token, no
  bot behind it, and **409, which is a different kind of unfixable**: another process is polling this
  same bot, and a token has one queue, so two pollers split the messages rather than each receiving
  them. Left as transient the two retry against each other for as long as both run and which one hears
  any message is a coin toss. The far side ends the *older* poll, so treating it as terminal means the
  instance already running stands down and the one just launched keeps the bot. **The handshake waits
  the same way the poll does** — the app runs it once at startup, so a machine whose wifi is not up
  yet, or that is behind a VPN still connecting, would otherwise have no bridge until somebody noticed
  and restarted the app.
- **Nothing the channel says about a failure carries the token.** The Bot API puts it in the URL
  *path* and an HTTP client names the URL in its own error text, so a dropped connection — the most
  ordinary thing that happens to a long poll — would print a working credential to stderr and undo
  everything the Telegram plugin's `secret` module is for. `Telegram::redact` stands between the two, and `call` takes the
  bot and a method name rather than a finished URL precisely so the one place holding the credential is
  also the one place that turns a failure into words.

### Unattended runs

[docs/unattended.md](unattended.md) is the whole account; this is the part a change elsewhere can
break. **The switch is per project** (`ProjectRoot::unattended`, kept in the workspace file by path
like a pin) and off until turned on: from the project's ••• menu, the project page's menu, or the list
of switches in Settings ▸ Workspace, all through `Shell::toggle_unattended`. `[unattended]` in
`onehand.toml` has no switch of its own any more — only the label (default `auto`), the interval,
the timeout, the mode and the agent. A tick on `Shared` (one per process, like the bridge) looks for
the oldest open issue **you** opened that carries the label, in the opted-in projects, in rail order;
a project row says `auto`, `auto · #N` while a run is on issue N, or `auto · #N waiting` while it
waits on a card (`crate::unattended::live_runs`,
with `cx.refresh_windows()` at start and settle because nothing the rail watches changes).
**Everything outside the checkout goes through a connector, and a project that cannot be worked
says so.** `onehand_core::connector::Connector` is the trait — account, whether it serves a project,
issues, labels, comments, default branch, pull request — and `plugins/builtin/connector-github` is the
one implementation, where `gh` is the whole API layer. `connector::serving` hands a project to the
first connector whose `serves_blocking` accepts it (GitHub reads `origin` locally before any call),
and each connector's `account_blocking` says who it acts as. A run's fetch is the connector's too
(`Connector::fetch_blocking`, plain `git fetch` by default): GitHub retries a failed one over HTTPS
with `gh`'s own sign-in when `origin` is an ssh remote (the HTTPS URL is built from `origin` itself,
alias resolved, never from `gh`'s default repository), since an ssh remote often cannot authenticate
from an app opened off the desktop while `gh` is already signed in. Only the fetch — the agent's own push still uses `origin`.
**An issue lives on the forge or in onehand** (`unattended::Tracker::Forge` / `Local`, the latter the
project's Issues tab), and a run carries both where its issue lives and the forge its work goes to,
because the two come apart. The project's own issues are searched first. A local issue on a project
with a forge still ends in a pull request, one that must not reference `#N` (onehand's numbers are not
the forge's). A project **no connector serves** is still worked on its own issues: branched off the
branch checked out, told to commit and not push, judged by its commits past the start
(`unattended::Verdict`, `worktree::commits_since_blocking`). The outcome is a comment on the forge or
a note on the local issue. What refuses a project is nothing to work (no forge, no storage) or not
being a repository. **A project whose issues are kept in step with its forge is searched through the
sync and only there** (`Tracker::Synced`), so no issue is found twice: each search syncs first, a
claim takes the label off locally and the sync takes it off the forge, and every note is also left
as a comment on the forge's issue. The run's pull request references the forge's number, never
onehand's. **An imported issue is taken only if the forge says the user wrote it** — the same rule
the forge's own search keeps, because an issue's body goes into the prompt word for word and anybody
with triage rights can label an issue anybody wrote; the forge is asked since only it knows the
author. **An issue brought in from a forge is never run as the user's own by any other route**
(`LocalIssue::imported_from`, kept after the link goes): `Tracker::Local` takes only issues
`written_here`, so turning sync off, a forge unreachable for one tick, or a link lost cannot turn
somebody else's text into a prompt.
What either finds is kept per project (`Unattended::problems`) and shown as the pill in the warning
ink with the reason on hover — never only on stderr, since a switch that is on while nothing can
happen looks exactly like one that is working. **A config that stops every run** (empty label,
unparsable interval, a mode the agent lacks) is `Unattended::blocked`, and it is kept rather than
leaving the state unset: the switches stay on screen, so the reason has to be readable by the rows
and by Settings, and every connector is still asked. A project is looked at when it is switched on
(`check_now`), when its window registers and on *Check again* (`recheck`), and on **every tick, a
run included** — all three through one `look_blocking`. One row per connector, signed in and as whom, is on
Settings ▸ Connections, with its *Check again*; Settings ▸ Workspace keeps *Look now* and the switches. A run's own worktree never offers the switch (`ProjectFacts::unattended` is
`None` there). **A run can also be picked by hand**: *Work an issue…* in either project menu opens
`dialogs::pick_issue` over `unattended::open_issues_blocking` (every open issue, author on the row,
bounded at `ISSUES_SHOWN`), and `unattended::start_picked` runs it now. That run is shown as it starts
(`Shell::show_unattended`) and kept on screen when it ends. *Look now* in Settings runs the search at
once. Every run writes its own log into its transcript as notices (`unattended::note`) — the start,
the prompt going out, a cancel, and the words the issue was told at the end. A run the search finds claims its issue by removing the label,
branches a worktree off `origin/<default>` and mints a session there. **Neither step moves anything on
screen**: `ChatPane::open_unshown` connects without showing, and the worktree's root is
`ProjectRoot::transient`, which `to_config` never writes. One prompt, one turn. The pull request the forge
finds — or, with no forge, the commits on the branch — is the verdict, on every ending. **A parked
ask waits for a person and is never answered by the run**: the card stays up, announced like any
other, and answering it from anywhere lets the turn carry on — answering is not taking over. While it
waits the run's timeout does not count (`unattended::Budget`) and it **gives up the slot**
(`Unattended::runs`), so the search looks for the next issue at once; a run is only *started* while
none is working, but one whose card is answered carries on beside it, since a turn under way cannot
be held. Cards are watched for being answered by observing the session, because an answer emits no
event of its own. An adapter
lost or a session closed while waiting ends the run as `Ending::Asked`, with the question on the
issue. A prompt of anybody else's is a **take-over**: the run stops watching, which clears
`transient` and saves. Teardown is
`Shell::forget_root`, never `remove_root`, because that one re-shows the active session and takes the
caret with it. The rules that decide are core's (`onehand_core::unattended`); the calls are the connector's.

### The chat pane

[crates/app/src/chat/](../crates/app/src/chat/):

- `session.rs` — one ACP session: the `Chat` model from core, the event pump, and the front-end-shaped
  caches (parsed markdown per block, decoded images). The pump is **held**, not detached: dropping the
  session drops the task, which drops the receiver, which kills the adapter. Nothing else has to
  remember to shut an agent down.
- `transcript.rs` — one element per `ChatItem`, following DESIGN-ANSWER.md §5. Bounded (§8).
- `composer.rs` — the card the pane mounts: the input, `@`/`/` completion, attachments,
  agent-advertised selectors and Send. It draws itself and reports the send press as an event,
  because which of Send and Stop was pressed is a question about the turn, not about the click.
  **Everything that floats over it is built here and drawn as one object** — the card, a parked
  permission, a parked question, an adapter still connecting, a queued prompt — sharing one
  surface, hairline, radius and shadow, because written out per card they had already come apart
  and a permission parked above a queued prompt read as two unrelated things rather than as the
  same interruption twice. The card is capped **narrower than the transcript's reading column**
  (`COMPOSER_COLUMN`) and everything pinned above it takes that cap; the three settings lists the
  chips open all come from `picker_rows`, one answer because opening a list, walking it and drawing
  it each ask for it. **`picker_rows` answers `Some` for all three**, empty vec included — its
  `None` arms are the completion list and the attachment tray — so anything asking "does this
  control exist" has to ask `mode_action`/`options_action`/`fast_action` instead, which is what the
  chips themselves are drawn on. Asked the other way, a row was offered for a setting the agent
  never advertised, and taking it opened an overlay with nothing to draw while still holding the
  arrow keys.
  **The popup is one shell for every overlay**, and what changes between them is the contents of a
  row. It carries a **pinned title and a pinned footer**, both outside the scroll, each with a
  hairline on the edge that faces the list: held among the rows, the title went away the moment the
  list was long enough to need it and the footer sat under whatever half-row the scroll stopped on.
  The bound that has to come out a whole number of rows is on the **scrolling box** and not on the
  surface, because the surface also carries that chrome — floored there, the fold landed wherever
  the chrome happened to leave it. Its height is **measured once, against an empty query**, and held
  until it closes: the height belongs to the list rather than to what is typed, and taken from what
  was on screen it held while a query narrowed and grew when a character was deleted.
  **The `@` list is three groups** — files, folders, then what this session already touched
  (`Chat::artifacts`, read back out of the transcript). Folders are derived from the file list
  rather than walked for, carry what is beneath them, and insert a trailing slash, which is a
  listing rather than a read. **The `/` list leads with the composer's own controls** — `/model`,
  `/mode`, `/fast`, `/attach`, `/mention` — then the agent's, unprefixed first and each namespace as
  a run of its own with the prefix taken off the rows and put on the heading. Those rows *act*
  rather than complete, so accepting one takes the text that reached it back out of the buffer
  (`completion::remove`): it is a way to a control, not a message.
  **Standing state is a bare strip under the card**, outside it: the project's branch on the left,
  the permission mode on the right — left is the project, right is the turn. The branch is a
  control rather than a label, emitting `ChatPaneEvent::Project` so the shell opens the same menu
  the rail's project rows carry, branch rename included.
- `pane.rs` — what the shell mounts: session switching, the resume picker, the project page, unseen
  badges, and the run plan the virtualized list reads.
  **The transcript stops being drawn at the composer's middle** and fades into the surface over the
  last few lines before it (`SMOKE`): the overlay is transparent around its surfaces, so an unclipped
  row stayed visible either side of the card and read as the card having been dropped on the text,
  while an unfaded clip is a line — full-strength text for one row and gone the next. The fade is
  drawn between the list and every control, so what it takes is the conversation alone, and it ends
  at the clip rather than at the card, which is narrower than the panel.
  Its **header is the panel's only chrome** (the dock draws the conversation bare), and it is split by
  what a control is *about*. **The conversation's name is prose and the vertical-dots mark at its
  end is its menu** — the name full-strength ink and semibold against an otherwise muted row, the
  mark the same small ghost button as the row's other controls, with the popup anchored to the mark
  so it opens directly under the dots that were pressed. The name truncates while the mark never
  shrinks, so narrowing the panel shortens the name and never takes the control; and the mark
  carries a tooltip, which the name-as-button never could — the library builds a button's
  accessible name from its label alone, and the name had to be a child to ellipsize. Behind it: *Rename…*,
  *Export as Markdown…*, *Export as JSON…* (named and disabled — it is planned, and leaving it out
  would say otherwise), *Resume in this session…* — named for what it does *to this session*, since
  the header now carries a control reaching the same archives that leaves the session alone —
  *Restart the agent*, and then, alone in the
  danger tint, *Delete conversation* — the only entry there that ends something for good.
  **The header is drawn on the project page too**, and the page no longer prints the project's name
  itself: the row names the project there and its menu is the project's (`chat::pane::project_menu`)
  — *Pin to top*/*Unpin*, *Work labelled issues* (checked while on), *New worktree…* on repositories,
  *Copy project path*, *Refresh Git status*,
  then *Remove from workspace* in the danger tint. **Not a copy of the rail's**: *New session* is the
  primary button in the middle of that page and *Open terminal* is a button at the end of the same
  row, and offering either again would be the page saying one thing twice within an inch of itself.
  The two facts that menu needs — pinned, and whether it is a git repository — are pushed by
  `Shell::sync_project_facts` from the three moments either changes (arriving at a project, pinning
  one, a git sweep landing), and the arrival push must happen **after** `clear_active`, which builds
  the page's state fresh and would throw an earlier one away. *Close session* is the one control that
  goes on that page, since it has nothing to act on; the terminal, the Workbench
  and the way back to a hidden rail stay, because all three are about the project and dropping the
  row took them away at the one moment there is no conversation to reach them from.
  **There is no status badge beside the name.** There was one — a pill with the rail's own
  `signal_mark` and either `Chat::activity_status` or the signal's short word — and what it said was
  said twice: connecting, working and awaiting approval are all on the running line at the foot of
  the transcript, a few inches below and nearer what they are about, while the rail's row for this
  session carries the same mark for the same condition. A second copy in the one row that never
  scrolls is a thing permanently on screen restating what is already on screen, and it took its room
  from the conversation's own name, which is the only thing in that row nothing else says.
  **What went with it** is the one state neither of those two spells out in words: a lost adapter
  now shows as the rail's mark and its tooltip rather than as a sentence in this header. That is the
  cost, taken deliberately; `Chat::activity_status` stays and still feeds the running line, the
  project page and the pane's own state.
  The row's controls (`ChatPane::header_control`, one builder so the call sites cannot drift) each
  sit on the side of what they act on: the way back to a hidden rail at the row's *left* edge, the
  side the rail returns to, and the right-hand end reading outward from the name — the
  past-conversations menu, *Close session* (offered only while there is one), the terminal, and
  last the Workbench, always, since its dock is the window's right edge and the outermost control
  should move the outermost panel. The terminal button carries a **dot in success ink
  at its corner while a shell is alive** — a child process outliving a closed dock is the one thing
  the icon cannot say, and closing the window is what would end it. The fact is pushed down from the
  shell (`ChatPane::set_terminal_live`, from `Shell::sync_panel_facts` and from every project switch,
  since a shell belongs to a project); the push is guarded on both sides because a terminal notifies
  once per chunk a build prints. They are **a size up and a tone down** — big enough to aim at,
  muted so four icons in a row do not out-shout the conversation's name beside them; the library's
  hover fill brings the ink back on the one about to be pressed. **Closing is a control and deleting is
  not**, and that is the split: closing keeps every word — the transcript is written at the end of
  every turn — while deleting is the one thing the app cannot undo, so it stays behind the dots, two
  presses and a warning away, and the two are never adjacent. The vertical dots beside the name are
  the row's one menu mark, and everything behind them is done to the conversation the name is.
  **The past-conversations menu** (`ChatPane::history_control`) is the one route to an archive that
  keeps the session on screen: picking a row emits `StartSession { agent, resume }`, so the old
  conversation comes up as a second session and the current one stays where it is in the rail. The
  other two routes both cost it — the project page mints a session on the archive picked, but it is
  what the centre of the window shows *instead of* a conversation, so reaching it means closing every
  session in the project; and the title menu's own picker takes the session in front of the user off
  its conversation to ask the question. It is offered **only while a session is showing**, because
  with none the project page is already that list. It reads **every agent's** conversations, as that
  page does and for the same reason: the question is which conversation, and which agent had it is a
  property of the answer. A conversation **already open in this window is listed and refused, not
  hidden** — two sessions on one archive each believe the transcript so far is on disk, so the
  second's first turn writes the file back holding only what came after it, and dropping the row
  instead would leave the conversation a user is most likely to look for missing with nothing said.
  The menu **scrolls and lists everything the project has** — a project worked in for months has
  more conversations than a menu is tall, and the cap on it (`HISTORY_ROWS`) sits far past what
  anybody scrolls to, bounding the *work* of building rows rather than editing the list; the one time
  it bites it says so.
  The listing is **held, not read when the menu opens** (`ChatPane::archives`): building a menu
  happens inside a render and a render cannot wait on a directory of files. It follows the *project*,
  so switching between two sessions of one root does not re-read it, and it is re-read at the two
  moments the store changes under a running window — a turn ending, which is what writes a
  conversation, and a session closing, which is when somebody is most likely to want it back.

- **The workspace page** (`ChatPane::show_workspace`, reached from the rail's *Workspace
  overview*) answers across every project what the project page answers for one: the unattended
  runs **waiting on you** (a parked card) and those **working**, then every project's **open
  issues**, most recently changed first, with a project filter and a line counting the closed ones
  left out. It is a third thing the centre can show, beside a session and the project page:
  `active` is `None` and `workspace` is `Some`, and both `show` and `clear_active` clear it —
  `clear_active` counts it in its "unchanged" check, or clicking the project the user came from
  would leave them on the page. **It holds no run store.** Runs are read per frame from
  `unattended::live_runs` (a `LiveRun` each), which is cheap because a run starting, parking or
  ending already calls `cx.refresh_windows()`. Issues come from **the projects' own files alone**
  (`issues::open_across`, core, tested): a forge's issue kept in step is imported into the same
  file, so reading the forge too would list it twice. They are read off the UI loop when the page
  is shown and at every `Shell::refresh_worktree` (turn end, window activation) while it shows,
  and the filter is applied when it changes rather than per frame, so the cap and the closed count
  follow it (`Across::left_out` is the count the cut line prints). A row is `dialogs::issue_row`,
  the picker's row, so an issue reads the same in both places. Pressing an issue emits
  `ChatPaneEvent::OpenIssue` and the shell selects the project, opens the Workbench on Issues and
  sends `Request::ShowIssue`. The mode makes the project's entry if its read has not started, and
  the read only fills that entry, so the selection survives the load. Pressing a run emits
  `ShowRun`. In another window, that window is activated and its shell shows the session
  (deferred, since one shell must not be reached into from inside another's update). Both
  lists are capped (`PAGE_RUNS`, `PAGE_ISSUES`) and say so when a cap bites. An unbound
  workspace says it keeps no issues, as the Issues mode does.

The **model** is core's (`onehand_core::chat`): `Chat` + `apply(AcpEvent)`, the conversation store, the
Markdown export and the activity-run rules. `ChatSession` derefs to it, which is what lets the whole
renderer read `chat.items` / `chat.busy` without knowing where the model lives.

### Workbench

[crates/app/src/workbench/](../crates/app/src/workbench/) — one dock panel, five modes. **The panel
draws none of them**: each is a crate implementing one trait, holding its own state and handing back
its own view, and what is left here is the list, the active ID, the strip and the two facts the frame
reads off the showing mode's declaration (see *Built-in plugins*).

**The mode strip is the terminal's tab strip drawn again**: same padding and gap, and each mode is a
chip rather than a library `Button` — `accent` with the ink that goes on it for the one showing,
nothing until the pointer arrives for the rest, muted ink otherwise. As buttons the showing mode took
`primary`, which is the theme's strongest fill and is reserved for the single most important action
on a screen, spent here on a control that only says which view is up; and two rows of the
same kind an inch apart then disagreed about what a selected tab looks like. No icon, unlike the
terminal's tabs: those are all the same program and need the glyph to read as tabs at all, while
these are different things their names already tell apart. The chips are `flex_none` inside a
`flex_1 min_w_0` box, so what gives way when a fourth mode arrives is the box and never the maximize
and hide buttons at the other end.

**It draws itself as a card floating in its dock**: inset on every side but the seam, one border, one
radius, `overflow_hidden` so the strip's hairline and the file tree's own border stop at the rounded
corners, and `crate::theme::dock_surface` under it — which is the reading surface, the same one the
conversation is on, so the border and the inset are the whole of what says where the panel begins.
The terminal takes the same answer, both through one function so the two cannot drift.
**The seam is flush, and that is the resize grip's doing**: the dock's grip is a fixed band a few
pixels either side of the dock's own edge and there is no hook to move it, so an inset there leaves
the one line a user reads as draggable sitting outside the only place a drag is taken — the panel
resized from a strip of apparently empty surface while the border did nothing. Flush, the border *is*
the grip. It costs nothing to look at, because what is on the other side is the conversation on that
same surface: the card still stands off its neighbour by whatever that neighbour keeps clear. The
other three stay inset, since a card held off nothing reads as part of the window, and `track_focus`
stays on the *outer* box so the gap belongs to the panel and a click landing in it is a click on the
Workbench. **The gap is doing the work a fill used to share**: with both cards level with the
conversation it is what says a dock is something put down on the window rather than a piece of it,
and a dock drawn edge to edge reads as the window having been *divided* — two regions meeting along
a line, which is what the arrangement stops being the moment either dock closes and the conversation
takes the space back.

**`dock_surface` is the reading surface, and a dock card is marked by its border alone.** It was the ramp's
well step, one notch up from the conversation — which in the dark palette made the two docks the
*lighter* regions on screen with the conversation as the dark gap between them. Lighter reads as
nearer, so two panels that are about the work were drawn in front of the work, and with both open the
one region nothing had raised was the one being read.
**Neither a smaller step nor a step the other way is available**, which is why it is no step rather
than a quieter one. A midpoint was tried, on the reasoning that a whole panel drawn in the fill a
quoted command takes is a slab of it the height of the window: it measures **1.07** against the
reading surface in both palettes, under the **1.14** floor the ramp's own tests hold every surface
pair to — and the light palette has only 1.15 between white and the well to divide in the first
place, so no value between them can clear that floor twice. Half a step is a step nobody can see.
Going *down* instead would need a value below a near-black reading surface, and there is none.
**What the flip gives back** is the well *inside* a panel. While the card was the well, anything sunk
into it had to borrow the reading surface to be seen; now those are the well again — the Markdown
mode's code blocks (which still name their own fill rather than take the component library's default,
so the next surface change is one edit in one place) and the hover fill on both tab strips. A
*selected* thing takes `accent`. **The rail keeps the well and is now the only panel that has it** —
the one panel not about the work at all — with its filled row on a ramp step of its own (`marked`);
see the rail, below.

- **Editor** (`Ctrl+Shift+E`): the project's file tree down the left, the buffers opened out of it on
  the right, one draggable divider between them. A quick editor, not an IDE: buffers in the plugin,
  rules in core (`onehand_core::editor`) — the size bound, the tab set, the **mtime guard**, labels,
  blocking read/save. Highlighting is gpui-component's tree-sitter over a deliberately small grammar
  set (decision D3) — no LSP. Reopening an already-open file **never reloads it**: a second click on
  a path must not discard what the user just typed.
  **The tree is still `onehand-workbench-files`, its own crate and its own `WorkbenchMode`**, held
  inside the Editor's `Mode` and forwarded to rather than copied in; `plugins.rs` lists the pair
  once. They were two entries on the strip, and the strip is where that cost showed — picking a file
  meant Files, reading it meant Editor, and the next file meant the strip again, which is a mode
  switch per file in the panel opened to move between files. The tree itself is unchanged:
  `tree::visible_rows` from core, bounded per directory and in total, `.git` skipped, git state as
  one-letter badges with a dot on a directory holding changes, and indentation as padding by depth
  rather than nested containers — hundreds of nested rows are hundreds of wasted elements.
  **The divider's position is not persisted**: one number per window, back to its starting width on
  every launch, against another key in the workspace file for something a drag re-answers in a
  second. **Both halves declare a 40px floor, and that is a budget rather than a preference**: a dock
  clamps its own width at `gpui_base::PANEL_MIN_SIZE` with no per-panel hook to raise it, so the two
  floors have to fit inside 100px minus the card's inset and border. They did not — 140px of tree
  against the library's default 100px floor for the other half is 240px of minimum inside a box that
  can be 82px wide, and the card clips what will not fit, so dragging the dock to its narrowest
  pushed the editor off the end and left the tree alone with nothing to say where the other half had
  gone.
- **Markdown** (`Ctrl+Shift+M`): the project's `.md` files on the left, the one being read rendered on
  the right. The index is the plugin's own (`onehand_workbench_markdown::index`) rather than core's,
  because nothing outside this mode wants a list of a project's documents — and it is a **walk of the
  root**, not a filter over the Files tree: that tree is lazy, so filtering it draws a project as a
  column of empty folders with every document still one unfold away. A directory appears iff a
  document was found under it. The walk is breadth-first and capped at 400 documents and 2 000
  directories, so a cut lands on the deepest level reached rather than on an arbitrary subtree, and
  the list says when one did; dependency and build directories are skipped by name, since one
  introductory document per package is hundreds of documents nobody here wrote. The folded set is
  what is *closed*, the opposite of the file tree's, because the whole index is known by the time it
  is drawn and a document list that opened folded would be hiding the one thing it is for.
  **It is live**: the open document's mtime is checked every 750 ms — one `stat`, and only while a
  document is open — and a change re-reads the file and re-sets the *same* `TextViewState`, so a file
  the agent is writing does not throw the reader back to the top on every touch. The read is core's
  `editor::read_blocking`, so the size bound and the mtime are the editor's. A file that vanishes or
  outgrows the bound is complained about **once**, because the failed check still records the new
  stamp. The mode is **read-only** — a second buffer here would be a second copy of the editor's tab
  set, mtime guard and unsaved-edit rules, so the header's *Edit source* hands the file to the editor
  instead. The walk runs when the mode is next **drawn** after the index went stale — a project switch,
  a turn ending, or `Ctrl+Shift+M` pressed on the mode already showing, which is why selecting the
  showing mode is deliberately not a no-op — and never at boot: a workspace with a dozen roots must
  not walk a dozen projects for a mode nobody opened, and being drawn is the moment that is known not
  to be the case.
- **A tab whose child exits is dropped**, in both the Neovim mode and the terminal's panel
  (`NeovimView::reap`, `TerminalPanel::reap`, fed by `spawn_pty`'s exit callback). Nothing notices otherwise: the grid keeps
  drawing the last screen the child painted, which after `:q` or `exit` is an empty one with a cursor
  on it, and the tab takes keystrokes nothing will ever read. Three things this owes.
  It is **deferred** through `Window::defer`, inside `spawn_pty` so both callers inherit it — the
  callback fires from inside the grid's own render, and the view that owns the tab is the thing
  currently rendering that grid, so reaching into it there is a panic rather than an error, at the
  exact moment somebody typed `:q`.
  It is a **sweep** over every tab rather than a removal of the one that spoke, because `PtyTab::finished`
  asks the process (`try_wait`) — so a child killed from somewhere else, or gone while its root was off
  screen, is collected too, and reaped rather than left a zombie.
  And it moves **focus only if focus was already inside that panel** — which is why in the Workbench
  the reap splits across the seam, the mode collecting its own children and the panel deciding the
  caret: a grid dropped while holding the
  caret leaves the window pointing at an element no frame contains, which takes the whole keymap with
  it — while a background shell exiting must not steal the caret from what the user is doing.
- **Neovim** (`Ctrl+Shift+N`): the real thing, in a PTY, on the project root. Here and not in the
  terminal dock because this is the panel about files. One per root and never a second — several
  shells is what somebody opens on purpose, while two editors on the same files are two views of one
  buffer with no way to tell which holds the unsaved copy. It is spawned through
  `onehand_terminal_ui::spawn_pty`, so it inherits the shared rules about `TERM`, resize, clipboard and
  reaping.
- **Issues**: the project's own issues, listed on the left (open first, then closed, each newest
  first), the one being read or written on the right, with *New issue*, *Edit* and *Close*/*Reopen*.
  The rules are core's (`onehand_core::issues`): numbering (`#1`, `#2`, … per project, never reused),
  the draft check (a title is required), labels as comma-separated words. **Kept in the workspace's
  storage directory, one file per project** (`issues/<folder>-<digest>.json`, named by
  `workspace::stem_for` as the workspace's own folder is), never in the checkout — a file there would
  leave a clean repository dirty and travel with every clone. So **an unbound workspace keeps no
  issues**, and the mode says so rather than offering a form whose work would be thrown away; it
  learns where to write from `Request::SetStorage`, told once when the panel is built and again on
  bind and unbind. **Every change is one read-change-write** (`issues::update_blocking`) under a
  process-wide lock and through `config::write_atomic`, made against what is on disk rather than the
  copy on screen, so a person editing and anything else in the process writing cannot each save a copy
  missing the other's change; a file this build cannot read is refused and never written over. The
  file is read again when the mode is next drawn after being shown or after a turn ends. What runs
  said about an issue is kept on it as **notes** and drawn under its body, the latest few, with the
  cut said. Unattended runs work these issues too — see *Unattended runs*. No shortcut yet, and no
  deletion — closing is the way an issue leaves the work.
  **Kept in step with the project's forge, both ways, when switched on** (`onehand_core::issues::sync`;
  the switch is the list's sync bar and is stored in the issue file as `synced_with`, so it needs no
  workspace key). A linked issue carries a `Link` whose `base` is the snapshot both sides last agreed
  on, and a sync is a **three-way merge per field** against it: title, description and state take the
  side that changed, a field both sides changed differently is a **conflict** that moves nothing until
  a person picks *Keep mine* or *Take GitHub's* (both reset `base` to the forge's side and let the
  next sync do the rest; taking the forge's takes only the fields in dispute, so a change here that
  nobody disagreed with is still sent), and labels merge as a set, compared without order, and never
  conflict. **A linked issue is followed after it closes**: the forge lists every issue changed since
  the last sync (`Issues::last_synced`, asked from a day earlier since its search is by day), so a
  closed issue edited on either side still crosses. The open list and the changed list are cut at
  the cap separately; where the changed one is cut or there is no sync point yet (a file from before
  it existed), every linked closed issue is asked about one by one instead, so a forge edit is never
  assumed away. The point moves on after any sync in which nothing failed to be asked. **In and out are not
  symmetric, by decision**: every open issue on the forge is imported (capped at `SYNC_CAP`, the cut
  said), while an issue written here goes nowhere until *Publish to GitHub* — a draft that published
  itself on a timer is one nobody could write. A push that fails leaves `base` alone so the change is
  retried rather than read later as the forge's; an issue gone from the forge is unlinked with a note.
  Forge line endings are normalized first, or every sync would see an edit nobody made. It runs when
  the file is read (at most once a minute), after every change made here, every five minutes on the
  project on screen, and on *Sync now*; the whole sync holds the issue file's lock across the forge's
  calls, so an edit made meanwhile waits for it; an edit saved while a sync runs sets a flag that runs
  one more when it lands. Every write moves `Issues::revision` on, and the mode keeps whichever copy
  is newer, since two landings can reach the screen out of order.

- **Plugins**: two tabs under one switch — **Installed**, the Claude Code plugins that reach a
  session started in the project on screen, one row per plugin (capped at 200, the cut said); and
  **Marketplace**, the known marketplaces' plugins, most installed first, searched, capped at 60
  rows with the cut said, and installed at the scope picked beside the search. Two lists and not one
  page, because stacked, a few hundred catalog rows pushed the installed ones out of reach and the
  search scrolled away with them; the search and the scope picker are pinned above the catalog's
  list, and each tab keeps a scroll of its own. The switch is `onehand_plugin_host::switch`, the one
  the rail's *Projects* / *All sessions* uses. **Each row
  carries a switch per scope — Global (Claude Code's `user`), Project, Local — showing what is in
  force *at* that scope** (what it sets itself, else what the next wider one sets), outlined where
  the scope sets it and ghosted where it inherits; pressing one writes the opposite answer at that
  scope alone, so turning a global plugin off for one project touches only that project. **A scope
  is offered only where the plugin is installed at it or wider** — Global for a plugin installed for
  this project alone would write a setting naming a plugin no other project has. The row also says
  in words what a session here gets (*on here* / *off here*), and **when that disagrees with the
  listing's folded answer it says *by settings not shown*, in the warning ink**: managed settings,
  a policy or a settings flag decided it, and the switches describe the three files rather than the
  outcome. *Remove* is per scope the plugin is installed at. **Every change goes through `claude plugin … --scope`
  and nothing writes Claude Code's files**: their layout belongs to Claude Code, and its install
  record already carries a version number. **What each scope sets is read from the three
  `settings.json` files' `enabledPlugins`**, because the listing's `enabled` is the answer with every
  scope already folded in — the same on every record of one plugin — so it cannot tell a project
  turning a plugin off from a plugin never turned on. The folded answer is never the fallback — it
  includes the narrower scopes, so it would read a project's *off* back as *off* everywhere — and a
  plugin no scope mentions is off, which is what the listing says of one. It is kept only to be
  compared against, which is how a source the files do not show is noticed. The
  listing names every project's installs; only the global ones and this project's are kept.
  **Each record is read on its own**, and one this build cannot read — a scope it does not name,
  such as `managed` — is skipped rather than failing the listing, so a plugin installed by managed
  settings is not listed here while every other one still is.
  **An install never passes `-y`** — a marketplace can declare a command to run, and `-y` accepts it
  unseen — so one that wants a command refuses on the status line, left to a person in a terminal.
  **Remove keeps the plugin's data** (`--keep-data`): reinstalling undoes a removal, nothing undoes
  a deleted data directory. One change at a time, since each rewrites a settings file or the install
  record, with the pressed control showing it working; the list is read again after each, when the
  mode is shown and at every `Rescan`. A change reaches a session when its agent next starts, and
  the mode says so. There is no *Remove override* — the command line can set a scope on or off but
  not back to saying nothing, so a scope once set stays set until its file is edited.

State is per project root, held by the mode that works on it, so switching roots swaps the whole
thing.

Three things the Neovim mode owes that the other two do not, all because it is a live PTY rather than
an element tree:

- **Its zoom is a font size, not the rem scale** wrapped around the other bodies. The grid is
  *measured* from a shaped glyph, so scaling the box around it stretches the container while the cell
  stays put and every column lands past its own character. `Workbench::set_zoom` broadcasts the size
  instead, which is why the shell hands it the whole value rather than `&mut` to the field.
- **The panel takes the key context `Terminal` while this mode shows** — read off the mode's own
  declaration, not worked out from its ID — and it must be that name and
  not one of its own: `Ctrl+S` is bound `Shell && !Terminal` exactly so a program in a PTY keeps it,
  and a grid mounted with no such context would have the quick editor's save fire over the top of
  `:w`.
- **Switching to the mode does not spawn.** `Ctrl+Shift+N` spawns and then switches, and the empty
  state carries a *Start Neovim* button; a mode strip where one of three buttons launches a process
  is one nobody can click to look around. The key opens and focuses Neovim; `Ctrl+Shift+J` hides its dock. Hiding
  puts the editor aside rather than ending it, since the panel entity outlives the
  dock and the PTY, the scrollback and the unsaved buffer are all still there on the next press.
  `nvim` is looked up on `PATH` in the app rather than handed to the PTY to fail on, because a failed
  spawn comes back as "No such file or directory" naming nothing; it is `nvim` and not `$EDITOR`,
  since honouring that would open `vi` for somebody who set it years ago for `git commit`.

### Terminal panel

[crates/app/src/terminal.rs](../crates/app/src/terminal.rs) over `vendor/gpui-terminal`. A tab per root,
spawned lazily; dropping a tab drops its PTY, so the child dies with it and there is no separate
shutdown to forget.

**It is a card in its dock, the Workbench's shape exactly** — inset on every side but the seam, which
here is the top, one border, one radius, `overflow_hidden` so the strip's hairline stops at the
corners, `crate::theme::dock_surface` under it and `track_focus` on the outer box so the gap belongs to the
panel. Two docks answering "where does this panel begin" differently would read as two separate
decisions, and that includes which side is flush: each is flush against its own dock's resize grip,
for the reason given there. **Its right edge stays inset although a grip runs down that too** — the
Workbench's dock is a sibling of this whole column, so its grip is the height of the window and
passes this panel as well; but the border that seam moves is the Workbench card's, flush against it
and the thing a user aims at, and a second flush edge here would leave the two cards' borders
touching with no gap between them. **It is the one this costs something**: the grid measures its own
bounds and resizes the PTY to match, so the inset is a column of cells and half a row — paid once,
since the inset is fixed while the dock is dragged.

**The grid is drawn in the panel's own surface**, and has to be told which one rather than reading the
theme: a terminal fills every cell it has not been told otherwise about with its palette's default
background, so `terminal_palette` takes the surface as an argument and `spawn_pty` passes it through.
Both callers hand it `dock_surface` — the terminal dock from the app, the Neovim mode from the plugin host
— and the parameter is there so neither has to guess what the other did. Two consequences worth
knowing: whatever that value is, it is also ANSI *black*, deliberately, since a program asking for
black means "the background" and answering with anything else puts a plate of the wrong shade behind
the runs that asked to disappear; and `TerminalThemeKey` watches both the reading surface and the
well even though it reads neither, or a change that moved only the one in force would leave every
live grid painting the old surface.

**Lazy about roots, not about the clock.** A launch restoring a saved layout used to mount the panel
and stop, so a user who left the terminal open was met on the next launch by an empty dock asking
them to press *New terminal* — a question they had already answered by leaving it open.
`Shell::fill_open_terminal` holds one sentence instead — **a terminal dock on screen has a shell in
it** — and what earns the shell is the panel being *drawn*, not any particular route to it: arriving
at a project, returning to one, a launch restoring a layout, or moving between sessions in a project
whose last shell was closed. It runs at the handover and nowhere else, for the reason the handover
itself exists — the dock is read rather than assumed — so it is the arriving root alone and only
where that root's dock is showing: a workspace of a dozen projects still starts at most one shell, in
the project on screen. The session-switch case is an `else` rather than an early return for exactly
that coverage, which is also the one place the *New terminal* button is still reachable: closing the
last shell leaves the panel empty until the next arrival.

**Every tab here is a login shell; Neovim is not one of them** — it is a Workbench mode, because that
is the panel about files and a tab called `nvim` between two called `zsh` says the editor is a kind of
shell. The `onehand-terminal-ui` crate owns spawning and `Program`, so the Workbench starts its grid
through the same rules about `TERM`, the resize
callback, the clipboard hook and reaping the child. A second copy of those is a second copy to keep in
step.

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

The grid answers the questions a full-screen program asks. **Terminal replies go back to the PTY** —
alacritty hands out Device Attributes, the cursor position report and colour queries as events because
it has no idea where the PTY is, and upstream dropped them; a colour query is resolved against the
palette in force, so an editor's light/dark detection follows the app's appearance. **Mouse reporting**
picks its encoding from what the child enabled (SGR where it asked for 1006, the legacy byte form
otherwise — sending SGR to a program that only asked for 1000 *types* the escape sequence into it),
reports motion per cell rather than per pixel, and forwards all three buttons. **Holding `Shift` takes
a gesture back from the child**, which is what keeps selection possible under a program that has
grabbed the mouse. On the alternate screen with no tracking, the **wheel becomes arrow keys** — there
is no scrollback there for it to move. **`DECSCUSR`** is drawn: shape, `DECTCEM` hiding, a hollow
outline when unfocused, and the character repainted over a block cursor that would otherwise hide it;
blinking is deliberately absent, since it needs a repaint on a timer in a view that otherwise draws
only when bytes arrive. **`OSC 52` is answered for writes and refused for reads** — a yank reaching the
system clipboard is the point, and answering a read hands the clipboard to whatever is running in the
terminal, including at the far end of an ssh session.

**Focus is reported** (mode 1004, `view::focus_report`), and it is the app's own reason for existing:
an editor asks for this so it can re-read a file that was written while the user was elsewhere, and
here "elsewhere" is one click away with an agent writing those same files. The window's *activation*
counts as well as the focus tree's — the caret being in the grid while the whole window sits behind
another application is not having the keyboard. The first frame reports nothing, because it is not a
change.

**The attributes that colour a cell are honoured, through one function.**
`render::cell_ink` applies `INVERSE` (how most colour schemes draw a status line, a visual selection
and a search hit — unswapped they come out dark on dark, which reads as a broken theme), then `DIM`,
then `HIDDEN`, in that order and for the background pass, the glyph pass and the cursor's repaint
alike. **One function and three callers is the point**: three places working the same rule out
separately is exactly how the cursor came to be drawn over the character underneath it.
`render::underline_style` draws every underline the protocol has — curly for `UNDERCURL`, straight for
the double, dotted and dashed forms GPUI cannot express — and takes the colour from
`Cell::underline_color`, which is the half that carries the meaning: a language server marks an error
and a warning with the same squiggle and a different colour. Strikethrough is drawn too; it had been
hard-coded to `None`.

### Window shell

[crates/app/src/shell.rs](../crates/app/src/shell.rs) owns the window: the rail plus a `DockArea` whose
centre is the chat, right dock the Workbench, bottom dock the terminal.

- The **rail** ([rail.rs](../crates/app/src/rail.rs), gpui-component's `Sidebar`) is app chrome and
  lives *outside* the dock, so a layout restore cannot lose it. **It is drawn in the ramp's well and
  is now the only panel in the window lifted off the reading surface**, asked for at the call site
  rather than left to the `sidebar` token, which ships a value of its own and would bring the panel
  up level with the conversation beside it. The library applies the caller's refinement after its own
  `bg`, which is what lets it win. The two docks took this same step for a while, which made lifted
  mean nothing more precise than "not the conversation"; they are flat on the reading surface now,
  marked by their cards, and what is left is the one panel that is not about the work at all. **`Sidebar`'s right border goes off with
  it** (`border_r_0`): the fill is the edge, and a rule beside it draws a line along a boundary that
  was not in doubt. That flag has been both ways — it had to be *on* while the rail was still on the
  reading surface, and off before that, when the library's drag handle ruled the same seam in the
  same colour. **`sidebar_accent` is a ramp step of its own** (`marked`), because none of the others
  fits a filled row on a rail drawn in the well: `hover` is 1.04 from it in the light palette, a fill
  nobody can see, and the reading surface is 1.19 in the dark one, which punches a near-black hole
  through the panel rather than lifting a row out of it. `marked` is 1.12 either way and lifts in
  both. It can afford to be quieter than a *surface* pair because a row has ink at full strength and
  a weight beside it, so the fill is the third thing saying which row it is — which is what the test's
  separate `ROW` floor records. The library draws a hovered row at 0.8 of that token and a selected
  one at full, so the two stay apart without a second token. It is **session-first**: every folder
  row lists its sessions, each row selecting root *and* session in one click. A session row is named
  by its **conversation** (`Chat::conversation_title` — the first prompt, or a rename), falling back
  to the agent's name until it has been prompted; the agent's name rides in the suffix only where
  **that project's own sessions disagree about it** (`rail::runs_more_than_one_agent`). The count
  used to be the configured agent menu's, which is the wrong set: a second entry in `onehand.toml`
  put the same word on every session of every project, including the nine running one agent
  apiece. A footnote is for telling two rows apart, so the question is asked of the rows.
  **Every list row is the rail's own** (`rail::RailRow`), not the library's `SidebarMenuItem`,
  because that component holds its label as a bare string in its own clipping box — no tooltip
  hook, no ellipsis, nowhere to hang a fade — so every answer to an overlong name was a guess made
  outside the row about what would fit inside it (a character cap derived from the rail's width
  through an assumed glyph, charged again for the nest inset and the footnote, wrong by a
  character either way). **A name that runs out of room now fades into the row's own fill**
  (`rail::faded`) instead of being cut at a character with an ellipsis: the cut happens in pixels
  where the room actually ends, an ellipsis asserts "there is more" even when the name fit
  exactly, and the fade only takes text that is actually leaving. The overlay is painted in the
  row's composited surface per state (`rail::row_surfaces`) — rest, hover via `group_hover`,
  active — because a fade into the resting colour over a hovered row is a smudge on exactly the
  row being looked at; painted right it is invisible wherever the name already ended, so nothing
  needs to know whether the name overflowed. **The fade needs a box that is the room and not the
  text**, which is why the stretch lives inside `faded` rather than at its call sites: the band is
  pinned to its box's right edge, so on a box that shrink-wraps its string it lands on the last
  glyphs of a name that *fitted* — `main` on a branch row came out as `m` dissolving into the
  fill, which is the ellipsis's dishonesty back in a worse form, since nothing says a cut
  happened. So the two **names** fade (a row's label, the workspace's) and the two capped
  **footnotes** — the agent-or-project word beside a session, the branch beside a project — keep
  `truncate` and its ellipsis, because each is sized by its own string and a fixed box for them
  would reserve the width the whole arrangement exists to give the name.
  **Every row carries its full text on hover**: the conversation's whole title and its footnote
  named (`Agent:` / `Project:`) on a session row — the flat list is where that bites, since the
  footnote is the only thing on the row saying which project a session belongs to; the project's
  name, branch, change count in words and root path on a folder row, which also covers the
  project that is neither a repository nor changed, the one row the old suffix-tooltip could not
  reach. The label is still cost-capped at `LABEL_SHAPE_CAP` characters, far past what the widest
  rail can draw — a fit rule in pixels, a cost bound in characters, and the bound must never be
  the visible cut, which a test holds against `PanelLayout::RAIL_MAX`.
  `Sidebar` itself stays: the frame, the scroll and the header/footer slots are the half of the
  component worth keeping, and its `Clone` child bound is why `RailRow`'s handlers ride in `Rc`s.
  A trailing mark appears only while that session carries a signal, and
  **the one state that is wrong has a shape of its own**: a warning icon for a lost adapter, because
  that is the mark that must not depend on colour. The other three are one dot in three tints — a
  warning dot for busy, an accent dot for a parked question, a success dot for a turn finished
  unseen. Busy is deliberately *not* a spinner: a session is busy for minutes at a time, and the one
  moving thing on an otherwise still rail pulls the eye for as long as it runs. Every mark names
  itself in a tooltip (`rail::signal_hint`) — colour alone is a code that has to be learned first
  and cannot be read at all by someone who does not separate red from green.
  **The selected project row is marked whether or not it
  holds the session on screen**, only the selected project starts expanded, and a project with no
  sessions expands into a *Start a session* row rather than into nothing. Branch and
  change count ride in the suffix — the count as a badge, not a coloured number — with the full
  branch, the count in words and the root's path on the row's own hover. **The branch is written
  out on the selected row alone**: it is what you read while working *in* a project, and on the
  ten rows you are not in it is ten strings cut short, where `feat/consol` and `feat/codoh` say
  nothing to tell their projects apart while taking the width from the name that would. The count
  stays on every row, because it is a signal rather than detail. The primary *New session* button
  names the project it would start in, in its tooltip.
- **Selecting a project and folding it away are two different targets.** While the whole row
  toggled, every click on a project both switched to it *and* snapped its sessions shut, so reaching
  a session in the project just arrived at meant clicking the row a second time to undo what the
  first click did. **Open and never toggle**, which is not the same as leaving the fold alone: a row
  that only selected would hide the sessions of every project the user had ever folded, and arriving
  at one would mean hunting a caret to see what is in it — the same extra click in mirror image.
  Going to a project is asking what is in it, so `Shell::select_root` reveals; only the caret puts
  it away again.
- **The fold belongs to the window, not to the row** (`Shell::folds` / `project_unfolded`), and the
  rail draws the nesting itself rather than through the library's submenu. gpui keeps an
  element's state only across consecutive frames its key is *accessed* in, and the tab showing the
  flat list draws no project row at all — so a row-owned fold was destroyed on every tab switch and
  re-seeded on the way back, springing a folded project open and snapping an unfolded one shut. By
  path, like pinning, and a `HashMap<PathBuf, bool>` rather than a set: absent means untouched, and
  an untouched project follows the selection. A **folded project builds no session rows at all**,
  which is what keeps a workspace of ten roots cheap.
- **The list is two tabs, not two stacked groups** (`rail::RailTab`, drawn by `onehand_plugin_host::switch` in the
  header): *Projects* is the tree, *All sessions* is every session in the workspace, flat — the same
  set twice, so stacked it would be one panel listing every session below the tree already holding
  them, and the tree is what a workspace is read by. **The
  selected half is `accent` with the ink that goes on it** — the same spelling the terminal's tabs
  and the Workbench's mode chips use, so one condition keeps one code. It was the reading surface,
  which worked while the rail was drawn in that surface too; once the rail moved into the well
  the plate became the one thing in the window painted a step *below* what it sits on, which is a
  hole rather than a plate, and the `shadow_sm` under it could not say otherwise at that size. The
  shadow went with the change: a fill that differs lifts by itself, and the component library's own
  drop shadow over a near-black surface is invisible anyway — every step of that ladder is pure
  black at a tenth of an alpha, which against the dark palette's floating surface is a difference of
  three parts in 255. That is a fact about *those* values and not about shadow: where one has to be
  seen, `crate::theme::lift` draws the app's own, with the alpha chosen per palette. The dark one
  needs more than four times the light one, because a black shadow on white has the whole range to
  fall through and on near-black it has almost none.
  The flat list **sorts by when each session was made and by nothing else** — ascending `Session.uid`,
  which is the process-wide counter, so it is creation order across every root. It sorted by
  `SessionSignal::rank` first and then by recency, which put a parked question or a dead adapter at
  the top; what that cost is a list that rearranges under the pointer, since a session starting work,
  finishing it or parking an ask moves rows in the panel being aimed at, and being looked at moved one
  too. The mark on the row still says what each session wants, in a place that does not move. A flat
  row is still named by its session's uid and never by its place, since a closed session shifts
  everything under it.
  **Both lists draw the same row** (`rail::session_row`) — same click, same menu, same mark — and
  everything they disagree about is `rail::Note`, the footnote beside the mark: the agent on a tree
  row, the project on a flat one. They were briefly written out separately, which left the flat one
  a near-verbatim copy that would have drifted at the first edit to either.
  Neither list is capped, and that is one ceiling rather than two: every row is a session somebody
  minted by hand, and the tree draws the same set once its projects are unfolded, so a cap on the
  flat list alone would report as truncated a workspace the tab beside it draws in full.
  An empty workspace gets the *Start a session* offer rather than a blank panel, as a project with
  no sessions does in the tree.
  The tab is **not persisted**: a launch that came up on the flat list would be one where the tree,
  the thing that says what a workspace *is*, had to be found before anything else could be read.
- **The rail's header is a block one step above the list** (`rail::lead_row`): the workspace
  name and *New session*, taller, at a larger text size and a weight up, with the identity's icon in
  full ink rather than muted. At the list's own scale they read as its first two entries, which is
  what they are not. **The 16px icon column does not move** — only the row around it grows, or the
  header's labels would sit a few pixels off every label below them. *Add project…* sits between
  them, quieter than either: it is what a workspace with no project needs first and it is about the
  workspace rather than about the list, and it is done once per project where *New session* is done
  all day. It was the last row *inside* the Projects group, which is a place a tab bar cannot have.
  *Workspace overview* sits under it, just as quiet, and opens the workspace page (see the chat
  pane). It takes the selected fill while that page shows (`rail::rail_row_marked`, the same
  look as a selected list row), and **no project or session row is
  marked meanwhile** (`Shell::workspace_shown`, read off the pane rather than mirrored on the
  shell), since the page is about none of them. The active project's ••• menu stays, because the
  project is still the selected one.
- **The caret beside *New session* picks the project** (`rail::new_session_menu`), and the row itself
  is unchanged: one click still starts the default agent on the selected project. What the menu adds
  is the two things that click has to choose silently — every project in the workspace under *Start
  in* (each selecting that root on the way, since a session bound to a root the rail is not showing
  is an agent nobody is watching), and the agents under *With agent*. **Each section needs more than
  one of its own kind, and so does the caret**: one project and one agent leaves a control whose
  whole menu is a single row doing what the button beside it does, and gating one section that way
  and not the other is the rule applied in one place and not the other. A popup and not the list
  that used to expand in the rail: that list pushed the whole tree down while it was open, which is
  affordable for two agents and not for a workspace's worth of projects, and the flag saying whether
  it was open had to be carried on the shell and cleared on every path that started a session.
  It is drawn by `rail::menu_button`, the same builder the two ••• menus use — three controls, one
  shape, after the second copy of it appeared here.
- **The workspace identity row *is* the switcher** (`rail::workspace_menu`) — the whole row opens the
  menu, and nothing marks it but the hover, the pointer and the tooltip: no chevron, because a caret
  on the rail's topmost row competed with the primary action directly below it. The menu is the
  recents list — each row named by
  its folder with the parent path beside it (shortened from the *front*, since a path is read from
  its tail), the one on screen checked and unpickable — then *Open workspace…* and *New workspace…*.
  **The tooltip leads with the workspace's name**, because this row is the one place that name is
  written and it is written on one line: users put sentences in that field, and truncated there
  with nothing but *"Switch to another workspace"* on hover it could be read nowhere at all.
  **Nothing is replaced in place**: every entry funnels through `Shell::open_recent` /
  `open_or_focus`, so a pick opens another window or focuses the one already showing that folder.
  **This is now the only copy of that list.** Settings drew it too, as a column of ghost buttons each
  printing one absolute path whole, uncapped, in a dialog that does not scroll — so the longer the app
  was used the further the list pushed the fields above it off the bottom. Settings keeps the storage
  binding, which has nowhere else to live, and *New workspace…* / *Open workspace…*, which are about
  making a workspace rather than switching to one.
  A row can be a menu trigger at all because of `controls::MenuTrigger`: the library opens a menu
  from anything `Selectable`, `Stateful<Div>` is not, and both of those are other crates' — so the
  newtype that answers `Selectable` for a row is what stops the target being the icon at its end.
- **Both row kinds carry one ••• menu on the active row**, never a ✕. A project's holds *Pin to top*
  / *Unpin*, *Work labelled issues* (checked while on), *New session*, *New worktree…* (git repositories only), *Open terminal*,
  *Copy project path*, *Refresh Git status*, then,
  separated and in the danger tint, *Remove from workspace* (still guarded by a second click while
  the root has live sessions or unsaved buffers). A session's holds *Rename…*, *Restart the agent*,
  *Export as Markdown…* and, in the danger tint, *Close session* (guarded only mid-turn, since the
  transcript is written at the end of every turn — `Shell::close_session`, also `Ctrl+Shift+W`). The
  session menu is **also** the row's right-click menu, on every row and not just the active one;
  Restart and Export select the session first, so they always act on what is on screen.
- **Four `Dialog`s have no trigger: renaming a conversation, splitting a project into a
  worktree, renaming a branch, and Settings.** Every other dialog is opened by a control that carries
  `Dialog::trigger`; the first three are opened from a menu entry that is gone by the time they
  appear, so `Shell::renaming` / `Shell::worktree_draft` / `Shell::branch_draft` being `Some` is what
  puts each on screen and Esc/Cancel/close must all clear it. Settings is opened by a key as well as
  a rail row, so `Shell::settings_open` is its flag, and closing it hands the caret back to where it
  was (`settings_return_focus`). A conversation rename archives
  immediately rather than at the end of the next turn. **The branch rename is `git branch -m` and
  never `-M`** (`worktree::rename_branch_blocking`): the forced form overwrites a branch already
  carrying the new name and throws away what was on it, while all the user asked for is that this
  branch be called something else — so a collision is git's refusal, shown against the name that
  caused it. A name that has not changed closes the form and does nothing, since git accepts it
  and a sweep plus a notification for a no-op reads as something having happened.
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
  partition, pinning moves no root, and pins are stored by path so a root added elsewhere in the file
  cannot slide a pin onto another project. Nothing reorders the list on the app's own initiative.
- **The tree's order is dragged, and the two orders do not fight.** A project row is dropped onto
  another project row (`ProjectDrag` → `Shell::move_root` → `Workspace::move_root`) and a session row
  onto another of the *same* project's session rows (`SessionDrag` → `Shell::move_session`). Five
  things this owes. The numbers a project drag carries are **display positions and not `roots`
  indices**, since the rail drags what it draws and pinned projects are drawn first; the permutation
  is written back into `roots`, so the order somebody dropped a row into is the order the workspace
  file keeps and `display_order` stays a stable partition of it. A drag that would **cross the pin
  line is clamped to the near side of it** — pinned projects are held at the top on purpose, and a
  row that appeared to cross and then sprang back would say nothing about why, so a pinned project
  dropped on an unpinned one goes last among the pinned and stays pinned. `active_root` and
  `active_session` are **remapped**, because they are the only indices either permutation can move —
  everything else about a root is keyed by its path and everything else about a session by its uid —
  and the session on screen has to stay the session on screen. A **project drag persists and a
  session drag does not**, which is not an inconsistency: the roots' order is in the workspace file
  and sessions are not persisted at all. And the drags are **two payload types rather than one
  enum**, because gpui dispatches a drop by the payload's type: in one type a project row lights up
  under a session being dragged and has to refuse the drop afterwards, where in two it never offers.
  A session of another project is refused the same way one level down, and refused **twice** — in
  `drag_over` so the row does not promise a drop, and in `on_drop` so it cannot take one.
  **The flat list is deliberately not draggable**: it is in creation order across every project,
  which is not an order stored anywhere, so a drop would have nothing to write into.
  **The drop target is the row's own hover fill**, which is free to mean "the pointer is aiming here"
  because gpui suppresses hover styles for the length of a drag — so the fill is not two things at
  once. No line above or below it: a drop lands *at* the row it was made on, and everything between
  closes up behind the row that moved. What the drag does not have is **edge autoscroll** — a
  project past the bottom of a full rail has to be scrolled to first.
- **A project row rolls up its sessions' signals** (`SessionSignal::most_urgent`, same rank as a
  single session's `pick`), so a collapsed project is not silent about an agent waiting or dead
  inside it.
- **Closing the Workbench goes through one path.** `Shell::toggle_workbench` and the panel's own
  hide button both call `Shell::hide_workbench` rather than toggling the dock themselves. It had its own copy, and the copy was
  missing the half that breaks worst: the app-direction zoom belongs to the `DockArea` and knows
  nothing about which docks are open, so `Ctrl+Shift+K` then `Ctrl+Shift+E` closed the dock and left
  the panel filling the window with the rail gone and the caret in a composer no frame was drawing.
  `set_terminal_visible` carries the same guard for the same reason.
- **`Ctrl+Shift+B` hides the rail; it never narrows it.** An icon-width rail is ten identical folder
  icons, which is the one thing a session-first rail must not become. The way back is a button in the
  agent panel's header, shown only while the rail is hidden (`ChatPaneEvent::ShowRail`).
- **Every panel is a bare `DockItem::panel`; nothing in the window is a tab group.** `DockItem::tab`
  wraps its panel in a `TabPanel` whose title bar draws a tab carrying the panel's title — for the
  conversation that is the conversation's own name, printed directly above the header that already
  says it, and for the Workbench the word *Workbench* printed over the strip naming its modes;
  one tab that can never gain a sibling is not a tab. **The terminal's several tabs are
  its own**, drawn inside the panel with the shell labels, their ✕ and the `+`; the library tab group
  around it held exactly one panel and added a second strip saying "Terminal" over the strip that
  already names every shell. **The strip is drawn here and not with the library's `TabBar`, and that
  was tried before it was decided.** Every variant that component offers states more than this strip
  wants to: the default is browser chrome (a filled bar, a raised plate under the selected tab, a
  hairline ruled under the lot), `pill` makes the selected tab a capsule in the theme's strongest
  fill, `outline` rings it, and `segmented` and `underline` each bring a border of their own. The
  panel is one surface with a shell's rows on it and the tabs are a label on that surface, so what is
  wanted is a small filled rectangle in `accent` and nothing else — which is none of the five, and is
  not reachable from outside either, because the component writes the fill and the radius into the
  same style refinement the call site does and writes them later. It was the right reach and the
  wrong fit: **the thing to check first is whether the component's own choices are the ones you
  want**, since everything else it was holding here — the label, the glyph, the ellipsis, the
  accessible name, the ✕ — had already been written out at the call site to get the rest of the look.
  A hairline runs under the whole strip, spanning the panel rather than stopping under the tabs:
  neither the strip nor the grid carries a fill, so the only thing telling the chrome from the shell
  was the gap between them, and a gap reads as spacing where a line reads as an edge — which is what
  the top of a terminal is. The rule the flat variants drop is the one the *component* drew, seated
  under a filled bar and a raised plate; this is the same pixel doing a different job.
  **The tabs sit in a box of their own, and it is the only part of the row that gives way.** Flat
  beside the controls they pushed `+` and the way out past the panel's right edge at the fourth
  shell — the two controls wanted precisely when there are too many tabs were the two the tabs took
  away. So the list is `flex_1` + `min_w_0` and the controls are `flex_none`. **The box scrolls and
  the tabs inside it never narrow** — they did for a while, down to a floor, on the reasoning that a
  short tab can still be aimed at where a scrolled-out one cannot, and what that bought was every tab
  getting worse the moment a fourth appeared in order to spare the fourth a gesture. The cap that is
  left is on the *name*, so a long project ellipsizes rather than making one tab as wide as three.
  **The newest tab is not scrolled into view**, so past the width of the strip a new shell can be the
  active one with its tab off the end of the list — the grid is right, the strip is behind. Fixing it
  means a `ScrollHandle` on the panel, and `scroll_to_item` on a tab that has never been painted
  needs deferring past the frame that first draws it.
  **The strip's right-hand end carries `+` and the way out.** The dock was openable from four places
  and closable from all four, every one of them outside the panel — so the one place a user is
  certainly looking when they want it gone was the one place that could not do it. It is a chevron
  pointing down, which is where the panel goes, and not a ✕: the shells are not being ended, and the
  ✕ an inch to its left on every tab is. **The strip is drawn whether or not there are any shells**,
  which is what an early return on the empty set got wrong: the way out lives on that row, so closing
  the last shell took it away and left an open dock with no control inside it to close — the exact
  state the chevron exists for, reached with the ✕ an inch away from it. The one case with no strip is
  having no project root, where there is no shell to start and nowhere to start it.
  The panel asks rather than acts (`TerminalPanelEvent::Hide`,
  one variant) because the `DockArea` is the shell's, and it asks to *hide* rather than to toggle —
  the button is drawn only where the panel already shows, while `Shell::show_terminal` can open a hidden dock or spawn a shell in an empty one.
  **A tab is named `<shell> <n>`, and numbered only where there is more than one**, composed in the
  panel rather than in `PtyTab::label`: the PTY knows only its program, which is the same word for
  every tab a root has open, so three shells came out three tabs reading `zsh` — while `zsh 1` beside
  no `zsh 2` is a question about where the rest went. The project was tried in that name and is
  worse, not better: this panel draws one root's tabs and only ever that root's, so the project is
  constant on every tab **by construction**, repeated N times and first in line to be cut by the
  width cap — and which project the terminal is on is the project the whole window is on. The Neovim
  mode reads `PtyTab::label` too and is deliberately left alone — it has one grid and no strip, so a
  name built to separate siblings has nothing there to separate it from. `min_w_0` on the label is
  what lets it ellipsize at all, since a flex child's floor is otherwise its own content.
  **Closing a tab keeps the selection on the same shell**, which is `selection_after`: the index
  loses one step per tab dropped *ahead* of it, and the clamp is only for the selected tab going
  itself. Clamping alone keeps a number rather than a shell — three tabs with the middle one on
  screen, close the first, and the panel silently swaps to the third, which reads as a misclick. It
  is one pure function because both `close_tab` and `reap` had written the arithmetic out and both
  had it wrong. `close_tab` also takes a `Window`, and must: the ✕ is pressed with the caret in the
  grid about to be dropped, and the exit callback cannot cover it because a grid no longer drawn
  never runs one. The ✕ shows on hover alone, off a group named per
  tab — one name shared by the strip lights every tab's cross at once — and it is `invisible()`
  rather than absent, so the tab does not change width under the pointer. Two consequences for every
  panel: `zoomable` returns `None` (there is no tab bar for the library to draw a control in), and
  each must call `track_focus` itself (see the focus gotcha in [rules-and-gotchas.md](rules-and-gotchas.md)). What the agent pane's tab bar
  used to carry moved into `ChatPane::header`, and what the Workbench's did moved to the right-hand
  end of its mode strip.
  **The terminal and the Workbench draw the *app* direction themselves**, each in its own strip
  beside that panel's other chrome. The dock-only zoom `zoomable` used to ask the library for is
  gone with the tab bars, and nothing replaces it: the conversation already fills everything right
  of the rail whenever both docks are closed, which is the case it was for. It goes through
  `TerminalPanelEvent::ToggleMaximize` to `Shell::toggle_maximize_panel`, which is the key's path
  **with the panel named**: `Ctrl+Shift+K` maximizes whatever holds the caret, and a button sitting
  in the terminal's strip that blew up the conversation because that is where the user was typing
  would be lying about its own location. The icon is the *state* and not the action, so the panel is
  told which way round it is (`set_maximized`, pushed by `Shell::set_app_maximized` — one setter,
  because the field moves from three places and a push left off one of them is a button whose icon
  says the opposite of what it does).
- **`Root`'s overlay layers are the app's to mount.** `Root` stores dialogs, sheets and notifications
  but draws none of them — `Shell::render` calls `Root::render_{sheet,dialog,notification}_layer`.
  Forget that and `Dialog::trigger` opens into a list nobody reads, which is exactly what happened
  between P2 and P7: every dialog was dead and nothing pointed at why.
- **Transient status is a notification**, pushed with `window.push_notification`. The exception is the
  line a Workbench mode draws under its own body — a save conflict, a document that has outgrown the
  read's size bound, a Neovim that would not start — each a standing condition rather than news: a
  toast that fades leaves the user believing the save went through. It is cleared by whatever answers
  it. Drawn by the mode and not the panel, because the panel no longer knows what any of them mean —
  so each line **shows only while its own mode does**, which is a narrower audience than the single
  panel-wide line this replaced. That is the trade taken deliberately: the old one was readable from
  any mode, and any mode's message also cleared any other's, so a save conflict could be wiped by a
  document that failed to open.
- **Two things are said on the *desktop*, outside the window** (`chat::session::notify_desktop`, over
  `notify-rust`, fire-and-forget on its own thread because `show()` blocks on the bus): a turn that
  finished, and an agent that has parked a permission or a question and stopped. The pane gathers what
  it can see into `chat::Presence`, because it is the half that knows what is on screen, and
  `Attention::telling` decides — for the badge, the desktop and the chat at once — under **different
  rules per kind of news, and that is the point**. A finished turn says nothing while any part of its
  window is in front of the user, since the rail badge is already there and the work is done. A parked
  ask says something unless the user is looking at *the conversation that asked*: an agent waiting is
  an agent standing still for as long as it takes to notice, and reading one conversation is exactly
  when a dot on another row goes unseen. A lost adapter is on that same wider rule and is deliberately
  never put on the desktop at all, since the rail's row for that session marks it for as long as it is
  true. **That rests on one mark now and not two** — the conversation header carried it as well until
  the badge beside the name went, so hiding the rail is a way to be left with no report of a dead
  agent anywhere; see [known-gaps.md](known-gaps.md). The table has no wildcard arm, so a fourth kind of news cannot be added
  without deciding what each place does with it. It is sent at critical urgency so most desktops will not fade it while the agent is
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
- **There is no status bar.** There was one — a row under the rail and the dock reading out the
  project, its branch and change count, the running agent, how many buffers were unsaved and any
  panel left off 100%. Every fact on it was either already said by something nearer to what it was
  about (the project and its branch by the rail row naming them, the agent's condition by the same
  `signal_mark` on that row and in the conversation header) or was chrome reporting on chrome, and
  what it cost was a permanent strip across the bottom of every window in every project. What went
  with it: `PanelFacts` and `panel_facts`, `zoomed_panels`, `reset_zoom`, and — the one real loss —
  the **away switch**, which had no other home at the keyboard. `remote::set_away` is still reached
  by `/away` and `/here` from a chat; `Shell::toggle_away` and `remote::broadcast` went, and
  `Chats::everyone` went with them, since a list nothing outside core names is one core should not be
  holding. What survived the cut is `Shell::sync_terminal_live`, which is now one fact with one
  reader: the dot on the conversation header's terminal button, the only thing on screen that can say
  a child process outlived a closed dock. It keeps the guard it had, on the pane's side, because it
  runs on every chunk a build prints.
- **Dialogs** ([dialogs.rs](../crates/app/src/dialogs.rs)): the conversation rename, the worktree
  split, the branch rename and the issue picker. **Settings** lives in its own module,
  [settings.rs](../crates/app/src/settings.rs), and is a **roomy modal** (`settings::dialog`): as wide
  and tall as the window less a margin, capped at 960×680px, centred on both axes through
  `margin_top` rather than the library's tenth-of-the-viewport drop, which would push a box that
  tall off the bottom. It was capped at 1200×860 for one change, which left a one-control page
  looking lost; the nav is 13.5rem with 1rem of padding and the page 2rem, the sizes the Settings
  proposal in `docs/` settled on. **Beside the ✕ is a word on the last write** made from the page
  showing — *Saved*, or *Not saved — why* in the danger ink (`Shell::report_write`, which every
  appearance, agents and workspace write goes through; the Shortcuts page's is the editor's own
  `note`, and its failures stay under the field they are about). **A workspace write is only noted
  when Settings asked for it** (`Shell::workspace_note_wanted`, set by the name field, the unattended
  switches and binding a folder): runs, pins and the dock write the same file, and one of those
  landing while Settings is open is not a change the page on screen made. The flag is dropped on a
  page change (a rename still waiting on its debounce would otherwise say *Saved* on the next page)
  and on close (an unbound workspace writes nothing, so nothing would ever take it). Settings apply as they are made, so whether the
  write took is the one thing left to say; the word goes with a page change or a reopen. The
  Connections page says **when the connectors last answered** (`unattended::accounts_checked_at`,
  through `chat::pane::rel_time`) and turns *Check again* into a refusing *Checking…* while one is out
  (`accounts_checking`, a count of checks still out rather than a flag, since a scheduled look
  landing first would otherwise give the button back while the one asked for is running). It was drawn in the `DockArea`'s place for a while; that bought room and cost
  a mode — every app command had to be switched off while it was up, and it had to be left by
  navigating — so it went back to a modal with the room kept. **It is drawn by a view of its own**
  (`settings::SettingsView`, holding the shell weakly) placed in the dialog's content, never built
  inside `Shell::render`: every page reads the shell, and reading an entity from inside its own
  render is the panic *"cannot read Shell while it is already being updated"*. A child view renders
  after the parent's render has returned. The library's padding (`p_0`) and ✕ are off: the view pads
  itself and draws its own ✕ in the corner, since the library's is a plain button with the arrow
  cursor. Esc is the dialog's, plus the input's own `Escape` action caught on the view for when a
  field holds the caret — the shortcut field's Esc still cancels the edit instead. **Settings is a nav column and a page**, five pages wide: *Appearance* (the
  light/dark/system picker), *Workspace* (the name and the storage binding, then *New* / *Open
  workspace…*, then unattended runs), *Agents* (the global agent menu and the form that edits it),
  *Connections* (every connector in `plugins::connectors`, signed in and as whom, and *Check again* —
  a page of its own because more are coming and a connection is a fact about the machine rather
  than about the one feature that needed it first; today that is GitHub alone, over `gh`. It was
  called *MCP Servers* for one change and renamed for what it holds: none of these is an MCP
  server, and a real MCP manager would be a group of its own) and *Shortcuts* (the keymap).
  **Every page's head carries its scope** as a tag beside the title (`settings::page_head`) —
  *App*, or *Workspace: name* on the one page about this window's workspace — so a setting is never
  changed in the belief that it reaches the other way. **The default agent is the first in the
  list** (`agents.first()` is what *New session* and the warm-up start); *Make default* on any other
  row moves it there (`Shell::make_default_agent`, with `settings::draft_after_promote` keeping an
  open form on its agent), so being default is an order and not a second setting to drift from it.
  **An agent can be tested** (*Test*, `Shell::check_agent`): its command is looked for the way a
  session would — a path as a path, a relative one from the project root the session would start it
  in, a bare name along `PATH` (`onehand_core::config::find_command`)
  — off the UI loop, and the answer sits under the command, filed under the whole command line
  (`settings::check_key`) so two agents on one launcher do not share an answer and an edit drops it.
  It names the program it found, since the program is all it looked for. It is asked for and never run on opening, and it only finds a file: whether the program
  speaks ACP can be known only by starting it, which a check ahead of a session must not do.
  **Closing Settings with an edit pending asks first** (`Shell::request_close_settings`, behind the
  ✕, Esc, the backdrop and a field's own Escape): an agent form holding changes, or a shortcut whose
  field no longer matches its keys (`keymap::Editor::dirty`), gets *Keep editing* / *Discard* — a
  shortcut opened and left as it was is nothing to lose; with nothing pending it closes at once, since a
  question on every close is one people learn to click through. The drafts already survive moving
  between pages — the agent form lives on the shell, the shortcut edit on its editor. Agents and the keymap were dialogs of their own behind two
  more rail rows, so "where is that setting" had three answers and which was right depended on which
  row somebody remembered; the rail footer is one row now. What is in here belongs to three scopes —
  the theme is app-wide, the name and binding are one workspace's, the agent list is every
  workspace's — and stacked in a single column the only thing that said so was a row of `text_xs`
  labels. A page per scope says it without a sentence, and the appearance page says its own out loud
  because the theme is a global: two windows cannot be drawn in two modes.
  Three things that shape carries. **The page scrolls and the frame does not**, so the nav stays
  reachable from the bottom of the keymap; the page's content is held to a centred reading measure
  so a wide window does not stretch a field across it. **One surface, and the only lines on it are
  between groups** (and the dialog's own edge): no border between the nav and the page, no header
  bar (the ✕ sits in the top-right corner), no box around a group. A group (`settings::section`) is a heading and a
  hairline above it; the page's first group is untitled and takes no rule, since a line between a
  page's head and the first thing it heads divides what belongs together. A setting is **stacked**
  (`settings::field`): its name, a line about it, and the control under both at the column's full
  width, so an input is as wide as what goes in it. Things there are many of — agents, commands —
  are `settings::list_row` instead, name left and actions right, where stacking would make every
  entry three lines tall. The library's outlined `GroupBox` held these groups for one change and
  was taken back out: a box per group on a page that is already one panel is a border saying what
  the spacing says. The Shortcuts page groups commands by where they work (*Window*, *Composer*, the
  fixed *Terminal* keys) and draws keys as the library's `Kbd` caps. The nav is headed *Settings*
  and ends on the build (`onehand x.y.z`), which is about the app rather than any one page. A nav
  row is a `div`, not the app's button wrapper, for the reason the rail's rows are: a full-width
  library `Button` centres its content and cannot be refined out of it, so a column of them reads
  as a stack of banners. Each leads with its page's mark — palette, folder, bot, and a keyboard,
  which is the one `crate::icons` entry this page needed — so the column reads as the same kind of
  list the rail beside it is.
  **There is no footer**, and that is the rule it keeps: every control sits under what it acts on.
  *Choose folder…* and *Unbind* were in one, three items below the folder they name and under a list
  of arbitrary length — and a `.primary()` button at the foot of a settings page reads as *Save*,
  while that one opens a picker and re-points where the workspace is written.
- **Multi-window**: one window hosts exactly one workspace. Opening a workspace whose storage dir is
  already on screen focuses that window instead of duplicating it; storage dirs are canonicalized on
  the way in so symlink and `..` aliases dedupe.

### Keyboard, zoom, maximize

Most app commands default to `Ctrl+Shift`; ordinary terminal control keys remain available:
`B` rail · `E` Workbench Editor, tree included · `M` Workbench Markdown ·
`N` Workbench Neovim · `A` composer · `O` new session · `R` guarded restart ·
`W` guarded close · `K` maximize · `J` Workbench visibility. Plus `Ctrl+,` Settings, `` Ctrl+` `` terminal, `Ctrl+S` save, `Ctrl+1…9` session by position, `Ctrl+Tab` session by recency,
`Ctrl+=`/`Ctrl+-`/`Ctrl+0` zoom, and inside the composer `Up`/`Down` (its completion list) and
`Ctrl+V` (an image or a file on the clipboard becomes an attachment; text is handed back to the input)
and `Shift+Tab` (the next session mode, wrapping — `Chat::cycle_mode`).

**GPUI resolves these itself.** Key bindings are matched against the focus context stack *before* the
key is delivered to whatever is focused, so an app binding reaches the app even while a PTY holds
focus, and the terminal never sees that keystroke. Window commands require `Shell && !Dialog`,
so editing Settings cannot trigger a command in the panel underneath. Important scoped bindings: `Ctrl+S`, bound `Shell && !Terminal` because the PTY has a real claim on it; the
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

Workbench visibility and mode selection are separate commands. `Ctrl+Shift+J`
toggles the whole dock regardless of focus and reopens its previous mode;
`Ctrl+Shift+E / M / N` open and focus a mode without hiding it on a repeated
press. The terminal key toggles visibility directly. Visibility buttons share
the same close paths, which unwind maximize and recover focus before hiding.

**Zoom is per panel** ([zoom.rs](../crates/app/src/zoom.rs)) and overrides the *rem base* for that
panel's subtree, so everything sized in rems scales together — which is why sizes must be rems and
not pixels. The terminal is the exception: it is a measured glyph grid, so its zoom is a font size
that re-measures the cell and resizes the PTY.

**Maximize has one direction**: `Ctrl+Shift+K` fills the frame and hides the rail. The two dock
panels each carry a button for it in their own strip, and that button **names its panel** rather
than using the key's focused-panel rule — a control sitting in the terminal that blew up the
conversation because that is where the user was typing would be lying about its own location. The
library's dock-only zoom went with the tab bars that were the only place it could be drawn.

Settings' Shortcuts page edits the same command registry that installs the app
bindings (`crates/app/src/keymap.rs`). Overrides are persisted under `[keymap]`
in the resolved config, then replace only app-owned bindings in every window.
An empty key list unassigns a command; Reset removes its override. Validation
rejects collisions, unknown keys and component-control conflicts. Contexts and
terminal Tab suppression are fixed. Dispatch tests cover remap, unbind, reset,
modal isolation and terminal passthrough. `Ctrl+,` opens Settings even with the
rail hidden; the command palette remains unimplemented.

### Persistence

- **Transcripts** ([crates/core/src/chat/store.rs](../crates/core/src/chat/store.rs)). Every conversation
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
  immediately after they chose not to resume one. It is reached from a live session's title menu
  (*Resume in this session…*), and the choice still happens *before* anything reconnects:
  connecting first would start a fresh conversation and archive it. It is the **narrow** half of the
  pair: this one swaps what the session on screen is on and lists that session's own agent only,
  while the header's past-conversations menu opens an archive as a session of its own and lists every
  agent's.
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
- **A workspace made by *New workspace…* is bound before its window opens**, and it is bound to
  `workspace::storage_for(root)` — `<config_dir>/onehand/workspaces/<folder>-<digest of the path>` —
  never to the project folder itself. Two things this settles. An unbound workspace persists nothing,
  so one that opened unbound was remembered nowhere: it was gone on the next launch and the folder it
  was created in did not open it either, since nothing had been written there. And a config written
  *into* the project would leave a clean repository dirty with a file that shows in that project's own
  tree and change count, and publish every root's absolute local path to anyone who cloned it. The
  digest is FNV-1a written out in core rather than `DefaultHasher`, which is not promised to be stable
  across Rust releases — a derived path that moved under a toolchain upgrade would point every project
  at a fresh empty workspace. Being stable is what lets the overwrite guard mean something here:
  creating a workspace on a folder that already has one **opens it** rather than starting a second one
  nothing distinguishes from the first. *Open workspace…* tries the picked folder and then that
  derived storage, because the folder a user can find in a picker is the project, not the data root.
- **The overwrite guard is one function** (`shell::workspace_in`): a folder already holding a
  workspace, a folder free to write into, and a config that exists and cannot be read — that last
  never reading as the second, or a workspace with one bad character in its file is a workspace
  deleted by a folder picker. The sentence refusing it lives there too, so the two write paths cannot
  come to refuse the same thing in different words.

### Config

[crates/core/src/config.rs](../crates/core/src/config.rs) loads `onehand.toml` (or
`<config_dir>/onehand/config.toml`, else built-in defaults) into `AppConfig`. `#[serde(default)]`
means a partial file overrides only the keys it sets. The default agent is Claude Code over ACP.
`load_resolved` returns the config *and the path to write back to*, so in-app agent edits land in the
file the next launch reads.

`appearance` is the one key the settings dialog writes: `system` (the default) · `light` · `dark`.
There are two palettes and the app only chooses which one is loaded — `shell::apply_appearance`
is the single place that does it, at boot and on every change. Each is the library's own config with
the app's surface ramp written over it (`crate::theme::install`, run once before the first mode is
chosen); see the theme module for what is ours and what is inherited. **A token the library keeps
as a slot of its own has to be written out, or it silently keeps the shipped palette's value** —
the rule the sidebar tokens were already there for, and `secondary_hover` / `secondary_active` were
the second family to be caught by it: the shipped dark hover is *darker* than this ramp's bubble, so
the rail's one filled control receded toward the well when it was pointed at, and its pressed step
landed 1.04 from the well, which is a hole rather than a button. Both now take the ramp's one step
above the bubble fill, and their being equal is load-bearing rather than lazy — gpui refines a hover
style over the base and `hover_style` is crate-private, so a trigger that fills itself while its menu
is open cannot outrank the hover underneath it, and two different shades would hide the pressed one
for as long as the pointer stayed on the control that opened it. `system` **keeps following** the desktop
(each window observes its own appearance), which is also what settles the Linux startup race where the
platform answers with its default until the desktop portal replies. An unrecognized value reads as
`system` rather than failing the file, because the agent list is in that same file. Three things the
switch has to repair: the resolved monospace family, since loading a mode re-applies a whole theme
config over it; **the resize handle's resting colour**, cleared to transparent so a dock draws no
divider of its own (a panel on the far side of a seam already marks its own edge — each dock card's
four borders, and at the rail a change of surface — so the library's rule was a second line beside a
first; dragging still paints, which is the one moment the seam is what is being looked at).
That write is on `gpui_base::Theme`, the layer *under* gpui-component, because that layer paints
the handle and the component library re-exports no route to its theme global — hence the `gpui-base`
dependency, same git source and rev so it is the crate already in the graph. It must come **after**
`Theme::change`, which rebuilds the Base copy from scratch and would throw an earlier write away. And
every *other* window, since the mode is global while a refresh is per window. The
embedded terminal has its own ANSI palette and does not follow the mode. Declaration order matters —
`appearance` is a bare TOML key, so it must be declared before the sections or saving the config fails
outright.

`[remote.telegram]` is off unless asked for — a bridge that came up by default would put a process on
the network on the strength of a file nobody edited. It carries `enabled`, `allowed_chats` and an
optional `token_env`, and **it deliberately has no key for the token**; see the remote bridge above
for where that is read from and why it is not here. Declaration order does not bite for this one,
since it is a table like `[font]` and only the bare `appearance` key has to lead.

`[font]` carries exactly one key, `monospace`, which `shell::use_installed_mono` takes as the first
preference when it picks a mono family the machine actually has (see the font gotcha in
[rules-and-gotchas.md](rules-and-gotchas.md)). It used to
carry a body size, a master zoom, a sans family and a fallback list, and there was an `[icons]` table
of per-role hex overrides beside it; decision D1 makes gpui-component's theme the look, so none of
them ever reached the screen. **They parsed, which is what made them worse than absent** — a file
setting `font.size = 18` loaded without complaint and changed nothing, and there was no way to tell
that from the app ignoring a value it disagreed with. They are gone, and a config that still sets
them keeps loading — **because serde ignores unknown fields by default and nothing here opts into
`deny_unknown_fields`**, which is the same reason a legacy agent's `kind` still parses. That is a
different tolerance from `#[serde(default)]`, which covers the keys a file *omits*; reaching for
`deny_unknown_fields` would leave `default` in place and still break every config written for an
older build. If per-role tinting is ever wanted, the hook is `AppConfig` itself: adding one field
back is smaller than carrying a table that says the feature is wired up.

