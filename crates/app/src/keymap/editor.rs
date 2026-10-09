//! The Settings page that lists every command with its keys, and edits them
//! one at a time.

use super::*;

pub struct Editor {
    editing: Option<usize>,
    focus: gpui::FocusHandle,
    input: Entity<InputState>,
    error: Option<String>,
    /// A save just went through, until the next edit starts.
    saved: bool,
}

impl Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            editing: None,
            focus: cx.focus_handle(),
            input: cx.new(|cx| InputState::new(window, cx).placeholder("ctrl-shift-j")),
            error: None,
            saved: false,
        }
    }

    fn edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(index);
        self.error = None;
        self.saved = false;
        let value = COMMANDS[index].keys(&Shared::global(cx).keymap).join(" ");
        self.input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.editing = None;
        self.error = None;
        cx.notify();
    }

    fn save(&mut self, index: usize, reset: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut overrides = Shared::global(cx).keymap.clone();
        let command = &COMMANDS[index];
        if reset {
            overrides.remove(command.id);
        } else {
            let keys = typed_keys(&self.input.read(cx).value());
            overrides.insert(command.id.to_string(), keys);
        }
        let existing: Vec<_> = cx.key_bindings().borrow().bindings().cloned().collect();
        if let Err(error) = validate_controls(&overrides, &existing) {
            window.push_notification(Notification::error(error.clone()), cx);
            self.error = Some(error);
            cx.notify();
            return;
        }
        let path = Shared::global(cx).config_path.clone();
        if let Err(error) = AppConfig::update_in_place(&path, |cfg| cfg.keymap = overrides.clone())
        {
            let error = format!("Shortcuts were not saved: {error}");
            window.push_notification(Notification::error(error.clone()), cx);
            self.error = Some(error);
            cx.notify();
            return;
        }
        cx.update_global::<Shared, _>(|shared, _| shared.keymap = overrides);
        cx.global_mut::<LoadWarning>().0 = None;
        install(cx);
        self.focus.focus(window, cx);
        self.editing = None;
        self.error = None;
        // Said beside Settings' ✕ rather than in a toast, the way every other
        // page says a change was written.
        self.saved = true;
        cx.notify();
    }
}

/// The keys a shortcut field holds: alternatives separated by spaces, none at
/// all when it is empty.
fn typed_keys(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

/// Whether what is typed in a shortcut field differs from the keys the command
/// has now.
pub(super) fn edit_changes(value: &str, current: &[String]) -> bool {
    typed_keys(value) != current
}

impl Editor {
    /// Whether a shortcut is open for editing.
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Whether the shortcut open for editing holds keys it does not have yet --
    /// opened and left as it was is nothing to lose.
    pub fn dirty(&self, cx: &App) -> bool {
        self.editing.is_some_and(|index| {
            let current = COMMANDS[index].keys(&Shared::global(cx).keymap);
            edit_changes(&self.input.read(cx).value(), &current)
        })
    }

    /// `Saved` right after a save went through, until the next edit starts. A
    /// failure is not repeated here: it is already written on the page, under
    /// the field it is about.
    pub fn note(&self) -> Option<Result<(), String>> {
        self.saved.then_some(Ok(()))
    }

    /// Drop that word, when Settings opens or leaves this page.
    pub fn forget_note(&mut self) {
        self.saved = false;
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::settings::{APP, field, list_row, page_head, section};

        let overrides = Shared::global(cx).keymap.clone();
        let muted = cx.theme().muted_foreground;
        let danger = crate::theme::status_ink(cx).danger;
        if let Some(index) = self.editing {
            let command = &COMMANDS[index];
            return div()
                .track_focus(&self.focus)
                .key_context("ShortcutEditor")
                .v_flex()
                .gap_6()
                .w_full()
                .on_action(cx.listener(move |editor, _: &SaveShortcut, window, cx| {
                    editor.save(index, false, window, cx);
                }))
                .on_action(cx.listener(|editor, _: &CancelShortcut, window, cx| {
                    editor.cancel(window, cx);
                }))
                .child(page_head("Edit shortcut", command.label, APP, cx))
                .child(
                    section(None, None, cx)
                        .child(field(
                            "Keys",
                            Some(
                                "Separate alternatives with spaces. Leave empty to unassign. \
                             Enter saves; Escape cancels."
                                    .into_any_element(),
                            ),
                            Input::new(&self.input),
                            cx,
                        ))
                        .child(
                            div()
                                .h_flex()
                                .gap_2()
                                .text_sm()
                                .text_color(muted)
                                .child(format!("Works: {}.", command.scope))
                                .child(keys(command.defaults.iter().copied(), "none", cx))
                                .child("by default"),
                        )
                        .children(
                            self.error.as_ref().map(|error| {
                                div().text_sm().text_color(danger).child(error.clone())
                            }),
                        )
                        .child(
                            // The same order the agent form keeps: the action
                            // first, under the field it acts on.
                            div()
                                .h_flex()
                                .gap_2()
                                .child(
                                    crate::controls::action("save-shortcut")
                                        .label("Save")
                                        .primary()
                                        .on_click(cx.listener(move |editor, _, window, cx| {
                                            editor.save(index, false, window, cx)
                                        })),
                                )
                                .child(
                                    crate::controls::action("cancel-shortcut")
                                        .ghost()
                                        .label("Cancel")
                                        .on_click(cx.listener(|editor, _, window, cx| {
                                            editor.cancel(window, cx)
                                        })),
                                ),
                        ),
                )
                .into_any_element();
        }

        // Two groups, by where a command works: the window's own commands reach
        // over any panel, the rest only while the composer holds the caret.
        let mut window_rows = Vec::new();
        let mut composer_rows = Vec::new();
        for (index, command) in COMMANDS.iter().enumerate() {
            let bound = command.keys(&overrides);
            let row = list_row(
                command.label,
                Some(command.scope.into_any_element()),
                div()
                    .h_flex()
                    .gap_2()
                    .child(keys(bound.iter().map(String::as_str), "Unassigned", cx))
                    .when(overrides.contains_key(command.id), |row| {
                        row.child(
                            crate::controls::action(("reset-shortcut", index))
                                .ghost()
                                .small()
                                .label("Reset")
                                .on_click(cx.listener(move |editor, _, window, cx| {
                                    editor.save(index, true, window, cx)
                                })),
                        )
                    })
                    .child(
                        crate::controls::action(("edit-shortcut", index))
                            .ghost()
                            .small()
                            .label("Edit")
                            .on_click(cx.listener(move |editor, _, window, cx| {
                                editor.edit(index, window, cx)
                            })),
                    ),
                cx,
            )
            .into_any_element();
            match command.context.starts_with("Shell") {
                true => window_rows.push(row),
                false => composer_rows.push(row),
            }
        }

        div()
            .track_focus(&self.focus)
            .v_flex()
            .gap_6()
            .w_full()
            .child(page_head(
                "Shortcuts",
                "The keys for every window. Edit a command to change or unassign them.",
                APP,
                cx,
            ))
            .children(cx.global::<LoadWarning>().0.as_ref().map(|error| {
                div().text_sm().text_color(danger).child(format!(
                    "Loaded defaults because the saved keymap is invalid: {error}"
                ))
            }))
            .children(
                self.error
                    .as_ref()
                    .map(|error| div().text_sm().text_color(danger).child(error.clone())),
            )
            .child(section(Some("Window"), None, cx).children(window_rows))
            .child(section(Some("Composer"), None, cx).children(composer_rows))
            .child(
                section(
                    Some("Terminal"),
                    Some(
                        "Fixed, because a program running in the terminal has a claim on them."
                            .into(),
                    ),
                    cx,
                )
                .child(list_row(
                    "Copy / paste",
                    None,
                    keys(["ctrl-shift-c", "ctrl-shift-v"], "", cx),
                    cx,
                ))
                .child(list_row(
                    "Focus traversal",
                    Some("Go to the program in the terminal instead.".into_any_element()),
                    keys(["tab", "shift-tab"], "", cx),
                    cx,
                )),
            )
            .into_any_element()
    }
}

/// A command's keys as the component library's key caps, or a muted word
/// where it has none.
fn keys<'a>(
    keys: impl IntoIterator<Item = &'a str>,
    none: &'static str,
    cx: &App,
) -> gpui::AnyElement {
    let caps = keys
        .into_iter()
        .filter_map(|key| Keystroke::parse(key).ok())
        .map(gpui_component::kbd::Kbd::new)
        .collect::<Vec<_>>();
    if caps.is_empty() {
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(none)
            .into_any_element();
    }
    div().h_flex().gap_1().children(caps).into_any_element()
}
