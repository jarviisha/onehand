# Design-system migration

- Status: proposal, 2026-10-12. Brings the code in line with the rules `DESIGN.md`,
  `docs/ui-patterns.md` and the UI skills were rewritten to that day. The rules already live in
  their owners; this file holds the order of the work, the decisions it needs, and what "done"
  means. It leaves `docs/proposals/` with the last step.
- Delivery: one branch, one pull request carrying the documents, a commit per step. Each step
  ends on its checks and a short summary; the user walks its acceptance cases (`make showcase`,
  `make dev`) before the next one starts. Nobody but the user launches the window.
- Every step's checks:
  - `.claude/skills/ui-review/scan.sh` prints nothing left unexplained;
  - `cargo test -p onehand --locked`, and `cargo test -p <crate> --locked` for every other crate
    touched;
  - `make clippy CLIPPY_EXTRA="-- -D warnings"` and `make fmt`;
  - the guards' ratchet counts for the files touched go down, and none goes up;
  - a state transition the step changes gets a unit test.
- Sizes: **S** under about 100 changed lines; **M** up to about 400; **L** more, or a change of
  structure or behaviour.

## Decisions this plan carries

Approved with the plan; each step below builds on them.

### Keys

- A region that takes keys of its own gets a key context: `IssuesPage`, `PluginList`,
  `QuestionCard`, `PermissionCard`, beside the existing `Rail` and the composer's.
- The validator in `crates/app/src/keymap.rs` (`validate`) learns which contexts share a focus
  stack. A window command (`Shell…`) shares one with every region; the composer's scopes nest in
  one another; the rail, the Issues page, the Plugins list and the two cards never hold one
  another, so one key may serve one command in each. A real clash, two commands in one stack,
  still fails.
- Escape stays reserved for dialogs and fields. A region command may bind it only with a context
  that excludes both (`IssuesPage && !Dialog && !Input`).
- A region command on a key without a modifier excludes `Input`, so typing in a field (the Issues
  search, a card's own answer) never reaches it.

### Settings as a page

- What saves on change (Appearance, Workspace, Connections, a binding once saved) has nothing to
  keep or discard.
- The two drafts, an agent being added or edited and a shortcut being edited, are kept when
  Settings is left by any path (a session, a page, a key, a dock) and shown again on return,
  until the form's own *Save* or *Cancel* ends them. They live as long as the window, as now.
- One rule for every path, so nothing has to guard navigation: the dialog's close question
  (*Keep editing*, *Discard*) goes, and no page-level *Discard* suggests it could undo what was
  already saved.
- Opening Settings focuses its nav. Leaving lands focus where the destination puts it (the
  composer, through `reclaim_focus`, for a session). The docks go away while it shows and come
  back as they were, as on every page.
- Esc on the page does nothing: a page is a place, not a layer. Inside a field, Esc is the
  field's.
- With the caret in a Settings field, window commands with a modifier still act; a command
  acting on a region the page hides (`Ctrl+S` saving the Workbench editor) excludes a `Settings`
  context.
- A failed write is one `error` toast; the word beside Settings' close button goes.
- Settings is never what the window reopens on.

### Closing a busy terminal tab

- Busy: the PTY's foreground process group is not the shell's own (portable-pty's
  `process_group_leader` against the child's id), that is a command running in the foreground: a
  build, a server, an editor, `ssh`. A shell at its prompt is idle, and its background jobs do
  not count.
- Unknown (the system does not say): asks, saying it cannot tell. A running command is never
  ended unasked.
- Tabs get an id, stable within their project's set. The question carries the project and the id;
  the answer looks the tab up again: gone (it exited, or was closed) means nothing happens, and
  idle by then still closes it.

## Order

Most-seen first: what is always on screen, then the pages and the Issues plugin, then the rarer
screens. The keymap's groundwork comes before any step that adds a key.

| # | Step | Size | Gate |
|---|---|---|---|
| 0 | Foundation: tokens, shared builders, guards | L | |
| 1 | Keymap: contexts and validator | M | before 7, 8, 12 |
| 2 | Transcript and the pane around it | M | |
| 3 | Composer | S | |
| 4 | Rail | S | |
| 5 | Window frame | S | |
| 6 | Pages: the overview, Tasks, a project's page, Workflows | M | |
| 7 | Issues: page, tab, detail, review, form | L | 1 |
| 8 | Workbench modes: Markdown, Files, Editor, Plugins, Neovim | L | 1 |
| 9 | Dialogs | M | |
| 10 | Settings becomes a page | L | *Settings as a page* |
| 11 | Behaviour: toasts, failures, words | M | |
| 12 | Keys and focus | M | 1 |
| 13 | Terminal: closing a busy tab asks | M | *Closing a busy terminal tab* |
| 14 | Close-out | S | every row of *Reconciliation* |

## Steps

### 0. Foundation (L)

- `crates/plugin-host/src/tokens.rs`, which the app names `crate::tokens`: `BAR_H` and
  `PAGE_WIDE` (moved from the host's `lib.rs`), `ROW_H`, `CONTROL_H`, `CODE_TEXT`, `LEADING`,
  `LEADING_TIGHT`, `PAGE_NARROW`, `PAGE_PROSE`, and `lift` (moved from `crates/app/src/theme.rs`,
  its alpha exposed for the palette's test). Every alike in `design-tokens/reference.md`
  (*Shared sizes*) points at its token; the `f32` page widths become `Rems`.
- Shared builders in the plugin host, the app re-exporting them:
  - `Refuses` and `resting`; the six `.disabled(busy)` in `workbench-plugins` become
    `.refuses(busy)`, and `confirm.rs`'s own `resting().disabled(true)` goes with the move;
  - `Ask` and `ask_on`, so a plugin confirms a removal as the shell does (`Shell::ask` stays a
    wrapper);
  - `well`, the mono well that `plain_box`, `mono_well` and the Issues wells will share.
- Guards in `crates/app/src/guards.rs`, each a ratchet that may only shrink: a colour literal
  outside the palette (none today, so a plain rule), spacing off the steps, gpui's shadow steps,
  type off the scale.
- Acceptance: nothing moves on screen but a document's line height (1.65 to 1.6) and a dialog
  title's (1.3 to 1.4); the Plugins mode's busy buttons show the arrow.

### 1. Keymap: contexts and validator (M)

- The four contexts of *Keys*, set on their regions' roots; nothing bound to them yet.
- `validate` takes the stack model and the Escape rule of *Keys*.
- Tests: the defaults still validate; a key shared by two regions validates; a key shared by a
  region and a window command fails; Escape in a region excluding `Dialog` and `Input`
  validates, anywhere else fails; for each region, `action_at` with its own stack, with an
  `Input` focused inside it, under a `Dialog`, and in the `Terminal`.

### 2. Transcript and the pane around it (M)

- Corners: `radius_tag` and `radius_control` to `sm`; `radius_block` to `radius`; `plain_box`
  (now `well`), code blocks and a question card's choice rows from `radius_lg` to `radius`.
- The status `pill` becomes a small `Tag`, its tint kept.
- Spacing on the steps (`py_2p5`, `pt_3p5`, `pb_3p5`, `px_2p5`, `gap_2p5`, `BUBBLE_TAIL_GAP`,
  `ASK_HINT_PAD`); `pl_5` in seven places becomes `FOLD_INDENT`; `.text_size(TEXT_SM)` becomes
  `.text_sm()`.
- A pinned card takes `lift`, an answered one is flat; the to-bottom button takes `radius_lg`
  and `lift`.
- The numbers left in `chat/transcript/tool.rs` and `chat/viewport.rs` are named.
- `ask.rs`'s `resting().disabled(true)` becomes `.refuses()`.
- Acceptance: a whole Mock UI turn streaming in, every block kind, fast and slow; a parked
  permission and a question pinned above the composer, then answered and drawn in the
  transcript; zoom at 80, 100 and 150 percent; a pane narrower than `NARROW_PANEL`; both
  palettes.

### 3. Composer (S)

- The popup from `radius_lg` to `radius`, like every menu; `CHIP_TEXT` to `.text_xs()`;
  `card.rs`'s `resting().disabled(true)` becomes `.refuses()`.
- Acceptance: the `+` menu, `@`, `/`, the model and effort chips; a popup opened over a parked
  card; the composer split at its narrow width; Send refusing with its tooltip.

### 4. Rail (S)

- The Tasks count and the *auto* pills become small `Tag`s; `pl_7` and `ml_5` become the
  rail's `INDENT`; *Empty* and *No sessions match* follow the empty pattern.
- Acceptance: counts at zero (none drawn) and above; every *auto* pill; a search with and
  without matches; names lined up under their project; hover against the chosen row.

### 5. Window frame (S)

- Every strip padded `px_3`: the pane's header (`px_4` today) and the terminal's (`px_2`).
- The window keeps 720 by 480 pixels at least (`window_min_size` in `shell/boot.rs`).
- Acceptance: the header's and the terminal's first items in one line; the window dragged to
  its minimum with the rail and the Workbench open.

### 6. Pages (M)

- `card_box`, a project page's conversation box and a Workflows step's box take `radius_lg`;
  `mono_well` becomes `well`.
- The Workflows page pads `p_6` and runs `PAGE_PROSE` wide; its step's number is named.
- Empty and loading lines follow their patterns (*Reading conversations…* and *Looking for past
  conversations…* become one).
- *Retry* opening its dialog reads *Retry…*.
- Deleting a conversation and discarding a workflow's edits ask through `Ask`.
- Acceptance: the overview with and without projects; Tasks with every box empty; a project's
  page without sessions; a workflow edited, then left with *Keep* and with *Discard*; zoom.

### 7. Issues (L)

- The page's head drops its title; the pane's header names Issues as it names every page,
  whether the rail shows or not (`chat/pane/header.rs`).
- The issue floating over the list takes `lift`; labels and the state badge become small
  `Tag`s; the detail's headings take `text_lg` and `text_base`, its body `text_sm`; the Issues
  wells become `well`; the numbers left in `full.rs` and `review.rs` are named.
- *Drop draft* asks through `Ask` with *Discard*; *Close issue…* through `Ask`, its word in the
  danger tint.
- *Revise…* without a note refuses with its reason, here and on the step strip.
- ↑, ↓ and Esc become `IssuesPage` commands; Esc closes the floating issue, then the review
  block and the form.
- *reading…* reads *Reading the issue…*; publishing or updating an issue shows on its button.
- Acceptance: ↑ and ↓ step the list, but move the caret while typing in the search; Esc closes
  the innermost layer only, and leaves the page in place; a draft dropped with *Keep* and with
  *Discard*; an issue closed; publishing with the forge slow and with it failing; labels in both
  palettes.

### 8. Workbench modes (L)

- `hint` draws in meta text, and the terminal's copy of it goes.
- Markdown: headings 20, 18, then 16 semibold, through the renderer's `heading_font_size`; its
  empty and loading lines through `hint`; its padding and code size named.
- Files draws a loading line through its first scan; the editor while a file opens; its number
  named.
- The editor's *discard unsaved edits* asks through `Ask`.
- Plugins: chips become `Tag`s; `py_2p5` becomes `py_2`; *Uninstall…* asks through `Ask`
  (*Keep*, not *Cancel*); a row draws focus through `focus_visible`, and its Enter and Space
  become `PluginList` commands; *Restart agent* reads *Restart the agent*; its number named.
- Neovim failing to start shows in the danger ink, as the terminal's shell does.
- Acceptance: a long document's headings; a large project's first scan; an edited file closed
  with *Keep* and with *Discard*; a plugin uninstalled and kept; the Plugins list walked by
  keyboard (the ring shows) and by pointer (no ring); Neovim missing from `PATH`.

### 9. Dialogs (M)

- The shell's dialogs (renaming, a worktree, a branch, the workflow launcher, the issue picker,
  the retry and review questions, the step strip's review and revision) follow the dialog
  pattern: the dismissing button first, then the one primary, at the default size.
- Their numbers are named (`dialogs.rs`, `dialogs/issue.rs`, `chat/pane/step_strip.rs`); their
  `resting().disabled(true)` become `.refuses()` (`dialogs.rs`, `dialogs/issue.rs`, `retry.rs`).
- Each opens with focus in its first field (the issue picker does not today) and hands focus
  back to what opened it on closing (`dialogs.rs`, `dialogs/issue.rs` do not today), or to the
  conversation when that is gone.
- Acceptance: each dialog opened by pointer and by key, closed by Esc, by its dismissing button
  and by its primary; the caret where it was afterwards; a refusing primary showing its reason.

### 10. Settings becomes a page (L)

- The lifecycle of *Settings as a page*: the rail's *Settings* and `Ctrl+,` open a page in the
  agent pane, named by the header, its nav column at the start and its content padded `p_6`.
- `field` moves into the plugin host, its hint in meta text; `section` stands `gap_4`; the
  nav's hover is `list_hover`; the numbers in `settings.rs` and `settings/pages.rs` go with the
  dialog's bounds or are named.
- The agents form's *Save* refuses with its reason; *No agents yet — add one below.* follows the
  empty pattern.
- Tests: the drafts survive leaving and coming back; a failed write reports once.
- Acceptance: Settings opened by the rail and by key, left by a session, a page, `Alt+↑`, a dock
  toggle; an agent draft and a shortcut edit, each left and found again; a failed write; typing
  in every field triggers no command; focus after leaving; the docks as they were.

### 11. Behaviour (M)

- The 36 toasts take their kind by meaning and the form *<What> failed: <why>*; saving the
  keymap reports once; a failed *Export as Markdown* gets its toast; a refusal shown as a toast
  (no project to start in) refuses on its control instead, as the rail's *New* does.
- Words: an ellipsis on every action asking first (*Delete conversation…*, *Remove from
  workspace…*, *Answer the pull request review…*); *Delete*, *Remove*, *Discard*; *Keep*,
  *Cancel*, *Close*; no period on hints and toasts. A string core puts on screen changes in
  core, with its tests.
- Acceptance: each failure path forced once (an unwritable export path, a keymap file made
  read-only); each renamed control found where it was.

### 12. Keys and focus (M)

- A control bound to a command shows its key in its tooltip: the pane's header, the terminal
  and Workbench strips, the rail.
- Each of the 14 `track_focus` sites is sorted: a panel tracks focus and draws nothing; a control
  draws the ring through `focus_visible`.
- The question and permission cards' keys become `QuestionCard` and `PermissionCard` commands.
- Esc closes the Workflows form (asking when it holds changes), the task detail and a plugin's
  details.
- Acceptance: typing in any field fires no page or card command; Esc closes the innermost layer
  only; the ring shows for the keyboard and never for the pointer; a rebound key shows in its
  tooltip; after anything closes, focus is on something drawn.

### 13. Terminal: closing a busy tab (M)

- The rules of *Closing a busy terminal tab*. Telling busy from idle is a function of the two
  process ids, with a test; finding the tab by id is a pure function beside `selection_after`,
  with tests.
- Tests: a tab gone while its question is open makes the answer do nothing to the others; the
  selection stays on the shell it was on.
- Acceptance: an idle tab closes at once; a tab running `sleep 100` asks, *Keep* leaves it
  running, *Close* ends it; the shell exiting while the question is open; switching project while
  it is open.

### 14. Close-out (S)

- Every row of *Reconciliation* done, or moved to `docs/known-gaps.md` with its reason.
- The ratchet lists that reached zero go; their guards stay as plain rules.
- The skills drop *Being merged*, *Migrating old values* and their notes about the migration.
- `scan.sh` run against `main` over the whole branch; this file is deleted.

## Reconciliation

Every departure the survey found, the step that ends it, and how its end shows.

| Departure | Step | Done when |
|---|---|---|
| Lengths allowed by `every_length_has_a_name`: `chat/transcript/tool.rs`, `chat/viewport.rs` | 2 | their entries leave the list |
| the same: `dialogs.rs`, `dialogs/issue.rs`, `chat/pane/step_strip.rs` | 9 | as above |
| the same: `workflows_page/step.rs` | 6 | as above |
| the same: `settings.rs`, `settings/pages.rs` | 10 | as above |
| the same: `workbench-issues` `full.rs`, `review.rs` | 7 | as above |
| the same: `workbench-markdown` `document.rs`, `workbench-files` `view.rs`, `workbench-plugins` `details.rs` | 0, 8 | as above, and the list is gone |
| The ratchets step 0 adds (spacing, shadows, type) | 2–10 | every list empty |
| Size alikes kept apart (*Shared sizes*) | 0 | reference.md lists one home each |
| Small labels drawn by hand: `pill`; the rail's counts and *auto* pills; Issues `chip`, `tag` and state badge; Plugins `chip`; the issue picker's labels | 2, 4, 7, 8, 9 | none left; catalog's entry gone |
| `hint` copied into `terminal.rs` | 8 | one `hint` |
| Wells drawn apart: `plain_box`, `mono_well`, the Issues review, diff and conflict wells | 0, 2, 6, 7 | every well is `well` |
| Removals confirmed by hand: the project page, Workflows, the editor, Plugins, Issues; Settings' close question | 6, 7, 8, 10 | `open_alert_dialog` left only in `Ask` and the retry and review questions |
| Refusals without their cursor or reason: the Plugins buttons, `resting().disabled(true)` (8 sites), the agents form's *Save* | 0, 2, 3, 9, 10 | `.disabled(` only inside `refuses` |
| Corners off their role; shadows from gpui's steps | 2, 3, 6, 7 | the scan prints none |
| Type off the scale: Issues `CHIP_TEXT`, headings, `BODY_TEXT`, the page's head; the composer's `CHIP_TEXT`; Markdown's headings | 3, 7, 8 | the type ratchet is empty |
| Strip padding; no minimum window size | 5 | one padding; the minimum holds |
| Page padding and widths | 6 | every page on `p_6` and a `PAGE_*` |
| Settings as a dialog; its hover by alpha | 10 | the page as *Settings as a page* says |
| Loading drawn four ways; no loading in Files, the editor, publishing an issue | 6, 7, 8 | every read and action shows its pattern |
| Empty lines drawn four ways | 4, 6, 7, 8, 10 | every one meta text, no period |
| A failure only logged (*Export as Markdown*); a failure reported twice (Settings, the keymap) | 10, 11 | each failure reported once |
| Toast kinds and wording drifting (36 sites) | 11 | every toast reviewed |
| The same refusal as a toast and as an inline warning (*Say what to change*); a refusal as a toast (no project) | 7, 11 | each refuses on its control |
| Neovim's failure in the warning ink | 8 | the danger ink, as the terminal's |
| Library default buttons in the Issues questions | 7 | through `Ask` |
| Keys outside the keymap: Issues, Plugins rows, the two cards | 7, 8, 12 | no `on_key_down` but for typing |
| Bound controls whose tooltips hide the key | 12 | every one shows it |
| Focus: the issue picker opens without it; dialogs that do not hand it back; Esc ignored by the Workflows form, the issue form, the review block, the task detail, a plugin's details | 7, 9, 12 | each case of steps 9 and 12 passes |
| A focus ring drawn by `.focus` in the Plugins rows | 8 | `focus_visible` |
| Words: ellipses, verbs, dismissals, periods | 6, 7, 8, 11 | every on-screen string reviewed in its step |
| Closing a terminal tab ends its command unasked | 13 | the case of step 13 passes |
| Stop, Dismiss, taking an item off the queue, Unbind, Reset, removing a label: removals left unconfirmed | none | the rule says these go ahead without asking; nothing to change |
