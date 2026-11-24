# The Telegram bridge in Settings

## Intent

The Telegram bridge works, but nothing about it is in the window. It is set up by editing
`[remote.telegram]` in `onehand.toml` and putting a token in an environment variable or in
`telegram.token`, it starts once at launch, and everything it has to say about itself
(enabled with no chat allowed, no token, token refused, another process polling the same bot,
connected as @bot) goes to stderr, which nobody launching onehand from a desktop sees. A broken
bridge is a silent one.

The owner wants to set the bridge up, and see whether it is working, from the app.

## Goals

- Turn the bridge on and off from Settings, in force at once, no restart.
- Enter, replace and remove the bot token from Settings.
- Edit the allowed chats from Settings, in force at once.
- See what the bridge is doing now: off, missing its token, connecting, connected as which bot,
  or stopped and why.
- See the chats that messaged the bot and were ignored, and allow one with a press, so nobody
  has to dig up a chat id.

## Non-goals

- `token_env` stays a `onehand.toml` key; Settings shows its effect, never edits it.
- No second channel, no transcript streaming (both stay in `docs/known-gaps.md`).
- No change to what the bridge does once a message arrives: commands, announcements, the
  allow rule (`Chats::allows`) stay as they are.

## Anti-goals

- The token in `onehand.toml`, in a log, in a toast, or shown back on screen after it is saved.
- A chat off the list getting any answer from the bot. The ignored-chats list is the window's
  alone; the bot stays silent to them, as now.
- The page claiming a state the bridge is not in (saying *Connected* from the config rather
  than from the channel's own report).

## Constraints

- Rules and what happens next live in core (`RemoteDrive`, `SettingsDrive`); the app carries
  them out. No drive changes another drive's model: the app's carrier hands a settings change
  to the remote drive.
- Core stays GUI-free and runtime-free; the token write is a blocking core function run on the
  background executor.
- The token rule changes from *read, never written* to *written only to its own file, owner-only
  (0600), never to `onehand.toml`*. `crates/telegram/src/secret.rs`'s header and AGENTS.md's
  *Telegram token* line say the new rule.
- An environment variable still wins over the file (`config::secret`). While one holds a token,
  Settings cannot override it and says so.
- Every list capped and saying what it left out.

## Approaches

1. **Live (picked).** Enabled and the allowed chats go through `SettingsDrive` as *Outside
   control* does: shown at once, written after, put back when the write fails. The bridge's
   state moves into `RemoteDrive`; the app's carrier stops and starts the channel when the
   switch or the token changes. Picked because the hard parts (state in core, a safe token
   write) are needed either way, and stopping is already cheap: dropping the event stream ends
   the poll.
2. **Save and restart.** Settings writes the config and token and says *Restart onehand to
   apply*; the state shown is the last launch's. Less code, but correcting a mistyped chat id
   costs a restart, and the state shown can be stale, which is the problem being solved.

## What gets built

### State in core

`RemoteDrive` gains the bridge's state, read by the page:

- `Off`: the switch is off.
- `NoToken`: on, and no source holds a token.
- `Connecting { retrying }`: the channel is started and has not reported in. While the
  handshake or the poll is retrying, `retrying` holds why (*Cannot reach Telegram: …*), carried
  by a new non-fatal `RemoteEvent::Retrying(why)` the Telegram crate sends where it now only
  prints to stderr, and cleared by the next success. Without it a blocked `api.telegram.org`
  reads as *Connecting* forever, which is the silent failure being fixed. (Builder-check call.)
- `Connected { name }`: the channel's `RemoteEvent::Connected`, which today returns early in
  the app with an `eprintln!`; it becomes an `Input` the drive takes. A poll that starts failing
  after the handshake moves it back to `Connecting { retrying }`.
- `Stopped { why }`: `RemoteEvent::Disconnected(why)`, the refusals retrying cannot fix. The
  switch stays on, and the state line carries *Try again*, which restarts the channel: a 409
  (another process polling the same bot) clears once that process stops, with no setting
  changed, and a token from the environment has no token button to press. (Builder-check call.)

A channel restart carries a generation number, so a late event from the channel being replaced
never lands on its successor. (Builder-check call.)

The `eprintln!`s in `crate::remote::boot` become these states (stderr may keep one line each).

**An empty allowed list no longer keeps the channel from starting.** It runs and answers
nobody, which is what the allow rule already does with an empty list; the page says *No chat
is allowed yet; the bot answers nobody*. This is what makes the ignored-chats list useful on
first setup: turn on, paste the token, message the bot, press *Allow*.

### Ignored chats

The drive remembers, in memory only, the chats whose message or press it refused: chat id and
sender id, and the sender's name and @username as Telegram gives them, newest first, at most
`IGNORED_CAP` (5), deduplicated by the pair. Nothing about the message's text is kept.

**The name is what makes *Allow* safe.** Anyone who finds the bot can message it, and two rows
of bare ids cannot be told apart; pressing *Allow* on the wrong one hands a stranger the
machine. So `RemoteEvent::Message`, `Unreadable` and `Pressed` (and core's `Input`) gain an
optional sender label, filled from the update's `from.first_name`/`last_name`/`username` (and
the chat's title for a group), and the row reads *Minh Tran (@minhtran) · 987654321*. The
label is used for this list only, never for the allow rule. (Builder-check call.) The list empties when the channel stops. *Allow* on one adds the
chat id, and, for a group (sender differs from chat), the sender's id too, since
`Chats::allows` needs both; it then leaves the list.

### Settings

`SettingsDrive` gets the switch and the allowed chats as settings, after *Outside control*'s
shape: `Setting::Telegram` / `Value::TelegramOn(bool)`, `Value::TelegramChats(Vec<String>)`,
`Edit` writing `cfg.remote.telegram.enabled` / `.allowed_chats` in place.

Carried out in the app:

- The switch: on starts the channel (or lands on `NoToken`), off stops it (`Off`).
- The allowed chats: handed to the running drive's `Chats` at once; **no channel restart**,
  so `/use` bindings and follows survive an edit. A chat taken off the list loses its binding
  and follows.
- A token saved or removed: written off the UI thread, then the channel is restarted with a
  fresh drive (a different token may be a different bot, so bindings do not carry over).

### The token

A blocking core function writes a secret to a file: created owner-only (0600) before any byte
is written, written to a temporary file beside it and renamed, trimmed. Another removes it. The
app runs both on the background executor and restarts the channel when they land; a failure is
said in the group, the field keeps what was typed.

The token is never read back into the UI. The page says where it comes from:

- from the environment: *Read from $ONEHAND_TELEGRAM_TOKEN* (or the `token_env` name); no field,
  no buttons;
- from the file: *Saved in <path>* in mono, with *Replace…* (shows the field) and *Remove…*
  (asks *Remove the bot token?*);
- from nowhere: the field, masked, with *Save*.

### The page

A *Telegram* group on Settings ▸ Connections, after *Outside control* (both are something
reaching onehand from outside). Sketch:

```
Telegram
A bot that tells you when an agent needs you, and takes your answer back

[ switch ] Let a Telegram bot reach onehand
Connected as @onehand_bot                      (muted; warning ink for NoToken / Stopped)

Token     Saved in ~/.config/onehand/telegram.token        [Replace…] [Remove…]

Allowed chats
  123456789                                                 [Remove…]
  -100222333 (group)                                        [Remove…]
  [ chat id ............ ] [Add]

Ignored: messaged the bot and got no answer
  Minh Tran (@minhtran) · 987654321                        [Allow]
```

A stopped bridge's line: *Stopped: another program is using this bot* (warning ink) and
*Try again*. A retrying one: *Connecting… Cannot reach Telegram: …* (warning ink).

- Turning the switch off clears *away*, as a stopped channel already does.
- The token file's folder is created on write; `make dev` and `make showcase` keep their own
  token file, since `config_dir` follows each build's data root.
- Off: the switch, and the token and chats rows stay editable (setting up before turning on is
  normal); no state line beyond *Off*.
- Allowed chats: at most 50 rows, then *N more not shown*; *No chat is allowed yet; the bot
  answers nobody* when empty. *Add* refuses an empty or non-numeric id (*A chat id is a number*)
  and one already listed (*<id> is already allowed*). Remove asks nothing: it is undone by
  adding it back, and the list is the person's own.
- Ignored: only while the channel runs and the list is not empty.
- Every button secondary, `short`; the field `tall`; inks by role (state at rest muted, a
  failure in the warning ink), as the connectors' rows above it do.

### Documents

- `docs/screens.md`, Settings: the Connections page gains the Telegram group.
- `crates/telegram/src/secret.rs` header, AGENTS.md *Telegram token*: the new token rule.
- `docs/known-gaps.md`: nothing removed (no entry covered this).

## Left to the builder

- Whether this is one PR or two (core, then the page). Builder's call: two if the core half
  passes ~400 lines.
- Exact type and function names; where the secret writer lives (beside `config::secret`).
- Whether *(group)* is shown for a negative id. Builder's call: yes, Telegram's group ids are
  negative, and it explains why a group needs its members listed too.
