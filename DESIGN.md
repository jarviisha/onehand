# DESIGN.md: onehand UI contract

The whole-app UI contract for `onehand`: the principles every view follows, the window's frame,
the transcript and the visual system, as structure and behaviour, not values (the palette is the
component library's own: *Colour and state*). It binds at the level written here; below
it, the code and its tests decide. `docs/screens.md` says what each region holds and in what
order, a feature's own document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) its
screens' detail, and `docs/ui-patterns.md` the patterns every screen repeats and how they behave.
Code never cites this file.

This contract defines onehand's own visual language: quiet navigation, clear content hierarchy,
compact controls and predictable alignment. It keeps no standing exceptions: a treatment that
differs is a rule of the role it belongs to, written where that role is. A departure is a bug,
put right or recorded in `docs/known-gaps.md` with its reason within the change that makes it.

**The palette is gpui-kit's Default Light and Default Dark** (*Colour and state*), as the
component library ships them, the primary alone made blue (`crate::theme::install`, with its
contrast tests). Every colour,
radius and size is a token read at the call site (see `.claude/skills/ui/tokens.md`);
a hex literal is a bug even when it looks right, as no theme switch reaches it.

## Principles

1. **The conversation is the centre while a session shows**, the only region that flexes. Docks
   start closed and open on demand. Beside the Workbench it keeps a minimum, times its zoom; too
   narrow for both, the Workbench takes the area under *← Conversation*, which steps it aside. Pages
   take the agent pane and put the docks away; a page opens over a session only when picked.
2. **Content leads; navigation recedes.** Neutral surfaces carry the layout. Hue identifies a
   state, a link, a label's kind or code in prose, never a decorative section background.
   Focus is a grey, as the library's palette has it; the primary action is the palette's one blue. Attention is earned: the signals that call for the person
   (a state's hue, a count or badge of what waits, a dot, weight past what the role takes) mark
   only what needs them or has just changed, so a view at rest is calm. Hierarchy and identity are not attention: a heading keeps
   its weight, a list its muted count, a label its kind's hue. Nor is a mode switched on: the
   control that switches it may carry a state's hue on its own mark while the mode is on, and
   nowhere else. All colours are theme tokens with
   legible contrast on their actual surfaces in both modes.
3. **Mono for machines, sans for people.** Code, paths, terminal output and diffs use
   `mono_font_family`. Anything a person wrote uses the default family.
4. **Same role, same treatment.** Across app and feature crates, equivalent rows, headers and controls
   share size, spacing, corners and states. A valid token alone does not make a consistent view.
5. **Say nothing twice.** If a fact is already on screen near what it describes, don't repeat it
   in a badge, a status bar or a title.
6. **Bounded, and say so.** Every list has a named cap, and when the cap cuts something off, the
   screen says how many were left out.
7. **Calm and immediate.** What a person changes shows at once, and the work behind it follows.
   Nothing moves for show: the one motion is a `Spinner` on work still running, beside what the
   library's own overlays and controls do (*Components*). `docs/ui-patterns.md` (*Immediate feedback*, *Motion*) holds how.

## Layout

One window hosts one workspace. A title bar holding the app's menus runs along the top and a
status bar along the foot; between them the Workbench runs the full height and the terminal sits
under the agent pane only. The diagram is plain ASCII and box-drawing, so it stays aligned in any
mono font.

```
┌──────────────────────────────────────────────────────────────────────────┐
│ File  View  Go  Session                                        _  []  x  │
├──────────────────┬────────────────────────────────────┬──────────────────┤
│ W Workspace   v <│ Title (...)    past close workers  │ modes   max hide │
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
│ Settings         │ terminal (bottom dock)             │                  │
├──────────────────┴────────────────────────────────────┴──────────────────┤
│ rail  branch                                          terminal  wb       │
└──────────────────────────────────────────────────────────────────────────┘
        rail                     DockArea
```

What each region holds, and in what order, is `docs/screens.md`'s, under the region's name:

- **Title bar**: the app's menus, and the window's own controls where the app decorates it.
- **Rail**: app chrome outside the dock, from the workspace bar down to *Settings*.
- **Agent pane header**: the conversation's name, its menu and the session's controls; chrome alone over
  a page that names itself in its head.
- **Step strip**: a run's steps under the header, while it drives the session on screen.
- **Composer**: the input surface under the transcript, and what waits pinned above it.
- **Docks**: the Workbench and the terminal, bare panels flush with their docks, each opening on a tab bar; a lead's
  workers' region takes the Workbench's place, never beside it.
- **Pages without a session**: a project's page, the overview, Tasks, Issues and Workflows.
- **Settings**: a page of its own, a nav column beside the sub-page it shows.
- **Status bar**: the rail's toggle and the branch, then the dock toggles.
- **Persistence**: what the layout saves.
- **Nothing else in the title or status bar, and no right toolbar.** Transient status goes in a
  toast; modals are `Dialog`s.

## Transcript

- **Two sides.** The user's prompt is the library's `Muted` bubble (a grey fill, the `2xl`
  corner), against the right edge. Everything the agent produces starts on a shared left axis
  across the column, as the library's ghost bubble draws it. Nothing else is right-aligned.
- **A centred reading column**, narrowed on small panels, read at the reading size; what is
  wider scrolls inside its well. A scrollbar runs on the panel's edge down to the composer.
- **One turn, many blocks**: prose, thoughts, plans, activity clusters (tools only), commands,
  diffs, a subagent's card (a fold block where the agent handed work off), answered
  permissions and questions (a line each, those in a row sharing one hairline edge with no fill),
  notices (a remark is a divider: the words centred between two hairlines across the column, in
  the meta ink) and errors (the neutral card, flat: the alert mark in the danger ink beside the
  words, *Turn failed* over them when a turn ended on it, the restart key for a lost agent), the files the turn changed,
  and, while it runs, the running line (a spinner, its clock, *N running*, what the agent says it
  is doing). A workflow's steps and an interrupted turn are notices, never prompts.
- **Folding blocks open from one line**, the chevron first, with no box: no edge, no fill and no
  rule beside what they hold, which sits indented under the line's words (a cluster, a thought, a plan, a subagent, a turn's changes). What runs is open until done, the
  plan stays open, and a reader's fold wins. Long output folds and is capped, an opened cluster
  too, each saying what it holds back. Wells are filled with no edge, their text at
  `CODE_TEXT`. A finished turn ends on Copy (its closing paragraph) and how long it took, said as
  *Failed after …* where the turn ended on a failure.
- **Its rows are lines, not list rows**: a fold line or an activity row is as tall as its text,
  answers the pointer by raising its ink rather than with a fill, leads with an `.xsmall()` mark
  in one slot (a spinner while it runs, so nothing moves as it settles), and takes no focus. A
  path or a command is mono; the agent's own wording, a folder and what the agent did (a
  thought, a command's output, a read or search list) read in the middle ink. One copy control
  (`.small()`, quiet, named in its tooltip, in a column of its own; a permission's on the line
  under its command, beside where it runs; a prompt's under its bubble, with *Fork from here* beside it, shown while
  the prompt is pointed at or a control holds focus) and one show-more control serve every block. Text
  that scrolls keeps a gutter for its scrollbar.
- **A table keeps its columns readable**: each is as wide as its text down to a floor, and past
  the column the table scrolls sideways in its own track while the vertical wheel stays the
  conversation's. Under every table, Copy (as Markdown) and *Open*, which shows it whole in a
  dialog as wide as the window allows.
- **Calm once it is past**: a failure a later prompt has moved past keeps its mark, title and
  words in the muted ink; a settled *N failed* drops the danger ink; a refusal's badge is
  neutral. An error, and the failure a turn ended on, is the waiting cards' neutral shell, flat
  and on the page's fill: the alert mark in the danger ink, *Turn failed* in it too where a turn
  ended on it, then the words in the body ink, what it attached folded under *Show details* in mono; a lost agent's names the
  key that restarts it. A failed turn holds until a prompt goes out; a restart does not answer it. An
  error that failed no turn (one answering a *Stop*, a resume that fell back) is past at once.
- **The pinned cards** (a permission, a question) share one neutral shell: an edge in `input` at
  `radius`, no wash, the tone only in the mark and the title (a permission's warning ink). While
  they wait they stand a step off the page (`theme::waiting_card`), lifted, as wide as the
  composer, their foot tucked under it (`TUCK`) so the composer reads as laid over them, without
  motion; the ones waiting behind peek over the shown one's top edge as slivers, at most two,
  and its head says *1 of N waiting*. Back in the history the shell is on the page's fill and
  flat. It holds a head, the body, the answers at the end at `.short()` (a hairline under the head,
  padded as a box whose body scrolls, its cut end fading out while more lies below), the
  dismissing one a quiet ghost. A choice is a bare row at rest, on the pointer's fill under the
  pointer or the keys, and on the selected step when taken, a radio or checkbox drawn in ink at
  its head (the library's rings vanish on the selected step) and its number with Alt as a `Kbd`, at least
  a field's height; a typed answer sits under the list, with the answers and no line between them,
  as a field with its `Kbd`: edged in `input` on the card's own fill, two
  lines tall at rest and growing as it is written (Shift+Enter a newline), ringed while the keys
  are on it. A permission's command sits in a well on the page's fill, a step below the card. Each answer in a pinned card's footer shows its key beside its words, always in
  sight, as that key reaches the card from anywhere in the conversation.
  The card holds the one primary while it shows. A question card always ends on *Skip*; one
  asking a single pick-one question answers on the pick, its primary only for a typed answer.

## Type, space and shape

| Role | Size |
|---|---|
| Meta, hints, counts, a small label | `.text_xs()`, in `muted_foreground` when it is meta |
| A caption inside a small object (an attachment's plate) | `MICRO_TEXT` (11px), a step under the meta size |
| Machine text | `CODE_TEXT`, in `mono_font_family` |
| Chrome: a panel's rows, boxes, forms and strips, a menu's rows, a `.tall()` control's words | `CHROME_TEXT` (13px), a step under the library's control size |
| A control's words (`.short()`) | `.text_xs()` |
| Reading text: the transcript, the composer's field, a document | 13px on the reading leading: the transcript's `TEXT`, `CHROME_TEXT` elsewhere |
| The transcript's record: activity, thoughts, notices, a card's question | a step under the reading size, the transcript's `TEXT_SM` (12px) |
| A heading in reading text | a step up the scale per level, two at most; then the body's size |
| A page's or an issue's title | `.text_lg()` (18px) |

- **Weight carries hierarchy before size does**: `font_medium` for a name in a header,
  `font_semibold` for a page's title and any other title or heading; nothing bolder. A list page's rows,
  at the meta size, carry their name and the column read down the list in `font_medium`
  (`docs/ui-patterns.md`, *List pages*). Line height is gpui's,
  but `LEADING` for reading text and `LEADING_TIGHT` for a wrapped title.
- **Inks by role.** Text on a neutral surface (the reading surface, a dock, the rail, a floating
  surface, a well) takes one of three: `foreground` for content and the current
  location; `theme::meta_ink` for a detail beside a name on its line (its folder, its number, its
  kind); `muted_foreground`, still readable, for labels, counts, hints, placeholders and a row's
  second line. Text on a fill takes the ink paired with that fill: a
  button's variant and the library's controls set their own, a selected row `accent_foreground`,
  a badge its `Tint`'s ink. Meaning takes its semantic ink: a state `status_ink()`, a link `link`,
  what runs `info`, code in prose `hue_ink`. No fourth neutral ink, and a quieter ink is a mixed
  step, never an alpha on the text or its container: see-through text reads differently on
  every fill under it.
- **The UI and mono font families ship with the app**, so text looks and centres the same everywhere;
  `[font].monospace` can still name another installed mono family.
- **Space** takes gpui's steps `0p5`, `1`, `1p5`, `2`, `3`, `4`, `6` and `8`; a named spacing
  constant equals one of them. Pick by relationship using `docs/ui-patterns.md`'s *Spacing and
  density*, not by what fits one screenshot. Text below a title shares its text axis, and icons,
  labels and buttons in one column share one axis down it.
- **Sizes are rems**, as per-panel zoom overrides the rem base. Pixels are for what zoom must
  not reach: a stroke (a line, a ring, a mark), a shadow, a bound measured against the window, an
  inset cancelling one the library draws in pixels, and a grid that zooms on its own (the
  terminal's). Fixed chrome heights stay outside the
  zoom wrapper. A size the steps lack is a named constant (shared ones in
  `onehand_ui::tokens`, others beside their use), its reason in its doc comment.
- **The library's corners.** What a person presses or what holds content takes `radius`, 6px:
  a control, a field, a row, a chip, a well, a box on a page (`card_box`). A dock and its tab bar's tabs stand square, flush with the dock. A large surface takes
  `radius_lg`, 8px: a floating panel, a dialog, a toast, and a Settings group; the library's own
  follow. The composer and a pinned card take `radius`, so the card tucked under the composer
  meets it at one corner. A badge (a `Tag`) takes the library's
  own corner and size, its edge halfway from its fill to its ink (`badge_edge`); inside a line of
  meta text a label is written in its kind's ink instead, unframed. A dot or a status mark is
  `rounded_full`. The library's small parts keep its half
  corner (*Components*). A part joined to its container's edge takes its corner there.
- **Lines** part regions and are a 1px `border`. A **mark** says where the person is, not where
  a region ends: the stroke of the primary along an open tab's top is 2px, a step heavier than a
  line. A list page's line over its foot is a step fainter (`faint_rule`), so the edge-to-edge
  list reads as one surface; a page's top draws no line under it. A box on a page takes a step
  stronger edge, a control's (`input`); a folding block and a Settings group keep the hairline.

## Colour and state

The palette is the component library's own: gpui-kit's **Default Light** and **Default Dark**,
as the library ships them (`default-theme.json`). `crate::theme::install` takes both configs and
changes no colour in them, so a library part and the app's own parts read one palette. A role
takes the library token named for it; a step no token holds is derived in `crate::theme` or
`onehand_ui::ink` from the tokens, never written as a value. The terminal's sixteen colours are
not the library's: they stay VS Code's (`onehand_terminal_ui::ansi`), as a program's colours are
its own. A chart's series and a label's kind take the library's base hues.

The library's surfaces are neutral greys a step apart and its focus ring is a grey. Its primary,
near black on the light palette and near white on the dark one, outweighed every row it stood in,
so the app overrides that one colour: the kit's own `blue-600` in both modes, darker under the
pointer and pressed (`theme::install`). On the dark palette its
surfaces are near black, so a floating surface is told apart by its edge rather than its shadow.
Pairs under WCAG AA are in `docs/known-gaps.md`; `crate::theme`'s tests hold each at what it
reaches.

| Token | Use |
|---|---|
| `background` / `foreground` | surface and text |
| `theme::dock_surface` | the Workbench and the terminal, a step off the reading surface |
| `muted` / `muted_foreground` | quiet fills (every well), meta text |
| `theme::meta_ink` | the middle ink: a detail beside a name, between prose and meta, contrast-tested |
| `border` | every hairline, a folding block's and a Settings group's edge; a box on a page takes `input` |
| `ring` | keyboard focus, drawn only when the keyboard moved it; the composer while a file is dragged over it |
| `accent` | the one item selected among several (a fill, not a hue) |
| `link` | a link: the library draws it in the prose ink, underlined |
| `info` | what runs |
| `list_hover` | hover on a pickable row |
| `primary` | the single primary action in a view, an open tab's mark; blue, the palette's one override |
| `status_ink()` | danger, warning and success text, and `closed` for what is over and settled (a closed issue) |
| `popover` | floating surfaces |
| `theme::raised` | the composer card |
| `secondary` | a document's code on a dock |

- Reading content is the visual anchor; rail and dock chrome remain quieter. A nested container
  inherits its surface unless it has a distinct role: well, selection or floating content.
- Boxes are borders; wells and the bubbles are fills. Ordinary sections remain unboxed. Do not
  combine a border, contrasting fill and shadow just to make a group visible.
- Hover and selection are fills: distinct steps on a row (`list_hover`, `accent`; on the rail's side bar surface its
  own pair, `sidebar_accent` and `rail_chosen`, as a fill is a step from the surface under it), and the library's own hover on a ghost button. A secondary button rests on a fill of its own and takes
  its pointed-at step under the pointer; the selected step is a chosen row's. Keyboard
  focus on a control is the ring. A field is edged at rest and ringed on focus, as the library
  draws it; the composer, the input surface, shows focus by its caret (*Components*).
- At most one primary in a view; a view needs none (the Settings page has none). Where layers
  stack, what waits on the person holds it while it shows (a floating issue, an open form, a
  pinned card, a run's approval), and what it covers steps down to `secondary`.
- Surfaces and inks are greys, so a hue on screen means something: a state, a link, a label's
  kind, or code in prose. A missing surface or
  ink is derived from the library's tokens with its contrast asserted, never a value in one
  view; a step between two tokens is mixed from them perceptually (`mix_oklab`), so both modes
  derive it by one rule.

## Components

Reuse gpui-component before building anything: `Root`, `DockArea`, `Sidebar`, `Dialog`, `Switch`,
`InputState`, `Editor`, `TextView`, plus gpui's own `list` for the transcript. Controls go through
the UI crate's builders, which set the pointer cursor and the library's corner (`radius`): buttons through
`action`; fields through `text_input`, `search_input` and `text_area`. **A field is the
library's**: edged in `input` on the page's fill and ringed on focus, in a form, a dialog, a
page's top and a search alike; a search adds the search mark and the clear button, and the
composer's field is its card's. A button names its variant by role, each drawn as the library
draws it: `primary` the one commitment in a decision area, `secondary` (edged, on its own fill)
an ordinary action beside it, a set picker's trigger (an unset one is a ghost, through `picker`),
every Settings button and the rail's *New*; `outline` an action that needs a boundary with less
weight; `choice` in a pick-one row; `danger` as below; and ghost (`.quiet(cx)`) the familiar,
low-emphasis commands of a toolbar, a strip or a row. A control that refuses says so and why
(`.refuses(why)`, the reason its tooltip) and goes back to the arrow.

**A ghost never starts a line alone.** Without a fill, a ghost's padding sets its words in from
the edge the text above and below it starts on, and nothing drawn says where it begins. A ghost at
the start of its line has something before it on that line; one that would stand there alone
takes the secondary fill instead (a form's picker, a fold toggle). A ghost ending a line or
centred on it may stand alone.

**A ghost is known by where it stands**: in a strip, a toolbar, a footer, a row's action slot,
beside other controls. Beside a sentence (a preflight finding, a notice, a status line) it reads
as more of the text, so an action there takes the secondary fill (*Check the agent*, *Retry…*).

**An action with a menu of alternatives is one control.** The action and its caret half are joined
edge to edge, one variant and one height, the inner corners square, parted by a seam in the colour of the surface
under them (`split_action`); a caret never
stands as a button apart beside the action it qualifies. A caret otherwise ends a picker or a tab,
after its words.

**Danger is a word, and a fill only to end or confirm.** A destructive action is a word in the
danger tint. The solid `danger` fill marks two buttons: *Stop* in the composer while a turn runs,
and the button confirming a removal inside its modal. Nothing that destroys nothing takes the
danger ink.

**The composer is the input surface**, a role of its own rather than a box or a field: a card
edged in `input` at `radius`, standing off the page on its own fill (`theme::raised`) and a light
lift (`composer_lift`), showing focus by the caret alone; what waits on it (the workers' line, the
strip of queued and staged things, over any waiting card) takes the same edge. Its pickers (the model, the effort, the mode, *Fast*, the
branch) are `.short()` ghosts with a caret whatever they hold (*Fast* is its bolt alone, lit in
the warning ink while fast, as a mode switched on is, and as a toggle opens no menu and carries none), where a picker elsewhere takes
the secondary fill once set: each always holds a choice, and a fill on every chip would crowd
the field they sit under.

**The library's own parts are the library's.** Inside a gpui-component control or overlay, what
it draws itself stays as it draws it: a refusing or loading button's alpha, a picker's caret, a
badge under the pointer, a switch's thumb and the motion of its overlays; the `sm` radius, half
the one corner, of its small parts drawn inside a control (a `Kbd`, a checkbox, its own tabs),
where a full corner would round them to a pill; and an open dropdown's trigger on its pressed
fill, in the ink the caller gave it. The app corrects the
library only through a shared wrapper: where the library's mark vanishes on a fill the app
draws on (a radio's or a checkbox's ring on the selected step), `choice_mark` draws it in ink.

**A size names a role, and the role follows the room the control stands in.** A control takes
one of the library's two sizes. On a page, where there is room and the person reads and types
what a control holds, it takes `.tall()` (26px; a button is the library's small held at `FIELD_H`, its words at `CHROME_TEXT`): a page's head and a page's
form, its fields, pickers and buttons alike, so a field's words stand at its title's size; a
dialog holding a form at `DIALOG_FORM` is that room too. In dense chrome (the rail, a dock, a
strip, a dock's filter bar, a dialog that asks one thing, a row, a box's title row, the composer) it takes
`.short()` (22px; a button is the library's extra small, a field its small, held at `CONTROL_H`), so it never outweighs what it acts on. An icon alone is
square at its row's height: `.small()` beside `.short()` peers, `.tall()` beside tall ones.
`.tall()` (`FIELD_H`) is the ceiling: no control stands taller.
**What a small object holds scales with the object.** Inside a fixed object smaller than a row of
controls (an attachment's plate, a thumbnail), an icon-only control takes `.xsmall()` and a caption
`MICRO_TEXT`, so neither crowds what it sits on; anywhere else `.short()`, `.small()` and the meta
size are the floor, and a worded control never takes `.xsmall()`, whose words run to its edges.
Every control in one row of actions stands at one height. A correction a component needs lives
in its shared wrapper (`Short`, `Tall`), never at a call site. `docs/ui-patterns.md` (*Control
sizes*) maps each role to its component and size.

The app owns only its own: the transcript renderers, the icon registry, the terminal panel,
per-panel zoom, and the composer popup (one shell for `@`, `/`, the pickers, the `+` and branch
menus and the attachment tray, with a pinned title and no key hints, at most ten rows, grouped, then
how many more; a menu opens just above the control that opened it).

Scope: the Workbench editor is a quick editor (tree-sitter, no LSP). Neovim is a mode with a PTY of
its own per project. Files lives inside Editor, not as a mode. Every mode is built in.

## Icons

Every icon is an SVG from gpui-component's `IconName`, the library's default subset of Lucide.
`crate::icons` holds only what that set
cannot draw: shapes it has no drawing of, picked from the kit's complete Lucide catalog, and brand
marks and forks of bundled shapes for stroke weight, synced from `assets/icons/manifest.toml` by
`scripts/sync-icons.sh`. Where the bundled
set lacks a shape, an approximate icon is accepted. An icon at rest is `muted_foreground`. One that
carries state uses a semantic token. One that sits beside text takes that text's colour.
Equivalent icon roles use the same library size through a shared builder; optical corrections
belong there. Add an icon only when it helps recognition or names an action without visible text;
a word reads faster than a shape nobody has learned. An icon beside text is no taller than that
text's line and sits on its centre; icons down a column share one slot width, so the labels after
them start on one edge. No icon sits on a tile of its own; a fill behind one belongs to the
control or row it is part of.
