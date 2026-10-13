---
name: ux-patterns
description: Keep onehand feedback, interaction states and language predictable. Use for loading, empty, failure and refusal states, selection, keyboard focus, confirmations, toasts and on-screen wording.
---

# Behaviour

The rules are [UI patterns](../../../docs/ui-patterns.md)'s *Behaviour*; visual priority comes
from [DESIGN.md](../../../DESIGN.md)'s *Colour and state*. This skill holds code examples. Walk
every view the change touches through the sections below.

Done when every state the view can reach (waiting, empty, failed, refusing) is drawn by its
pattern, every key it adds is a keymap command, and every word it adds follows *Words*.

## Waiting

Follow the state-stability rule in UI patterns: keep content axes and action slots in place.
Check loading-to-content and empty-to-content transitions, not just their final appearance.

```rust
// Content on its way: one meta line where it will appear
div().text_xs().text_color(cx.theme().muted_foreground).child("Reading the workflows…")
// An action in flight, on the button that started it
action("run-check").secondary().tall().label("Run check").loading(running)
    .refuses(running.then_some("The check is running"))
```

## Immediate feedback

A change the person can take back is drawn before it is saved; the save follows off the UI thread
(the spawn pattern of `crates/app/src/shell/sessions.rs`). Work that leaves the machine waits
instead.

Changes come faster than saves: rename A to B, then B to C, while B is still being written. So
the view keeps, per thing it saves, the value shown, the value known saved, the number of the
newest change, and the save in flight. Each save waits for the one before it, so they land in
order; a failure puts the screen back only while its change is still the newest, and then to the
value known saved, never to the value before that change.

```rust
// Fields: title (shown), saved_title (on disk), title_edit: u64, title_save: Option<Task<()>>
self.title_edit += 1;
let edit = self.title_edit;
self.title = title.clone();
cx.notify();
let earlier = self.title_save.take();
self.title_save = Some(cx.spawn_in(window, async move |this, cx| {
    // One save at a time, in the order the changes were made
    if let Some(earlier) = earlier {
        earlier.await;
    }
    let written = title.clone();
    let saved = cx.background_executor().spawn(async move { save(&written) }).await;
    this.update_in(cx, |this, window, cx| match saved {
        Ok(()) => this.saved_title = title,
        // Still the newest change: back to what the disk holds, said once
        Err(why) if this.title_edit == edit => {
            this.title = this.saved_title.clone();
            window.push_notification(crate::toast::failed("Renaming the session", why), cx);
            cx.notify();
        }
        // Superseded: the newer change's own save decides what shows
        Err(_) => {}
    })
    .ok();
}));
```

## Motion

Nothing animates but a `Spinner` on work still running.

```rust
// Wrong: a fade drawing the eye to a row that changed
row.with_animation("arrived", Animation::new(duration), |row, t| row.opacity(t))
// Right: the new state lands at once and says itself in ink and a word
row.child(div().text_color(status_ink(cx).success).child("Done"))
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
window.push_notification(crate::toast::failed("Exporting the conversation", e), cx);
// A condition standing until something answers it, under a plugin's view
onehand_plugin_host::status_line(message, cx)
// Cannot act yet: refuse, and say why
action("save-agent").primary().tall().label("Save").tooltip("Save the agent")
    .refuses((!ready).then_some("Give it a name and a command first"))
```

A failure reaches the screen once. A log alone leaves the person believing it worked. Apply
semantic colour to the local message or mark; do not recolour an entire panel. Keep a word or
recognizable mark alongside colour, and preserve the refusal's explanation when simplifying UI.

## Confirming a removal

From `crates/app/src/shell/roots.rs`; `confirm_on(entity, ask, window, cx, then)` asks from
outside the shell.

```rust
let ask = Confirm {
    id: "remove-project",
    title: format!("Remove {label} from the workspace?").into(),
    description: format!("This {}. The folder itself stays on disk.", losses.join(" and ")).into(),
    act: "Remove",
    ..Default::default()
};
self.confirm(ask, window, cx, move |shell, window, cx| shell.drop_root(&path, window, cx));
```

## Keys and focus

- A new command goes into `COMMANDS` in `crates/app/src/keymap.rs` (id, label, default keys,
  action, context), never into an `on_key_down` arm.
- A window command's context starts `Shell` and its key has a modifier. A region's starts with the
  region's own context (`Rail`, `IssuesPage`, `TasksPage`, `WorkflowsPage`, `PluginList`,
  `QuestionCard`, `PermissionCard`): a key without a modifier adds `&& !Input`, and Escape needs
  `&& !Dialog && !Input`. One key may serve one command in each region; `validate` refuses the
  rest.
- A control bound to one shows its key, through the library's own lookup so a rebound key shows:

  ```rust
  // A button
  .tooltip_with_action("Hide the Workbench", &crate::shell::ToggleWorkbenchVisibility, None)
  // Any other element
  .tooltip(move |window, cx| Tooltip::new(tip).action(&crate::shell::ShowTasks, None).build(window, cx))
  ```

- An element you draw that takes focus tracks it and shows the ring for the keyboard only:
  `.track_focus(&self.focus_handle).focus_visible(|s| s.border_color(cx.theme().ring))`.
- Esc in a field reaches the view through the library's action:
  `.on_action(cx.listener(|this, _: &gpui_component::input::Escape, window, cx| this.close(window, cx)))`.
- A panel that held focus hands it back before it leaves the frame:
  `ChatPane::reclaim_focus`, or the keymap goes dead (see `CLAUDE.md`, *Gotchas*).
- Check a row's keyboard-focus path as well as pointer hover: its actions must be reachable,
  selection must remain visible, and revealing actions must not shift the label.

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
