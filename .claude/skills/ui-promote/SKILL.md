---
name: ui-promote
description: Use when moving an idea proven in labs/ui-labs into crates/app, or when a Departures row in labs/ui-labs/DESIGN.md is to be closed. Also after bumping gpui or gpui-component in the workspace, to re-seed the lab's Cargo.lock.
---

# Promote a lab idea into the app

A promotion is the one task that may change the app's contract to match the lab. Follow the
`gpui-ui` skill for the drawing itself, on the app's side of its route table.

## 1. Scope it to one idea

- **One idea per pull request**: one Departures row, one rule, one screen. If the request holds
  several, list them and ask which goes first.
- **Gather what the idea is made of** before writing code: its section in
  `labs/ui-labs/DESIGN.md`, its Departures row, the lab code that draws it, the constants in
  `tokens.rs` it uses, and the lab tests that hold it.
- **Read what the app does now:** the root `DESIGN.md` lines the Departures row points at, and
  the app code behind them. A difference the Departures table does not list is still a difference;
  name it before changing it.
- **If the app already draws it**, there is nothing to promote: say so, and name the differences
  that remain, so the person can pick one of those instead.
- **Check it against the acceptance table** at the foot of `labs/ui-labs/DESIGN.md`. An idea
  that has not met its rows there is not proven yet; say which rows are open.

## 2. Translate, don't copy

The lab is drawn on its own palette and tokens; the app reads the theme. Nothing from the lab's
`tokens.rs`, `Palette`, `paint` or `controls.rs` is imported or pasted into the app.

| In the lab | In the app |
|---|---|
| A `Palette` role | the theme token that means the same (root `DESIGN.md`, Colour and state); a surface the ramp lacks goes into `crates/app/src/theme.rs` with its contrast asserted |
| A spacing role (`TIGHT`, `RELATED`…) | the gpui base-4 step that matches |
| A `RADIUS_*` | the theme's radius (root `DESIGN.md`, Typography and spacing) |
| A width budget or chrome size | a `const` in the module that uses it, in rems, its reason beside it |
| `TEXT_*` | the app's type roles (root `DESIGN.md`, Typography and spacing) |
| A lab helper | the app's helper for the same thing (`gpui-ui`'s `references/components.md`) |

The lab keeps its own constant while it still draws the idea: the lab is where the next version
is tried.

## 3. Build it in the app

- Reuse the app's helpers, the `action` wrapper and `IconName`. Mind the app's own gotchas in
  `CLAUDE.md` (key contexts, focus on panel close, `mx_auto` in a list row).
- **A rule that is a pure function gets a unit test** beside its module (`foo/tests.rs`), the way
  the lab's `presentation` is held by `layout.rs`'s tests. Port the test's cases, restated in the
  app's terms.
- Keep each file under about 800 lines of code; split along a seam in the same change.

## 4. Move the contract in the same pull request

- **Root `DESIGN.md`:** write the rule as structure and behaviour, naming no value, within the
  `design-contract` budget. If a feature document owns the screen, the detail goes there.
- **`labs/ui-labs/DESIGN.md`:** remove the Departures row, or rewrite it if part of the
  difference stays. Where the lab and the app now agree, the lab's section may stay as it is.
- **Code never cites either document.**

## 5. Check and report

Run the app checks from `gpui-ui`'s checklist and answer its questions for the app. In the
report, name the Departures row closed, the root `DESIGN.md` lines changed, the test added, and
anything of the idea left in the lab for a later pull request.

## After bumping gpui or gpui-component

The lab is outside the workspace and pinned only by its own lock, seeded from the workspace's. In
the same change as the bump, copy the root `Cargo.lock` over `labs/ui-labs/Cargo.lock` and run
the lab's tests, so the lab builds on the app's gpui commit.
