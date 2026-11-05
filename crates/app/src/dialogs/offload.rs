//! The form that offloads a project to a host: its new sessions run there,
//! on a copy of its folder the host keeps.

use super::host_project::{PickHost, host_choices, host_menu};
use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::text_input;
use crate::shell::{Shell, offload_refusal};
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ClickEvent, Context, ParentElement, Styled, Window, div};
use gpui_component::StyledExt as _;
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use onehand_core::workspace::HostName;
use onehand_ui::Tall as _;
use std::rc::Rc;

/// The offload form. **No trigger**, for the other forms' reason: the shell
/// decides whether it exists.
pub(crate) fn offload(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(draft) = shell.offload_draft() else {
        return Dialog::new(cx);
    };
    let host = draft.host.clone();
    let sending = draft.sending.clone();
    let refusal = match &sending {
        Some(_) => Some("Sending the code".to_string()),
        None => offload_refusal(&host.read(cx).value(), shell.busy_here(&draft.root, cx)),
    };
    let title = format!("Offload {} to a host", draft.label);
    // What it does while it does it: a first copy of a big project takes a
    // while, and the form stays up until it lands.
    let act = match &sending {
        Some(to) => format!("Sending {} to {to}…", draft.label),
        None => "Offload".to_string(),
    };
    let error = draft.error.clone();
    let danger = crate::theme::status_ink(cx).danger;
    let (choices, more_hosts) = host_choices(draft.aliases.clone(), shell.used_hosts());
    let weak = cx.entity().downgrade();
    let on_branch = draft.on_branch;
    // The branch named as it will be, from what *Host* says now.
    let typed = host.read(cx).value().trim().to_string();
    let branch = match typed.is_empty() {
        true => "a branch of its own".to_string(),
        false => onehand_core::host::offload::branch::branch_for(&HostName(typed)),
    };
    let about = match on_branch {
        false => "Its new sessions run on the host, on a copy of this folder. Its files, \
                  terminal and git stay here, and its code comes back into this folder when \
                  the host's sessions finish; edits here wait meanwhile. Sessions open here \
                  now are closed."
            .to_string(),
        true => format!(
            "Its new sessions run on the host, on {branch}, started from this folder's last \
             commit; what is not committed stays here. Its files, terminal and git stay here \
             and are never locked: the branch is fetched here for you to merge when you \
             choose. Sessions open here now are closed."
        ),
    };

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            let pick = {
                let weak = weak.clone();
                Rc::new(move |host: String, window: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |shell, cx| shell.pick_offload_host(host, window, cx));
                }) as PickHost
            };
            let modes = onehand_ui::switch(
                "offload-mode",
                &["Into this folder".into(), "On a branch".into()],
                usize::from(on_branch),
                onehand_ui::ControlSize::Tall,
                {
                    let weak = weak.clone();
                    move |at: &usize, _, cx: &mut App| {
                        let _ = weak.update(cx, |shell, cx| shell.pick_offload_mode(*at == 1, cx));
                    }
                },
                cx,
            );
            content
                .child(title_row(title.clone(), cx))
                .child(modes)
                .child(
                    div()
                        .text_sm()
                        .text_color(gpui_component::ActiveTheme::theme(cx).muted_foreground)
                        .child(about.clone()),
                )
                .child(field(
                    "Host",
                    crate::settings::about("An alias from your SSH config, or user@machine"),
                    div()
                        .h_flex()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(text_input(&host, cx).tall()))
                        .when(!choices.is_empty(), |row| {
                            row.child(host_menu(choices.clone(), more_hosts, pick, cx))
                        }),
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
                    crate::controls::action("cancel-offload")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_offload(cx);
                        })),
                )
                .child(
                    crate::controls::action("commit-offload")
                        .tall()
                        .primary()
                        .label(act)
                        .loading(sending.is_some())
                        .refuses(refusal)
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.commit_offload(window, cx);
                            }),
                        ),
                ),
        )
        // Esc clears the state putting this on screen, or the next frame
        // renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_offload(cx);
        }))
}
