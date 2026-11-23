# The project page as where work starts

## Intent

Today a project's page is a page of facts, and it only shows while the project has no session:
pressing the project on the rail, or its tile on the overview, shows the last session instead
(`rail_click_project` → `select_root`; the page is built in `chat/pane/sessions.rs` only with
no session to show). Once work begins, the project's settings (check command, build hosts,
worktrees, the host's card switch) and its host's sessions are out of reach. The owner hit this
adding a build host to the very project their session runs in: reaching the page meant closing
that session.

The owner wants the page redesigned into **the place every session starts from**: the empty
session's look (*What are we building?* over the composer), with a composer that picks the
agent, its model and mode, where the session runs (the project's folder, a worktree, or a new
worktree), and how its cards are answered, and under it more of the project's information and
features than the page holds now. A past conversation can be carried into a new session, with
the same agent or another.

## Goals

- The page is always reachable: pressing a project's row shows its page, whether or not it has
  sessions.
- Every *New session* (the rail's `+`, the page, the keymap command, the header) lands on the
  page. A session is made only when the first prompt is sent, so no empty session is left on the
  rail.
- The page's composer is the session composer's look: text, `+`, ⚡, an **agent** pick, the
  picked agent's **model** and **mode** chips. Under it, left, the **place** chip (the project's
  folder, each worktree, *New worktree…*); right, the **Cards** chip.
- **Cards** (the approval level) has three levels: *Manual* (every card waits for the person), *Allow edits*
  (cards whose tool kind is reading or editing files are allowed; the rest wait), *Bypass*
  (every card answered as a host's bypass answers it). The project keeps a default; a session
  starts on it and can change its own from a chip under its composer, without writing back to
  the project.
- **The agent's options come from what each agent last said** (owner's pick, approach B below).
- **A past conversation continues in a new session**, with any configured agent: *Continue in a
  new session* on its row puts it on the page's composer as a context chip. The context goes as
  a Markdown file the agent is pointed at; *Summarise first* on the chip is an option.
- Under the composer, the page keeps only what is about starting and finding work: *Past
  conversations* (or *Sessions on <host>*), *Worktrees*, and the offload box of an offloaded
  project, scrolling under it. The *Tasks*, *Schedules* and *Settings* boxes leave the page
  (owner's call): Tasks and Schedules are reached on their own pages, narrowed by project; the
  project's settings move to Settings (below).
- **Settings gets a *Projects* section** beside Workspace, Agents and Hosts: a project picker,
  then the picked project's *Check command*, *Build hosts* and *Worktree folder* (moved out of
  the Worktrees box). A project's `⋯` menu (rail and header) gains *Project settings…*, which
  opens it on that project. The Cards default is changed on the page's chip alone.

## Non-goals

- Resume stays as it is: the same agent's `session/load` of the same conversation.
- Conversations started outside onehand (the agent's own history) are not listed.
- No change to how a session behaves once started, beyond the approval chip under its composer.
- The overview, Tasks, Workflows and Schedules pages are untouched; Settings only gains the
  *Projects* section.

## Anti-goals

- Onehand must not keep its own list of each agent's models or modes (approach A): it would
  chase every agent release, and a declared agent's list would be the person's to write.
- No throwaway agent session opened just to learn its models (approach C).
- No second composer implementation that drifts from the session's: the page and the session
  share the composer's widget and its draft behaviour.
- A level that answers cards must never be on without the page or session saying what it costs,
  as the host's switch says today.

## Constraints

- Core owns the rules and a drive decides what happens next (`docs/drives.md`); the app carries
  out. The approval level, which cards a level answers, the remembered agent facts, the place
  pick and the hand-off's lead-in are core's.
- ACP gives an agent's models and modes only in the answer to `session/new`. The warm adapter
  (`crates/io/src/acp.rs`) starts the process, not a session, so it cannot tell them.
- Card answers already exist in core: `PermissionRequest::bypass_answer`,
  `Elicitation::bypass_answer` (`crates/core/src/acp/types.rs`). Local sessions answer no card
  by themselves today; the host service does (`Hub::bypass`, per folder, kept on the host).
- A host project has no worktrees, and its card switch lives on the host and is on or off.
- Worktree names follow `crates/core/src/worktree.rs`'s rule; the folder is derived, as in the
  *New worktree* dialog (`dialogs/branch.rs`).
- The composer's IME rule (`Composer::insert_trigger`), focus handling and bounded rendering
  apply on the page as in a session. Files stay under ~800 lines; the project page is already
  split under `chat/pane/project_page/`.
- UI follows the `ui` skill and its checklist; `DESIGN.md`, `docs/screens.md` and
  `docs/ui-patterns.md` are updated with the behaviour in the PR that builds it.

## Approaches

### Where the page's model and mode chips get their lists

- **A. Onehand declares each agent's models and modes.** Instant, but onehand chases every agent
  release, a declared agent's list is the person's to write, and a stale name fails at send.
- **B. Onehand remembers what each agent last said (picked).** Every connected session of an
  agent records the models and modes it reported, and the person's last pick, in `state.toml`.
  The page offers those; on send the session is made in the right folder and the picks are set.
  Self-updating and agent-neutral. Costs: an agent never yet run shows *Agent's default* until its
  first session; a remembered pick the agent dropped starts on the agent's default with a line
  saying so.
- **C. Probe on every page show.** Fresh lists, but a real session per page show, seconds of
  Claude start-up, an empty session left in the agent's history, and wasted when the place is a
  new worktree.

### How a past conversation's context reaches a new session

- **1. The whole transcript in the first prompt.** Complete, but long conversations burn tokens
  or overflow the context, mostly on tool output.
- **2. A Markdown file the agent is pointed at (picked).** The conversation is exported (the
  header's *Export as Markdown* already writes it) to a file the new session can read, and the
  first prompt leads with one line naming it, then the person's prompt. Cheap and agent-neutral;
  relies on the agent reading it.
- **3. A summary first (picked as an option).** With *Summarise first* on the chip, the lead-in
  asks the agent to read the file and open its reply with a summary (decisions made, work left,
  files touched) before going on with the prompt. *Builder's call, see below: the summary is the
  new session's own first step, not a separate agent run.*

## The design

### Reaching the page

- A project row's press shows the project's page; session rows show sessions. The rail's `+`,
  *New session* anywhere, and the keymap's new-session command show the page of the project in
  question with the composer focused. Closing the last session shows the page as today.
- The header over the page keeps today's no-session form (no name, the project's dots menu).
- The empty-session screen goes: a session exists only once a prompt is sent, so a connected
  session with nothing in it is no longer a state the person lands in. (A restart or a failed
  first send may still leave one; it draws as a session with a transcript does.)

### The page

```
                 What are we building?
   ┌──────────────────────────────────────────────┐
   │ [context: <conversation> · Summarise first ×] │   ← only when continuing
   │ Ask the agent…                                │
   │ +  ⚡  Claude ▾  Opus 5.5 ▾  Default ▾     [↑] │
   └──────────────────────────────────────────────┘
     ⑂ main ▾                            ◯ Manual ▾
     (cost line when the approval level answers cards)

   Past conversations (or Sessions on <host>) · Worktrees · offload box   (scroll)
```

- The heading and composer sit near the top, not centred, so the boxes start within the first
  screen; everything under the composer scrolls.
- The agent chip lists the configured agents; the model and mode chips show the picked agent's
  remembered lists, or *Agent's default* while none is remembered. Picking an agent keeps the
  text.
- The place chip: the project's folder (named by its branch), each recorded worktree on disk
  (the page's existing cap of 20 and its *N more not shown*), then *New worktree…*, which opens a
  branch-name field in the chip's menu checked by the worktree rule as it is typed; the folder is
  derived as in the dialog. The worktree is made on send, before the session. A host project's
  chip offers its folder alone.
- The draft (text, picks, context chip) is the page's own per project, kept while the person
  goes elsewhere, cleared when sent.
- Send: (new worktree → make it) → open the session in the place with the agent → once
  `session/new` answers, show the session → set the remembered picks → send the prompt (with the
  hand-off lead-in when continuing). While the worktree is made and the agent starts, the page
  says *Starting <agent>…* and the draft is locked. A failure before the session exists (the
  worktree, `session/new`) stands on the page, inline under the composer, the draft kept, and no
  session is left on the rail; a worktree made before it stays, listed in *Worktrees*. After the
  session shows, a pick that fails is a line in its transcript and a prompt that fails is left in
  its composer, as a session's send does today. *(My call.)*
- The entries that pick a place or an agent today (the rail's *Start in <worktree> with
  <agent>*, a worktree row's *New session*, the keymap's start beside the file on screen) land on
  the page with those chips set; text already typed in the draft is kept. *(My call.)*
- The place chip is *Start in*, the existing word, not "place", which names a run's checkout.

### Cards (the approval level)

Named *Cards* on screen and in the glossary (extending *permission bypass*), since "approval"
already names a workflow step and "auto-approve" is a word the glossary avoids.

- **Only sessions the person opens take the project's default** (owner's answer): one sent from
  the page, a *Continue*, and every entry that lands on the page. A session opened by a task, a
  workflow, a schedule, a lead (a worker), the control tools or a fork starts on *Manual*, so
  "only a person answers what an agent parked" holds; the person can still change it on that
  session's chip.
- **Independent of the agent's own mode** (owner agreed): each agent reports modes of its own
  (Claude's `acceptEdits`, `bypassPermissions`; Codex's access levels; some none), so onehand
  cannot tell which of them approve. *Cards* decides only the cards an agent does send; a mode
  that never asks leaves it nothing to answer, which is not wrong. No rule links the two chips.
- A session's level is kept with its conversation (its metadata), so a restart or a Resume keeps
  it; a *Continue* starts on the project's default.
- Levels: *Manual*, *Allow edits*, *Bypass*. Which cards each answers is core's, beside the
  existing bypass answers. *Allow edits* allows a permission whose tool kind is read or edit
  with its narrowest grant; every other card waits. *Bypass* answers as a host does today.
- The project's default is kept in its project file; the page's chip changes it. A session takes
  it when made and keeps its own from then; the chip under the session's composer changes only
  that session.
- A level other than *Manual* says its cost under the chip, the host's sentence adapted.
- Local sessions answer their cards in the session's drive as they arrive; a card answered so is
  drawn answered and raises no notice, as a host's is.
- A host project offers *Manual* and *Bypass*, and the chip is the host's existing switch (the
  *Settings* box that held it goes); a session on a host has no chip of its own, since the host answers per
  folder.

### Remembered agent facts

- When a session connects, everything the agent reported for its composer is written for that
  agent to `state.toml`: its config options by role (model, mode, effort, fast, as
  `crates/core/src/acp/roles.rs` sorts them) and its `/` commands, with the person's last pick
  of each (on the page or in a session). So the page's ⚡ and `/` work before a session exists.
  *(My call.)*
- The page reads them; a session made from the page sets the picks after `session/new`. A pick
  the agent no longer offers is skipped and the transcript says the session runs on the agent's
  default. A `/` command the agent no longer has goes as typed, and the agent answers it.

### Continuing a past conversation

- Each past conversation row gains *Continue in a new session* (its `⋯` and right-click). It puts
  the conversation on the page's composer as a chip (its title, *Summarise first* toggle, ×);
  the agent chip starts on the conversation's agent. One conversation at a time.
- On send, the conversation is exported as Markdown to a file in the session's place, under a
  folder git ignores or outside the checkout (*builder's call*); on a host project it is written
  through the host's `Disk`. The first prompt is a lead-in naming the file (and, with *Summarise
  first*, asking for the summary first), then the person's text.
- The new conversation records which one it continued, so its row can say *Continued from
  <title>*.

## Calls left to the builder

- The order of the work into PRs (a natural cut: page reachable and composer-on-page; place chip
  and new worktree; approval levels; remembered agent facts; continue).
- Where the hand-off file goes and when it is cleaned.
- How the page's composer shares the session composer's code.
- Exact words, ordering of the boxes under the composer, and the session chip's place among the
  composer's existing chips, within the `ui` skill.
- Keys: whether the page's composer takes focus on arrival from the rail, as *New session* does.
