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
exactly our patches). `make fmt` / `make lint` scope to first-party crates and exclude `vendor/`.

Tests are inline `#[cfg(test)]` modules — there is no `tests/` directory.

## Architecture

### Repo layout

| Path | Crate | What |
|---|---|---|
| `crates/app` | `onehand` | the GPUI front end + the binary |
| `crates/core` | `onehand-core` | GUI-free logic: config, the workspace tree, ACP, the chat model, the remote bridge, editor rules, completion, git status, worktree rules, the directory flatten |
| `crates/plugin-api` | `onehand-plugin-api` | GUI-free plugin IDs, descriptors, capabilities and registration contract |
| `crates/plugin-host` | `onehand-plugin-host` | startup registry and typed contribution factories/lifecycle hooks |
| `crates/terminal-ui` | `onehand-terminal-ui` | shared PTY/grid ownership used by the terminal dock and Neovim |
| `plugins/builtin/*` | built-in plugins | Editor, Files, Neovim and Telegram contributions compiled into the binary |
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

### Built-in plugins

`crates/app/src/plugins.rs` is the composition root. It registers every plugin,
attaches its typed factory, and seals the registry before `Shared` exists and
before a window is opened. Workbench order is declared there as Editor, Files,
Neovim. Registration is deliberately not dynamic in API v1: duplicate plugin
or contribution IDs, an unsupported API version, a capability mismatch, or a
missing factory abort startup with the plugin/contribution ID in the error.

The Rust traits are versioned `0.x` and are an internal composition seam, not a
stable third-party ABI. A future external-plugin system is expected to use a
process protocol rather than Rust dynamic libraries.

### The remote bridge

A second channel into the app, for the times nobody is at the machine. Same shape as the ACP bridge
above, deliberately: [crates/core/src/remote/](crates/core/src/remote/) owns the GUI-free neutral
model and channel contract, while the Telegram wire implementation and secret loading live in
`plugins/builtin/remote-telegram`. [crates/app/src/remote.rs](crates/app/src/remote.rs) drives it on a
tokio runtime of its own with events crossing on a `futures` channel.

**The layer is general; Telegram is the first adapter.** `remote::types` is the neutral model — chat
ids are strings, a message carries text and rows of `Button`s, and `RemoteChannel::connect` folds its
serve loop into the stream it returns. The built-in Telegram plugin is the only implementation, a long poll
plus `sendMessage` and `answerCallbackQuery`. Everything that is not the wire is pure and tested:
`access` (who may reach the app), `command` (the little language a chat drives it with), `press`
(what a button means). Telegram's plugin owns `secret` (where its credential comes from).

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
  **The empty list allows nobody**, so forgetting to fill it in fails closed. It is **permission and
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
  It is read in exactly one place, `ChatPane::here`, which `watching` is then built on, so the switch
  cannot reach one rule and miss another. Global rather than per window, because walking away from one
  window is walking away from all of them, and **not persisted** — a launch that came up believing the
  user was elsewhere would message somebody sitting in front of it about every turn. Thrown from the
  status bar (`Shell::toggle_away`) or from the chat (`/away`, `/here`), both through
  `remote::set_away`, since a mode with two setters is a mode that means two things. Two things that
  setter owes, both because the switch has no other way home. **A dead channel clears it**: the status
  bar draws the switch only while one is live and `/here` would arrive over the channel that just
  died, so leaving it set is a mode with no exit short of a restart. And **coming back clears the badge
  on what is on screen**, on the active window only — every turn is unwatched while away, including
  one that ended in the conversation being read, and `unseen` is otherwise cleared when a window
  *becomes* active, which never happens to one that was focused the whole time. That clearing is
  **deferred**, and has to be: one of the two ways in is a click on the status bar, whose handler is
  already holding the very shell it reaches into, and updating an entity that is already being updated
  is a panic rather than an error — it would take the app down on the second press of a control whose
  whole job is to be pressed twice. The switch is
  drawn only where a channel is live, is an eye and its absence because that is literally the question
  it answers, and is silent when off and named in the standing-condition colour when on.
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
  `remote::forget` drops both wherever a session is found closed — an entry naming a session neither
  `/sessions` nor `/status` can print is one nobody could afterwards remove.
  `/follow [n]` and `/unfollow [n]` are the explicit pair, bare meaning the pointed-at session. Both
  move **all three moments together**, the parked ask included, which is the honest reading of being
  told to say nothing: half a subscription is a mode whose rule nobody can state. A word where a
  number was meant (`Aim::Unreadable`) is refused rather than read as the bound one — on `/unfollow`
  that would silence a conversation nobody named, and what it costs is every message that session
  would have sent, until somebody thinks to wonder why it went quiet. `/sessions` marks both facts in
  one margin column: `→` where typing goes, `•` for followed-but-not-pointed-at, which is the row
  that is otherwise indistinguishable from a silent one.
  **The whole thing is decided in `announce` and never by the pane**, because it is a fact about the
  channel alone — the pane's rules are about what is on screen and hold for the desktop notification
  too, so pushing a subscription up there would quiet the desktop over instructions that never
  mentioned it.
- **`/status` is the answer to "why is it quiet", and silence being the default is what makes it
  necessary.** Every fact that decides whether anything arrives is invisible from the far side by
  construction: following nothing shows itself as messages that do not come, and so does being at the
  keyboard, and so does a bot whose process died an hour ago. So it prints the two facts no session
  carries — away on or off, where this chat is pointed — and then **names** what it follows rather
  than counting it, since the point is to check the list against what you believe you asked for.
  **Not a second `/sessions`**, which answers what onehand is running and marks these rows in passing.
  The word is `status` and not `watching` although that is the question being asked:
  `ChatPane::watching` already names the user's eyes being on a conversation, which is what decides
  whether a turn is announced at all, and one word answering two questions inside one feature is how
  the two answers end up swapped. It is the one reading command that writes: a binding or a
  subscription onto a session that has since closed is dropped as it is reported, since "pointed at 7"
  about a session that is gone is the confusion the command exists to end.
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

[crates/app/src/workbench/](crates/app/src/workbench/) — one dock panel, three modes:

- **Editor**: a quick editor, not an IDE. Buffers here, rules in core (`onehand_core::editor`): the
  size bound, the tab set, the **mtime guard**, labels, blocking read/save. Highlighting is
  gpui-component's tree-sitter over a deliberately small grammar set (decision D3) — no LSP.
  Reopening an already-open file **never reloads it**: a second click on a path must not discard what
  the user just typed.
- **Files**: the active root's tree, `tree::visible_rows` from core, bounded per directory and in
  total, `.git` skipped. Rows carry git state as one-letter badges; a directory holding changes gets a
  dot. Indentation is padding by depth, not nested containers — hundreds of nested rows are hundreds
  of wasted elements.
- **A tab whose child exits is dropped**, in both this panel and the terminal's (`Workbench::reap_neovim`,
  `TerminalPanel::reap`, fed by `spawn_pty`'s exit callback). Nothing notices otherwise: the grid keeps
  drawing the last screen the child painted, which after `:q` or `exit` is an empty one with a cursor
  on it, and the tab takes keystrokes nothing will ever read. Three things this owes.
  It is **deferred** through `Window::defer` — the callback fires from inside the grid's own render,
  and the panel that owns the tab is the thing currently rendering that grid, so reaching into it
  there is a panic rather than an error, at the exact moment somebody typed `:q`.
  It is a **sweep** over every tab rather than a removal of the one that spoke, because `PtyTab::finished`
  asks the process (`try_wait`) — so a child killed from somewhere else, or gone while its root was off
  screen, is collected too, and reaped rather than left a zombie.
  And it moves **focus only if focus was already inside that panel**: a grid dropped while holding the
  caret leaves the window pointing at an element no frame contains, which takes the whole keymap with
  it — while a background shell exiting must not steal the caret from what the user is doing.
- **Neovim** (`Ctrl+Shift+N`): the real thing, in a PTY, on the project root. Here and not in the
  terminal dock because this is the panel about files. One per root and never a second — several
  shells is what somebody opens on purpose, while two editors on the same files are two views of one
  buffer with no way to tell which holds the unsaved copy. It is spawned through
  `onehand_terminal_ui::spawn_pty`, so it inherits the shared rules about `TERM`, resize, clipboard and
  reaping.

State is per project root, so switching roots swaps the whole thing.

Three things the Neovim mode owes that the other two do not, all because it is a live PTY rather than
an element tree:

- **Its zoom is a font size, not the rem scale** wrapped around the other bodies. The grid is
  *measured* from a shaped glyph, so scaling the box around it stretches the container while the cell
  stays put and every column lands past its own character. `Workbench::set_zoom` pushes the size into
  the view instead, which is why the shell hands it the whole value rather than `&mut` to the field.
- **The panel takes the key context `Terminal` while this mode shows**, and it must be that name and
  not one of its own: `Ctrl+S` is bound `Shell && !Terminal` exactly so a program in a PTY keeps it,
  and a grid mounted with no such context would have the quick editor's save fire over the top of
  `:w`.
- **Switching to the mode does not spawn.** `Ctrl+Shift+N` spawns and then switches, and the empty
  state carries a *Start Neovim* button; a mode strip where one of three buttons launches a process
  is one nobody can click to look around. The key is three-state like the other two Workbench keys —
  closing the dock puts the editor aside rather than ending it, since the panel entity outlives the
  dock and the PTY, the scrollback and the unsaved buffer are all still there on the next press.
  `nvim` is looked up on `PATH` in the app rather than handed to the PTY to fail on, because a failed
  spawn comes back as "No such file or directory" naming nothing; it is `nvim` and not `$EDITOR`,
  since honouring that would open `vi` for somebody who set it years ago for `git commit`.

### Terminal panel

[crates/app/src/terminal.rs](crates/app/src/terminal.rs) over `vendor/gpui-terminal`. A tab per root,
spawned lazily; dropping a tab drops its PTY, so the child dies with it and there is no separate
shutdown to forget.

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
`B` rail · `E` Workbench Files · `O` Workbench Editor · `N` Workbench Neovim · `A` composer ·
`F` find · `R` guarded restart ·
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
- **The terminal has no `APP_KEYPAD`.** The numeric keypad's application mode is unimplemented,
  because gpui does not report a keypad key differently from the digit above it. The keys work; they
  always send the ordinary form. Nothing else on decision D4's parity list is outstanding.
- **The terminal's cursor does not blink**, by decision — it would mean a repaint on a timer for the
  life of every tab, in a view that otherwise draws only when bytes arrive.
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
  past its glyph. `onehand_terminal_ui::spawn_pty` hands the grid the resolved family for exactly that reason;
  the vendored default is the string `monospace`, which is a CSS generic and not a family anything
  enumerates.
- **`use super::*` in a test module inside `vendor/gpui-terminal` breaks `#[test]`.** That file imports
  gpui with a glob, and gpui exports an attribute macro of its own called `test`. Globbing it into a
  test module shadows the built-in attribute, and `gpui::test` expands to code carrying `#[test]` —
  which resolves to `gpui::test` again, until rustc gives up with *"recursion limit reached while
  expanding `#[test]`"*. Nothing in the message points at the glob. Import the two or three items the
  tests actually need by name. This is what upstream's note about "macro expansion issues with the
  test attribute" was, and it is why `view.rs` had no tests at all.
- **The grid's paint runs once per visible character, so anything it allocates is multiplied by the
  screen.** A modal editor redraws the whole grid on every keystroke, which is what turns a cost a
  shell hides into typing latency. Three things were being built per glyph and are not any more: the
  text (`ch.to_string()` then a `SharedString`), the `Font` (whose `family` is a `SharedString` built
  from a `String`, and whose `FontFeatures::default()` is an `Arc<Vec<_>>`), and — per *row* — a
  `Vec`, a `HashSet` and a discarded batching pass. `render::ascii_glyph` and
  `TerminalRenderer::font_variants` are what keep the common case at zero allocations. **Measure
  before assuming the shaping is the cost**; here the allocations around it were.
- **The measured cell has to reach the view, not only the paint.** `TerminalRenderer::measure_cell`
  needs the window, and the window exists only inside the canvas paint — so it runs on a *clone* of
  the renderer, and writing the result back to the view's own copy is a separate step. Skip it and
  every pixel-to-cell conversion the view does divides by the constructor's guesses instead
  (0.6 and 1.4 times the font size). **Nothing about the drawing looks wrong**, because the drawing
  uses the measured clone; what is wrong is everything aimed *at* the drawing — the cell a click lands
  on drifts further from the pointer the lower down the grid it is, and the height guess is out by
  more than the width one, so the drift is mostly vertical. It reads as a context menu appearing in
  the wrong place, or a drag selecting the wrong line, rather than as a measurement that never
  arrived.
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
  literal-next), bracketed paste, copy-on-select, typing-snaps-to-bottom, mouse reporting and its
  `Shift` bypass, terminal replies going back to the PTY, `DECSCUSR` cursor shapes and the modified
  key sequences are all onehand's, marked
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
