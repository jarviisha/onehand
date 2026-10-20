mod head;

pub(super) use head::{editor_head, more_menu, run_menu, unreadable};

use super::WorkflowsPage;
use super::click;
use super::draft::Kind;
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::settings::{about, field, section};
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, Entity, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::button::ButtonVariants;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::Place;
use onehand_plugin_host::Tall as _;

/// The least a field of a pair takes before the pair stacks: a name stays
/// readable beside its neighbour, or goes under it.
const PAIR_MIN: gpui::Rems = gpui::rems(14.);

/// The editor's workflow column: what it is and how it runs, what a
/// prompt may use, then what is wrong with it above *Save*.
pub(super) fn workflow_column(
    handle: &Entity<WorkflowsPage>,
    draft: &super::WorkflowDraft,
    writing: bool,
    cx: &App,
) -> AnyElement {
    // What is wrong with it, said above *Save* before the press: a name
    // another workflow has is one of them, since the launcher lists them by
    // name.
    let mut problems = draft.problems(cx);
    problems.extend(super::actions::name_clash(draft, cx));
    let danger = crate::theme::status_ink(cx).danger;
    // A shipped workflow is read, never saved: it is changed by duplicating it.
    let shipped = draft.shipped;
    let place = draft.place;
    // Where it works, picked from a menu under a picker naming the choice:
    // the column is too narrow for both choices side by side. A shipped
    // workflow's refuses, as its fields are disabled.
    let place_picker = {
        let trigger = onehand_plugin_host::picker(
            "workflow-place-trigger",
            place.label(),
            true,
            onehand_plugin_host::ControlSize::Tall,
            cx,
        );
        if shipped {
            trigger
                .refuses(Some("Built in: duplicate it to change it".to_string()))
                .into_any_element()
        } else {
            let handle = handle.clone();
            crate::controls::menu_below("workflow-place", trigger, move |mut menu, _, _| {
                for choice in Place::ALL {
                    let handle = handle.clone();
                    menu = menu.item(
                        crate::controls::menu_item(choice.label())
                            .checked(choice == place)
                            .on_click(move |_, window: &mut Window, cx: &mut App| {
                                handle.update(cx, |page, cx| {
                                    page.edit_workflow_draft(window, cx, |d, _, _| d.place = choice)
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
        }
    };
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
            div().h_flex().child(place_picker),
            cx,
        ))
        .child(
            // A pair as *About*'s is: side by side where the column holds
            // both, one under the other where it does not.
            div()
                .h_flex()
                .items_end()
                .flex_wrap()
                .gap_4()
                .child(half(
                    field(
                        "Misses allowed",
                        about("Turns that fail their gates before the run stops"),
                        input(&draft.misses),
                        cx,
                    )
                    .into_any_element(),
                ))
                .child(half(
                    field(
                        "Timeout",
                        about("Working time, such as 45m or 2h"),
                        input(&draft.timeout),
                        cx,
                    )
                    .into_any_element(),
                )),
        );
    // A new form nobody has touched yet is not wrong yet: *Save* refuses
    // and says why on hover, and the problems wait for the first change. One
    // opened on something (a file, a copy, an import) says at once.
    let untouched = draft.file.is_none() && draft.original.name.is_empty() && !draft.dirty(cx);
    let foot = div()
        .v_flex()
        .gap_3()
        .when(!untouched && !problems.is_empty(), |foot| {
            foot.child(problems_box(&problems, danger, cx))
        })
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
        .child(about_group)
        .child(runs_group)
        .child(variables_hint(cx))
        .when(!shipped, |form| form.child(foot))
        .into_any_element()
}

/// The editor's steps column: the *Steps* box, its count and *Add step* on
/// its title row, the steps down their rail.
pub(super) fn steps_column(
    handle: &Entity<WorkflowsPage>,
    draft: &super::WorkflowDraft,
    cx: &App,
) -> AnyElement {
    let shipped = draft.shipped;
    let steps = super::step::steps_box(handle, draft, cx);
    let add_step = {
        let handle = handle.clone();
        crate::controls::menu_below(
            "add-step",
            crate::controls::action("add-step-trigger")
                .quiet(cx)
                .short()
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
    // The steps decide what a run does, so they are the one group on the
    // form standing in a box, its count and *Add step* on its title row,
    // where the fields around it stand plain.
    let steps_group = crate::chat::pane::page_parts::page_card(
        "Steps",
        Some(draft.steps.len()),
        (!shipped).then(|| add_step.into_any_element()),
        cx,
    )
    .p_4()
    .gap_4()
    .child(steps);
    steps_group.into_any_element()
}

/// What keeps the form from being saved, on the danger tint's fill so it
/// reads as one standing condition rather than loose words over *Save*.
fn problems_box(problems: &[String], danger: gpui::Hsla, cx: &App) -> impl IntoElement {
    let tint = onehand_plugin_host::tint(danger, cx);
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .bg(tint.fill)
        .text_xs()
        .text_color(tint.ink)
        .child(Icon::new(IconName::TriangleAlert).xsmall().flex_none())
        .child(
            div()
                .v_flex()
                .gap_1()
                .min_w_0()
                .children(problems.iter().map(|problem| div().child(problem.clone()))),
        )
}

/// What a prompt may use, a hint in meta text with each variable in mono, so
/// it reads as something to type.
pub(super) fn variables_hint(cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
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
