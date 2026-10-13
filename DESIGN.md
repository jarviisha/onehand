# DESIGN.md: onehand UI contract

The whole-app UI contract for `onehand`: how the window is laid out and the rules every view
follows, as structure and behaviour, not values. It binds at the level written here; below it,
the code and its tests decide. A feature's own document (`docs/tasks.md`, `docs/workflows.md`,
`docs/unattended.md`) holds its screens' detail, and `docs/ui-patterns.md` the patterns every
screen repeats and how they behave. Code never cites this file.

This contract defines onehand's own visual language: quiet navigation, clear content hierarchy,
compact controls and predictable alignment. It is the target for UI work, not proof that every
existing screen conforms. The contract refresh changes documentation only; implementation work
must report the surfaces migrated and any remaining mismatches within its scope.

**No palette lives here.** gpui-component's theme, overridden at boot by the app's palette
(`crate::theme::install`, with its contrast tests). Every colour, radius and size is a token read
at the call site (see `.claude/skills/design-tokens/reference.md`); a hex literal in the render
layer is a bug even when it looks right, as no theme switch reaches it.

## Principles

1. **The conversation is the centre while a session shows**, the only region that flexes. Docks
   start closed and open on demand. Beside the Workbench it keeps a minimum, times its zoom; too
   narrow for both, the Workbench takes the area under *← Conversation*, which steps it aside. Pages
   take the agent pane and put the docks away; a page opens over a session only when picked.
2. **Give each boundary a purpose.** Separate with space first, then a line, then a fill, then a
   shadow, taking each step only for a reason the one before cannot give. Docks use a distinct
   surface; independent boxes and tables use a 1px `border`; a subsection is a heading and space,
   not a frame. Shadows only for what floats, the app's own in
   `lift`: dialogs, popovers, menus, over the transcript the composer card, the cards pinned above
   it, the to-bottom button, and on the Issues page the issue floating over the list.
3. **Content leads; navigation recedes.** Neutral surfaces carry the layout. Hue identifies a
   state, a link, a label's kind or code in prose, never a decorative section background.
   Attention is earned: the signals that call for the person (a state's hue, a count or badge of
   what waits, a dot, weight past what the role takes) mark only what needs them or has just
   changed, so a view at rest is calm. Hierarchy and identity are not attention: a heading keeps
   its weight, a list its muted count, a label its kind's hue. All colours are theme tokens with
   legible contrast on their actual surfaces in both modes.
4. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
5. **Same role, same treatment.** Across app and plugins, equivalent rows, headers and controls
   share size, spacing, corners and states. A valid token alone does not make a consistent view.
6. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
7. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.
8. **Calm and immediate.** What a person changes shows at once, and the work behind it follows.
   Nothing moves for show: the one motion is a `Spinner` on work still running, beside what the
   library's own overlays do. `docs/ui-patterns.md` (*Immediate feedback*, *Motion*) holds how.

## Layout

One window hosts one workspace. The Workbench runs the full height; the terminal sits under the
agent pane only. The diagram is plain ASCII and box-drawing, so it stays aligned in any mono font.

```
┌──────────────────┬────────────────────────────────────┬──────────────────┐
│ W Workspace   v <│ Title (...)    past close term wb  │ modes   max hide │
│ Search...  New v │                                    │                  │
│ Overview         │ workflow > step > step   Stop      │                  │
│ Tasks         2  │                                    │                  │
│ Issues           │                                    │                  │
│ Workflows        │                                    │                  │
│ Sessions  2 need │                                    │    Workbench     │
│ ──────────────── │          agent pane                │  (right dock,    │
│ v project  main  │        (centre, flexes)            │   full height)   │
│   o session      │                                    │                  │
│     Idle . agent │  ┌──────────── composer ──────┐    │                  │
│ + Add project... │  │ +  Fast  model        Send │    │                  │
│                  │  └────────────────────────────┘    │                  │
│                  │    branch                mode      │                  │
│                  ├────────────────────────────────────┤                  │
│                  │ shell tabs  +                      │                  │
│         Settings │ terminal (bottom dock)             │                  │
└──────────────────┴────────────────────────────────────┴──────────────────┘
        rail                     DockArea
```

- **Rail.** App chrome outside the dock, so a layout restore cannot lose it. Top to bottom: the
  workspace bar (letter tile, name, switcher, *Hide the rail*); the search over session titles and
  project names (`Ctrl+K` outside a terminal, showing a hidden rail) beside *New*, which starts in
  the keyboard's project, else the one on screen, its caret picking *Start in* or *With agent*;
  *Overview*, *Tasks* (a pill counting tasks that need attention, none at zero), *Issues* and
  *Workflows*, each filled while its page shows; *Sessions* with a warning-ink *N need attention*
  chip (shown above zero or while on, toggling that filter and back) and the filter menu (*By
  project*, *All sessions*, *Needs attention*, *Recent activity*) over a hairline; the scrolling
  list, ending on *Add project…*; a foot with *Settings*, marked while it shows.

  *By project* lists pinned projects first, a gap between projects. A project row: the fold
  chevron, folder and name, its git line (branch, uncommitted changes, commits ahead and behind,
  each only above zero and named in full on hover), the pin, the unattended pill (*auto*, *auto ·
  N*, *auto · N waiting*; a run's pill opens its issue on the Issues page), and while folded its
  sessions' most urgent state. Its chevron alone folds it (not during a search); the rest of the
  row shows the project's last session, or its page. `+` and `⋯` show on hover, right-click opens
  the same menu. A session row, indented under its project: its state mark, title and `⋯` (on
  hover, always on the one shown), and under them `state · agent · age` (*waiting Nm* while it
  needs input), over which hovering lays *Stop* (running), *Send the last prompt again* (failed)
  and *Close*. The other filters are flat, the project in place of the agent. A group lists six
  sessions, then *N more*; an open project with none says *Empty*, a search or filter with none *No
  sessions match*. Each state has its own ink and, but for failed and disconnected, its own
  mark, none animated (running is a filled dot): failed or disconnected danger, needs input warning, running the accent, done unread success, idle
  muted. `↑`/`↓` move the list's row, `Enter` and `←`/`→` act on it, `Alt+↑`/`↓` step through
  sessions in its order. The tree's order is the user's, set by dragging; pinned projects stay on
  top and sessions never leave their project. `Ctrl+Shift+B` hides the rail completely, never to an
  icon column. It resizes between 232 and 448px, on the ramp's lifted surface.
- **Agent pane header**, left to right: the show-rail button (only while the rail is hidden); the
  conversation's name, medium weight; a dots menu (*Rename…*, *Export as Markdown…*; *Resume in
  this session…*, refused mid-turn, and *Restart the agent*; *Delete conversation* in the danger
  tint, refused until the first turn ends); a spacer; past conversations and *Close session*
  (only while a session shows); the terminal, an accent dot on it while its dock is closed over a
  live shell; last, the Workbench. The two dock buttons are filled while their dock is open. On
  a page (the overview, Tasks, Issues, Workflows, Settings) it reads the page's name, with no dots
  menu and no dock buttons. As the pane narrows, the name gives way first, down to a minimum.
  Without a session it names the project, its dots menu holding the project's
  actions but *New session* and *Open terminal*, which the page and the header already offer.
- **Step strip.** Under the header, only while a run drives the session on screen: the workflow's
  name muted, each step's label between chevrons (done ones with a muted check, the current one in
  full ink), then *Review…*, *Revise…* and *Continue* (primary) while it waits for approval, and
  *Stop* always. Only the step labels clip. *Review…* reads the answer from the run; *Revise…*
  refuses an empty note; a press the run no longer waits at shows the new answer, in the warning
  ink, rather than approving it unread.
- **Composer.** A card with no edge, set off by its raised fill and lift; the caret alone shows
  focus, and no line is ever added to it (a refusal is Send's tooltip, a failure a toast). The
  field, then the `+` menu, *Fast* (a toggle when its choices read as on and off), the model and
  effort chips, then *Send*, or *Stop* (solid danger, worded) and *Queue* over a draft. Under it
  the branch and the mode, stacked when narrow. Each chip's menu opens just above it, without
  its tooltip. Above the card: one pinned permission or question, the oldest, with how many more
  wait; then the queue and a reconnect as plain lines; then the staged files as one row of chips,
  faded where cut beside *Show all N*. *Run a workflow…* (the `+` menu, the keymap, the Workflows
  page's *Run…*) opens the launcher, and an issue's *Run workflow…* its start form; both are
  drawn as `docs/workflows.md` describes.
- **Docks.** Bare panels on the dock surface with strips of their own, no library tab bars; the
  terminal is a block held off its neighbours by a gap, drawn no taller than leaves the
  conversation a readable height (its dragged height kept).
  - Strips as tall as the header. The Workbench's: *← Conversation* while it has the area, the
    modes (Editor, Markdown, Issues, Plugins, Neovim; a select when narrow), maximize, hide. The
    terminal's: shell tabs (capped, scrolling), `+`, maximize, hide. Crowded file tabs: a select.
  - A list beside its detail (files, documents, issues) only while the dock holds both; else one at
    a time, the detail under a link back naming the list. Picking shows it; going back keeps it.
  - The Issues tab's issue: a trail (project › reference) with its actions (one primary, one
    other, ⋯); its tags (state badge, project, labels); the title, large; one line of facts; what
    it waits on (a conflict, where its work stands, the review); then tabs: *Description* (the
    body, the files it names, what the work left, *Before*) and *Activity N* (the history).
    ⋯ ▸ *Open in Issues* and *Review…* open it on the Issues page, the latter with its review open.
  - Hiding a dock keeps its buffers and processes; the terminal's open state follows the project.
- **Pages without a session.**
  - A project with no session shows *New session*, *Run check* when the project has a check
    command, its *Check command* field, one line counting its tasks that need attention, run or
    wait (not drawn at zero, and opening the Tasks page narrowed to the project), and its past
    conversations: capped, scrolling, each with a *Delete* word.
  - *Workspace overview* shows, across all projects, the cards *Waiting on you* (runs waiting
    for an answer or an approval, and sessions waiting, finished or lost) and *Working*, then
    *Projects* as a grid of tiles (not drawn without projects), then *Recent conversations* and
    *Open issues*.
  - *Tasks*: four cards, *Needs attention* (with the project filter), *Running*, *Queued*,
    *Finished*, each with a count, an empty line and a cap. A row is the title over workflow, step
    or outcome and project, with ghost actions by state (*Open session*, *Stop*, *Resume*, *Retry*,
    *Dismiss*); its text opens the task's detail in the same column.
  - *Issues*, in the overview's column: a head (*N open*, *Refresh*, *New issue*); a
    filter bar (search, then chips for open/closed, progress, project, label); a hairline table,
    *Title*, *Labels*, *Updated*, a row a line, rows keeping their place while a person reads.
    Nothing is open on arrival; a picked issue floats over the list's right side, its width
    dragged for the session; `×`/Esc close it, ↑/↓ step. *No issues yet* or *No issues match*
    when empty. Filters, search, selection, the open issue and scroll persist.
  - The **review block** opens below where the work stands only on *Review…*, as
    `docs/unattended.md` draws it.
  - *Workflows* lists the workflows, capped (shipped ones *Built in*, read-only), each with
    *Run…* (projects, the rail's first). *New workflow* or *Edit* opens a form below, one hairline
    box per step, a margin marking where a failure goes back to, problems above a spent *Save*.
- **No top bar, no status bar, no right toolbar.** Transient status goes in a toast; modals are
  `Dialog`s. Settings is a page: a nav column with Appearance, Workspace, Agents, Connections and
  Shortcuts, its groups split by hairlines, not boxes.
- **Persistence.** The layout is saved as the Workbench's dragged width, the terminal and rail
  sizes and whether each dock is open, never as the library's `DockAreaState`.

## Transcript

- **Two sides.** The user's prompt is the one filled bubble, against the right edge. Everything
  the agent produces starts on a shared left axis and runs bare. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read a step over the chrome; what is
  wider scrolls inside its well. A scrollbar runs on the panel's edge down to the composer.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters (tools only), commands,
  diffs, answered permissions and questions, notices and errors. A workflow's steps and an
  interrupted turn are notices, never prompts.
- **Folding blocks open from one line**, the chevron first: what runs is open until done, the
  plan stays open, and a reader's fold wins. Long output folds and is capped. Wells are filled
  with no edge. A finished turn ends on Copy (its closing paragraph) and how long it took.

## Type, space and shape

| Role | Size |
|---|---|
| Meta, hints, counts, a small label | `.text_xs()`, in `muted_foreground` when it is meta |
| Machine text | `CODE_TEXT`, in `mono_font_family` |
| Chrome: a panel's rows, boxes, controls and forms | `.text_sm()` |
| Reading text: the transcript, the composer's field, a document | the inherited size, `.text_base()` |
| A heading in reading text | a step up the scale per level, two at most; then the body's size |
| A page's or an issue's title | `.text_xl()` |

- **Weight carries hierarchy before size does**: `font_medium` for a page's title and a name in
  a header, `font_semibold` for any other title or heading; nothing bolder. Line height is gpui's,
  but `LEADING` for reading text and `LEADING_TIGHT` for a wrapped title.
- **Inks by role.** Text on a neutral surface (the reading surface, a dock, the rail, a floating
  surface, a well, the bubble) takes one of three: `foreground` for content and the current
  location; `theme::meta_ink` for a detail beside a name on its line (its folder, its number, its
  kind); `muted_foreground`, still readable, for labels, counts, hints, placeholders and every
  navigation row but the current one. Text on a fill takes the ink paired with that fill: a
  button's variant and the library's controls set their own, a selected row `accent_foreground`,
  a badge its `Tint`'s ink. Meaning takes its semantic ink: a state `status_ink()`, a link or
  what runs `link`, code in prose `hue_ink`. No fourth neutral ink, and a quieter ink is a mixed
  step, never an alpha on the text or its container: see-through text reads differently on
  every fill under it.
- **The UI and mono font families ship with the app**, so text looks and centres the same everywhere;
  `[font].monospace` can still name another installed mono family.
- **Space** takes gpui's steps `0p5`, `1`, `1p5`, `2`, `3`, `4`, `6` and `8`; a named spacing
  constant equals one of them. Pick by relationship using `docs/ui-patterns.md`'s *Spacing and
  density*, not by what fits one screenshot. Text below a title shares its text axis, and icons,
  labels and buttons in one column share one axis down it.
- **Sizes are rems**, as per-panel zoom overrides the rem base; pixels only for lines, rings,
  shadows, bounds measured against the window, an inset cancelling one the library draws in
  pixels, and the terminal's grid, which zooms on its own. Fixed chrome heights stay outside the
  zoom wrapper. A size the steps lack is a named constant (shared ones in
  `onehand_plugin_host::tokens`, others beside their use), its reason in its doc comment.
- **Corners by role**: `radius_tokens().sm` for what is pressed, picked, typed into or lifted out by
  a fill (a control, a row such as the rail's selected project or session, a tab, a chip); the
  control wrappers set it. `radius` for a block read inside a column (a well, code, a diff, a
  notice). `radius_lg` for a box standing on its own (a page's box, a table, the bubble, the
  composer, the terminal, a floating panel, a dialog, a toast) and for a badge (a `Tag`).
  `rounded_full` for a dot. A part joined to its container's edge takes its corner there; the
  library's menus keep `radius`.
- **Lines** are a 1px `border`; the mark under an open tab is 2px.

## Colour and state

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `theme::dock_surface` | the Workbench and the terminal, a step off the reading surface |
| `muted` / `muted_foreground` | quiet fills (the prompt bubble, every well), meta text |
| `theme::meta_ink` | the middle ink: a detail beside a name, between prose and meta, contrast-tested |
| `border` | every hairline |
| `ring` | keyboard focus, drawn only when the keyboard moved it; the composer while a file is dragged over it |
| `accent` | the one item selected among several (a fill, not a hue) |
| `link` | the accent hue: what runs, and links |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view |
| `status_ink()` | danger, warning and success text |
| `popover` | floating surfaces |
| `theme::raised` | the composer card |
| `secondary` | inline code in the prompt bubble, a step off its fill; a document's code on a dock |

- Reading content is the visual anchor; rail and dock chrome remain quieter. A nested container
  inherits its surface unless it has a distinct role: well, selection or floating content.
- Boxes are borders; wells and the bubble are fills. Ordinary sections remain unboxed. Do not
  combine a border, contrasting fill and shadow just to make a group visible.
- Hover and selection are fills: distinct steps on a row (`list_hover`, `accent`), one step on a
  ghost button (the selected one). Keyboard focus is the ring.
- At most one primary in a view; a view needs none (the Settings page has none).
- The palette is neutral: surfaces and inks carry next to no hue, so a hue on screen means
  something. A missing surface or ink goes into the ramp with its contrast asserted, never into
  one view; a step between two the ramp holds is mixed from them perceptually (`mix_oklab`), so
  both modes derive it by one rule.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, plus gpui's own `list` for the transcript. Controls go through
the plugin host's builders, which set the pointer cursor and the `sm` corner: buttons through
`action`; fields through `text_input`, `search_input` (the search mark, the clear button; the rail's
alone has no edge, on `theme::inset`) and `text_area`. A button names its variant by role: `primary`
the main action, `secondary` an alternative beside it, a picker's trigger and every Settings button,
`choice` in a pick-one row, `danger` only under *Exceptions*, and ghost (`.quiet(cx)`, whose hover
is the selected step) the rest. **No button draws an edge** (no default variant, no `outline()`): a
field is the one control marked by its edge, on no fill of its own. A control that refuses says so
(`.refuses()`) and goes back to the arrow. Sizes: `.tall()` (`FIELD_H`, chrome-size words) in a
dialog's footer, a form and a page's head; `small` in a box or a strip; `xsmall` on a row, in a
transcript block and as a chip; never the library's default, which letters a button at the reading
size. The app owns only its own: the transcript renderers, the icon registry, the terminal panel,
per-panel zoom, and the composer popup (one shell for `@`, `/`, the pickers, the `+` and branch
menus and the attachment tray, with a pinned title and no key hints, at most six rows, grouped, then
how many more; a menu opens just above the control that opened it).

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a mode with a PTY of
its own per project. Files lives inside Editor, not as a mode. Plugins are built in.

## Icons

Every icon is an SVG from gpui-component's `IconName`. `crate::icons` holds only what that set
cannot draw (shapes it has no drawing of, brand marks, and forks of bundled shapes for stroke
weight), synced from `assets/icons/manifest.toml` by `scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
Equivalent icon roles use the same library size through a shared builder; optical corrections
belong there. Add an icon only when it helps recognition or names an action without visible text;
a word reads faster than a shape nobody has learned. An icon beside text is no taller than that
text's line and sits on its centre; icons down a column share one slot width, so the labels after
them start on one edge. No icon sits on a tile of its own; a fill behind one belongs to the
control or row it is part of.

## Exceptions

The whole list: any other departure from a rule above is a bug until agreed and listed here.

- **The composer card has no edge.** It stands off the page on its own fill (`theme::raised`) and
  its own lift (`composer_lift`).
- **A solid danger fill marks two buttons**: *Stop* in the composer, and the button confirming a
  removal inside its modal. Anywhere else a destructive action is a word in the danger tint.
