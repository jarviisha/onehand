//! The Settings page that lists every command with its keys, and edits them
//! one at a time.

use super::*;
use crate::controls::Short as _;
use crate::controls::text_input;
use onehand_ui::Tall as _;

pub struct Editor {
    editing: Option<usize>,
    focus: gpui::FocusHandle,
    input: Entity<InputState>,
    /// Why the keys typed were refused, said under the field they are in.
    error: Option<String>,
}

impl Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            editing: None,
            focus: cx.focus_handle(),
            input: cx.new(|cx| InputState::new(window, cx).placeholder("ctrl-shift-j")),
            error: None,
        }
    }

    fn edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(index);
        self.error = None;
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
        // Refused keys are the field's to say, under it; a write the disk
        // refused is a toast. Each is said once, in one place.
        if let Err(error) = validate_controls(&overrides, &existing) {
            self.error = Some(error);
            cx.notify();
            return;
        }
        // Shown at once and written after; a write that fails puts the keys
        // back as they were last saved, said once.
        let before = Shared::global(cx).keymap.clone();
        // Only this command's keys go into the file, so a shortcut written
        // there by hand while the app runs survives the save.
        let id = command.id.to_string();
        let written = overrides.get(command.id).cloned();
        crate::saves::write_config(
            "keymap",
            "Saving the shortcuts",
            move |cfg| match written {
                Some(keys) => {
                    cfg.keymap.insert(id, keys);
                }
                None => {
                    cfg.keymap.remove(&id);
                }
            },
            Some(crate::saves::Change {
                before,
                after: overrides.clone(),
                put: |keymap, _, cx| {
                    cx.update_global::<Shared, _>(|shared, _| shared.keymap = keymap);
                    install(cx);
                },
            }),
            window,
            cx,
        );
        cx.update_global::<Shared, _>(|shared, _| shared.keymap = overrides);
        cx.global_mut::<LoadWarning>().0 = None;
        install(cx);
        self.focus.focus(window, cx);
        self.editing = None;
        self.error = None;
        cx.notify();
    }
}

/// The keys a shortcut field holds: alternatives separated by spaces, none at
/// all when it is empty.
fn typed_keys(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::settings::{APP, field, lead_section, list_row, list_rows, page_head, section};

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
                // The page and its nav stay: the command is named by a line,
                // not a title the size of a page's.
                .child(page_head("Edit the keys of a command", APP, cx))
                .child(
                    div()
                        .text_size(crate::tokens::CHROME_TEXT)
                        .font_semibold()
                        .child(command.label),
                )
                .child(
                    section(None, None, cx)
                        .child(field(
                            "Keys",
                            Some(
                                "Separate alternatives with spaces, or leave it empty to \
                                 unassign; Enter saves, Escape cancels"
                                    .into_any_element(),
                            ),
                            text_input(&self.input, cx).tall(),
                            cx,
                        ))
                        .child(
                            div()
                                .h_flex()
                                .gap_2()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Works: {}", command.scope))
                                .child(keys(command.defaults.iter().copied(), "none", cx))
                                .child("by default"),
                        )
                        .children(
                            self.error
                                .as_ref()
                                .map(|error| problem(error.clone(), danger, cx)),
                        )
                        .child(
                            // The action first, under the field it acts on.
                            div()
                                .h_flex()
                                .gap_2()
                                .child(
                                    // Secondary: Settings holds no primary.
                                    crate::controls::action("save-shortcut")
                                        .tall()
                                        .label("Save")
                                        .secondary()
                                        .on_click(cx.listener(move |editor, _, window, cx| {
                                            editor.save(index, false, window, cx)
                                        })),
                                )
                                .child(
                                    crate::controls::action("cancel-shortcut")
                                        .tall()
                                        .secondary()
                                        .label("Cancel")
                                        .on_click(cx.listener(|editor, _, window, cx| {
                                            editor.cancel(window, cx)
                                        })),
                                ),
                        ),
                )
                .into_any_element();
        }

        // The window's own commands, then a group for each place the rest
        // work in, named by it, in the table's order. Where a command works is
        // said once, by its group, and on a row only where it differs.
        let mut window_rows = Vec::new();
        let mut groups: Vec<(&'static str, Vec<gpui::AnyElement>)> = Vec::new();
        for (index, command) in COMMANDS.iter().enumerate() {
            let bound = command.keys(&overrides);
            let window = command.context.starts_with("Shell");
            let scope = (window && command.scope != super::WINDOW_SCOPE).then_some(command.scope);
            let row = list_row(
                command.label,
                scope.map(IntoElement::into_any_element),
                div()
                    .h_flex()
                    .gap_2()
                    .child(keys(bound.iter().map(String::as_str), "Unassigned", cx))
                    .when(overrides.contains_key(command.id), |row| {
                        row.child(
                            crate::controls::action(("reset-shortcut", index))
                                .secondary()
                                .short()
                                .label("Reset")
                                .on_click(cx.listener(move |editor, _, window, cx| {
                                    editor.save(index, true, window, cx)
                                })),
                        )
                    })
                    .child(
                        crate::controls::action(("edit-shortcut", index))
                            .secondary()
                            .short()
                            .label("Edit")
                            .on_click(cx.listener(move |editor, _, window, cx| {
                                editor.edit(index, window, cx)
                            })),
                    ),
                cx,
            )
            .into_any_element();
            if window {
                window_rows.push(row);
            } else if let Some((_, rows)) = groups.iter_mut().find(|(s, _)| *s == command.scope) {
                rows.push(row);
            } else {
                groups.push((command.scope, vec![row]));
            }
        }

        div()
            .track_focus(&self.focus)
            .v_flex()
            .gap_6()
            .w_full()
            .child(page_head(
                "The keys for every window; edit a command to change or unassign them",
                APP,
                cx,
            ))
            .children(cx.global::<LoadWarning>().0.as_ref().map(|error| {
                problem(
                    format!("Loaded defaults because the saved keymap is invalid: {error}"),
                    danger,
                    cx,
                )
            }))
            .children(
                self.error
                    .as_ref()
                    .map(|error| problem(error.clone(), danger, cx)),
            )
            .child(
                lead_section(
                    "Window",
                    Some("Anywhere in the window, the terminal included".into()),
                    cx,
                )
                .child(list_rows(window_rows, cx)),
            )
            .children(groups.into_iter().map(|(scope, rows)| {
                let about = (scope == super::RAIL_SCOPE)
                    .then(|| "While the rail's list has the keyboard".into());
                section(Some(scope), about, cx).child(list_rows(rows, cx))
            }))
            .child(
                section(
                    Some("Terminal"),
                    Some(
                        "Fixed, because a program running in the terminal has a claim on them"
                            .into(),
                    ),
                    cx,
                )
                .child(list_rows(
                    [
                        list_row(
                            "Copy and paste",
                            None,
                            keys(["ctrl-shift-c", "ctrl-shift-v"], "", cx),
                            cx,
                        )
                        .into_any_element(),
                        list_row(
                            "Focus traversal",
                            Some("Go to the program in the terminal instead".into_any_element()),
                            keys(["tab", "shift-tab"], "", cx),
                            cx,
                        )
                        .into_any_element(),
                    ],
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
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(none)
            .into_any_element();
    }
    div().h_flex().gap_1().children(caps).into_any_element()
}

/// A problem with the keymap, in a box on the danger tint led by the warning
/// mark, so it reads as something to answer rather than as one more line.
fn problem(text: String, danger: gpui::Hsla, cx: &App) -> gpui::AnyElement {
    crate::chat::pane::page_parts::notice_box(
        gpui_component::IconName::TriangleAlert,
        danger,
        vec![text],
        cx,
    )
}
