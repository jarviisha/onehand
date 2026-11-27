# UI rules: every rule names what checks it, and the agent sees what it draws

## Why

The UI contract was written on 2026-10-12. Since then 133 of 254 commits touched `crates/ui`, the
contract documents or the `ui` skill, and `DESIGN.md` alone took 54 of them. The contract turned
round on itself: the palette changed three times (#157, the redraw of 2026-10-31, #336), control
sizes stepped down three times and back up (#150, #161, #165, #337), "no button draws an edge"
(#147) became edged boxes five days later (#162), and the chosen item went blue and back to grey
in a day (#336, #340).

The rules that a machine can check are followed. No colour is written outside `theme.rs` and
`ansi.rs`; no `Button::new` stands outside the wrapper, against 221 `action(` calls; all 95
`text_size(` calls name a constant; none of the 24 `.disabled(` calls stands in an `action(` chain. The `ui`
skill was loaded 29 times in the sessions on record, so the drift is not agents skipping the rules.

What the owner kept correcting is elsewhere. Read from their messages between 2026-10-28 and
2026-11-26 (a rough count by hand):

| What was reported | About | What the contract says about it |
|---|---|---|
| Layout under strain: two words wrapping, heights that differ in one row, a clipped chevron, a long name breaking the bar, a button moving with the workspace's name, a scrollbar over content, rows squeezed in a box that scrolls | 13 | a few gotchas; one guard, added after the fact (#426) |
| Values and taste: too dark, too tall, "why 35?", an edge too strong | 13 | nothing: the contract says it holds no values |
| Where a thing stands: workers moved four times, the search three | 9 | `docs/screens.md` records the outcome, not the grounds |
| A control or token rule broken | 3 | most of the contract's words |

Four faults in the rule system produce this:

1. **Nobody but the owner looks.** The agent may not run the app, answers a 17-question checklist
   about its own work, and no guard reads a visual rule. Tests pass; the owner opens `make dev`
   and it is broken.
2. **The rules aim at the smallest class of fault.** Which token a role takes is ruled at length.
   What a row does when its name is two hundred characters long is not ruled at all.
3. **A rule cannot be checked or even pointed at.** `DESIGN.md`, `docs/screens.md`,
   `docs/ui-patterns.md` and the skill's three reference files are about 32,000 words of prose
   with no rule numbers and about 216 exception clauses, many naming one screen. The skill says to
   read only the sections a change touches, so an exception written elsewhere is missed. "Every
   departure becomes the rule of its role" (#151) makes *same role, same treatment* true by
   definition.
4. **Layout has no owner in the code.** The shared builders are frames: `row`, `strip`,
   `card_box` and `tab_bar` return a bare `Div`, and what stands in it (a name, a trailing slot,
   what shrinks) is composed by hand at each call site. Only `page_head` owns its truncation and
   its fixed actions. Beside them stand 147 local size constants, each within the rules.

## Goals

- **Every rule has an id and names what checks it**: a lint, a guard, a test, a picture, or the
  owner. A rule only a reader can check says so, and they are few.
- **The foundation is locked by a failing test, not by a sentence.** The shared sizes, the palette
  and the local size constants change only in a diff that shows the change by name.
- **Layout under strain is ruled, and tested where it was reported broken**: long text, a narrow
  container, zoom, an empty list and one past its cap, on the real views, without a screen.
- **The agent sees what it changed before it says it is finished**, and someone other than the
  author reads the picture.
- **Rules read as conditions, not as a tour of the screens.** A statement only one screen can
  meet is that screen's fact and moves to `docs/screens.md`.
- **One system.** The new rules replace the old text in the documents that already own it.

## Non-goals

- Changing how any screen looks. No step here changes a value, a size, a colour or a layout the
  owner sees, except where a new test proves a fault of the kind already reported; such a fix is
  listed in its PR.
- Migrating screens onto the rewritten rules, and moving hand-composed rows onto builders that
  own their slots. That is later work, by class of fault; each migration brings its strain test
  with it.
- A gallery of the shared builders. Most are bare frames, so it would show little the views do
  not; add one when picturing a builder through the views proves too costly.
- A newtype around the library's `Button`. It would make the wrong call unrepresentable, and it
  touches 221 call sites.
- Lints of our own (dylint). `clippy.toml` first; a lint crate tied to a nightly compiler only if
  that proves too little.

## Anti-goals

- **No second set of documents.** No `docs/uiux/`, no decision log, no exception log. A rule's
  exception is its own *Unless*; why a value is what it is stays in the comment beside it and in
  git.
- **No rule without a checker named**, and no checker claimed that does not exist yet.
- **No normalising.** A difference between two sizes is not a fault until a rule and a test say so.
- **No rule id in the code.** Code never cites a document: an id stays out of comments, test
  names, lint reasons and anything drawn. The rules point at the tests, never the reverse.

## Approaches

1. **Run `localDocs/prompt-ui.md` as written.** Its rule format and its lock on the foundation are
   right, and both are kept here. It is not taken whole: its validation is still the author's own
   yes or no; its situations are a web application's and leave layout under strain out; "the
   baseline is the code as it stands" would lock the local constants and forbid fixing heights
   that differ; and it writes four new documents beside the three that own these rules.
2. **Rewrite the prose only.** Cheaper, and it leaves fault 1 standing: better-worded rules that
   still only the owner can check.
3. **Checks first, then the rules written against them** (this design). Lints, the size census,
   the strain tests and the reviewer land before the rewrite, so each rewritten rule names a
   checker that exists, and whatever a lint now holds leaves the prose.

## How it works today (what this touches)

- `DESIGN.md` (*Type, space and shape*, *Colour and state*, *Components*, *Icons*) and
  `docs/ui-patterns.md` hold the rules; `.claude/skills/ui/` applies them (`SKILL.md`,
  `checklist.md`) and describes the code (`tokens.md`, `catalog.md`).
- `crates/app/src/guards.rs` scans sources, test files included. One guard is about drawing
  (`a_scrolling_box_is_not_a_flex_column`); `docs/rules-and-gotchas.md` (*Guard coverage*) says no
  guard scans the visual rules.
- CI runs `make clippy CLIPPY_EXTRA="-- -D warnings"` over first-party crates. There is no
  `clippy.toml`. A probe showed `disallowed-methods` naming `gpui::Styled::cursor_pointer`,
  `gpui_component::button::Button::new` and gpui's free functions through the `gpui-pre` rename.
- The palette is hex strings in `theme.rs` (`palette`); the terminal's colours are integers in
  `ansi.rs`. `gpui::rgb` and `rgba` are called nowhere, `gpui::hsla` in `theme.rs` alone;
  `gpui::black()` draws the lift in `onehand_ui::tokens`.
- `action()` returns the library's `Button`. Its `disabled` and a `Switch`'s are the
  `Disableable` trait's; a menu item's and an `Input`'s are their own.
- 45 test files build a `TestAppContext`, and `crates/app/src/shell/rig.rs` builds a shell in
  one. None reads `debug_bounds`, which gpui's test context offers with `debug_selector` in a
  debug build with `test-support`. The test platform measures text with `NoopTextSystem`.
- `crates/app/src/theme.rs` already tests the corners at four and six pixels and each ink's
  contrast: a precedent for holding a value by test.
- `/drive` runs the app in a headless sway session on a data root of its own and takes
  screenshots. The model may not invoke it, and AGENTS.md says *Don't self-verify UI by launching
  or screenshotting*.
- On the owner's machine `cargo` is routed to a build host: an environment variable set on a test
  run does not reach it, and a file a test writes stays there.
- `ui-ux-pro-max` is a global skill on the owner's machine with a row in AGENTS.md. It was invoked
  in none of the 66 sessions on record; its description competes with `ui` for the same requests.

## The design

### Who checks a rule

| Checker | Holds | Where |
|---|---|---|
| `lint` | a call that may be made only inside a named place | `clippy.toml`, `disallowed-methods` |
| `guard` | a shape in the source clippy cannot tell apart | `crates/app/src/guards.rs` |
| `test` | a geometry, a value, a contrast | beside the view or the builder |
| `picture` | what only real fonts and both palettes show | the changed view, driven headless, read by a reviewer that did not write the change |
| `owner` | taste: a value, a place for a thing | `make dev` or `make showcase`, before the documents are written |
| `review` | a judgment on the whole view (one primary in a view, nothing said twice) | the same reviewer, from the diff and the pictures |

A rule takes the strongest checker that can hold it. `review` is the last resort, and the rewrite
reports how many rules rest on it.

### Lints

`clippy.toml` at the workspace root bans, with the reason clippy prints:

| Banned call | Lawful inside |
|---|---|
| `gpui::rgb`, `gpui::rgba`, `gpui::hsla`, gpui's named colours (`black`, `white`, `red`, …) | `crates/app/src/theme.rs`; the lift in `onehand_ui::tokens` |
| `gpui_component::button::Button::new` | the wrapper in `crates/ui` |
| `gpui::Styled::cursor_pointer` | the builders in `crates/ui` |

A lawful place carries `#[expect(clippy::disallowed_methods, reason = "…")]` on the item, never
on a whole module of `crates/ui`: the lint is one, so a module-wide `expect` there would license
a colour too. The 27 `cursor_pointer` calls outside `crates/ui` today each take an `expect` with
its reason in step 1. Moving them onto builders is the migration this work leaves for later, and
an `expect` fails by itself the day its call is gone.

### Guards

Source scans join the existing ones, each with its limits written in *Guard coverage*. They skip
test code, which needs a helper the guards lack today.

- **A colour is written in `theme.rs` alone.** No hex colour string and no `Rgba {` or `Hsla {`
  literal outside it and `ansi.rs`. The lint holds the calls; this holds what is not a call.
- **A size is written once, in a constant.** A `px(` or `rems(` holding a number other than zero
  stands only in a `const` item. Four places break it today. It does not read the spacing and
  type steps (`.gap_2()`, `.text_xs()`), which are the scale itself, nor a size computed from
  other constants.
- **A button refuses, it is not disabled.** No button takes `.disabled(`. Held by the lint on
  `gpui_component::Disableable::disabled` if every switch reaches it through `toggle`; else by a
  scan of an `action(` call's chain, which misses a button held in a variable or returned by a
  helper.
- **The size census.** The guard gathers every `const … : Rems | Pixels` in the UI sources (189
  today: 42 shared, 147 local), every constant in `onehand_ui::tokens` whatever its type, and
  every colour in `theme.rs`'s palette, into sorted lines of `path  name = the value as written`,
  and compares them with a list checked in beside the guards. A size added, changed or removed
  fails the test, and the failure prints the lines to add and to drop, so the list is brought up
  to date by hand from the test's own words, with nothing written on the machine that ran it. The
  list holds no prose: it exists so that a reviewer reads `sizes.txt  +3 −1` in a PR and asks why.
  A size typed as a bare `f32` outside `tokens` is outside it.

AGENTS.md gains the sentence the census enforces: a shared size or a palette value changes only
when the owner asked for it; a new local size is said in the PR.

### Strain tests

The faults were in views composed by hand, so the tests stand on the views. A helper beside the
shell's test rig draws a view at a given size, and the test reads its parts back with
`debug_bounds`; the parts are marked with `debug_selector`, which a release build drops. Each
view where a fault was reported, and `page_head`, is drawn across:

| Axis | Values |
|---|---|
| Container width | the narrowest its region allows; wide |
| Words | short; two hundred characters; two hundred with no space |
| Zoom | 100%, 150% |
| Count | none, one, one past the cap |

The first views: the title bar, a rail row, a pinned card's footer, a list inside a box that
scrolls (a project's page), the board's columns, a page's head. The test platform's text is not
the real font's, so these tests hold structure (a height, a slot that stays put, a row that does
not shrink, a part that stays inside its container) and not where a real line breaks. That belongs
to the pictures.

### Layout rules

A new family of rules, each drawn from a fault the owner reported. The seeds:

| Rule | From |
|---|---|
| A control's words never wrap | "two words and it wraps" |
| Every control in one row of actions has one height, and an icon is never clipped by its box | heights that differed; the chevron |
| A name that can run long has a bounded width and is cut by a fade or an ellipsis; it never moves or resizes its neighbours | the workspace's name breaking the bar |
| A control with a fixed place keeps it whatever stands beside it | the toggles and the search moving with the name |
| A row's trailing slot keeps its width while the name beside it truncates | hover actions, counts |
| A row of actions never wraps to a second line: its words shorten, or the least needed go behind a menu | a card's actions on two lines |
| The last item of a list stands off its container's end by the list's own inset | the last item against the edge |
| What scrolls keeps a lane beside its scrollbar, and its rows keep their height | #424, #426 |
| Content wider than its container scrolls sideways or reflows, as its pattern says, never clips silently | the board |
| A popup never covers the field it serves | the palette over its input |

The rewrite turns each into a rule that names its strain test, or a picture where no test can
hold it.

### The reviewer

A change a person can see ends with a review its author does not write. The author builds the
app, drives the changed view in the headless session (both palettes, the narrowest width its
region allows and a wide one), and hands the screenshots and the diff to a fresh reviewer. The
reviewer answers by rule id, says what it could not see, and its findings are fixed or listed in
the PR. A change to words alone, or one no screenshot can show, skips it and says so.

This replaces the checklist's self-answered questions; the owner's look in `make dev` stays, for
taste. AGENTS.md's rule becomes: the agent does not open the owner's app or desktop, and does
drive what it changed in the headless session before it says a visible change is finished.

### The rule format

An illustration; the test and its path are made up:

```
### L-03 A long name never moves its neighbours
When: a name of unknown length stands in a row or a bar beside other parts.
Then: the name takes the room left (`min_w_0`) and is cut by a fade or an ellipsis; the parts
      beside it are `flex_none`.
Unless: the name is the only part on its line.
Why: the toggles in the title bar moved with the workspace's name.
Checked by: test `a_long_name_leaves_the_toggles_in_place` (crates/app/src/shell/title_bar/tests.rs)
```

- Families: `T` tokens and type, `C` controls, `K` containers and composition, `L` layout, `S`
  states and feedback, `W` words and keys. An id is never reused.
- *Why* is one line and optional. *Unless* is the rule's only exception; a second exception is a
  second rule or a fault.
- A rule names a situation, never a screen. A sentence that names one screen (*the Issues tab's
  filter bar*, *the composer's pickers*) moves to that screen's entry in `docs/screens.md` as a
  fact about it.
- A rule a lint or a guard holds whole is one line in the rules: the id, the sentence, the checker.

### What happens to the documents

| Document | After |
|---|---|
| `DESIGN.md` | *Principles*, *Layout*, *Transcript* stay as they are. *Type, space and shape*, *Colour and state*, *Components*, *Icons* become rules `T` and `C`. The opening loses "keeps no standing exceptions" and "not values". |
| `docs/ui-patterns.md` | Becomes rules `K`, `L`, `S`, `W`. The tables that map a role to a size or a gap stay as tables, each row under a rule id. Its section headings keep their names, or the links into them (`tokens.md`, `catalog.md`, `checklist.md`) move with them. |
| `docs/screens.md` | Receives the single-screen statements. Nothing else changes. |
| `.claude/skills/ui/SKILL.md` | The procedure: find the rule ids a change touches, build, run the checks, call the reviewer. Its *Right and wrong* pairs stay. |
| `.claude/skills/ui/checklist.md` | Removed in step 5. The reviewer's brief replaces it, inside the skill. |
| `.claude/skills/ui/tokens.md`, `catalog.md` | Stay as lookups. `tokens.md` loses the values the census now holds and points at it. |
| `.claude/skills/drive/SKILL.md` | Its opening and its "only when the owner asks" follow the changed rule; the model may invoke it. |
| `docs/rules-and-gotchas.md` | *Guard coverage* gains the new lints and guards and loses "no source guard scans the visual rules"; the line restating the verify rule follows it. |
| `AGENTS.md` | The lock on shared sizes; the changed rule on verifying, where it is stated and in the `drive` row; the lines naming the checklist and "reviewed by hand"; the `ui-ux-pro-max` row goes. |

### Skills

- `ui` stays the one entry point. A second skill for layout is not written: the layout rules are
  a family in the same rules, and a skill of their own would be a second place to look.
- The reviewer is a section of `ui`, with its brief: what it is given, that it answers by rule id,
  and that it reports what it could not see.
- `ui-ux-pro-max`: its row leaves AGENTS.md. Removing the link from the owner's global skills is
  theirs to do or to ask for; it is outside this repository.

## Open for the owner

1. **May the agent drive what it changed in the headless session before it says a visible change
   is finished?** Recommended: yes. It costs a build and a few minutes on each visible change.
   Without it the lints, the census and the strain tests still land, step 3 is dropped, and every
   rule a picture would check reads *Checked by: owner*.
2. **May the rules name a product to fall back on where no rule decides?** Recommended: yes, one
   sentence: where the rules are silent, do as VS Code does. The palette and the selection grey
   were chosen that way, and the skill today forbids resting a rule on another product.

## Documents to update with the build

Listed under *What happens to the documents*. Each step carries the ones its change makes true;
the last step removes this spec and its plans.

## Steps (one PR each)

1. **Lints, guards and the census.** `clippy.toml` and the `expect`s on today's sites; the guards
   and their test-skipping helper; the census list; the four size literals moved into constants;
   *Guard coverage* and the AGENTS.md lock. Nothing a person sees changes.
2. **The strain tests.** The helper, the matrix, the tests on the first views against the seed
   rules. A view that fails a test is fixed here, and the fix is listed.
3. **The reviewer.** The reviewer's brief in `ui`, the drive scripts taking a width and a
   palette, the changed rule in AGENTS.md and in the `drive` skill. Depends on question 1.
4. **The rules, rewritten.** `DESIGN.md` and `docs/ui-patterns.md` in the rule format, the
   single-screen statements moved, each rule naming a checker that exists. The draft stops for the
   owner before it is a PR.
5. **The skill.** `SKILL.md` as the procedure, `checklist.md` removed, `tokens.md` trimmed, the
   `ui-ux-pro-max` row gone, this spec and its plans removed.

Steps 1, 2 and 3 do not depend on each other. Step 4 follows them, so that it names only checkers
that exist.

## Left to the builder

- **A probe opens each of steps 1 to 3**, run with the machine's own `cargo` rather than the
  routed one: that the bans fire under `make clippy` as CI runs it and an `expect` on a lawful
  item is fulfilled; that `debug_bounds` returns bounds for a view drawn in the shell's rig (it
  reads the frame last rendered, so a drawn element may need a redraw before it is there); that
  the headless session can hold a window at a given width and switch the palette.
- Whether the strain tests can be given a real text system (`TestPlatform::with_text_system`),
  which would let them hold wrapping too. If not, wrapping stays with the pictures.
- The widths of the matrix, taken from each region's own minimum.
- Which named colours join the ban, and whether `transparent_black()` stays free (it is no
  colour).
- How the census writes a computed constant and a name that repeats across files, and whether the
  palette's list is the same file or a second one.
- The final count and wording of the rules. The seeds above are the floor for family `L`.
