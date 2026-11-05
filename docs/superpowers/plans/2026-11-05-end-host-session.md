# End a host session from the window

Known gap: a host session is never ended from the window; closing it, removing its project or
quitting only stops watching it, so idle agents pile up on the host. The owner chose
(2026-11-05) an *End on <host>…* entry in a session's menus (the rail row's ⋯ and the
conversation header's) and on each running row of the project page's *Sessions on <host>* box.
*Close session* keeps meaning "stop watching".

## Steps

1. Wire: `Call::End { session }`, answered `Done`. Ending one already ended is `Done` too.
2. Host: each session keeps its drive task's `AbortHandle`. `Hub::end_session` refuses another
   laptop's mirror as a request does, aborts the task (dropping the agent's stream kills the
   adapter's process group), journals `Disconnected` *the session was ended from the laptop*
   and marks it ended. Tests in the hub.
3. Core client: `HostConn::end_session`.
4. App: one confirmation, *End <label> on <host>?*, *Its agent stops there, and a running turn
   is lost. The conversation stays readable on the project page.*, act *End*. A yes attaches,
   ends, closes the session in this window if one watches it, and lists the page again. A
   failure is an error toast, *Ending <label> on <host> failed: <why>*.
   - Rail session menu and header menu: *End on <host>…* on every host session; one the host has
     not named yet says it has not started there.
   - Project page rows: a ⋯ in a hover slot and the same acts on right-click (`row_more`,
     `row_menu_items`): *Attach*, then *End on <host>…* in the danger ink while the row is not
     ended.
5. Docs: hosts.md (ending a session), screens.md (the menus and the page row), known-gaps.md
   drops the entry.

One PR, `feat/host-session-end`.
