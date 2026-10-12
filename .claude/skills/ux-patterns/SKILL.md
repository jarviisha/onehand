---
name: ux-patterns
description: Behaviour for waiting, empty, failed and refusing states, toasts, confirming a removal, keys, focus, Esc and on-screen words. Use when a view loads, fails, refuses, has nothing to show, removes something, takes keys, or gains a label or message.
---

# Behaviour

The rules are `docs/ui-patterns.md`'s (*Behaviour*); this skill holds the code for each. Walk
every view the change touches through the sections below.

Done when every state the view can reach (waiting, empty, failed, refusing) is drawn by its
pattern, every key it adds is a keymap command, and every word it adds follows *Words*.

## Waiting

```rust
// Content on its way: one meta line where it will appear
div().text_xs().text_color(cx.theme().muted_foreground).child("Reading the workflows…")
// An action in flight, on the button that started it
action("run-check").small().label("Run check").loading(running).refuses(running)
```

## Nothing to show

```rust
// Filling the panel: centred
onehand_plugin_host::hint("No markdown in this project", cx)
// In a list: at its place, left-aligned
div().px_2().py_1().text_xs().text_color(cx.theme().muted_foreground).child("No issues match")
```

## Failing and refusing

```rust
// An action the person started failed
window.push_notification(Notification::error(format!("Export failed: {e}")), cx);
// A condition standing until something answers it, under a plugin's view
onehand_plugin_host::status_line(message, cx)
// Cannot act yet: refuse, and say why
action("save-agent").primary().label("Save").refuses(!ready)
    .tooltip(if ready { "Save the agent" } else { "Give it a name and a command first" })
```

A failure reaches the screen once. `eprintln!` alone, as the export in
`crates/app/src/chat/pane/sessions.rs` does, leaves the person believing it worked.

## Confirming a removal

From `crates/app/src/shell/roots.rs`; `ask_on(entity, ask, window, cx, then)` asks from outside
the shell.

```rust
let ask = Ask {
    id: "remove-project",
    title: format!("Remove {label} from the workspace?").into(),
    description: format!("This {}. The folder itself stays on disk.", losses.join(" and ")).into(),
    act: "Remove",
    ..Default::default()
};
self.ask(ask, window, cx, move |shell, window, cx| shell.drop_root(&path, window, cx));
```

## Keys and focus

- A new command goes into `COMMANDS` in `crates/app/src/keymap.rs` (id, label, default keys,
  action, context), never into an `on_key_down` arm.
- A control bound to one shows its key:

  ```rust
  .tooltip(move |window, cx| {
      Tooltip::new("Show the Workbench")
          .key_binding(crate::keymap::first_key("toggle_workbench", cx).map(Kbd::new))
          .build(window, cx)
  })
  ```

- An element you draw that takes focus tracks it and shows the ring for the keyboard only:
  `.track_focus(&self.focus_handle).focus_visible(|s| s.border_color(cx.theme().ring))`.
- Esc in a field reaches the view through the library's action:
  `.on_action(cx.listener(|this, _: &gpui_component::input::Escape, window, cx| this.close(window, cx)))`.
- A panel that held focus hands it back before it leaves the frame:
  `ChatPane::reclaim_focus`, or the keymap goes dead (see `CLAUDE.md`, *Gotchas*).

## Words

| Wrong | Right | Why |
|---|---|---|
| *Delete* on a project | *Remove* | the folder stays on disk |
| *Retry* opening a dialog | *Retry…* | it asks before it acts |
| *Restart agent* | *Restart the agent* | a command reads as a sentence |
| *Drop draft*, *Drop and close* | *Discard* | unsaved work is discarded |
| *Cancel* on a confirmation | *Keep* | dismissing a removal keeps the thing |
| *reading…* | *Reading the issue…* | sentence case, naming the work |
| *No conversations in this project yet.* | *No conversations in this project yet* | a hint ends without a period |
| *Conversation not saved — {e}* | *Saving the conversation failed: {e}* | one form for every failure |
