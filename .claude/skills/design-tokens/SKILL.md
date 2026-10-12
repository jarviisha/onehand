---
name: design-tokens
description: Tokens for colour, spacing, type, corners, shadows and fixed sizes in crates/app, plugins/builtin and crates/plugin-host. Use when writing any style value (a colour, gap, padding, text size, weight, line height, corner, shadow, border or width), or adding a constant, palette step or token.
---

# Design tokens

Every value a view draws with is a token: a theme slot, a gpui scale method, or a named constant.
`DESIGN.md` rules which token serves which role (*Type, space and shape*, *Colour and state*,
*Exceptions*); read the section for what you draw. [reference.md](reference.md) looks every token
up by name: where it lives, its value, the slot names that mislead, and what each old value
becomes.

## Writing a value

1. Name the role: meta text, a row's gap, a box's corner, a floating shadow.
2. Write the token `DESIGN.md` gives that role, spelled as reference.md spells it.
3. When no token fits, add one (below).

Done when the change writes only tokens and named constants: `.claude/skills/ui-review/scan.sh`
prints nothing for its lines, and `cargo test -p onehand --locked guards` passes.

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
// Wrong: a standing box at a control's corner
.rounded(cx.theme().radius)
// Right
.rounded(cx.theme().radius_lg)
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

## Adding a token

- **A colour**: a step in both palettes of `Ramp` in `crates/app/src/theme.rs`, written into its
  slot in `paint`, its contrast asserted in that file's tests against every surface it lands on.
  A plugin reads it through the theme slot.
- **A size one module uses**: a `const` of type `Rems` beside its use, its reason in its doc
  comment. A spacing value equals a step.
- **A size a second module needs**: move the constant into `onehand_plugin_host::tokens` in that
  change, and point both modules at it.
- **A new step in a scale** (a type size, a corner, a spacing step, an elevation): a change to the
  system. Agree it with the user first, then change `DESIGN.md` with it.
- Add the token to reference.md in the same change.

A file written before the rules names a number when a change touches its line:
`every_length_has_a_name` in `crates/app/src/guards.rs` lists those files with counts that only
go down.
