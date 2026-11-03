//! The form that adds a project whose folder is on a host, and the words of
//! the question asked when that host runs another build.

use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ClickEvent, Context, ParentElement, Styled, Window, div};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use onehand_ui::Tall as _;

/// Why *Add* refuses while the host attaches and answers.
const ATTACHING: &str = "Attaching to the host";

/// How many of a busy host's sessions the restart question names.
const BUSY_NAMED: usize = 5;

/// Why *Add* cannot go yet, from what the two fields hold.
///
/// The folder has to start at `/`: the host reads the path as written, with
/// no shell to expand `~` and no folder of the person's to start from.
pub(crate) fn add_refusal(host: &str, folder: &str) -> Option<&'static str> {
    let folder = folder.trim();
    if host.trim().is_empty() {
        Some("Name the host")
    } else if folder.is_empty() {
        Some("Name the folder")
    } else if !folder.starts_with('/') {
        Some("The folder starts at /")
    } else {
        None
    }
}

/// What the restart question says: the host, and its mid-turn sessions by
/// the host's own ids, five of them and then how many more.
pub(crate) fn busy_body(host: &HostName, busy: &[HostSession]) -> String {
    let mut names = busy
        .iter()
        .take(BUSY_NAMED)
        .map(|id| id.0.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let more = busy.len().saturating_sub(BUSY_NAMED);
    if more > 0 {
        names.push_str(&format!(" and {more} more"));
    }
    format!(
        "{host} runs another build of the host service, and these sessions are mid-turn: \
         {names}. Restarting stops them."
    )
}

/// The add-a-project-on-a-host form.
///
/// **No trigger**, for the rename's reason: the rail's rows are rebuilt every
/// frame, and the shell decides whether the form exists.
///
/// Two fields, so it stands at a form's width with a form's controls.
pub fn add_host_project(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(draft) = shell.host_project() else {
        return Dialog::new(cx);
    };
    let (host, folder) = (draft.host.clone(), draft.folder.clone());
    let (error, busy) = (draft.error.clone(), draft.busy());
    let refusal = match busy {
        true => Some(ATTACHING),
        false => add_refusal(&host.read(cx).value(), &folder.read(cx).value()),
    };
    let danger = crate::theme::status_ink(cx).danger;

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            content
                .child(title_row("Add a project on a host", cx))
                .child(field(
                    "Host",
                    crate::settings::about("An alias from your SSH config, or user@machine"),
                    text_input(&host, cx).tall(),
                    cx,
                ))
                .child(field(
                    "Folder",
                    crate::settings::about("The project's folder on that host, from /"),
                    text_input(&folder, cx).tall(),
                    cx,
                ))
                // The host's own words, or that the folder is not there: shown
                // against the names that caused them, which is why the form
                // is still up.
                .when_some(error.clone(), |form, why| {
                    form.child(div().text_xs().text_color(danger).child(why))
                })
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-host-project")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_host_project(cx);
                        })),
                )
                .child(
                    crate::controls::action("add-host-project")
                        .tall()
                        .primary()
                        .label("Add")
                        .loading(busy)
                        .refuses(refusal)
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.commit_host_project(window, cx);
                            }),
                        ),
                ),
        )
        // Esc and the ✕ clear the state putting this on screen, or the next
        // frame renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_host_project(cx);
        }))
}

#[cfg(test)]
mod tests;
