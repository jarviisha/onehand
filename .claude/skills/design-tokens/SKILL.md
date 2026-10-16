---
name: design-tokens
description: Apply onehand's visual roles consistently through theme and size tokens. Use for colour, spacing, density, typography, corners, shadows and dimensions in the app and plugins, or when auditing drift between equivalent components.
---

# Design tokens

Every value a view draws with is a token: a theme slot, a gpui scale method, or a named constant.
[DESIGN.md](../../../DESIGN.md) owns visual roles (*Principles*, *Type, space and shape*,
*Colour and state*, *Components*). [UI patterns](../../../docs/ui-patterns.md) owns spacing
relationships and density. [reference.md](reference.md) maps those decisions to existing tokens;
its values describe the code, not a promise that every caller uses them correctly.

## Writing a value

1. Name the content role and its context: navigation, reading content, metadata, control,
   independent summary, well or overlay. Determine its states and the surface it sits on.
2. Find an equivalent component elsewhere in the app or plugins. Compare spacing, height,
   type, corner, fill and state treatment against the contract, not just against current code.
3. Use the pattern's token choices through its shared builder. Repair a shared default at its
   owner when that is the cause; avoid per-screen overrides that leave peers inconsistent.
4. For text on a neutral surface, take one of the three neutral inks by its role; on a fill, the
   ink paired with that fill; for a state, a link or code in prose, its semantic ink (UI
   patterns, *Hierarchy*). For a colour the ramp lacks between two it holds, mix those two
   (`mix_oklab`) rather than thinning one with an alpha or picking a value of your own.
5. Add a token only for a missing role. A new name for an existing value does not justify a new
   visual treatment. If a shared change affects other callers, include them in the review.

For implementation changes, run the source guards through `ui-review`. They catch literal
values, not incorrect role assignments. Report which equivalent components were compared;
passing the guards alone is not completion. Documentation-only work follows that skill's
document path.

## Right and wrong

```rust
// Wrong: a size nobody can find again
.text_size(rems(0.8125))
// Right
.text_size(tokens::CODE_TEXT)
```

```rust
// Wrong: 10px, between two steps
.py_2p5()
// Right
.py_2()
```

```rust
// Wrong: gpui's shadow, unseen on the dark palette
.shadow_lg()
// Right
.shadow(tokens::lift(cx))
```

```rust
// Wrong: a corner of its own, or the library's half step
.rounded(cx.theme().radius_lg)
.rounded(cx.theme().radius_tokens().sm)
// Right: the one corner
.rounded(cx.theme().radius)
```

```rust
// Wrong: a badge at the library's small-tag corner, or the common one
Tag::custom(fill, ink, fill).small().child(text)
// Right: a badge is fully round (or call `badge`)
Tag::custom(fill, ink, fill).small().rounded_full().py_1().child(text)
```

```rust
// Wrong: `accent` is the selected fill, not a hue
.text_color(cx.theme().accent)
// Right: the accent hue
.text_color(cx.theme().link)
```

```rust
// Wrong: a hover made by thinning another token
.hover(move |row| row.bg(accent.opacity(0.5)))
// Right
.hover(|row| row.bg(cx.theme().list_hover))
```

```rust
// Wrong: a quieter ink made see-through, a different shade on every fill under it
.text_color(cx.theme().foreground.opacity(0.6))
// Right: the ink the role names, a solid step
.text_color(crate::theme::meta_ink(cx))
```

## Adding a token

- **A colour**: a key's value in both palettes of `Ramp` in `crates/app/src/theme.rs`, from the
  source `DESIGN.md` names (*Colour and state*), the key beside it and a row in that table, written
  into its slot in `paint`, its contrast asserted in that file's tests against every surface it lands on.
  A plugin reads it through the theme slot. A step between two the ramp holds is derived from
  them with `mix_oklab`, as `meta_ink` and `tint` are, not written as a value of its own.
- **A size one module uses**: a `const` of type `Rems` beside its use, its reason in its doc
  comment. A spacing value equals a step.
- **A size a second module needs**: move the constant into `onehand_plugin_host::tokens` in that
  change, and point both modules at it.
- **A new step in a scale** (a type size, a corner, a spacing step, an elevation): a system change.
  Use the user's authorized scope; when it covers the system, update the owning contract and
  implementation together. Otherwise use an existing role or raise the specific unresolved decision.
- Add the token to reference.md in the same change.
