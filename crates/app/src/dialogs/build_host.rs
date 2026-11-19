//! The form that adds or changes one of a project's build hosts.

use super::host_project::{PickHost, host_menu};
use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::{text_area, text_input};
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ClickEvent, Context, ParentElement, Styled, Window, div};
use gpui_component::StyledExt as _;
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use onehand_ui::Tall as _;
use std::rc::Rc;

/// The build host form. **No trigger**, for the other forms' reason: the
/// shell decides whether it exists.
pub(crate) fn build_host(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(draft) = shell.build_host_draft() else {
        return Dialog::new(cx);
    };
    let (host, artifacts, into) = (
        draft.host.clone(),
        draft.artifacts.clone(),
        draft.into.clone(),
    );
    let (title, act) = match draft.editing {
        Some(_) => (format!("A build host of {}", draft.label), "Save"),
        None => (format!("Add a build host to {}", draft.label), "Add"),
    };
    let error = draft.error.clone();
    let danger = crate::theme::status_ink(cx).danger;
    // Every declared host a project can build on, one that runs no sessions
    // included: building is what such a host is for.
    let offered: Vec<String> = crate::state::Shared::global(cx)
        .settings
        .hosts
        .iter()
        .filter(|host| host.builds())
        .map(|host| host.name.to_string())
        .collect();
    let more = offered
        .len()
        .saturating_sub(super::host_project::HOST_CHOICES_CAP);
    let choices: Vec<String> = offered
        .into_iter()
        .take(super::host_project::HOST_CHOICES_CAP)
        .collect();
    let weak = cx.entity().downgrade();

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            let pick = {
                let weak = weak.clone();
                Rc::new(move |host: String, window: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |shell, cx| shell.pick_build_host(host, window, cx));
                }) as PickHost
            };
            content
                .child(title_row(title.clone(), cx))
                .child(field(
                    "Host",
                    crate::settings::about("A host declared in Settings, as ssh names it"),
                    div()
                        .h_flex()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(text_input(&host, cx).tall()))
                        .when(!choices.is_empty(), |row| {
                            row.child(host_menu(choices.clone(), more, pick, cx))
                        }),
                    cx,
                ))
                .child(field(
                    "Artifacts",
                    crate::settings::about(
                        "What a build brings back: one path a line, inside the project",
                    ),
                    text_area(&artifacts, cx),
                    cx,
                ))
                .child(field(
                    "Into",
                    crate::settings::about(
                        "A folder in the project they land under; empty for the same paths",
                    ),
                    text_input(&into, cx).tall(),
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
                    crate::controls::action("cancel-build-host")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_build_host(cx);
                        })),
                )
                .child(
                    crate::controls::action("save-build-host")
                        .tall()
                        .primary()
                        .label(act)
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.save_build_host(window, cx);
                            }),
                        ),
                ),
        )
        // Esc clears the state putting this on screen, or the next frame
        // renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_build_host(cx);
        }))
}
