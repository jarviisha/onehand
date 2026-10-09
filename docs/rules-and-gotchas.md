# Rules and gotchas, in full

The full text behind the short forms in `CLAUDE.md`, with the reason for each.

## Rules

- **Every icon is an SVG, and nearly every UI glyph comes from `gpui_component::IconName`.** No Unicode or
  emoji glyphs as icons. `IconName` is generated from the SVGs `gpui-component-assets` ships, which
  is also what the library's own components reach for in ~97 places — so that set has to stay loaded
  regardless. Three things this costs, all silent: the library **renames icons when it packages
  them** (its `close` is Lucide's `x`, its `delete` is the backspace key), an icon that fails to
  resolve draws *nothing* rather than failing the build, and **a shipped SVG can carry a hard-coded
  `stroke`** — `dash` is `minus` drawn again with `stroke="black"` written in, so it ignores
  `text_color` and comes out invisible on a dark panel while the identical `minus` beside it uses
  `currentColor`. Two names for one drawing is reason enough to check which; a colour baked into one
  of them is reason enough to check every time — `grep -l 'stroke="black"\|stroke="#'` over the
  bundled set answers it in a second, and today finds exactly `dash.svg` and `resize-corner.svg`,
  neither of which the app draws. Bumping the `gpui-component` rev means
  looking at the app's chrome afterwards, and re-running that grep.
  `crate::icons` holds **only what that enum cannot draw**: a shape the bundled set has no drawing
  of at all, and a brand mark, which belongs to the product it stands for rather than to a
  general-purpose UI kit. Plus one narrow exception, the `-light` entries: a shape the bundled set
  *does* draw, forked for its **stroke weight** alone. That weight lives inside the SVG and no API
  reaches it, so a glyph drawn much larger than the app draws glyphs anywhere else cannot be made
  lighter without a second copy. The composer's action row is that place — 1.25rem against about
  0.75rem everywhere else, where Lucide's stroke of 2 reads as a marker pen — and the library's own
  copies stay in use at every other call site, so the app carries two weights split by *how big a
  glyph is drawn* rather than by which glyph it is. When a call site stops drawing oversized the
  override goes with it: `square-slash` carried one while it was a button in that row and lost it
  on moving into the `+` menu, where it stands beside two icons at the library's own weight. The weight is declared in the manifest's
  `[stroke]` table and written in by `sync-icons.sh`, never edited into a file by hand: that script
  refetches every checked-in SVG, so a hand edit is one the next sync throws away in silence. It is a handful of shapes, each with its reason in the manifest, and **no brand marks** — the one there was, for
  the default agent, sat in the binary drawn by nothing, which is what the registry's
  `allow(dead_code)` guarantees nobody will ever notice. So an entry is added when a call site needs
  it, never in advance.
  An `IconName` whose *name* reads oddly does not qualify; a missing drawing does.
  To add one: update [assets/icons/manifest.toml](../assets/icons/manifest.toml) with the reason beside
  the entry, run [scripts/sync-icons.sh](../scripts/sync-icons.sh) (it knows Simple Icons for marks and
  Lucide for shapes), register it in the `icons!` macro. A test fails if manifest and registry
  disagree.
- **Code describes; it never cites.** No comment, doc comment or runtime string may name another
  document — not `CLAUDE.md`, not `DESIGN.md`, and no
  section number, anchor or item code belonging to one. The guard's list of forbidden names is
  deliberately longer than the set of documents that exist, because a name that was retired is
  exactly the one a stale comment would still be holding. Say the reason **in the comment's own
  words**, in full, so the comment stands alone.

  Two reasons. A citation *decays*: reorganize a document and every pointer at it silently starts
  aiming at the wrong place, and a confidently wrong pointer is worse than none. And a citation
  *tempts* — it lets a comment gesture at an explanation instead of giving one, so the reader ends up
  holding two files to understand one line. If a rule is worth a comment, the comment is worth
  writing out.

  **Still fine:** pointing at *code* — `[`crate::icons`]`, `gpui_component::dock`,
  `dock/tab_panel.rs:775`, an upstream rev. Those are checkable, and rustdoc links break the build
  when they rot. The rule is about prose that lives in a document.

  The traffic runs one way: **documents point at code, code does not point back.** Line comments
  are checked by `guards::tests::code_never_cites_a_document`; other forms need review.
- **Every `.md` file in this repo is written in English.** Not a style preference: these documents are
  the binding contracts, they are read alongside source that is entirely in English, and half of what
  they explain is quoted identifiers, compiler messages and upstream prose that has no translation.
  A file split across two languages is one that gets read in neither — the reader has to switch, and
  the terms stop matching the code they name. This covers prose, headings, tables and comments inside
  fenced blocks; a quoted string that is itself Vietnamese (a test fixture, a bug report being cited)
  is data and stays as it is. `guards::tests::documents_are_written_in_english` detects a subset
  of Vietnamese diacritics, not languages or quotation boundaries; a legitimate quoted fixture
  may need a narrowly scoped guard adjustment rather than a translation.
- **DESIGN.md is binding.** Read the theme; never hard-code a colour, radius or
  size. Sizes are rems.
- **Reuse gpui-component before building.** A hand-rolled equivalent will not follow the theme, will
  not follow the focus rules, and becomes ours to maintain.
- **Keep rendering bounded**, and say on screen when a bound bit.
- **Split a file before about 800 lines of production code, excluding tests.** The limit is a
  prompt to check responsibilities: a reader should be able to understand a strip, dialog,
  page section or parser without loading the rest of a growing file. Move inline tests to
  `foo/tests.rs` first, then extract a responsibility into `foo/bar.rs` or a sibling module
  when the production code needs it. Keep the interface narrow and the implementation private.
  Make the split in the change that would cross the limit, so the next change starts with a
  usable seam. The number is approximate; splitting one coherent operation arbitrarily only
  makes its control flow harder to follow. Source guards do not enforce a line-count limit.
- **Don't self-verify UI by launching or screenshotting.** Make the change, make sure it builds and
  tests pass, then stop — the user inspects the result visually.

## Guard coverage

The source guards catch recurring mistakes; passing them does not establish the whole contract.
Review the uncovered part when changing the relevant area:

| Rule | Automated coverage | Still needs review |
|---|---|---|
| Exhaustive handling of our events | Finds `matches!` naming discovered `pub enum *Event` types in UI sources | Partial `if let`, wildcard match arms, aliases and other enum declarations |
| Code does not cite documents | Checks `//` lines against document names and citation patterns | Block comments, runtime strings and citations outside those patterns |
| Fields have readers | Scans field-name uses; comments in the guard explain exceptions | Collection mutation counted as a read and unrelated fields sharing a name |
| Registry icons and button wrapper | Finds listed glyph literals and direct `Button::new(` spelling | Other glyphs, aliases and differently formatted construction |
| English documents | Detects selected Vietnamese diacritics | Other languages and quoted data |
| Named sizes | Finds a non-zero number written as the first argument of `rems(` or `px(` outside a `const`, ratcheted per file for the files that predate the rule | Arithmetic on a number (`x * 0.5`), sizes passed through other helpers, and the listed files until their counts reach zero |
| Theme, bounded rendering, module size and UI contract | Dedicated tests cover individual behaviours | No general guard proves these rules or synchronizes documentation with code |

Keep the checks that catch real regressions. Add a targeted check when a concrete failure
justifies it, and describe its limits rather than treating a source scan as a Rust parser.

## Gotchas

- **Key bindings beat `on_key_down`.** GPUI matches bindings against the focus context stack first and
  only delivers the key to focused elements if nothing matched. That is why the app keymap reaches
  over a PTY with no cooperation from the terminal widget — and why binding a key the terminal needs
  silently takes it away. A `!Context` predicate means "that context appears nowhere in the stack".
- **`with_rem_size` must be set in all three element phases.** `request_layout` is where rem sizes
  become numbers, but `prepaint` and `paint` re-resolve some of them; overriding in one phase gives a
  subtree measured at one size and painted at another.
- **A font family is a request, and a missing one fails silently.** gpui-component's default
  `mono_font_family` is one hard-coded name per platform, and on Linux it is DejaVu Sans Mono — which
  plenty of distributions do not ship. Every well in the transcript then drew in the body face while
  the code drawing it was, correctly, asking for mono, with nothing on screen or in the log to say
  the request went nowhere. `use_installed_mono` (`shell/boot.rs`) picks a family from
  `cx.text_system().all_font_names()` once at boot; the choosing rule is
  `onehand_core::config::resolve_monospace`, which is pure and tested. **Never assume a family name
  resolves** — check it against the enumeration. The terminal is the sharpest case: its grid is
  *measured* from a shaped glyph, so a family that does not resolve does not merely change the
  typeface — the cell is sized from one font while the row is drawn in another and every column lands
  past its glyph. `onehand_terminal_ui::spawn_pty` hands the grid the resolved family for exactly that reason;
  the vendored default is the string `monospace`, which is a CSS generic and not a family anything
  enumerates. The app now ships its two families (`crate::fonts`, registered at boot before the theme
  names them), so the theme's defaults always resolve; the scan still guards a family
  `[font].monospace` names. A system family also brings its own metrics: SF Pro Display, the
  `sans-serif` on one machine, sat every button label a pixel under its icon.
- **`use super::*` in a test module inside `vendor/gpui-terminal` breaks `#[test]`.** That file imports
  gpui with a glob, and gpui exports an attribute macro of its own called `test`. Globbing it into a
  test module shadows the built-in attribute, and `gpui::test` expands to code carrying `#[test]` —
  which resolves to `gpui::test` again, until rustc gives up with *"recursion limit reached while
  expanding `#[test]`"*. Nothing in the message points at the glob. Import the two or three items the
  tests actually need by name. This is what upstream's note about "macro expansion issues with the
  test attribute" was, and it is why `view.rs` had no tests at all.
- **Whatever the grid's paint does per cell is multiplied by the screen, and the screen is redrawn
  whenever bytes arrive.** A modal editor redraws the whole grid on every keystroke, which is what
  turns a cost a shell hides into typing latency. The paint is now per *run* rather than per
  character: `render::split_row_runs` groups the cells of a row that share a face, a colour and a
  decoration, and each group is one `shape_line` — a row of source costs a handful instead of eighty,
  and a visible-row cache retains those shaped lines until the cells or rendering settings change.
  Three hazards make batching
  wrong in a cell grid, and each has an answer: shaping asks for a **forced cell width** so gpui snaps
  every base glyph to its own column, `TerminalRenderer::face` builds the faces with **contextual
  alternates off** so no font can fuse two cells into one ligature glyph, and a **double-width
  character is a run of its own**. Under that, the per-glyph allocations still matter and are still
  gone — the text (`render::ascii_glyph`) and the `Font` (`TerminalRenderer::font_variants`).
  `render::RowCache` compares complete visible rows, then snapshots and rebuilds backgrounds,
  box commands and runs only for changed inputs. Changed rows retain shaped runs whose text and
  style still match, updating their vector in place. It also observes direct mutable-grid edits without
  consuming shared damage flags. Renderer clones share this bounded visible-grid cache; font,
  metrics, scale, palette, OSC colours, dimensions or window changes invalidate it. Selection,
  preedit and cursor remain live overlays. This saves CPU assembly and layout lookups, but GPUI
  still submits every visible primitive on each frame. **Measure before assuming shaping is the
  cost**; the original pass found allocations around it, and later profiling justified row caching.
- **A repaint asked for by a terminal is the whole window redrawing, not the grid.** So the reader
  task drains already queued reads with `view::take_batch`, asks once, and yields before continuing
  the next bounded batch. It must not sleep after asking: the former 8 ms pause held the tail of a
  Neovim redraw until after its first frame, forcing another frame for output already waiting in the
  queue. Idle stays push-based, parked on the channel with no output timer. The other half is
  `view::RepaintGate`: **ask once, then wait to be drawn before asking again**. A grid on screen is
  drawn within the frame; batches arriving before that draw share its request, while a closed dock is never
  drawn — which is what stops a `cargo build` running behind a closed terminal from repainting the
  conversation sixty times a second. Being rendered is the whole signal; there is no timer to cancel
  and a grid that comes back on screen re-arms itself by the act of returning. Being wrong about it
  costs the saving and nothing else: a host that renders an off-screen grid anyway leaves the gate
  permanently open.
- **Parsing runs on the main thread, so a batch is the UI held.** `cx.spawn` is the foreground
  executor. Two things follow. The drain has a bound of its own (`view::PARSE_BATCH_CHUNKS`) well
  under the channel's, because the channel's answers a different question — how far the reader may run
  ahead of the parser — and draining it whole meant a megabyte of escape sequences inside one update.
  And every batch yields (`view::YieldOnce`): `flume`'s receive completes without touching
  the executor when a message is already queued, so a child outrunning the loop would otherwise be
  parsed in back-to-back batches with the keyboard never getting a turn. A zero-length timer does not
  do it — gpui answers that with an already-complete task.
- **A keystroke is not a repaint.** Typing does not draw itself: the child echoes it and the echo
  repaints. The only thing typing changes on its own is what `view::write_typed` does first — snapping
  a viewport parked in the scrollback back to the bottom, and dropping a selection about to stop
  describing what is under it — and neither is true at a prompt or under a held key, which is where
  typing happens. So the repaint is asked for only when `view::typing_changes_the_view` says one of
  those two was true; unconditionally, every keystroke cost two whole-window repaints and one of them
  drew the frame already on screen. Pasting asks for none at all, because `write_paste` hands bytes to
  the child and touches nothing.
- **The measured cell has to reach the view, not only the paint.** `TerminalRenderer::measure_cell`
  needs the window, and the window exists only inside the canvas paint — so it runs on a *clone* of
  the renderer, and writing the result back to the view's own copy is a separate step. Skip it and
  every pixel-to-cell conversion the view does divides by the constructor's guesses instead
  (0.6 and 1.4 times the font size). **Nothing about the drawing looks wrong**, because the drawing
  uses the measured clone; what is wrong is everything aimed *at* the drawing — the cell a click lands
  on drifts further from the pointer the lower down the grid it is, and the height guess is out by
  more than the width one, so the drift is mostly vertical. It reads as a context menu appearing in
  the wrong place, or a drag selecting the wrong line, rather than as a measurement that never
  arrived.
- **`mx_auto` does nothing inside a `gpui::list` row.** The list lays every row out as its own
  *layout root*, and a root has no containing block for an auto margin to take its share of, so the
  margin resolves to zero — silently, with no warning and nothing wrong-looking in the row itself.
  Centre a list row with a flex parent (`h_flex().justify_center()`) around a `max_w` child instead.
  This cost a round trip once: the transcript sat against the left edge while the composer, centred
  inside an ordinary flex column, sat in the middle of the panel, and the two halves disagreeing was
  the only symptom.
- **A panel's focus handle is tracked by the dock, not by the panel — unless it has no tab group.**
  gpui-component's `TabPanel` calls `track_focus` on the active panel's handle, so `contains_focused`
  works without the panel adding it, and a panel that adds `track_focus` on top of that becomes
  doubly click-focusable. A `DockItem::Panel` renders bare, so nothing tracks it and
  `contains_focused` answers "no" however deep inside the pane the caret is — which silently points
  every focused-panel key (maximize, zoom) at the wrong panel. Every panel here renders bare, so
  `ChatPane::render`, `TerminalPanel::render` and `Workbench::render` all track their own handles —
  and one added later that forgets is a panel its own shortcut cannot find. Focus-on-click stays
  correct either way: gpui's handler runs in the bubble phase and an inner focusable takes the click
  first and calls `prevent_default`.
- **A panel closed while it holds focus takes the whole keymap with it.** GPUI resolves a key along
  the path from the dispatch tree's root down to the *focused* node; with nothing focused that path is
  the root alone, and every `on_action` the shell hangs on its own frame sits below it, unreachable.
  So unmounting the terminal — or closing the Workbench dock — with the caret inside leaves a window
  where no shortcut works at all, including the one that would reopen the panel. It reads as "the key
  only closes it, never opens it", which is nothing like a focus bug and sends you looking at the
  binding. Both close paths call `ChatPane::reclaim_focus`, which asks *before* the panel leaves the
  frame, since a handle that is not drawn cannot answer `contains_focused`. Any new panel that can be
  taken off screen owes the same call.
- **Zoom factors must snap to the step.** Binary floating point does not round-trip `1.0 - 0.1 + 0.1`,
  so an unsnapped factor drifts and `Ctrl+0` becomes the only way back to 100%.
- **Box-drawing strokes stay in the quad pass.** Straight segments use fill quads; rounded
  corners use a transparent rounded outline clipped to the cell plus its existing overlap.
  The corner is circular, with radius based on the smaller cell dimension. Its border widths
  match snapped straight-stroke edges, including at fractional scale. Reintroducing `PathBuilder`
  for these corners restores the intermediate GPU path passes that make rounded Neovim borders
  expensive. Keep the parent clip and transparent interior so selection and cell backgrounds survive.
- **`vendor/gpui-terminal` is a vendored render core plus the interaction layer upstream never had.**
  Scrollback, selection, copy/paste (`Ctrl+Shift+C/V` — plain Ctrl+C is SIGINT and Ctrl+V is
  literal-next), bracketed paste, copy-on-select, typing-snaps-to-bottom, mouse reporting and its
  `Shift` bypass, terminal replies going back to the PTY, `DECSCUSR` cursor shapes and the modified
  key sequences are all onehand's, marked
  `onehand patch`. Upstream is `zortax/gpui-terminal@51f0292`; the verbatim import is one commit and
  the patches the next, so the delta stays readable. `gpui` there is a **revless** git dependency:
  cargo keys a git source by URL plus rev, so any rev (or crates.io) yields a second `gpui` in the
  graph and "expected gpui::App, found App".
- **`gpui` carries no rev anywhere**, for the same reason; `gpui-component` *is* pinned by rev, and
  `Cargo.lock` is the pin for both.
- **Native dialogs and file scans run off the UI loop** — pickers, the `@`-mention scan and directory
  scans go through `cx.background_executor()`, never inline in a render or an action handler.
- **IME can swallow a typed `/` on Linux.** With a Vietnamese IME enabled, the composer may never
  receive the character, so the slash-command popup cannot be opened by typing. The workaround is the
  *Mention a file* and *Run a slash command* entries in the composer's `+` menu, which insert the
  trigger *from code* (`Composer::insert_trigger`) and bypass the IME. Keep them: they are not a
  convenience, and each has to keep drawing the character it types — for the user who cannot type
  it, the row is the only thing on screen naming it.
