---
name: ui-components
description: Components the app and its plugins already draw, and the states each one needs. Use when adding or changing a control, row, label, box, menu, popup or dialog, and before writing any new builder.
---

# Components

## Building one

1. **Look for it on screen first.** Search the code for the screen's own words (a title, a
   button's label): a request can name something already drawn. If it is, say so and ask what
   should change.
2. **Reuse, in this order**: a builder in [catalog.md](catalog.md), then a gpui-component control,
   then a builder of your own. When a second place needs something drawn inline, lift it into a
   shared builder in that change, into `crates/plugin-host` when a plugin draws it too.
3. **Draw every state it can be in** (below), at the size `DESIGN.md` gives its place
   (*Components*), with the tokens of the `design-tokens` skill.
4. **List a new builder in catalog.md** in the same change.

Done when the change draws by hand nothing catalog.md or the library draws, and every state below
that applies to the component is drawn.

## States

| State | When | Drawn as |
|---|---|---|
| Rest | always | its tokens; an icon at rest in `muted_foreground` |
| Hover | it can be pressed or picked | the pointer cursor (`action` sets it) and a `list_hover` fill on a row; the selected step on a ghost button (`quiet`); the library's own on its other controls |
| Selected or open | one of several is chosen, or its menu is open | the `accent` fill |
| Focus | the keyboard reaches it | the ring, on keyboard focus only: `.focus_visible(\|s\| s.border_color(cx.theme().ring))` on an element you draw; the library's controls draw theirs |
| Refusing | it cannot act now | `.refuses(true)`: no press, the arrow cursor, and a tooltip saying why |
| Loading | it started work still running | `.loading(true)` on the button that started it, its label kept |
| Waiting, empty, failed | it shows content | the `ux-patterns` skill |

## Variants

Every button names one, by role (the rule is in `DESIGN.md` *Components*):

| Variant | For |
|---|---|
| `primary` | the view's one main action |
| `secondary` | a weighed alternative beside the primary; a picker's trigger; every button on the Settings page |
| ghost, `.quiet(cx)` | dismissing, a strip's or a row's actions, an icon-only control; hovers on the selected step, never a bare `ghost()` |
| `danger` | only the two listed under *Exceptions* in `DESIGN.md` |
| `choice` | each option of a pick-one row |

No button draws an edge: never the library's default variant nor `outline()`. A text field is
the one control marked by its edge.

## Right and wrong

```rust
// Wrong: a div dressed as a button, with no focus, no refusal and its own cursor rule
div().px_2().cursor_pointer().child("Retry").on_click(retry)
// Right
crate::controls::action("retry-task").quiet(cx).small().label("Retry…").on_click(retry)
```

```rust
// Wrong: no variant, so the library's default, edged and filled apart from its surface
action("run-check").label("Run check")
// Right: a weighed alternative beside the primary
action("run-check").secondary().label("Run check")
```

```rust
// Wrong: refuses the press, still shows the pointer
action(("plugin-update", i)).xsmall().secondary().disabled(busy)
// Right: refusing in one call, cursor included
action(("plugin-update", i)).xsmall().secondary().refuses(busy)
```

```rust
// Wrong: a label drawn by hand
div().px(CHIP_PAD_X).py(CHIP_PAD_Y).rounded(cx.theme().radius).bg(fill).text_color(ink)
// Right: the library's label, at a badge's corner
Tag::custom(tint.fill, tint.ink, tint.fill).small().rounded(cx.theme().radius_lg).child(text)
```
