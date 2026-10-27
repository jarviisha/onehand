use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Context, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use onehand_core::config::Preset;
use onehand_ui::Tall as _;

/// The agent form: a new agent, or the one being edited, in a window of its
/// own over the list it belongs to.
///
/// **No trigger**, for the rename's reason: the list's rows are rebuilt every
/// frame, and the shell decides whether the form exists.
///
/// A form, so it stands at a form's width with its controls at a page
/// form's height, rather than at a question's.
///
/// Its fields are the shell's draft, so Esc, the ✕ and *Cancel* all clear it:
/// a form put away half-written and found again later offers to save over an
/// agent the list may since have moved.
pub fn agent_form(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let handle = cx.entity();
    let draft = shell.agent_draft();
    let (name, command, args, env) = (
        draft.name.clone(),
        draft.command.clone(),
        draft.args.clone(),
        draft.env.clone(),
    );
    let refusal = draft.to_spec(cx).err();
    let editing = draft
        .editing
        .and_then(|idx| shell.agents(cx).get(idx))
        .map(|spec| spec.name.clone());
    let kind = draft.kind;
    let title = match &editing {
        Some(name) => format!("Edit {name}"),
        None => "Add an agent".to_string(),
    };

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            // Every kind at one width across the form, so the row runs to the
            // fields' edge under it rather than stopping short of it.
            let kinds = Preset::ALL
                .into_iter()
                .map(|preset| (Some(preset), preset.info().shown_name))
                .chain([(None, "Another agent")])
                .collect::<Vec<_>>();
            let labels = kinds
                .iter()
                .map(|(_, label)| SharedString::from(*label))
                .collect::<Vec<_>>();
            let active = kinds
                .iter()
                .position(|(choice, _)| *choice == kind)
                .unwrap_or(0);
            let shell = handle.clone();
            let kinds = onehand_ui::switch(
                "agent-kind",
                &labels,
                active,
                onehand_ui::ControlSize::Tall,
                move |&at, window, cx| {
                    let choice = kinds[at].0;
                    shell.update(cx, |shell, cx| shell.set_agent_kind(choice, window, cx));
                },
                cx,
            );
            content
                .child(title_row(title.clone(), cx))
                .child(field(
                    "Kind",
                    Some(crate::settings::kind_about(kind).into_any_element()),
                    kinds,
                    cx,
                ))
                .child(field(
                    "Name",
                    crate::settings::about("What the New session menu calls it"),
                    text_input(&name, cx).tall(),
                    cx,
                ))
                .when(kind.is_none(), |form| {
                    form.child(field(
                        "Command",
                        crate::settings::about(
                            "The program to run; it has to speak the Agent Client Protocol \
                             over standard input and output",
                        ),
                        text_input(&command, cx).tall(),
                        cx,
                    ))
                    .child(field(
                        "Arguments",
                        crate::settings::about("Separated by spaces; quote one that holds a space"),
                        text_input(&args, cx).tall(),
                        cx,
                    ))
                })
                .child(field(
                    "Environment",
                    crate::settings::about(
                        "Variables set for this agent, as NAME=value; a second account sets \
                         its config directory here",
                    ),
                    text_input(&env, cx).tall(),
                    cx,
                ))
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-agent")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.close_agent_form(window, cx);
                            }),
                        ),
                )
                .child(
                    crate::controls::action("save-agent")
                        .tall()
                        .primary()
                        // Refused, saying why, until the agent could be started.
                        .refuses(refusal)
                        .label(if editing.is_some() { "Save" } else { "Add" })
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.save_agent_draft(window, cx);
                            }),
                        ),
                ),
        )
        // Esc and the ✕ clear the state putting this on screen, or the next
        // frame renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, window, cx| {
            shell.close_agent_form(window, cx);
        }))
}
