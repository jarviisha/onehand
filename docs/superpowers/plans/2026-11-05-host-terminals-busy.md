# Host terminals count as busy before a restart

Known gap: restarting the host service onto another build ends every host terminal without
asking, because the build check counts only sessions mid-turn. The owner chose (2026-11-05) to
count every open host terminal, an idle shell included, as busy: the existing wait and its
*Restart now* then ask first.

## Steps

1. Wire: `Reply::Welcome` gains `terms: usize` (`#[serde(default)]`, so an older service reads
   as none). The host service fills it from `Terms::list().len()`.
2. Core: `hello` returns the count; `settle` and `settle_again` treat a host on another build
   with sessions mid-turn *or* terminals open as busy. `HostError::Busy` gains `terms`.
3. App: `HostFailure::Busy` gains `terms`; the waits keep it beside the sessions.
   `waiting_line(host, sessions, terms)` says both (*Waiting for 1 session and 2 terminals on
   box to finish…*); the *Restart now* confirmation (`busy_body`) says the terminals it closes.
4. Tests: host serve answers the count; core settle refuses with terminals only; app words.
5. Docs: hosts.md (the build check), known-gaps.md drops the entry and its offload-poll note.

One PR, `fix/host-terminals-busy`.
