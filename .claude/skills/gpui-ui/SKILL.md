---
name: gpui-ui
description: Use when creating or changing any view, panel, card, popup, dialog, token or theme code, in crates/app, plugins/builtin, crates/plugin-host or labs/ui-labs. Routes to the contract that binds there, the source of every value and the helpers that already draw things, and ends on a review checklist.
---

# UI work in onehand

Two places draw UI, and they answer to different contracts. Work out which one the task is in
before reading anything else.

## 1. Route

| Editing | Binding contract | Every value comes from | Checks |
|---|---|---|---|
| `crates/app`, `plugins/builtin/*`, `crates/plugin-host` | `DESIGN.md` at the root, plus the `design-contract` skill; a feature's own screens are in `docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md` | `cx.theme()` for colour and radius; the surface ramp in `crates/app/src/theme.rs`; for spacing and every other size, root `DESIGN.md`, *Typography and spacing* | `cargo test -p onehand` (its `guards` included), the touched plugin's tests, `make clippy CLIPPY_EXTRA="-- -D warnings"`, `make fmt` |
| `labs/ui-labs` | `labs/ui-labs/DESIGN.md` | `labs/ui-labs/src/tokens.rs` only: spacing, chrome, type, radii, width budgets, state steps and the two `Palette`s | `cargo test --manifest-path labs/ui-labs/Cargo.toml --target-dir target` (its `guards` included), `cargo fmt --manifest-path labs/ui-labs/Cargo.toml` |

- **Never apply a labs rule to `crates/app`**, or an app rule to the lab. The only way a lab
  rule reaches the app is a promotion: use the `ui-promote` skill.
- **The Departures table** in `labs/ui-labs/DESIGN.md` lists where the two differ on purpose.
  A row there is a proposal, not an app bug to fix in passing.
- **Don't launch the window to check your work.** Build and test, then stop; the user looks
  (`make labs`, `make dev`, `make showcase`).

## 2. Look before drawing

1. **Is it already there?** Search for the screen's own words (a title, a button label) before
   drawing it. A request can name something already drawn; if so, say so and ask what should
   change.
2. **Read the rules** in the binding `DESIGN.md` for the region you touch, at the version in the
   working tree. [checklists/review.md](checklists/review.md) names the section that owns each
   rule, in both contracts; this skill restates none of them.
3. **Find what draws it now:** [references/components.md](references/components.md) lists what
   gpui-component draws and what each context draws itself, with paths. Reuse before writing.

## 3. Values

- Name every size, gap, radius, width and colour; never write the number or hex at the call site.
  This skill names no values either: read them where they live.
- **No constant fits?**
  - Labs: add one to `tokens.rs`, in its role's group, with the reason beside it. Name it for its
    role (`SUBLINE`), never for its arithmetic (`HALF_TIGHT`).
  - App: a missing surface or ink goes into the ramp in `crates/app/src/theme.rs` with its
    contrast asserted in that file's tests, never into the one view that needed it. A missing size
    follows root `DESIGN.md`, *Typography and spacing*.
  - App, files written before that rule: `every_length_has_a_name` in `crates/app/src/guards.rs`
    lists them with how many numbers each still writes. A change may only lower a count; when you
    touch one of those lines anyway, name its number and lower the file's entry.
- **In the lab, docs name constants, never numbers**, as its `DESIGN.md` says at the top. The
  root `DESIGN.md` states no drawing values; the few numbers it holds are behaviour, such as a
  cap or a resize range.

## 4. The same change carries its docs

- **App:** a visible change updates the root `DESIGN.md` in the same pull request, within the
  budget the `design-contract` skill sets.
- **Labs:** a changed rule updates `labs/ui-labs/DESIGN.md`; a deliberate difference from the app
  gets a Departures row; a value lives only in `tokens.rs`.
- **Code never cites a document**, in either place. A comment gives its reason in its own words.
- **A file past about 800 lines** of code splits along a seam in the same change.

## 5. Finish

Answer [checklists/review.md](checklists/review.md), run its checks, and report every item that
failed or was skipped, with the reason.
