# Agent layout: boxed regions, a full-width terminal, a quieter frame

Design for one UI overhaul of the window's frame. The owner approved its look as an HTML
mock-up (layout v4, kept outside the repo); this document says what that mock-up commits the
app to. It is a spec of work under way and leaves with the work's last PR, its behaviour by then
written into `DESIGN.md` and `docs/screens.md`.

## Intent

The owner wants onehand's window to read as a modern agent workspace: each region a box of its
own on the window's fill, the terminal under everything, less chrome. The reason given is taste;
success is the app matching the approved mock-up. The two things the owner chose explicitly:
the terminal spans the window's width by moving the Workbench into the dock's centre (option a),
and the composer stays exactly as it is.

## Goals

1. **Boxed regions.** The agent pane, the Workbench and the terminal are each a box: the page fill
   (`background`, #181818 dark), a radius, no edge, parted from each other and the window's edge
   by a small gap that shows the window's fill (the chrome fill, #1f1f1f dark). The rail and the
   title bar are not boxed: they sit on the window's fill. No hairline is left between regions
   or inside a box's strips (the Workbench's tab bar, the Files tree beside the editor, the
   terminal's tab bar); where two areas inside one box must still part, they part by fill.
2. **Full-width terminal.** The terminal dock spans under the agent pane and the Workbench
   together. The Workbench leaves the right dock and becomes the right slot of an `h_split` in the
   `DockArea`'s centre, beside the conversation; the terminal stays the bottom dock, which the
   library already draws under the whole centre.
3. **No status bar.** The branch it held already stands under the composer.
4. **Title bar.** It keeps, in order, the mark and *onehand*, the menus, the centred search, the
   toggles, the window's controls (as in the mock-up). A rail toggle joins the terminal and Workbench toggles (left, bottom, right
   panel glyphs, in that order), the same on/off inks. The rail's own *Hide the rail* button goes.
   The workspace menu moves here: the search field leads with the workspace's name, which opens
   the workspace menu (recents, *Open workspace…*, *New workspace…*) when pressed; typing still
   searches sessions and projects. The name is a segment inside the field, a tab stop of its own
   that opens the menu on Enter, as the rail's workspace bar did.
5. **Rail.** The workspace bar goes (its menu now lives in the title bar). *New* becomes a page-like
   row, *New session*, its key hint at its end, pressing it doing what *New* does; its caret
   menu (*Start in*, *With agent*) opens from a chevron at the row's end shown on hover and on
   the keyboard's row, and on right-click. The *Sessions* heading gains a search button (focusing
   the title bar's search, as `Ctrl+K` does) beside its existing filter menu and attention chip.
6. **Empty session.** While a session's transcript holds nothing, the pane shows a large heading
   (*What are we building?*) centred, with the composer directly under it, both centred
   vertically; the first item moves the composer to its usual foot. The header stays as it is.
   It applies to any connected session with no items and nothing pinned above the composer; while
   the session connects or fails to, or a card, banner or queue stands above the composer, the
   pane draws as it does today.
7. **Type weights**, read off the owner's reference: *New session* and the *Sessions* heading 600
   in the strong ink; page and project rows 500; session titles 400, their second line 400 muted
   at the meta size; the conversation's name 500; the empty heading 600 at a display size; tabs,
   tree rows, terminal tabs 400. Fonts stay Be Vietnam Pro and JetBrains Mono.
8. **Workbench and terminal bars**: the open tab is a rounded fill (`secondary`) instead of the
   tab fill under a primary stroke, tabs not parted by lines. The terminal's shells move from
   top tabs to a list down the box's right side when the dock holds more than one shell
   (the top bar keeps `+`, maximize, hide, and on its left the open shell's name); with one
   shell there is no list. A shell row keeps what its tab holds today (capped name, live dot,
   close on hover and on the keyboard's row); the list has a fixed width token, not dragged and
   not saved. The open-tab look applies to every `tab_bar` the Workbench and the terminal draw
   (modes, the Editor's file tabs with *N more*, Issues), so one look holds. The Files tree and
   Markdown's document tree stand on the right of what they open instead of its left.

9. **One Workbench row.** The Workbench's mode bar and the Editor's file tab bar merge into one
   row: a mode picker (*Editor ▾*) at its start, then the mode's own tabs or controls (the
   Editor's file tabs with *N more*), then maximize and hide. The `MODES_MIN_W` select is gone,
   the picker being one already. *← Conversation* still leads the row while the Workbench has
   the area.

## Non-goals

- The composer: unchanged in every part (the chips above it in early sketches were dropped).
- A *Changes* view: the mock-up drew a *Changes* tab; onehand has no such view and this work
  does not add one.
- Making each mode a tab of its own beside the files (the owner chose a mode picker instead, goal 9).
- A light-mode redesign: light mode takes the same structure with its existing tokens.
- Any change to what a page, the transcript or a dialog holds.

## Anti-goals

- The terminal dock's behaviour regressing: shells, focus, `Terminal` key context, the live dot,
  its saved height.
- The narrow-window behaviour regressing: the conversation keeps its minimum beside the
  Workbench, and too narrow for both, the Workbench takes the area under *← Conversation*.
- A second layout store: persistence stays `PanelLayout`'s fields, never the library's
  `DockAreaState` (`load` needs the global `PanelRegistry`, which the app refuses).

## Constraints

- `DESIGN.md` (*Layout*, its diagram, *Nothing else in the title or status bar*) and
  `docs/screens.md` (*Title bar*, *Status bar*, *Rail*, *Docks*, *Persistence*) say the opposite
  of goals 1-5 today; the owner's approval of the mock-up authorizes changing them, in the same PR.
  #349's surfaces are superseded by goal 1.
- Every size, radius and gap is a token or a rem from the UI skill's tokens; no hex literal.
- Files already at or past 800 lines that this touches (`shell/sessions.rs` 849, `terminal.rs`
  843, `shell/boot.rs` 807, `rail.rs` 800) are split along a seam in the same PR when the change
  grows them.
- Guards in `crates/app/src/guards.rs` must stay green, notably the unread-field, only-false flag
  and exhaustive event-match guards when `stepped_aside`, `fills_area` or `WorkbenchEvent` change.
- One PR, all phases and their docs, branch `style/agent-layout`; a stop for the owner after
  each phase.

## Approaches (terminal span)

- **(a) Workbench into the centre split** — chosen by the owner. gpui-base 0.8's `DockArea`
  draws the bottom dock inside `center_frame`, under the whole centre, so this alone spans it.
  A centre slot hides through `Panel::visible` (a hidden slot takes no space), resizes through its
  split's `ResizableState` with `set_split_sizes`, and maximizes with `set_zoomed_in`. Cost: the
  22 `DockPlacement::Right` sites (mount, open/close, sizes, the narrow fallback, zoom, focus,
  pages, skin) move to a small app-side Workbench slot API.
- (b) Terminal outside the `DockArea`, drawn under it with its own resize: fewer Workbench edits,
  but re-implements a dock's open/close, size and edge the library already gives.
- (c) Leave the terminal under the conversation only: does not meet the goal.

## Decided by the owner

- The overall look (layout v4): boxes without edges, radius, gaps; rail and title bar on the
  window's fill; the weights in goal 7.
- Option (a) for the terminal.
- One Workbench row with a mode picker (goal 9), over keeping two bars or tabs per mode.
- The composer stays as it is.

## Calls made here (the owner may overturn)

- The workspace menu moves into the title bar's search field (goal 4).
- The status bar goes (goal 3).
- No name in the empty heading: there is no reliable source for the person's name.
- The rail's *Hide the rail* button goes, the title bar's toggle taking its place.
- One shell draws no shell list.
- Maximizing the Workbench keeps today's result: the terminal stays as it is.
- Phase order: contract → frame and boxes → title bar, status bar, rail, weights → empty session →
  Workbench into the centre (terminal span) → Workbench and terminal bars.

## Left to the builder

- The radius and gap tokens' values within the UI skill's scale (the mock-up used 10px and 6px).
- How the Workbench slot API is shaped (where `workbench_open`, `show`/`hide`, width and zoom
  live), so long as callers stop naming `DockPlacement::Right`.
- Whether the narrow fallback hides the chat slot or keeps today's stepping aside, so long as
  its behaviour is unchanged.
- Where `split_frame`/`render_split_handle` draw the gap, so the drag handle stays usable.
