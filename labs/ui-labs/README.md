# ui-labs

A place to try UI before it reaches the app: proposed layouts, spacing and
components drawn with gpui-component on static data. Nothing here is the app
and nothing here is binding; an idea that works moves into `crates/app` (and
its contract into `DESIGN.md`) in a pull request of its own.

```bash
make labs       # this crate: the proposed UI
make showcase   # the app as it is, on scratch projects and the Mock UI agent
```

- Outside the workspace (`exclude = ["labs"]` in the root manifest) and built
  by no CI job, so an experiment never breaks a build and the app never
  depends on one.
- Its `Cargo.lock` was seeded from the workspace's so it runs on the app's
  gpui commit. After moving gpui or gpui-component in the workspace, copy the
  root `Cargo.lock` here again.
- `make labs` builds into the workspace's `target/`, so the shared gpui graph
  is compiled once.
- `src/tokens.rs` holds every value the proposal is built from: spacing
  roles, chrome sizes, type, radii, width budgets and both palettes, each with
  the rule it carries. `src/main.rs` is the window layout and the theme wiring;
  `src/pages.rs` the overview, Tasks and Issues; `src/composer.rs` every card
  and popup on the composer.

## Rules the proposal follows

These differ from `DESIGN.md` in places on purpose; that is what is being
tried. Rules tied to one value live beside it in `src/tokens.rs`.

- One primary action per region, never two side by side. Everything else is
  a ghost or an outline control; a destructive confirm is solid red.
- Sentence case in every label, no exclamation marks. Metadata is short facts
  joined by ` · ` (`main · 3 changes`). A back link names where it goes
  (`← Files`, `← Tasks`).
- Two font weights, 400 and 500.
- Icons come from `gpui_component::IconName`, never a glyph.
- Docks are continuous surfaces divided by one hairline, which is also the
  resize handle; no inset frames or gutters around them.
- Truncation is one line with an ellipsis and the full text on hover; state
  and primary actions always stay visible. A short name (`atlas-api`) never
  truncates at the minimum rail width; a long name in a dialog wraps.
- An empty list is one or two plain sentences saying where items come from,
  then the action that creates one.
- No motion beyond what a state change needs.
- The palette is pushed through the theme *config* and applied with
  `Theme::change`, so gpui-component's own controls resolve it too.

## Backlog

Specified in the proposal, not drawn here yet:

- **Settings:** an 11rem nav column while it fits, a select when not; forms at
  most 40rem; a row's label stacks over its control below 32rem; switches,
  segmented controls and selects for its fields.
- **Terminal dock:** opens at 15rem; shell tabs capped at 10rem each and
  scrolling, with `+`, maximize and hide outside the scrolling part; a
  readable conversation always kept above it.
- **Delete dialog:** at most 28rem over the scrim, the long name wrapping in
  the body, *Keep* before a solid red *Delete*.
- **Reading zoom:** `Ctrl+=` / `Ctrl+0` scale `TEXT_READ*` and the chat's
  minimum width only; every bar and the rail stay put.
- **Workbench modes:** Markdown at a 34rem measure; Issues as list → detail
  with search and *New issue* always reachable; Plugins with metadata on its
  own line and actions grouped at the end; Neovim's grid taking everything
  under the mode strip. The mode strip turns into a select below 30rem.
- **Composer context strip:** splits onto two lines below 36rem.
