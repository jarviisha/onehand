mod head;

pub(super) use head::unreadable;

use super::WorkflowsPage;
use super::click;
use super::draft::Kind;
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::text_input;
use crate::settings::{about, field, section};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::workflow::Place;
use onehand_plugin_host::Tall as _;

/// The least a field of a pair takes before the pair stacks: a name stays
/// readable beside its neighbour, or goes under it.
const PAIR_MIN: gpui::Rems = gpui::rems(14.);

/// The editor: its head, then the workflow's fields in three groups (what
/// it is, how it runs, its steps), then what is wrong with it and *Save*.
pub(super) fn form(
    handle: &Entity<WorkflowsPage>,
    draft: &super::WorkflowDraft,
    writing: bool,
    choices: &[(std::path::PathBuf, SharedString)],
    cx: &App,
) -> AnyElement {
    // What is wrong with it, said above *Save* before the press: a name
    // another workflow has is one of them, since the launcher lists them by
    // name.
    let mut problems = draft.problems(cx);
    problems.extend(super::actions::name_clash(draft, cx));
    let danger = crate::theme::status_ink(cx).danger;
    let muted = cx.theme().muted_foreground;
    // A shipped workflow is read, never saved: it is changed by duplicating it.
    let shipped = draft.shipped;
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
    let steps = super::step::steps_box(handle, draft, place, cx);
    let input = |state: &Entity<gpui_component::input::InputState>| {
        text_input(state, cx).tall().disabled(shipped)
    };
    let half = |element: AnyElement| div().flex_1().min_w(PAIR_MIN).child(element);
    // A pair stands on its controls' line: one field's hint must not push its
    // input below its neighbour's.
    let about_group = section(Some("About"), None, cx).child(
        div()
            .h_flex()
            .items_end()
            .flex_wrap()
            .gap_4()
            .child(half(
                field(
                    "Name",
                    about("What the launcher lists it as"),
                    input(&draft.name),
                    cx,
                )
                .into_any_element(),
            ))
            .child(half(
                field("Description", None, input(&draft.description), cx).into_any_element(),
            )),
    );
    let runs_group = section(Some("How it runs"), None, cx)
        .child(field(
            "Where it works",
            about(
                "In the checkout leaves the change uncommitted for you; a new worktree gets a \
                 branch of its own, and the work is committed there",
            ),
            onehand_plugin_host::switch(
                "workflow-place",
                &places,
                place_at,
                onehand_plugin_host::ControlSize::Tall,
                pick_place,
                cx,
            ),
            cx,
        ))
        .child(
            div()
                .h_flex()
                .items_end()
                .gap_4()
                .child(div().flex_1().child(field(
                    "Misses allowed",
                    about("Turns that fail their gates before the run stops"),
                    input(&draft.misses),
                    cx,
                )))
                .child(div().flex_1().child(field(
                    "Timeout",
                    about("Working time, such as 45m or 2h"),
                    input(&draft.timeout),
                    cx,
                ))),
        );
    let add_step = {
        let handle = handle.clone();
        crate::controls::menu_below(
            "add-step",
            crate::controls::action("add-step-trigger")
                .quiet(cx)
                .tall()
                .icon(Icon::new(IconName::Plus))
                .label("Add step"),
            move |mut menu, _, _| {
                for kind in Kind::ALL {
                    let handle = handle.clone();
                    menu = menu.item(crate::controls::menu_item(kind.label()).on_click(
                        move |_, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |page, cx| {
                                page.edit_workflow_draft(window, cx, |d, window, cx| {
                                    d.add_step(kind, window, cx)
                                })
                            });
                        },
                    ));
                }
                menu
            },
        )
    };
    let steps_group = section(Some("Steps"), None, cx)
        .child(variables_hint(muted, cx))
        .child(steps)
        .when(!shipped, |group| {
            group.child(div().h_flex().child(add_step))
        });
    let foot = div()
        .v_flex()
        .gap_2()
        .children(
            problems
                .iter()
                .map(|problem| div().text_xs().text_color(danger).child(problem.clone())),
        )
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    crate::controls::action("save-workflow")
                        .tall()
                        .primary()
                        .loading(writing)
                        .label("Save")
                        .refuses(problems.first().cloned())
                        .on_click(click(handle, |page, window, cx| {
                            page.save_workflow_draft(window, cx)
                        })),
                )
                .child(
                    crate::controls::action("clear-workflow")
                        .tall()
                        .secondary()
                        .label("Cancel")
                        // Asks first when the form holds changes, as Esc does.
                        .on_click(click(handle, |page, window, cx| {
                            page.close_workflow_form(window, cx)
                        })),
                ),
        );

    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(head::editor_head(handle, draft, choices, cx))
        .child(about_group)
        .child(runs_group)
        .child(steps_group)
        .when(!shipped, |form| form.child(foot))
        .into_any_element()
}

/// What a prompt may use, a hint in meta text with each variable in mono, so
/// it reads as something to type.
fn variables_hint(muted: gpui::Hsla, cx: &App) -> impl IntoElement {
    let mono = cx.theme().mono_font_family.clone();
    let code = |text: &'static str| div().font_family(mono.clone()).child(text);
    div()
        .h_flex()
        .flex_wrap()
        .gap_x_1()
        .text_xs()
        .text_color(muted)
        .child("A prompt may use")
        .child(code("{brief}"))
        .child(code("{instructions}"))
        .child(code("{check_output}"))
        .child(code("{revise}"))
        .child("and")
        .child(code("{output.<step id>}"))
        .child("for what an earlier step kept")
}
