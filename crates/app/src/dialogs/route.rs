//! The form that adds or changes one of a project's routed commands.

use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::{text_area, text_input};
use crate::shell::Shell;
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ClickEvent, Context, IntoElement as _, ParentElement, Styled, Window, div};
use gpui_component::StyledExt as _;
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use onehand_ui::{Short as _, Tall as _};

/// The routed command form. **No trigger**, for the other forms' reason:
/// the shell decides whether it exists.
pub(crate) fn route(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(draft) = shell.route_draft() else {
        return Dialog::new(cx);
    };
    let (words, artifacts) = (draft.words.clone(), draft.artifacts.clone());
    let (title, act) = match draft.editing {
        Some(_) => (format!("A routed command of {}", draft.label), "Save"),
        None => (format!("Add a routed command to {}", draft.label), "Add"),
    };
    let error = draft.error.clone();
    let danger = crate::theme::status_ink(cx).danger;
    // What the folder suggests, less what the project routes already.
    let routed = shell.routes_of(&draft.root, cx);
    let offered: Vec<&'static str> = shell
        .route_suggestions(&draft.root)
        .iter()
        .copied()
        .filter(|words| !routed.iter().any(|route| route.words == *words))
        .collect();
    let weak = cx.entity().downgrade();

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        // What is wrong with the entry is said in the form, which stays.
        .on_ok(super::enter_presses(
            weak.clone(),
            true,
            vec![words.focus_handle(cx)],
            |shell, window, cx| shell.save_route(window, cx),
        ))
        .content(move |content, _, cx: &mut App| {
            let muted = gpui_component::ActiveTheme::theme(cx).muted_foreground;
            let last = offered.len().saturating_sub(1);
            let suggested = (!offered.is_empty()).then(|| {
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child("Suggested:\u{a0}")
                    .children(offered.iter().enumerate().map(|(at, words)| {
                        let (weak, words) = (weak.clone(), *words);
                        div()
                            .h_flex()
                            .items_center()
                            .child(
                                crate::controls::action(("route-suggestion", at))
                                    .link()
                                    .short()
                                    .label(words)
                                    .on_click(move |_, window, cx| {
                                        let _ = weak.update(cx, |shell, cx| {
                                            shell.pick_route_suggestion(words, window, cx)
                                        });
                                    }),
                            )
                            .when(at < last, |item| item.child(",\u{a0}"))
                            .into_any_element()
                    }))
            });
            content
                .child(title_row(title.clone(), cx))
                .child(field(
                    "Command",
                    crate::settings::about(
                        "The program and the words after it, as an agent runs them",
                    ),
                    div()
                        .v_flex()
                        .gap_2()
                        .child(text_input(&words, cx).tall())
                        .children(suggested),
                    cx,
                ))
                .child(field(
                    "Artifacts",
                    crate::settings::about(
                        "What it brings back: one path a line, inside the project",
                    ),
                    text_area(&artifacts, cx),
                    cx,
                ))
                .when_some(error.clone(), |form, why| {
                    form.child(div().text_xs().text_color(danger).child(why))
                })
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-route")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_route(cx);
                        })),
                )
                .child(
                    crate::controls::action("save-route")
                        .tall()
                        .primary()
                        .label(act)
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.save_route(window, cx);
                            }),
                        ),
                ),
        )
        // Esc clears the state putting this on screen, or the next frame
        // renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_route(cx);
        }))
}
