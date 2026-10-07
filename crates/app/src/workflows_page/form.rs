use super::WorkflowsPage;
use super::click;
use super::draft::Kind;
use super::step::{Earlier, step_box};
use crate::controls::Refuses as _;
use crate::settings::{about, field, section};
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::Place;

/// How wide the margin beside the steps runs, in rems.
const MARGIN: f32 = 4.5;

/// A step's margin: where its failure sends the run back to, and which later
/// steps send theirs back to it, by step number. The arrows of a loop read
/// down the margin without opening a step.
fn margin(back: Option<usize>, from: &[usize], muted: gpui::Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .w(gpui::rems(MARGIN))
        .pt_3()
        .v_flex()
        .gap_1()
        .text_xs()
        .text_color(muted)
        .children(back.map(|to| {
            div()
                .h_flex()
                .gap_1()
                .child(Icon::new(IconName::ArrowUp).xsmall())
                .child(format!("to {}", to + 1))
        }))
        .children(from.iter().map(|k| {
            div()
                .h_flex()
                .gap_1()
                .child(Icon::new(IconName::ArrowLeft).xsmall())
                .child(format!("from {}", k + 1))
        }))
}

/// The form: the template's own fields, its steps, what is wrong with it,
/// and Save.
pub(super) fn form(
    handle: &Entity<WorkflowsPage>,
    draft: &super::WorkflowDraft,
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
            handle.update(cx, |page, cx| {
                page.edit_workflow_draft(window, cx, |d, _, _| d.place = place)
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

    // Where each failure goes back to, by step number, for the margin.
    let back: Vec<Option<usize>> = draft
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            matches!(step.kind, Kind::Command | Kind::StatusChecks)
                .then(|| ids[..i].iter().position(|e| e.id == step.target))
                .flatten()
        })
        .collect();
    let steps = draft
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let from: Vec<usize> = (0..back.len()).filter(|k| back[*k] == Some(i)).collect();
            div()
                .h_flex()
                .items_start()
                .gap_2()
                .w_full()
                .child(margin(back[i], &from, muted))
                .child(div().flex_1().min_w_0().child(step_box(
                    handle,
                    i,
                    step,
                    &ids[..i],
                    place,
                    cx,
                )))
        })
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
                    .on_click(click(handle, move |page, window, cx| {
                        page.edit_workflow_draft(window, cx, |d, window, cx| {
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
                    .on_click(click(handle, |page, window, cx| {
                        page.save_workflow_draft(window, cx)
                    })),
            )
            .child(
                crate::controls::action("clear-workflow")
                    .ghost()
                    .label("Cancel")
                    .on_click(click(handle, |page, _, cx| page.clear_workflow_draft(cx))),
            ),
    )
    .into_any_element()
}
