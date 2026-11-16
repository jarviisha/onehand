# A lead keeps its workers across a restart, closes them, and picks a model per turn

Two of the gaps `docs/known-gaps.md` lists for leads and workers, built together:

1. *A lead's workers do not outlive the run of the app.*
2. *A lead cannot close a worker, nor pick a model per turn.*

## Why

A lead's workers today live in memory only. After a restart the person resumes the lead's
conversation from history, and that lead has lost everything it held: its handles are refused as
"from an earlier run", its workers' conversations sit on disk attached to nothing, and their
worktrees are left with no session that owns them. The lead has to open new workers, and each one
rebuilds its context from scratch. The orphaned worktrees are also where the owner's original
complaint started: there is no way to reclaim them. That reclaiming is a separate, later piece of
work.

A finished worker also holds one of the four places under the cap until the person closes it by
hand, so a lead running a long job keeps stopping to wait for the person. A worker also stays on
the model it was opened with, even though onehand already changes a session's model mid-session
from the composer.

## Goals

- When the person opens a lead's conversation again after a restart, the lead's workers come back
  as they were when the app quit: listed under the lead, on their own conversations and folders,
  and named by the handles the lead already holds.
- A lead can close a worker it holds.
- A lead can name a model with a turn it sends.

## Non-goals

- Reclaiming worktrees, whether by hand or by a tool. It comes next, as its own piece of work.
- Restoring ordinary sessions after a restart. Only a lead's workers come back, and only when their
  lead's conversation is opened.
- An outside program leading workers, a worker's results shown inside the lead's conversation,
  a tool that answers cards, and a higher cap. These remain gaps.

## Anti-goals: failures even if the change works

- A worker coming back and running anything on its own. It comes back idle; its agent starts only
  when the person shows it or the lead calls a tool on it, as every session's agent connects
  lazily today.
- A lead closing a worker the person has taken over, or a worker with a card waiting on the
  person. Both belong to the person.
- One folder held by two sessions because a worker came back onto a folder something else took
  in the meantime.
- Anything deleted on disk: a closed worker keeps its conversation and its worktree, as a close by
  the person does.

## Approaches considered (item 1)

What happens to a lead's old workers when its conversation is opened again:

- **A. They come back by themselves (chosen by the owner).** Every worker the lead still held at
  quit is reopened as a session under the lead, on its conversation and folder, with the lead's
  old handles working. Its agent starts lazily. This is closest to "the lead takes its workers
  back", and the person sees in the rail what the lead holds. It costs folders reserved even for
  workers nobody needs any more, which closing them (item 2) now answers.
- B. They come back when the lead asks, by calling a tool with an old handle. Nothing starts
  that nobody uses, but the person cannot see what the lead used to hold.
- C. The person chooses which to bring back, from a *N workers from before* line in the lead's
  strip. This adds UI and a decision for every resume.

Item 2 has one sensible shape, a new tool and an optional argument that follow the existing
tools, so there were no approaches to weigh.

## Design

### 1. Workers kept across a restart

**What is kept on disk, per lead conversation.** For each worker the lead held (opened and not
closed), in the order it was opened:

- the worker's conversation;
- the agent it runs;
- its folder;
- its `dirs`;
- its name, as given;
- what the relation holds today: the lead's last turn number and text, and the count of
  prompts sent once the lead's last went (which tells whether the person holds it).

Whether the person holds the worker is stored as such (a flag, or a count rebased on coming
back), not only as a raw count. `Chat::prompts_sent` starts at 0 on a resume, so a raw count
read against it would lose the hold. *(My call.)*

It is written when the relation is written today, after the open's first prompt is accepted, and
again each time any of these change. It is kept beside the lead's conversation in the
conversation store, so it moves and is deleted with that conversation. Deleting the lead's
conversation drops it. A worker still opening at quit is not kept, because its open never answered.

**What ends a kept worker, and what keeps it** (the owner's choice):

- **Quitting the app or closing the window keeps it.** These are the only ways a lead's workers
  survive to come back.
- **The person closing the lead ends the relation for good, as it does today.** Its workers go
  back under their own project as the person's sessions, and their entries end. Opening the
  lead's conversation again brings none of them back.
- **A worker closed by the person or by the lead ends its entry.** A closed record stays beside
  the lead's conversation, so the lead's handle keeps answering `link: closed` in later runs
  instead of the refusal a stranger's handle gets. *(My call.)*

**Handles name the worker, not the run.** Today a handle is `<run>-<uid>`, which a restart
breaks. A handle is now an id onehand mints when the worker opens and keeps in its entry. It is not
the conversation id, because a resume that falls back to a fresh session (an agent without
`loadSession`, or a `session/load` that fails) moves the conversation to a new id. The same handle
names the same worker in every run, and the tools resolve it to the session it is in now. When a
worker's or a lead's conversation moves to a new id, the kept entries follow it. *(My call.)*
A handle in the old `<run>-<uid>` shape is still refused as from an earlier run, in the words used
today.

**One lead conversation is open in one session at a time.** *Resume in this session…* is refused
while the session leads open workers, so two sets of workers are never merged past the cap. Opening
a lead's conversation that a session of any window already has open is refused as well, in the
words the history uses for an open conversation. *(My calls.)*

**Coming back.** When a session opens the lead's conversation (from history, or by *Resume in this
session…*), each kept worker that can come back is reopened as a worker of that session:

- It resumes its own conversation, and its agent connects lazily, on first show or on any tool
  call the lead makes on it. Until it is connected, `session_state` reads `link: connecting` and
  `send_prompt` is refused while connecting, in today's words; the lead calls again.
  *(My call: no hidden wait inside a tool.)*
- It is listed under the lead in the order kept.
- Its folder is reserved again, through the same one-worker-per-folder check an open runs (the
  same updates, the same canonical paths). A folder that is no project of the window comes back
  as a transient project, selecting nothing, exactly as `open_worker` adds it.
- It counts under the lead's cap again.
- It keeps its relation: who holds it, the last turn sent, and its `dirs`, which are sent as
  `additionalDirectories` when its agent connects, as they are today.
- A turn that was running at quit reads `lost with the link` afterwards, as a cut-off turn does
  today.

**A worker that cannot come back** is not opened: its folder is gone, or something else holds it
(a session, another worker, a process working in it), or its conversation or its agent is gone.
Its entry becomes a closed record holding the reason. The lead's handle then answers
`link: closed` with one more line saying why it did not come back. The person sees nothing beyond the worker's absence from the
rail. *(My call: a toast for every missing worker would be noise at every resume.)*

**Request ids** stay per lead and per run of the app, as today. A lead retrying an open across a
restart opens afresh. *(My call: the open's answer is gone with the run, and the lead has the
handle of anything that did open.)*

### 2. A lead closes a worker; a turn names its model

**`close_worker`**, with `session` (the worker's handle), offered and refused under the same rules
as `cancel_turn`: for a session's agent only, never listed to an outside program, and only for the
caller's own worker. It is refused, saying why, while:

- the person holds the worker;
- a permission or a question waits, since only a person answers it;
- a turn runs (the lead calls `cancel_turn` first and closes once `session_state` says the turn
  is over).

Closing does what the person's *Close session* does on a worker. The session closes and its agent
ends. Its conversation is kept, and its project goes when it was added for this worker and holds
nothing else. The worktree stays on disk. Its folder and its place under the cap are freed, and
its relation and kept entry end. It answers `closed`. After that the handle reads `link: closed`,
as it does for a worker the person closed.

**`send_prompt` takes an optional `model`**, a value of the worker's agent's model option. Before
the prompt goes, the model is picked the way the composer picks it (`set_config_option`) and its
answer awaited. An agent that offers no model option, or not that value, or refuses the pick,
refuses the call, naming what it offers, and nothing is sent. The pick stays in force for the
worker's next turns, since it is the session's setting, and `session_state`'s `model:` line says
what is in force. It is checked after the checks `send_prompt` runs today, so a retried turn
(`turn <n> already sent`) picks nothing. *(My call: the model stays in force rather than reverting
after the turn. A model that reverted would be a second, hidden setting.)*

### Documents updated in the same PR

- `docs/control.md`: the handle's shape, *Coming back*, `close_worker`, and `send_prompt`'s
  `model`.
- `docs/known-gaps.md`: remove both gaps. The cap line no longer says a finished worker holds
  its place until the person closes it.
- `CONTEXT.md`: the **Worker** entry says the lead may also close it.
- `crates/core/src/control/catalogue.rs`: the tool descriptions agents read.

## Constraints

- Core stays GUI-free and decides everything: what is kept, what may come back, what a handle
  resolves to, and what `close_worker` refuses (`onehand_core::control`, its drive and its
  tests). The app only carries it out.
- Disk reads and writes and the folder checks run off the UI thread, as the open's checks do.
- No document is cited from code. New comments give their reasons in their own words.

## Left to the builder

- The on-disk shape and file name of the kept workers, and exactly what a stable handle looks like
  (it must not be mistaken for the old `<run>-<uid>` shape).
- Where in the resume path the workers are reopened, as long as it happens for every way a lead's
  conversation is opened.
- How the drive's `Relation`, `Requests` and `Reservations` are seeded on coming back, as long as
  the same rules hold as for a fresh open.
- The split into commits. Both items go in one PR.
