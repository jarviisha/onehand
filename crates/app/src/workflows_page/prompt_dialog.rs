//! A step's prompt written in a dialog the size of a page, for a prompt too
//! long to write in the form's field.
//!
//! **A copy, written back on *Done*.** The dialog holds a field of its own
//! filled from the step's: one field's state drawn in two places at once
//! would have both lay it out, and *Cancel* leaves the step as it was.

use super::WorkflowsPage;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use crate::controls::text_area;
use gpui::{App, AppContext as _, Entity, ParentElement, Styled, Window, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::TextareaState;
use gpui_component::{StyledExt, WindowExt as _};

/// How wide the dialog stands: a page's reading column and a little more,
/// so a prompt's lines run as long as they will in the agent's view.
const PROMPT_DIALOG_W: gpui::Rems = gpui::rems(56.);

/// How tall its field stands: most of a screen, scrolling past that.
const PROMPT_EDITOR_H: gpui::Rems = gpui::rems(28.);

/// Open the dialog on `prompt`, the field of step `label`. A shipped
/// workflow's opens to be read, with *Close* alone.
pub(super) fn open(
    page: Entity<WorkflowsPage>,
    label: String,
    prompt: Entity<TextareaState>,
    read_only: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let text = prompt.read(cx).value().to_string();
    let editor = cx.new(|cx| TextareaState::new(window, cx).default_value(text));
    let field = editor.clone();
    let width = crate::dialogs::width_in(window, PROMPT_DIALOG_W);
    window.open_dialog(cx, move |dialog, _, cx| {
        let done = {
            let (page, prompt, editor) = (page.clone(), prompt.clone(), editor.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let text = editor.read(cx).value().to_string();
                prompt.update(cx, |field, cx| field.set_value(text, window, cx));
                page.update(cx, |_, cx| cx.notify());
                window.close_dialog(cx);
            }
        };
        let dismiss = crate::controls::action("prompt-dialog-cancel")
            .short()
            .quiet(cx)
            .label(if read_only { "Close" } else { "Cancel" })
            .on_click(|_, window, cx| window.close_dialog(cx));
        let footer = crate::dialogs::footer_row()
            .justify_end()
            .child(dismiss)
            .children((!read_only).then(|| {
                crate::controls::action("prompt-dialog-done")
                    .short()
                    .primary()
                    .label("Done")
                    .on_click(done)
            }));
        dialog
            .w(width)
            .p_4()
            .close_button(false)
            .child(
                div()
                    .v_flex()
                    .gap_4()
                    .child(crate::dialogs::title_row(
                        match label.is_empty() {
                            true => "Prompt".to_string(),
                            false => format!("Prompt · {label}"),
                        },
                        cx,
                    ))
                    .child(super::form::variables_hint(cx))
                    .child(
                        text_area(&editor, cx)
                            .h(PROMPT_EDITOR_H)
                            .disabled(read_only),
                    ),
            )
            .footer(footer)
    });
    // After the dialog is up: it keeps what held the keyboard before it as
    // where the keyboard goes back to on closing, and takes the keyboard for
    // itself; the field then takes it from the dialog.
    field.update(cx, |field, cx| field.focus(window, cx));
}
