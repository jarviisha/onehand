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
| Hover | it can be pressed or picked | the pointer cursor (`action` sets it) and a `list_hover` fill on a row; the library's own on its controls |
| Selected or open | one of several is chosen, or its menu is open | the `accent` fill |
| Focus | the keyboard reaches it | the ring, on keyboard focus only: `.focus_visible(\|s\| s.border_color(cx.theme().ring))` on an element you draw; the library's controls draw theirs |
| Refusing | it cannot act now | `.refuses(true)`: no press, the arrow cursor, and a tooltip saying why |
| Loading | it started work still running | `.loading(true)` on the button that started it, its label kept |
| Waiting, empty, failed | it shows content | the `ux-patterns` skill |

## Right and wrong

```rust
// Wrong: a div dressed as a button, with no focus, no refusal and its own cursor rule
div().px_2().cursor_pointer().child("Retry").on_click(retry)
// Right
crate::controls::action("retry-task").ghost().small().label("Retry…").on_click(retry)
```

```rust
// Wrong: refuses the press, still shows the pointer
action(("plugin-update", i)).xsmall().outline().disabled(busy)
// Right: refusing in one call, cursor included
action(("plugin-update", i)).xsmall().outline().refuses(busy)
```

```rust
// Wrong: a label drawn by hand
div().px(CHIP_PAD_X).py(CHIP_PAD_Y).rounded(cx.theme().radius).bg(fill).text_color(ink)
// Right: the library's label, at the small corner
Tag::custom(tint.fill, tint.ink, tint.fill).small().child(text)
```
