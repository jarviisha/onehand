use super::click;
use super::draft::Kind;
use super::step::{Earlier, step_box};
use crate::controls::Refuses as _;
use crate::settings::{about, field, section};
use crate::shell::Shell;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::Place;

/// The form: the template's own fields, its steps, what is wrong with it,
/// and Save.
pub(crate) fn form(
    handle: &Entity<Shell>,
    draft: &crate::settings::WorkflowDraft,
    cx: &App,
) -> AnyElement {
    let problems = draft.problems(cx);
    let danger = crate::theme::status_ink(cx).danger;
    let muted = cx.theme().muted_foreground;
    let editing = draft.file.is_some();
    let place = draft.place;
    let places: Vec<SharedString> = Place::ALL.iter().map(|p| p.label().into()).collect();
    let place_at = Place::ALL.iter().position(|p| *p == place).unwrap_or(0);
    let pick_place = {
        let handle = handle.clone();
        move |at: &usize, window: &mut Window, cx: &mut App| {
            let place = Place::ALL[*at];
            handle.update(cx, |shell, cx| {
                shell.edit_workflow_draft(window, cx, |d, _, _| d.place = place)
            });
        }
    };
    // Every earlier agent step by id, for what a command goes back to and an
    // approval approves.
    let ids: Vec<Earlier> = draft
        .steps
        .iter()
        .map(|step| Earlier {
            id: step.id.read(cx).value().trim().to_string(),
            agent: step.kind == Kind::Agent,
            keeps: step.keep_answer,
        })
        .collect();

    let steps = draft
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| step_box(handle, i, step, &ids[..i], place, cx))
        .collect::<Vec<_>>();

    section(
        Some(if editing {
            "Edit workflow"
        } else {
            "New workflow"
        }),
        None,
        cx,
    )
    .child(field(
        "Name",
        about("What the launcher lists it as."),
        Input::new(&draft.name),
        cx,
    ))
    .child(field(
        "Description",
        None,
        Input::new(&draft.description),
        cx,
    ))
    .child(field(
        "Where it works",
        about(
            "In the checkout leaves the change uncommitted for you; a new worktree gets a \
             branch of its own, and the work is committed there.",
        ),
        onehand_plugin_host::switch(
            "workflow-place",
            &places,
            place_at,
            gpui_component::Size::Small,
            pick_place,
            cx,
        ),
        cx,
    ))
    .child(
        div()
            .h_flex()
            .gap_4()
            .child(div().flex_1().child(field(
                "Misses allowed",
                about("Turns that fail their gates before the run stops."),
                Input::new(&draft.misses),
                cx,
            )))
            .child(div().flex_1().child(field(
                "Timeout",
                about("Working time, such as 45m or 2h."),
                Input::new(&draft.timeout),
                cx,
            ))),
    )
    .child(div().text_sm().text_color(muted).child(
        "A prompt may use {brief}, {instructions}, {check_output}, {revise} and \
                 {output.<step id>} for what an earlier step kept.",
    ))
    .children(steps)
    .child(
        div()
            .h_flex()
            .flex_wrap()
            .gap_2()
            .children(Kind::ALL.into_iter().enumerate().map(|(at, kind)| {
                crate::controls::action(("add-step", at))
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::Plus))
                    .label(format!("{} step", kind.label()))
                    .on_click(click(handle, move |shell, window, cx| {
                        shell.edit_workflow_draft(window, cx, |d, window, cx| {
                            d.add_step(kind, window, cx)
                        });
                    }))
            })),
    )
    .children(
        problems
            .iter()
            .map(|problem| div().text_sm().text_color(danger).child(problem.clone())),
    )
    .child(
        div()
            .h_flex()
            .gap_2()
            .child(
                crate::controls::action("save-workflow")
                    .primary()
                    .refuses(!problems.is_empty())
                    .label("Save")
                    .on_click(click(handle, |shell, window, cx| {
                        shell.save_workflow_draft(window, cx)
                    })),
            )
            .child(
                crate::controls::action("clear-workflow")
                    .ghost()
                    .label("Cancel")
                    .on_click(click(handle, |shell, _, cx| shell.clear_workflow_draft(cx))),
            ),
    )
    .into_any_element()
}
