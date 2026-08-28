# DESIGN.md — UI constraints

The whole-app visual contract for `onehand`, held to by the render layer. The
traffic runs one way: this file points at code, and code never points back — a
source comment states its reason in its own words rather than naming a section
here, and a test enforces it. The transcript's own design language is
[DESIGN-ANSWER.md](DESIGN-ANSWER.md).

> **This file no longer carries a palette.** Until the GPUI migration it mirrored
> a hand-built token set into `theme.rs`, and the two had to be kept in sync by
> hand. Decision **D1** (DECISIONS.md §1) ended that: onehand uses
> gpui-component's theme as-is. So the rule here is not "these are the values"
> but **"never write a value"** — every color, radius and font size is read from
> `cx.theme()` at the call site. A hex literal in the render layer is a bug even
> when it looks right, because it is the one thing a theme switch cannot reach.

Guiding principles:

1. **Separate by hairline, not shadow.** Panels split from their neighbours by a
   1px `cx.theme().border`. Shadows belong to genuinely *floating* surfaces —
   dialogs, popovers, the completion popup.
2. **Chat is the centre.** The conversation is the dock's centre panel and the
   only region that flexes. Workbench and terminal are docks: closed by default,
   opened on demand, never crowding the conversation.
3. **Accent restraint.** One accent, from the theme. Semantic color marks
   *state* — adaptive danger ink for failure, warning ink for in-flight, and
   success ink for done.
   If a color is not carrying meaning, it is `muted_foreground`.
4. **Mono for machines, sans for people.** Code, paths, terminal output and
   diffs use `cx.theme().mono_font_family`; everything a human wrote is the
   default family.
5. **Icons are registry SVGs, never glyphs.** No `＋`, `●`, `✓`, `×`, `❯`, `⚙` in
   rendered UI (§6).

---

## §1 — Layout

One window hosts exactly one workspace. The frame is a navigation **rail** plus a
**`DockArea`**:

```
┌────────────────┬──────────────────────────────┬───────────────┐
│ workspace      │ title ⌄ · status   ⌕ ▣ ▤ ✕   │               │
│ + New session  │                              │   Workbench   │
│                │        agent pane            │  (right dock, │
│ PROJECTS       │      (centre panel)          │   closed by   │
│  project       │                              │    default)   │
│   session      │      ┌── composer ──┐        │               │
│   session      │      └──────────────┘        │               │
│                ├──────────────────────────────┴───────────────┤
│ ⚙ settings     │        terminal (bottom dock, closed)        │
├────────────────┴──────────────────────────────────────────────┤
│ project · branch · agent                    unsaved · 120%    │
└───────────────────────────────────────────────────────────────┘
     rail                        DockArea
```

- The **agent pane's header** is the row above the transcript, and it is split by
  what a control is *about*. **The conversation's name is the menu**: it is the
  one thing on the row drawn in full ink and weight, and pressing it opens
  everything done to the conversation — rename, the exports, resume another,
  restart, and, alone in the danger tint, delete. Its hover brings a background
  and a chevron whose space is held either way, so the name does not shift under
  the pointer about to press it. Beside it, a **badge** carries what the session
  is doing: the rail's own signal mark plus a few words, colour in the mark and
  the words muted, so a routine *Working…* is not as loud as a dead agent.
  A hairline under the row separates the chrome from the conversation.
  The right-hand end carries what is about the **window**, a size up and a tone
  down — big enough to aim at, muted enough not to out-shout the name: find, the
  terminal, the Workbench, the way back to a hidden rail, and last *Close
  session*. There is no `⋯`; a menu button beside the name it acts on says
  nothing the name could not say itself.
  **The agent pane is mounted as a bare panel, not a tab group**, so this is the
  only chrome it has: one tab that can never gain a sibling is not a tab, it is
  the conversation's own name printed a second time directly above the header
  that says it. Every way back to something the window has put away is therefore
  offered from here. The rail's button appears only while the rail is gone,
  because a button that unhides what is already on screen does nothing; the two
  docks' buttons stay, following the same three-state rule as their keys. The
  Workbench keeps its tab group, because it holds several tabs and switching is
  what a tab is for.
- **A project with no conversation open gets a page, not a sentence.** Selecting
  a project that has no session — every freshly added one, and any whose last
  session was closed — fills the centre with that project's name, a *New
  session* button, and the conversations already had in it, newest first and
  across every agent. Picking one starts a session on the agent that held it and
  resumes it. The list is capped and says how many older ones it left out, and
  it distinguishes *still looking* from *none yet*: a project of a hundred
  conversations must not be told it has none for the half-second a directory
  read takes. One line of grey text saying *Start a session in X* was the first
  thing a new user saw and the one screen in the app with nothing to press.
  Each row also carries the one way to **delete** a conversation, and it is a
  word in the danger tint rather than an icon: everything else this app offers
  can be done again, this cannot, and a destructive control should be read
  rather than recognized. It arms on the first press and says so, and it is
  offered here and nowhere else — this page is what shows when a project has no
  session on it, so every row on it is a conversation nothing is writing to.
- The **rail** is app chrome, not a panel: it lives outside the dock, so the dock
  cannot swallow it and a layout restore cannot lose it. `Ctrl+Shift+B` **hides
  it entirely** — it is never narrowed to an icon column, because at that width
  every project is the same folder icon and the one thing the rail is for
  (which project, which session) is exactly what it can no longer say. The way
  back is the sidebar button in the agent panel's header, offered only while
  the rail is gone. It **is** drag-resizable, between 232 and 320px: narrower
  and its rows say nothing, wider and it is taking the conversation's space to
  show padding. The width is remembered per workspace; whether it is showing is
  not.
- **Everything else is a dock panel**, and the arrangement persists as **five
  values, not the library's `DockAreaState`**: Workbench width, terminal height,
  whether each is open, and the rail's width. `DockAreaState` is serde and would
  be the obvious thing to store, but *restoring* one rebuilds every panel through
  a process-global registry — which would leave the shell holding handles to
  orphans and could not tell two windows' panels apart. The arrangement here is
  fixed by design, so what a user actually changes is those five numbers.
- **Docks open on demand.** Both the Workbench and the terminal start closed. A
  panel shortcut is three-state: closed opens and focuses, open-but-unfocused
  focuses, open-and-focused closes.
- **The terminal's open/closed state belongs to the project, not the window.**
  Its tabs, its shells and its working directory are all per root and none of
  them follow the selection, so a dock left open across a project switch showed
  the arriving project an empty panel where the previous one's shells had been —
  which reads as the terminal having lost them. Switching files the live state
  under the project being left and restores whatever the arriving one was left
  in; a project it has never been opened in gets it closed, because inheriting
  *open* just reproduces the empty panel one project further along.
  **The Workbench deliberately does not follow this rule.** Its state is per root
  too, but every root has a file tree, so an open Workbench after a switch is
  never empty — there is nothing there to misread.
- **A terminal that is not showing occupies nothing.** It is *mounted and
  unmounted*, not opened and closed: a closed bottom dock still draws a strip of
  title bar, because the library puts the button that reopens it there, and this
  terminal has no such button — leaving a bare band of chrome across the bottom
  of every window in every project, naming nothing and reopening nothing. The
  ways back are `` Ctrl+` `` and the terminal button in the agent pane's
  header — which carries a dot while a shell is alive, since a child process
  outliving a closed dock is the one thing the icon cannot say.
- **The terminal has no library tab bar either.** Its several tabs are its own,
  drawn inside the panel with the shell labels, their ✕ and the `+`; a tab group
  around it held one panel that could never gain a sibling and printed
  *Terminal* over the strip that already names every shell. Like the agent pane
  it is a bare `DockItem::panel`.
- **Maximize has two directions.** The *content* direction is the dock's own zoom
  (the button in a panel's tab bar) and keeps the rail. The *app* direction is
  `Ctrl+Shift+K` and hides the rail too — the rail is what tells them apart.
  Only the Workbench offers the content direction: the agent pane and the
  terminal are mounted bare and have no tab bar to put the button on, and the
  conversation already fills everything right of the rail whenever both docks
  are closed.
- **No global top bar and no right toolbar.** Transient status is a toast;
  modals are `Dialog`s (workspace settings, agent manager, help).
- **One row of chrome along the bottom: the status bar.** App chrome like the
  rail, outside the dock and under both it and the rail, and gone with the rail
  when a panel is maximized in the app direction. It carries **only what nothing
  else on screen carries** — the conversation's own name and what it is doing
  are the agent pane header's and are not repeated here. What is left is either
  invisible while the rail is hidden (the active project, its branch and change
  count, the running agent and its one signal) or invisible everywhere (how many
  open buffers are unsaved, and any panel left at something other than 100%).
  The terminal is **not** here: the two docks the conversation sits between are
  one decision, so both are offered from the panel that gives up the space.
  A cell shows the pointer and lights on hover **iff** pressing it does
  something: the project copies its path, the git cell re-reads status, the
  unsaved count opens the editor. The agent cell is a reading and is drawn flat. Zoom is read from the panels themselves
  rather than from whichever holds focus, because focus moves without telling
  the window and a stale factor has nothing on screen to admit it.
  The session signal is drawn through the rail's own mark: one condition, one
  shape, decided in one place.

---

## §2 — Typography

Two families, both from the theme: the default UI family, and
`cx.theme().mono_font_family` for anything a machine produced.

| Role | How to write it |
|------|-----------------|
| Body / prose | the inherited size — do not set one |
| Chrome — a panel's own rows, cards and controls | `.text_sm()` |
| Headings, titles | `.font_semibold()`, at the size of whatever they title |
| Meta, status, hints | `.text_xs()` + `.text_color(cx.theme().muted_foreground)` |
| Code, diffs, paths, terminal | `.font_family(cx.theme().mono_font_family.clone())` |

**A title is its body's size in bold, not a size of its own.** Weight is what
separates a name from the thing it names; a title that also steps up is two
signals for one distinction, and it is how a ladder grows a rung every time
someone needs a heading to feel slightly more important than the last one.

**Sizes are rems, never pixels.** This is what makes per-panel zoom work: zoom
overrides the *rem base* for one panel's subtree (`crate::zoom`), so everything
sized in rems scales together and a `px(13.)` written by hand does not. A fixed
pixel size is how a panel ends up with one label stranded at its original size
beside doubled body text.

**A borrowed component's pixel size is the same bug arriving from outside.**
gpui-component sizes some of what it draws from `Theme::mono_font_size`, which
is pixels, so those parts sit still while the panel around them zooms. Where
the component takes a style refinement, the fix is a rem size written at the
call site — the refinement is applied after the component's own. Where it takes
none, the size has to be handed in from the current rem size at render time.

Weight carries hierarchy before size does. Three sizes and two weights read as
one system; five sizes read as an accident.

---

## §3 — Dimensions & spacing

- **Spacing is gpui's base-4 scale** — `p_1` `p_2` `p_3` `p_4`, `gap_1` … Snap to
  it; a one-off `px(7.)` is noise no one will ever notice missing.
- **Radius is `cx.theme().radius`**, and its derivations for larger surfaces.
  Circles (`rounded_full`) are for avatars, status dots and true pills only.
- **Hairlines are `border_1` + `cx.theme().border`.** Not a shade of the
  background, not a shadow.
- Fixed chrome heights (tab strips, headers) stay outside the zoom wrapper, so
  they hold still while content scales.

---

## §4 — Color & state

Read the theme. The tokens this app leans on:

| Token | Use |
|-------|-----|
| `background` / `foreground` | the surface and its text |
| `muted` / `muted_foreground` | quiet fills; meta text, descriptors, hints |
| `border` | every hairline — the primary separator |
| `accent` / `accent_foreground` | the one item selected among several |
| `list_hover` | hover on a row or chip that is there to be picked |
| `primary` / `primary_hover` / `primary_foreground` | the single primary action in a view |
| `danger` / `warning` / `success` | status fills and borders |
| `status_ink().danger` | failure, destructive text, removed diff lines |
| `status_ink().warning` | in-flight text and "needs attention" |
| `status_ink().success` | completed text and added diff lines |
| `popover` | floating surfaces (menus, the completion popup) |
| `sidebar_accent` / `sidebar_accent_foreground` | the rail's selected row |

The surfaces above — `background`, `muted`, `secondary`, `accent`, `popover`,
`border` — and the greys drawn on them are the app's own, set once at boot as a
pair of overrides on the component library's configs. Every other value is the
library's. A call site never needs to know which is which: it reads the token.

Rules that outlive any particular theme:

- **State, not decoration.** A color must mean something. Three colors on screen
  that each mean nothing is worse than one that means "this failed".
- **One primary per view.** If two buttons are primary, neither is.
- **Cards are borders, not fills.** Depth comes from a hairline and padding. A
  lighter block inside a lighter block inside a lighter block is a hierarchy
  nobody can read.
- **Nothing is ringed — a state is a fill.** Neither hover nor selection draws a
  border, and the library's own list highlight is turned off to match. What
  separates them is which fill: hover is the faintest step in the ramp,
  selection a clear stage past it. Both are asserted against each other, because
  a row can be hovered *and* selected and the two must not read alike. A rule
  around a row costs the row width it has to reserve at rest, and a ring on
  hover makes the pointer resting somewhere look like a decision.
- **Never hard-code.** Not at a call site, not even for a colour the theme
  happens to lack. If a surface is genuinely missing, it belongs in the ramp
  that boot installs, with the contrast it owes its neighbours asserted — not
  written into the one view that noticed.

---

## §5 — Components

**Reuse gpui-component before building anything.** It is the reason the port was
worth doing, and every hand-rolled equivalent is a widget that will not follow
the theme, will not follow the focus rules, and will have to be maintained here:

| Need | Use |
|------|-----|
| Window frame, docks, panels | `Root`, `DockArea`, `Panel`, `DockItem` |
| The rail | `Sidebar` |
| Buttons, ghost/primary variants | `Button` + `ButtonVariants` |
| Modals | `Dialog` |
| Single-line and multi-line input | `InputState` + `TextInput` / `Textarea` |
| The file editor | `EditorState` + `Editor` (tree-sitter, no LSP — D3) |
| Markdown | `TextView` + `TextViewState` |
| Long lists | `list` / `virtual_list` — never a `div` per row over an unbounded set |

**Anything that acts on a click shows the pointer.** The library draws every
button variant but `link` and `text` with the arrow, which is a form's
convention; this app's rows, chips, tabs and candidates are hand-made and show a
pointer, and half the actions on screen answering the cursor while the other
half do not leaves the cursor meaning nothing — the only way left to learn what
is clickable is to click it. So buttons are built through the app's own action
wrapper, which overrides that one property and nothing else, and a control that
is *disabled* gives the pointer back: it is a promise that a press will do
something. A guard fails the build on a button built straight from the library.

What the app *does* own, because it is onehand's and not a widget library's: the
transcript block renderers (DESIGN-ANSWER.md), the icon registry (§6), the
terminal panel over the vendored grid, and per-panel zoom.

---

## §6 — Icons

**Every icon is an SVG.** No Unicode or emoji glyphs as icons. Shell-prompt
typography inside a code block (`❯`, `$`) is text, not an icon, and is exempt.

- **Nearly every UI glyph comes from gpui-component's `IconName`**, the enum
  generated from the SVGs it bundles. That set has to stay loaded anyway — its
  own components reference `icons/…` internally in ~97 places — and drawing the
  app's chrome from it is what keeps one stroke weight across the two.
- **`crate::icons` holds only what that set cannot draw**: a brand mark, which
  belongs to the product it stands for rather than to a general-purpose UI kit,
  and the occasional shape the bundled set has no drawing of at all — taken from
  Lucide, which it is packaged from, so the weight still matches. A name that
  merely reads oddly does not qualify; an absence does. To add one: update
  [assets/icons/manifest.toml](assets/icons/manifest.toml) with the reason
  beside the entry, run [scripts/sync-icons.sh](scripts/sync-icons.sh), register
  it in the `icons!` macro. A test fails if the manifest and the registry
  disagree. The two live in separate namespaces and the asset source serves
  both.
- The bundled set covers less than the app once carried, so a number of glyphs
  are approximations rather than the icon the design would pick. DECISIONS.md
  §6 lists which, and what each gave up.
- Tint by meaning: `muted_foreground` at rest, a semantic token when the icon is
  carrying state. An icon that tracks adjacent text (a rail row's folder, a
  selector's chevron) shares that text's color instead.

---

## §7 — Bounded rendering

Every code, diff and output renderer draws **one element per line**, so unbounded
content freezes the UI. Each cap is a named constant beside the renderer it
bounds: diff lines per card across all hunks, mono output lines per well, the
fold threshold beneath them, terminal lines, plan items, attachment rows and code
block height in `crates/app/src/chat/transcript.rs`; completion rows and tray
chips in the composer; mention candidates in the session; and `MAX_TERM_BYTES` at
parse time in core, which bounds the model rather than the view.

Keep any new content rendering bounded, and **say so on screen** when the bound
bites — a truncated view that does not admit it is a lie about the data.
