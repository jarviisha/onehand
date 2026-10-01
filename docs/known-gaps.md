# Known gaps in this build

Listed because a missing feature nobody wrote down reads as a bug in the ones that exist:

- **The header's icon buttons have no accessible names, the conversation menu's dots included.**
  The library builds a button's accessible name out of `label` and nothing else, and the only
  setter is an inherent method on the base button it keeps in a private field — so an icon-only
  button cannot be given one through this component. Every control on the row carries a tooltip
  instead. The menu used to be the name itself, drawn as a child so it could ellipsize, which was
  the same gap in a worse place; moving the menu onto the vertical-dots mark made the name plain
  prose and left one uniform row of tooltipped icon buttons. **The route out is
  `controls::MenuTrigger`**, which exists already for the rail's rows: `Stateful<Div>` does
  implement gpui's `StatefulInteractiveElement`, so `aria_label` reaches it, and what that costs is
  rebuilding by hand what the component gives for free — the ghost hover fill, the icon sizing and
  the selected-while-open state. Recorded rather than done, because it is the same rebuild at every
  one of the row's controls.
- **There is no search in the transcript**, and the removal was deliberate rather than pending. It
  matched whole *items* and never occurrences, so a word said ten times in one answer was one hit
  with no mark on the word itself, and a hit in text a block had truncated was counted, scrolled to
  and still not on screen. Worse, the two halves disagreed about what the transcript *is*: the pass
  read the whole model while the render plan drops a parked permission or question, which are drawn
  above the composer — so a query matching one was counted in "3 of 7" and Next moved the number and
  nothing else. Rebuilding it means per-occurrence offsets through the markdown renderer, which is
  the same span machinery a drag-selection across blocks would need; the two should be built
  together or not at all.
- **A lost adapter is reported by the rail's mark alone.** Nothing else on screen says it: the
  conversation header used to carry the same condition in words, and the badge that did went with
  everything else that row was saying twice. It is deliberately kept off the desktop too — an agent
  that has stopped answering is a standing condition rather than news, and a notification for one
  would fire again on every reconnection attempt. So with the rail hidden (`Ctrl+Shift+B`) or a panel
  maximized, a dead agent is announced nowhere at all. The two ways out, neither taken yet, are to put
  the mark back on the header for that one condition — not the whole badge, since what the badge
  otherwise said is on the running line already — or to let this one kind of news reach the desktop
  after all, which means deciding how often it may repeat.
- **No command palette** (`Ctrl+Shift+P`). It is a feature — a command registry plus a filtered
  popup — not a keymap entry.
- **The completion popup has no argument step.** A command that takes one is accepted like any
  other and leaves the caret after it; there is no chip for the chosen command, no trailing chevron
  saying an argument is coming, and no Backspace-returns-to-the-list. `Tab` accepts the highlighted
  row rather than completing the common prefix. The keys that do work — `Up`/`Down` wrapping,
  `Enter`, `Esc`, and a click, which go through one router so neither can act where the other
  cannot — are bound `ChatComposer > Input` and claimed only while a list is open, since gpui's
  dispatch reaches the focused input before any outer wrapper.
- **An accepted mention is plain text, not a token.** It inserts the whole path, so a long one is
  as wide as it reads; there is no single-unit deletion and no hover carrying the full path. That
  needs the input to own a span it treats atomically, which `Input` does not offer.
- **A turn ending settles the steps it left in flight** (`Chat::settle_running_steps`,
  beside `cancel_pending_permissions`). Nothing more arrives for a call the adapter never
  finished — a cancelled turn is the ordinary way that happens — so a step left `InProgress`
  stays that way for the rest of the conversation, and everything downstream reads it as
  live: its cluster says it is still running and never reports how long it took, and the
  line at the foot of the transcript counts it among the steps in flight for every later
  turn. It settles to `Failed` and not `Completed`: what is known is that it never reported
  finishing, and a card claiming a write went through is the one reading a transcript
  cannot recover from.
- **An exit status only exists for a command run through ACP's terminal extension.** The protocol
  carries one nowhere else, so an adapter reporting a failure as a plain `tool_call` has no code to
  give and the row says `failed` rather than `exit N`. Recovering it from the output was considered
  and refused: the code is in the footer this app itself appends, so parsing it back is parsing our
  own wording, and a number got that way is wrong the first time the wording moves. `ToolItem`
  carries it instead, lifted off the terminal at the one moment both are in hand — the turn-end
  flatten, after which the terminal is gone. `mock_terminal_agent.js` exits 101 on purpose so the
  path is reachable without breaking a real build.
- **A step's duration is stamped once, when it settles**, and only for work that arrived unfinished:
  a step that was already `completed` when it reached this process was timed by whoever ran it, and
  a clock started here would be measuring the wire. Both facts persist into the archive as optional
  keys, so a conversation written before they existed still loads and simply has nothing to say
  about either. Nothing reads the duration per row — it is summed onto the line standing for the
  cluster, where one number answers "how long was that" without twenty rows each answering it.
- **A turn's closing summary is derived, never persisted.** A finished turn ends
  on a block saying how many files it wrote, the turn's `+N −M` and how long it
  took, opening into a row per file — `onehand_core::chat::turn_changes` over
  that turn's own steps, rebuilt on every replan rather than written into
  `items.jsonl`. The diffs it adds up are already in the archive, and that file
  is appended to and never revisited, so a copy written at the end of a turn
  could not be corrected if the two ever disagreed. It is one row per *file* and
  not per edit: a turn that writes, tests and writes again is one row, because
  the question is what is different now and the route is what the clusters above
  it already are. A cancelled turn still gets one; a running turn does not, since
  a total growing under the eye is not a summary. Opening a file row diffs that
  file **at that moment** (`turn_file_diff`, first `old` against last `new`) and
  never during the replan — a conversation holds every turn it has had, and
  diffing all of them on the chance one is expanded is work paid a thousand
  times to be used once.
- **What the summary block cannot say, and where the data would have to come
  from.** *Renames* are absent because ACP's diff section is `{path, old, new}`
  and carries no second path — an adapter reports one as a delete and an add,
  so a fourth verdict would be one `turn_changes` could never return. *Test and
  lint results* are absent because nothing in the protocol is structured: a run
  is a `tool_call` whose output is text, so counting passes means parsing
  `cargo`/`jest`/`pytest` prose, which is a rule that is wrong the first time a
  tool changes its wording. The place for it is the ACP layer — a structured
  result on `ToolCall`, filled either by an adapter that knows what it ran or by
  a declared per-tool parser — not a scan of the transcript. *Undo* is absent
  because nothing in the app writes files back: the first `old` of each path in
  a turn is the snapshot it would need, so the missing half is a write path plus
  a second snapshot for undoing the undo, and both belong in core beside
  `editor::save_blocking` rather than in a renderer. A *suggested commit
  message* is absent for a different reason — writing one means asking the
  model, and the mechanical sentence a renderer could manage ("Update 3 files")
  is worse than none; there is no commit path either, only `gitstat`'s read.
  And `Deleted` is a **guess**: a file emptied and a file removed arrive as the
  same thing, a diff section whose new text is empty, so the letter on the row
  reads the commoner of the two while the counts and the bar stay right either
  way.
- **The remote bridge does not stream the transcript.** A finished turn carries the *end* of the
  agent's last answer (`Chat::answer_tail`) and nothing else: no tool cards, no diffs, no reasoning,
  nothing mid-turn. That excerpt is there because "finished a turn" alone is a notification whose only
  content is that there is content — it costs a walk back to the machine to find out whether anything
  needs doing — and the close of an answer is where it says what it did. Carrying the whole
  conversation is a different feature with its own questions (what a tool card becomes there, what a
  diff looks like, what happens to an answer longer than a message), and half of it would be worse
  than none.
- **Only Telegram.** The layer underneath is general and `RemoteChannel` is what a second one would
  implement, but nothing else does. There is no Discord adapter and no HTTP endpoint.
- **Only the agent's prose can be selected with a drag.** The one selectable thing in the
  transcript is what goes through `TextView`, which is a *markdown* renderer — so a command, an
  output, a diff and the **user's own prompt** are all plain elements a drag slides straight past.
  Neither of the two could simply be routed through it. A diff's three columns are layout and
  markdown has no notion of them; and a prompt is drawn *as typed* on purpose, so rendering it would
  turn `**/*.rs` into bold and a backtick into a code span — the transcript misquoting the person
  who wrote it. Each carries a Copy for the whole of what it holds instead, the prompt's sitting
  outside its bubble rather than over the one short sentence it is offering.

  **Two ways out exist, and both were weighed and declined for now.** The component library does
  ship a selectable plain-text control — `Editor` (and `TextArea`) with `.readonly(true)`, which
  *"keeps the normal appearance and still can be focused, selected and copied, it only rejects the
  changes made by the user"* — and with `.appearance(false)` and no explicit height it would sit in
  a bubble and grow with its text. What it costs is an `Entity<EditorState>` per prompt cached on
  the session (today that cache holds a handful of live cards, not every message of a long
  conversation), a click on a prompt taking focus off the composer, and selection that still stops
  at each block's edge — a diff stays unselectable either way.

  The thorough one is `gpui_base::TextSelectionHandle` with `TextSelectionRegistration` /
  `TextSelectionRun`: a document-wide selection spanning arbitrary elements, which is what
  `TextView` itself is built on, and under which sit `gpui::InteractiveText` and
  `TextLayout::index_for_position` — where `vendor/gpui-terminal` gets its own. It keeps the diff's
  columns, needs no per-message state and steals no focus, and costs one custom element: a hitbox,
  runs projected from a `TextLayout`, and the highlight painted behind the glyphs.
- **`path:line:col` tokens in agent prose are not clickable.** The transcript renders prose through
  `TextView::markdown` and does not scan it for path tokens. Only a tool card's path header opens a
  file, and it carries no line — ACP's diff payload has no hunk offsets. Core holds no parser for
  these tokens either: the feature is the detection pass, and a parser written ahead of it is a
  guess at an interface nobody has designed.
- **A fenced code block inside prose has no header, and cannot fold independently.** What the
  renderer opens to a caller is one `StyleRefinement` for the container and one closure for a box it
  pins to the top-right corner itself — so the surface (edge, corner, padding, size, leading) is
  ours, and a header *row* carrying a file path, a language and a copy is not: there is no slot
  above the code to put one in, the copy's position is written by the library, and
  `TextViewStyle::code_block` is one style for every block with nowhere to keep per-block fold
  state. The language is said in the corner box instead, since that is the only slot there is.

  **Owning the block is reachable and costs selection.** `TextView::markdown_block_parser` runs
  *before* the built-in conversion and can intercept `mdast::Node::Code`, and
  `markdown_block_renderer` then draws it — that is the supported hook, and `SyntaxHighlighter` is
  public so highlighting survives. What does not is selection: the element that carries it,
  `text::Inline`, is `pub(crate)`, so a hand-rolled block would draw `StyledText` and lose the drag.
  Trading a header for the ability to select code is the wrong way round.

  **The comment scope cannot be retinted either.** `TextViewStyle::highlight_theme` is a public
  field, but `ThemeStyle::color` inside it is private with no setter and no constructor — reachable
  only by round-tripping through its `Deserialize`, which means writing a colour literal back in,
  and a literal is the one thing the theme exists to stop.
- **The terminal has no `APP_KEYPAD`.** The numeric keypad's application mode is unimplemented,
  because gpui does not report a keypad key differently from the digit above it. The keys work; they
  always send the ordinary form. The rest of the required full-screen terminal behaviour is present.
- **The terminal's cursor does not blink**, by decision — it would mean a repaint on a timer for the
  life of every tab, in a view that otherwise draws only when bytes arrive.
- **The Plugins mode stops where the command line does.** It manages Claude Code's plugins only —
  no other agent's, so there is no agent picker and no per-agent enabling. There is no auto-update
  switch and no roll-back, since the command line offers neither and the mode writes none of
  Claude Code's files; for the same reason a scope once set on or off cannot be set back to saying
  nothing short of editing its `settings.json`. A plugin installed by managed settings is not
  listed, because the listing names a scope this build does not read. *Change scope* is an install
  and a removal, not one step. The pending banner counts only changes made in this mode, so one
  made from a terminal is not announced. The scope *Install* remembers lasts for the session. The
  catalog carries no dates and no component list before install, so there is no sort but
  popularity and no filter by kind. And the title row of a submenu (*Change scope ▸*, *Turn on for
  ▸*) draws the arrow cursor: the menu row that answers the pointer can only reach what goes inside
  a row, and a submenu's row is the library's own.
