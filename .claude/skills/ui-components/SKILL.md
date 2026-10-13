---
name: ui-components
description: Build and normalize onehand controls and content containers through shared builders. Use for buttons, fields, rows, badges, boxes, menus and dialogs, including inconsistent variants across app and plugin screens.
---

# Components

Read [DESIGN.md](../../../DESIGN.md) (*Components*, *Icons*, *Exceptions*) and
[UI patterns](../../../docs/ui-patterns.md) (*Choose the container*, *Control sizes*) for the role
being built.
The catalog describes available implementation, not permission to copy a mismatched caller.

## Building one

1. **Locate existing uses.** Search the screen's labels and [catalog.md](catalog.md). Identify
   the component and its peers before adding anything. An existing component is a starting point
   for the requested change; ask only if the intended behavior remains materially ambiguous.
2. **Reuse, in this order**: a builder in [catalog.md](catalog.md), then a gpui-component control,
   then a builder of your own. When a second place needs something drawn inline, lift it into a
   shared builder in that change, into `crates/plugin-host` when a plugin draws it too.
3. **Choose the container and variant by role.** A related group may need only a heading and
   space. Do not add a card, badge, icon or filled control solely to make it look finished.
4. **Draw every state it can be in** (below), at the size its role takes (UI patterns, *Control
   sizes*). Keep the same geometry across states and compare with same-role peers. A size a
   component gets wrong is fixed in its shared wrapper, never at the call site.
5. **List a new builder in catalog.md** in the same change. Distinguish a builder that exists
   from one being proposed; never document an unimplemented helper as available.

Done when the change draws by hand nothing catalog.md or the library draws, and every state below
that applies to the component is drawn.

## States

| State | When | Drawn as |
|---|---|---|
| Rest | always | its tokens; its text in the ink its role takes (UI patterns, *Hierarchy*); an icon at rest in `muted_foreground`, in a slot as wide as its peers', never on a tile of its own |
| Hover | it can be pressed or picked | the pointer cursor (`action` sets it) and a `list_hover` fill on a row, its inks unchanged; the selected step on a ghost button (`quiet`); the library's own on its other controls |
| Selected or open | one of several is chosen, or its menu is open | the `accent` fill |
| Focus | the keyboard reaches it | the keyboard ring; row actions become available without moving content; custom elements use `.focus_visible(\|s\| s.border_color(cx.theme().ring))`, library controls draw theirs |
| Refusing | it cannot act now | `.refuses(Some(why))`: no press, the arrow cursor, and `why` as its tooltip |
| Loading | it started work still running | `.loading(true)` on the button that started it, its label kept |
| Waiting, empty, failed | it shows content | the `ux-patterns` skill |

Check combined states, especially selected + hovered and selected + keyboard focus. Feedback
must not erase selection. Give icon-only actions the same hit area as peer controls and a tooltip;
do not shrink the hit area to the SVG's bounds. Shared optical adjustments belong in the builder.
States change in the frame they happen in: no transition between them, and no state shown by
motion (UI patterns, *Motion*).

## Variants

Every button names one, by role (the rule is in `DESIGN.md` *Components*):

| Variant | For |
|---|---|
| `primary` | the view's one main action |
| `secondary` | a weighed alternative beside the primary; a picker's trigger; every button on the Settings page |
| ghost, `.quiet(cx)` | dismissing, a strip's or a row's actions; hovers on the selected step, never a bare `ghost()`; an icon with no words through `.quiet_icon(on, cx)`, muted at rest |
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
// Wrong: no variant and no size, so the library's default, edged, filled apart from its
// surface and lettered at the reading size
action("run-check").label("Run check")
// Right: a weighed alternative beside the primary, at the size its role takes
action("run-check").secondary().small().label("Run check")
```

```rust
// Wrong: refuses the press, still shows the pointer, and says nothing of why
action(("plugin-update", i)).small().secondary().disabled(busy)
// Right: refusing in one call, cursor and reason included
action(("plugin-update", i)).small().secondary().refuses(busy.then_some("Another change is running"))
```

```rust
// Wrong: a row's everyday action at the dense size, and a form's button at the library's default
action(("session-more", i)).quiet(cx).xsmall().icon(IconName::Ellipsis)
action("save-agent").primary().label("Save")
// Right: the size each role takes
action(("session-more", i)).quiet(cx).small().icon(IconName::Ellipsis)
action("save-agent").primary().tall().label("Save")
```

```rust
// Wrong: the library's default size for a taller hit area, which grows the icon with it
action("refresh").quiet(cx).with_size(Size::Medium).icon(Icon::new(AppIcon::Refresh))
// Right: the tall size keeps the small icon in the taller box
action("refresh").quiet(cx).tall().icon(Icon::new(AppIcon::Refresh))
```

```rust
// Wrong: a switch stretched to the controls beside it
Switch::new("keep").small().h(tokens::FIELD_H)
// Right: the library's own geometry, centred in its row
div().h_flex().items_center().gap_2().child(Switch::new("keep").small()).child(field)
```

```rust
// Wrong: a label drawn by hand
div().px(CHIP_PAD_X).py(CHIP_PAD_Y).rounded(cx.theme().radius).bg(fill).text_color(ink)
// Right: the library's label, at a badge's corner
Tag::custom(tint.fill, tint.ink, tint.fill).small().rounded(cx.theme().radius_lg).child(text)
```

```rust
// Wrong: an icon on a tinted tile, and a slot as wide as each icon happens to be
div().p_1().rounded(cx.theme().radius).bg(tint.fill).child(icon.text_color(tint.ink))
// Right: the icon on the row's own surface, in a slot as wide as its peers'
div().w_4().flex().justify_center().child(icon.small())
```
