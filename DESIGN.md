# DESIGN.md: UI overview

The whole-app UI contract for `onehand`: how the window is laid out and the rules every view
follows. It describes structure and behaviour, not values. Details below that level belong to the
code and its tests. Code never cites this file; a comment gives its reason in its own words.

**No palette lives here.** onehand uses gpui-component's theme, plus one surface-ramp override
installed at boot (`crate::theme::install`, which carries contrast tests). Every colour, radius and
size is read from `cx.theme()` at the call site. A hex literal in the render layer is a bug even
when it looks right, because a theme switch cannot reach it.

## Principles

1. **Chat is the centre.** The conversation is the only region that flexes. The Workbench and
   terminal are docks that start closed, open on demand, and never crowd the conversation.
2. **Separate by hairline, not shadow.** A 1px `border` separates panels. Shadows are only for
   surfaces that really float (dialogs, popovers, the composer popup).
3. **Colour means state.** One accent, from the theme. Danger, warning and success ink mark
   failure, in-flight and done. Anything else is `muted_foreground`.
4. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
5. **Icons are registry SVGs, never glyphs.**
6. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
7. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.

## Layout

One window hosts one workspace. The Workbench runs the full height; the terminal sits under the
agent pane only. The diagram uses ASCII and box-drawing characters only, so it stays aligned in any
monospace font.

```
┌──────────────────┬────────────────────────────────────┬──────────────────┐
│ Workspace name   │ Title (...)    past close term wb  │ modes   max hide │
│ Add project...   │                                    │                  │
│ Overview         │                                    │                  │
│ [+ New session v]│                                    │    Workbench     │
│ ──────────────── │          agent pane                │  (right dock,    │
│ Projects | All   │        (centre, flexes)            │   full height)   │
│ > project  main  │                                    │                  │
│     session      │  ┌──────────── composer ──────┐    │                  │
│     session      │  │ +  Fast  model        Send │    │                  │
│                  │  └────────────────────────────┘    │                  │
│                  │    branch                mode      │                  │
│                  ├────────────────────────────────────┤                  │
│                  │ shell tabs  +                      │                  │
│ Settings         │ terminal (bottom dock)             │                  │
└──────────────────┴────────────────────────────────────┴──────────────────┘
        rail                     DockArea
```

- **Rail.** App chrome outside the dock, so a layout restore cannot lose it. Top to bottom:
  - the workspace row, which opens the workspace switcher;
  - *Add project…*;
  - *Workspace overview*, highlighted while that page shows;
  - *New session*, a filled split button whose caret picks the project and agent (shown only when
    there is a choice);
  - a hairline;
  - a *Projects | All sessions* switch;
  - the scrolling tree;
  - *Settings* in the footer.

  A project row carries its folder icon and name, then any of: a pin, the change count, an
  unattended-run pill, the most urgent session mark and a fold chevron. The selected project's
  row also shows its branch and an ellipsis menu.
  The order of the tree is the user's, set by dragging. Pinned projects stay on top, and
  sessions never leave their project. `Ctrl+Shift+B` hides the rail completely; it never
  collapses to an icon column. It resizes between 232 and 320px. It is the one panel drawn on
  the ramp's lifted surface.
- **Agent pane header**, left to right:
  - the show-rail button, only while the rail is hidden;
  - the conversation's name in bold, full ink;
  - a dots menu: rename, export, resume, restart, and delete in the danger tint;
  - a spacer;
  - past conversations and *Close session*, only while a session shows;
  - the terminal, with a dot while a shell is alive;
  - always last, the Workbench.

  Neither dock button is drawn on the workspace overview.

  When the pane narrows, the name gives way first, down to a minimum width; the controls keep
  their size. Without a session, the row names the project, and its dots menu holds the project's
  actions.
- **Composer.** A card at the foot of the transcript. Inside it, one row: the `+` menu, *Fast*,
  the model chip, and *Send* or *Stop*. Under the card, outside it, a strip shows standing state:
  the project's branch on the left and the turn's permission mode on the right; both open a
  menu. If neither exists, the strip is not drawn.
- **Docks.** Bare panels with strips of their own and no library tab bars.
  - The Workbench strip has the mode chips (Editor, Markdown, Neovim, Issues, Plugins), then
    maximize, then hide.
  - The terminal strip has its shell tabs and `+`.
  - Each dock is a card, inset on three sides and flush on the side it is dragged by, on the same
    reading surface as the conversation.
  - Hiding a dock keeps its buffers and processes.
  - A hidden terminal is unmounted and takes no room.
  - The terminal's open state follows the selected project.
- **Pages without a session.**
  - A project with no session shows *New session* and its past conversations: capped, scrolling,
    each with a *Delete* word.
  - *Workspace overview* shows these cards across all projects: *Waiting on you*, *Working*,
    *Projects*, *Recent conversations* and *Open issues*. While it shows, the header reads
    *Workspace*, and both docks are put away.
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast. Modals are
  `Dialog`s. Settings is a large dialog: a nav column with Appearance, Workspace, Agents,
  Connections and Shortcuts. Groups are separated by hairlines, not boxes.
- **Persistence.** The layout is saved as the Workbench width, terminal height and rail width,
  plus whether each dock is open. It is never saved as the library's `DockAreaState`.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, sized so 100 mono columns of a diff fit inside a card, and
  narrowed on small panels. The composer, and anything pinned above it (permission, question,
  queued prompt), are capped narrower and read as one stack.
- **One turn, many blocks**: prose, thoughts, activity rows and clusters, commands, diffs,
  permission and question cards, notices and errors. Long output folds and is capped. Content
  wider than its well scrolls inside the well.
- **Destructive actions are words in the danger tint**, confirmed through a modal that names the
  thing being removed, never through a button that arms on first press.

## Typography and spacing

| Role | How to write it |
|---|---|
| Body | the inherited size, never set |
| Chrome (a panel's rows, cards, controls) | `.text_sm()` |
| Titles | `.font_semibold()` at the size of what they title |
| Meta, hints | `.text_xs()` + `muted_foreground` |
| Machine text | `mono_font_family` |

- **Sizes are rems, never pixels**, because per-panel zoom overrides the rem base. Fixed chrome
  heights stay outside the zoom wrapper.
- Spacing uses gpui's base-4 scale. Radius comes from `cx.theme().radius`; `rounded_full` is only
  for dots and pills.
- Weight carries hierarchy before size does.

## Colour and state

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `muted` / `muted_foreground` | quiet fills, meta text |
| `border` | every hairline |
| `accent` | the one item selected among several |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text |
| `popover` | floating surfaces |

- Cards are borders, not fills.
- Nothing gets a ring: hover and selection are fills, at distinct steps of the ramp.
- One primary per view.
- If a surface is missing, add it to the ramp with its contrast asserted. Never add it in the one
  view that needed it.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, `list` / `virtual_list`. Buttons go through the app's action
wrapper, which sets the pointer cursor; a disabled control goes back to the arrow. The app owns
only what is onehand's own:
- the transcript renderers;
- the icon registry;
- the terminal panel;
- per-panel zoom;
- the composer popup. This is one shell for `@`, `/`, the pickers and the attachment tray, with a
  pinned title and footer, a fixed height, grouped rows, and its own scrollbar on its edge.

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a Workbench mode on
the shared PTY. Plugins are built in, because Rust has no stable ABI to load them at run time.

## Icons

Every icon is an SVG from gpui-component's `IconName`. `crate::icons` holds only what that set
cannot draw, synced from `assets/icons/manifest.toml` by `scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
