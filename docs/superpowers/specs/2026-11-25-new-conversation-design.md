# New conversation in the same session

## Intent

A session on the rail has finished its work, and the person wants a fresh conversation with the
same agent on the same project without adding another session to the rail. Claude Code's own
`/clear` does this in its terminal, but its ACP adapter does not advertise `clear`, so onehand's
`/` list never offers it.

## What exists

The session's title menu ▸ *Resume in this session…* opens the resume picker, whose *Start a new
conversation* drops the adapter and connects the same session on a fresh conversation
(`ChatPane::start(uid, None)`). The old conversation stays on disk and can be resumed. The way is
three steps deep, and the picker does not open at all in a project with no past conversation.

## Design

- **The title menu gains *New conversation***, beside *Resume in this session…*. It takes the same
  path as the picker's *Start a new conversation*, with the guards *Resume in this session…* has:
  disabled mid-turn, refused while the session leads workers (`may_leave_conversation`), and the
  old conversation's name and settings written before it is left.
- **`/clear` typed alone in the composer does the same** instead of being sent, and the `/` list
  offers it as a row of its own. When the session's agent advertises a `clear` of its own, the
  agent's wins and the text is sent as before. Which text is onehand's own command is a rule, so it
  lives in core; the app only carries it out.
- The remote chat is unchanged: `/clear` from Telegram is still a prompt.
- `docs/screens.md` records the menu item and the command. The glossary does not change: a session
  still runs one conversation at a time.

## Approaches

- **Chosen: reuse the picker's fresh start.** One existing path, already guarded and tested.
- *`session/new` on the live connection*: a second way to change conversation, different from
  resume and restart, for no gain the person sees.
- *Only the `/clear` command*: hard to discover.
- *Keep the old transcript above a divider* (the owner's option B): one session would hold several
  conversations, which changes the glossary, the store and forking. Rejected by the owner.

## Builder's calls

- Whether *New conversation* also gets a keymap command.
- The `/clear` row's description wording.
