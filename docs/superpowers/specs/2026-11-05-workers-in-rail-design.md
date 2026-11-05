# Workers in the rail

2026-11-05. Replaces the workers' region of `2026-10-30-workers-list-design.md`.

## Why

The owner finds the workers' region on the right uncomfortable and hard to work with: a worker
is reached only through its lead's strip and a region in the Workbench's place, with keys and a
composer of its own. They want a worker to be where every other session is: a row in the rail,
under its lead, that opens the worker in the agent pane as an ordinary session, with a mark that
says it is a worker and which lead coordinates it.

## Goals

- A lead's workers are listed in the rail as a tree, directly under the lead's row.
- Pressing a worker's row shows it in the agent pane like any session: header, transcript,
  cards and the normal composer.
- While a worker shows, a band above its composer says who coordinates it, and offers the way back.
- The workers' region, its keys and its *Write to …* composer are removed.

## Non-goals

- No change to what a worker is in core or control: `open_worker`, `session_state`,
  `send_prompt`, `cancel_turn`, the cap of four, holding and giving back stay as they are.
- No collapse toggle on a lead's tree: at most four workers sit under it (owner agreed it needs
  none; builder's call not to add one).
- No change to the rail for sessions that are not workers.

## Anti-goals

- A worker must not read as an ordinary session: the band always says it is a worker while its
  lead is open.
- A lead must never read as failed because a worker failed (kept from the current rail).

## Decided by the owner

1. **The rail.** Workers of a lead listed in the window are drawn as nested rows right under the
   lead's row: indented, joined to the lead by a vertical line, each its own row with its mark,
   name, state and age like any session row. The lead's row keeps its own state and its line's
   *N workers need you*.
2. **Showing a worker.** Pressing its row shows it in the agent pane as any session is shown,
   with the normal composer (no *Write to …*). The Workbench and terminal follow the worker's
   own project, as for any session: usually its worktree, whose project has no row in the rail,
   so only the worker's row is highlighted.
3. **The band (option B of the mockup).** Above the worker's composer, a band reads
   *<lead> is coordinating this worker* with *Back to lead*; once the person holds the worker
   (they sent it a prompt) it reads *You hold this worker* with *Give back to the lead*, offered
   when giving back is offered today. With no lead open, the worker is the person's ordinary
   session and there is no band.
4. **The workers' region goes**, with its list, detail, *Write to …* composer, the header's
   workers control, *Show the workers* and its keys.
5. **The lead's strip stays.** Pressed, it shows the most urgent worker in the agent pane
   instead of opening the region.

## Constraints

- The rail's rows stay one list for the pointer and the keys (`rail::model::rows` and `order`
  from the same source), so `↑`/`↓`, `Alt+↑`/`↓` and `Enter` reach worker rows as any row.
- Rendering stays bounded: nested rows count toward the rail's per-project session cap as any
  session row does.
- Every UI change follows the `ui` skill: row, band and buttons reuse existing builders and
  tokens; words follow UI patterns *Words*.
- Docs updated in the same change: `docs/screens.md` (rail, agent pane header, the strip, the
  removed region), `docs/control.md` (*What the person sees*, the `⋯` reference), `CONTEXT.md`
  (*Worker*: no longer "seen through its lead").

## Settled by the designer (my calls, for the owner to check)

- **Nesting is a parent link, not an order.** A worker usually lives in its own worktree
  project, so it is drawn under its lead whatever project it belongs to, in every view
  (by project, all sessions, *Needs attention*, *Recent activity*, a search). `Item`/`Row` carry
  the link; sorting a flat view sorts leads and keeps each lead's workers under it, in the order
  the lead opened them.
- **A tree is listed whole.** A lead listed in a view brings all its workers. A lead is listed
  when it or one of its workers matches the view (the rolled-up state already does this for
  *Needs attention*); a search matching only a worker lists the lead with its tree. A worker
  whose lead is not listed in this window is its own ordinary row, as today.
- **The cap counts trees.** A lead and its workers are one entry for the rail's per-project cap,
  so a tree is never cut across *N more*; at most four workers make it bounded.
- **Nested rows do not drag** and are not counted in their own project's badge or fold: their
  place, fold and badge follow the lead's project (no double count).
- **The band when held but not offered** (lead switched off): *You hold this worker*, no
  button. A lead in another window: *Its lead is coordinating this worker*, without *Back to
  lead* (this pane does not know the lead's name).
- **A worker stands for its lead's project** (added after review): folding from its row folds
  the lead's project, *New* beside a worker on screen starts in the lead's project (never the
  worker's transient worktree, which is not saved), and closing the worker on screen shows its
  lead. A worker that leads workers shows the lead's strip, not the band.
- **Arrival:** `Shell::turn_to_lead` goes, so arriving at a worker shows it; `mru_order` stops
  filtering workers (`Ctrl+Tab` stays per project, so it reaches only a worker sharing the
  project); `release_workers` on close is reviewed with the region's removal.

## Left to the builder

- **Where the nested rows' data comes from:** replace `rail::model::fold_workers` (which drops
  a worker) with the parent link above; keep the transient project's emptying, keyed on "listed
  under its lead".
- **Keys left:** `Alt+Y`/`Alt+A`/`Alt+N` keep answering the pinned card in `Chat`; the
  `Workers` key context and every command bound in it go; `Ctrl+S` loses `!Workers`.
- **What moves before the region is deleted:** `urgency()` (the strip picks with it),
  `Shell::locate`, `worker_name()`; the give-back wiring moves from the detail's `⋯` to the band.
  `Composer::compact` goes if nothing else uses it.
- **Order of work:** step 1, the rail tree, showing a worker in the pane, the band and the strip;
  step 2, deleting the region and its keys. One PR each.

## Tests

- Rail model: workers listed under their listed lead, in the lead's order; a worker whose lead
  is not listed stays in its own project; a transient project holding only nested workers is
  left out; the cap counts nested rows; filters as above.
- Arrival: showing a worker shows it, not its lead.
- The band's words follow the hold state (a pure function over lead name and held/offered).
- The strip's press picks the most urgent worker (the existing lead-strip tests, re-pointed).
