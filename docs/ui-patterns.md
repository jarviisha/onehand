# UI patterns

The patterns every screen repeats, in the app and its built-in plugins: how a screen is put
together, and how it behaves while it waits, fails, refuses or removes something. `DESIGN.md`
owns the window's layout and the visual rules these patterns are drawn with; a feature's own
document holds its screens' detail. Code never cites this file.

## Layout

### Window

The window keeps the rail's minimum beside the conversation's: it is never smaller than 720 by
480 pixels. Its regions are `DESIGN.md`'s.

### Strips

A strip along the top of a region is `BAR_H` tall and padded `px_3`: what it names at the start,
its controls at the end as `small` ghost buttons. An icon-only control carries a tooltip.

### Pages

A page takes the agent pane, its name in the pane's header. Its body scrolls as one column centred
in the pane, padded `p_6`, its sections `gap_6` apart, as wide as one of:

- `PAGE_NARROW`: one column of fields or rows (a project's page);
- `PAGE_PROSE`: an editor, or one thing read in full (Workflows, an issue);
- `PAGE_WIDE`: boxes side by side, or a table (the overview, Tasks, Issues).

A page holding sections of its own (Settings) puts their nav column at its start.

### Boxes

A box groups the facts about one thing on a page: a hairline at `radius_lg`, padded `p_3`, its
parts `gap_2` apart (`card_box`). Its title row (`card_title`) holds the title in semibold, a
muted count, and at most one control at the end, at a fixed height so boxes side by side line up.
Boxes sit side by side while the measured column holds two, and stack when it does not.

### Lists

A row is `ROW_H` tall, padded `px_2` with `gap_2`, at `radius`. Hover fills it with `list_hover`,
the selected row with `accent`. A row's own actions are `xsmall` ghost buttons, shown on hover and
on the selected row. Every list has a named cap, and its last line says how many more there are,
in meta text. A table is a box of rows split by hairlines, under a head row in meta text.

### List and detail

`list_detail` sets a list beside its detail while the measured container holds both widths; else
it shows one at a time, the detail under a `back_link` naming the list. Picking shows the detail;
going back keeps the list's place.

### Forms

Each input is a `field`: its title in chrome text, an optional hint under it in meta text, `gap_2`
to the control. Fields stand `gap_4` apart. An input takes the library's default size, `small`
when it sits inside a row. Problems stand above the submit button in the danger ink. The submit
is the form's one primary action; while the form cannot be sent it refuses, its tooltip saying why.

### Dialogs

A dialog is the library's `Dialog` at its own width, titled through `title_row`. Its footer sits
on the right: the dismissing button first, then the one primary action or the danger word, at the
default size. It opens with focus on its first field and closes on Esc. A removal asks through the
confirmation below, never through a dialog of its own.

### Details

A detail shows one thing in full (an issue, a task): a trail naming where it sits, with its
actions beside it (one primary, one other, `⋯`); its title in `text_xl` semibold; one line of
facts in meta text; what it waits on; then its tabs.

## Behaviour

### Waiting

- **Content on its way** is one meta line where the content will appear, naming the work and
  ending on an ellipsis: *Reading the workflows…*. A panel never sits blank while a read runs.
- **An action in flight** shows on the button that started it, `.loading(true)`, which refuses a
  second press; its label stays.
- A `Spinner` stands only inside a line or a control already on screen.

### Nothing to show

One meta line, in sentence case, without a period: at the list's place, left-aligned, inside a
list; centred (`hint`) when it fills the whole panel. *No <things> yet* when there have never been
any; *No <things> match* when a search or a filter hides them all; *Nothing <state>* for a status
box (*Nothing needs you*).

### Failing and refusing

- **An action a person started that failed** is an `error` toast: *<What> failed: <why>*.
- **A condition that stands until something answers it** (a save refused because the file changed
  on disk, an editor that would not start) is a `status_line` under the view, in the ink of its
  state, cleared by whatever answers it.
- One failure is said once: a toast or a line, never both.
- **A press that cannot act yet** refuses (`.refuses(true)`) with a tooltip saying why, and says
  nothing else.
- Every failure of something a person started reaches the screen; a log line alone is not enough.

### Toasts

Transient news goes in a toast, `window.push_notification`, its kind chosen by what it means:

- `error`: an action failed;
- `warning`: done only in part, or something now needs the person;
- `success`: a long or background action finished, which the screen does not already show;
- `info`: news that asks for nothing.

A toast is one sentence without a trailing period.

### Confirming a removal

A removal that cannot be undone asks first: data taken off disk, a process ended, unsaved work
thrown away. It asks through `Confirm` (`crates/plugin-host/src/confirm.rs`, named `Ask` in the
app), the same question in a plugin:

- the title asks the question naming the thing: *Remove <name> from the workspace?*;
- the description says what goes and what stays;
- the buttons read *Keep*, then the danger word, the same verb as the control that asked.

What can be undone or resumed goes ahead without asking: stopping a task that offers *Resume*,
taking an item off the queue, unbinding a key. The control starting a removal is a word in the
danger tint.

### Keys and focus

- **Every key that does something is a command** in the keymap (`crates/app/src/keymap.rs`), so
  it can be rebound; `on_key_down` is for typing only.
- **A control bound to a command shows its key** in its tooltip (a `Kbd`); a field shows it as a
  `Kbd` at its end. A label's text never spells a key.
- **Keyboard focus shows the ring**, only when the keyboard moved it there.
- A dialog, a popup or a floating panel opens with focus on its first field, or on its primary
  action when it has none, and closes on Esc, asking first when a form in it holds unsaved
  changes. On closing, focus goes back to what opened it, or to the conversation when that is gone.
- Esc closes the innermost layer only. A page is a place, not a layer: Esc leaves it where it is.
- A key with no modifier that a region handles never fires while a field inside it holds the
  caret.

### Words

- Sentence case everywhere.
- A label ends on an ellipsis when its action asks something before it acts: a dialog, a
  confirmation, a picker or a form (*Retry…*, *Delete conversation…*).
- *Delete* takes a thing off disk; *Remove* takes it out of a list and leaves it; *Uninstall* is
  for a plugin; *Discard* throws away unsaved changes.
- *Keep* dismisses a confirmation; *Cancel* leaves a form or a dialog; *Close* puts a panel away.
- A command reads as a plain sentence, its article kept: *Restart the agent*, *Hide the rail*.
- Hints, empty lines and toasts end without a period; a dialog's description is whole sentences.
- On-screen words are the glossary's (`CONTEXT.md`), never its _Avoid_ words.
