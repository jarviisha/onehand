---
name: ui
description: Build, change and review onehand's UI in the app and its feature crates. Use for any colour, spacing, type, corner, shadow or size; any control, field, row, badge, box, menu or dialog; any page, panel, list or form and how it splits or stacks; any waiting, empty, failing or refusing state, toast, removal, key, focus or on-screen word; before finishing a visible change; for UI audits; and when changing the UI contract or this skill.
---

# UI

The rules live in the documents; this skill is how to apply them. Read the sections the change
touches, not the whole set:

| Changing | Read |
|---|---|
| What a region holds, or its order | [screens.md](../../../docs/screens.md), or the feature's own document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`) |
| A principle, the window's frame, the transcript | [DESIGN.md](../../../DESIGN.md) *Principles*, *Layout*, *Transcript* |
| A colour, an ink, type, space, a corner, a line | DESIGN.md *Type, space and shape*, *Colour and state*; the token in [tokens.md](tokens.md) |
| A control, its variant, its size, an icon | DESIGN.md *Components*, *Icons*; [UI patterns](../../../docs/ui-patterns.md) *Control sizes*; the builder in [catalog.md](catalog.md) |
| A page, a panel, a list, a form, a dialog | UI patterns *Choose the container*, *Hierarchy*, *Spacing and density*, then its pattern; the screen to build from in catalog.md *Patterns* |
| Waiting, empty, failing, refusing, a toast, a removal, a key, focus, a word | UI patterns *Behaviour*; the wiring in catalog.md *Keys and focus* |

tokens.md and catalog.md describe the code as it stands: a lookup, not permission to copy a
caller that departs from the contract. Visual cleanup does not authorize removing actions or
changing navigation.

## Building

1. **Find the peer.** Search the screen's labels and catalog.md for the component and its
   same-role peers across app and feature crates. Compare height, text axis, ink, icon size, spacing,
   corner and states against the contract, not only against the code. Ask only when the intended
   behaviour stays materially ambiguous.
2. **Reuse, in this order**: a builder in catalog.md, a gpui-component control, a builder of your
   own. When a second place needs something drawn inline, lift it into a shared builder in that
   change (into `crates/ui` when a feature crate draws it too) and list it in catalog.md;
   never list one that is only proposed.
3. **Pick the pattern** by what the content is, in the least decorated container that says each
   relationship, keeping the boxes a feature requires, and build from its screen. Never add a
   card, badge, icon or fill to look finished.
4. **Measure and bound it.** A split or a stack reads the container it is given, never the window.
   Every list has a named cap and says how many it left out. Text that can run long truncates or
   wraps as its neighbours do; content of several lines grows rather than shrinking its font.
5. **Draw every state** (*States*) at the size its role takes, in one geometry across states, and
   check the way between them (loading to content, empty to content), not only where it lands.
   Fix a size a component gets wrong in its shared wrapper, never at the call site.
6. **Take each token by its role**: an ink by its role and the surface under it, a gap by the
   relationship, a column page's width from `PAGE_*`, a strip's height from `BAR_H`, a row's
   from `ROW_H`. Add a token only for a missing role (tokens.md, *Adding a token*); when a shared
   default changes, review its other callers too.
7. **Wire keys, words and saves**: each key a keymap command (catalog.md, *Keys and focus*),
   each word as UI patterns *Words* has it, and a change the person can take back saved as
   catalog.md's saving row shows.

## States

| State | When | Drawn as |
|---|---|---|
| Rest | always | its tokens; its text and icons in the inks their roles take (UI patterns, *Hierarchy*; `DESIGN.md`, *Icons*) |
| Hover | it can be pressed or picked | the pointer cursor (`action` sets it) and the hover step its surface takes (`DESIGN.md`, *Colour and state*); the library's own on its other controls |
| Selected or open | one of several is chosen, or its menu is open | the selected step (`DESIGN.md`, *Colour and state*) |
| Focus | the keyboard reaches it | the ring, for the keyboard only (`DESIGN.md`, *Colour and state*); row actions become available without moving content; custom elements use `.focus_visible(\|s\| s.border_color(cx.theme().ring))`, library controls draw theirs |
| Refusing | it cannot act now | `.refuses(Some(why))`: no press, the arrow cursor, and `why` as its tooltip |
| Loading | it started work still running | `.loading(true)` on the button that started it, its label kept |
| Waiting, empty, failed | it shows content | UI patterns, *Waiting*, *Nothing to show*, *Failing and refusing* |

Check the combined states, selected and hovered, selected and focused: feedback never erases the
selection. Walk a row's keyboard path as well as its hover: its actions reachable, the label
still as they appear. An icon-only action keeps its peers' hit area and is named with `.named(..)`, its tooltip and
its accessible name at once; never
shrink the hit area to the SVG. An optical correction peers share belongs in their builder.

## Right and wrong

Each pair: what is wrong, the wrong line, then the right one.

```rust
// A div dressed as a button: no focus, no refusal, a cursor rule of its own
div().px_2().cursor_pointer().child("Retry").on_click(retry)
crate::controls::action("retry-task").quiet(cx).short().label("Retry…").on_click(retry)
// No variant and no size: the library's default, edged and lettered at the body size
action("run-check").label("Run check")
action("run-check").secondary().tall().label("Run check")
// Refuses the press yet keeps the pointer, and never says why
action(("plugin-update", i)).short().secondary().disabled(busy)
action(("plugin-update", i)).short().secondary().refuses(busy.then_some("Another change is running"))
// A row's icon at the extra small size, under the height of a worded peer beside it
action(("session-more", i)).quiet_icon(false, cx).xsmall().icon(IconName::Ellipsis)
action(("session-more", i)).quiet_icon(false, cx).small().icon(IconName::Ellipsis)
// A page form's button at the library's default size, lettered at the body size
action("save-workflow").primary().label("Save")
action("save-workflow").primary().tall().label("Save")
// A badge drawn by hand
div().px(CHIP_PAD_X).py(CHIP_PAD_Y).rounded(cx.theme().radius).bg(fill).text_color(ink)
badge(text, tint)
// An icon on a tinted tile, in a slot as wide as it happens to be
div().p_1().rounded(cx.theme().radius).bg(tint.fill).child(icon.text_color(tint.ink))
mark_slot(icon.small())
// The window decides, so a narrow dock in a wide window still splits
let side_by_side = window.viewport_size().width > px(900.);
let layout = list_detail(self.split.read(cx), &DOC_LIST, self.width.get(), rem);
// A page padded its own way, its width a bare number
div().px_4().py_6().child(div().max_w(rems(EDITOR_COLUMN)))
div().px_8().py_6().child(div().max_w(tokens::PAGE_PROSE))
// A subsection framed, filled and lifted to look like a group
div().p_3().border_1().border_color(theme.border).bg(theme.muted).shadow(lift(cx)).child(group)
div().v_flex().gap_2().child(heading).child(group)
```

## Finishing

Take the path the diff needs; neither path's evidence proves the other complete.

**Contract and skill changes only.**

1. Keep each rule with its owner: DESIGN.md visual roles and the frame, screens.md what each
   region holds, UI patterns layouts and behaviour, tokens.md and catalog.md what is built; this
   skill applies them and is never a second system. No rule rests on another product; the one
   source the contract names is its palette's, VS Code's keys.
2. Follow the changed rule through this skill, tokens.md, catalog.md, checklist.md and
   `AGENTS.md`, resolving contradictions and broken links; preserve feature behaviour unless the
   user asked otherwise, and keep target rules apart from claims about the code. Walk realistic
   cases through the text (a new settings section, a row with secondary text, a hovered selected
   item, a narrow dock, a shared token change): each reaches an existing pattern or an explicit
   implementation gap, never a local style.
3. Check this file's frontmatter (`name` is `ui`, its directory's name; `description` is set) and
   run `git diff --check`; prose needs no runtime test. Report the documents changed, the checks
   and that no rendered UI was migrated.

**Implementation changes.**

1. **Answer [checklist.md](checklist.md)**: each item yes, no (its file and line) or n/a (why),
   after reading the rule the item names.
2. **Run the checks**: `cargo test -p onehand --locked`,
   `cargo test -p <crate> --locked` for every other crate touched,
   `make clippy CLIPPY_EXTRA="-- -D warnings"` and `make fmt`. Quote any failure.
3. **Carry the documents** in the same change: what a screen holds or its order to
   `docs/screens.md` (or the feature's own document); a principle, the frame, the transcript or a
   visual rule to `DESIGN.md`, which gets at most one line per new region; a pattern or a
   behaviour to `docs/ui-patterns.md`; a token or a builder to tokens.md or catalog.md.
4. **Compare peers and report**: the same-role component compared, the shared builder or tokens
   checked, a changed shared default's callers inspected; failures, fixes, remaining gaps and
   checks run. Hand the user a focused visual check in `make dev` or `make showcase` (both
   themes, narrow and wide containers, zoom, long text, the states that apply), and claim no
   visual verification: passing tests are not evidence of visual consistency.
