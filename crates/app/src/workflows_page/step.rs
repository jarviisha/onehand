use super::WorkflowsPage;
use super::draft::{Kind, StepDraft};
use super::{click, row_delete, row_icon};
use crate::settings::{about, field};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div, rems,
};

use gpui_component::input::{Input, Textarea};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::{GateKind, Place};

/// What a later step's menus need to know of an earlier one.
pub(super) struct Earlier {
    pub(super) id: String,
    pub(super) agent: bool,
    pub(super) keeps: bool,
}

/// One step of the form, in a box of its own.
pub(super) fn step_box(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    step: &StepDraft,
    earlier: &[Earlier],
    place: Place,
    cx: &App,
) -> AnyElement {
    let kind_menu = {
        let handle = handle.clone();
        crate::controls::menu_below(
            ("step-kind", i),
            crate::controls::action(("step-kind-trigger", i))
                .outline()
                .xsmall()
                .label(step.kind.label())
                .icon(Icon::new(IconName::ChevronDown)),
            move |mut menu, _, _| {
                for kind in Kind::ALL {
                    let handle = handle.clone();
                    menu = menu.item(crate::controls::menu_item(kind.label()).on_click(
                        move |_, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |page, cx| {
                                page.edit_workflow_draft(window, cx, |d, _, _| {
                                    d.steps[i].kind = kind
                                })
                            });
                        },
                    ));
                }
                menu
            },
        )
    };
    let head = div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_medium()
                .child(format!("Step {}", i + 1)),
        )
        .child(kind_menu)
        .child(div().flex_1())
        .child(
            row_icon(("step-up", i), IconName::ArrowUp, "Move up").on_click(click(
                handle,
                move |page, window, cx| {
                    page.edit_workflow_draft(window, cx, |d, _, _| d.move_step(i, true))
                },
            )),
        )
        .child(
            row_icon(("step-down", i), IconName::ArrowDown, "Move down").on_click(click(
                handle,
                move |page, window, cx| {
                    page.edit_workflow_draft(window, cx, |d, _, _| d.move_step(i, false))
                },
            )),
        )
        .child(row_delete(
            handle,
            ("step-delete", i),
            move |page, window, cx| {
                page.edit_workflow_draft(window, cx, |d, _, _| {
                    d.steps.remove(i);
                })
            },
            cx,
        ));
    let names = div()
        .h_flex()
        .gap_4()
        .child(
            div()
                .flex_1()
                .child(field("Id", None, Input::new(&step.id).small(), cx)),
        )
        .child(
            div()
                .flex_1()
                .child(field("Label", None, Input::new(&step.label).small(), cx)),
        );

    let body = match step.kind {
        Kind::Agent => {
            let gates = GateKind::ALL
                .iter()
                .filter(|gate| gate.fits(place) || step.gates.contains(gate))
                .map(|gate| {
                    let gate = *gate;
                    let on = step.gates.contains(&gate);
                    let handle = handle.clone();
                    div().flex_none().cursor_pointer().child(
                        Switch::new(SharedString::from(format!("gate-{i}-{gate:?}")))
                            .checked(on)
                            .small()
                            .label(gate.label())
                            .on_click(move |_: &bool, window: &mut Window, cx: &mut App| {
                                handle.update(cx, |page, cx| {
                                    page.edit_workflow_draft(window, cx, |d, _, _| {
                                        let gates = &mut d.steps[i].gates;
                                        match gates.iter().position(|g| *g == gate) {
                                            Some(at) => {
                                                gates.remove(at);
                                            }
                                            None => gates.push(gate),
                                        }
                                    })
                                });
                            }),
                    )
                })
                .collect::<Vec<_>>();
            let keep = {
                let handle = handle.clone();
                div().flex_none().cursor_pointer().child(
                    Switch::new(("keep-answer", i))
                        .checked(step.keep_answer)
                        .small()
                        .label("Keep its answer")
                        .on_click(move |_: &bool, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |page, cx| {
                                page.edit_workflow_draft(window, cx, |d, _, _| {
                                    d.steps[i].keep_answer = !d.steps[i].keep_answer
                                })
                            });
                        }),
                )
            };
            div()
                .v_flex()
                .gap_3()
                .child(field(
                    "Prompt",
                    None,
                    Textarea::new(&step.prompt).h(rems(8.)),
                    cx,
                ))
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(div().text_sm().child("Gates, checked when its turn ends"))
                        .child(div().h_flex().flex_wrap().gap_3().children(gates)),
                )
                .child(div().h_flex().child(keep))
                .into_any_element()
        }
        Kind::Push => div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("onehand pushes the commit the check passed on, as the run's branch.")
            .into_any_element(),
        Kind::PullRequest => div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(
                "onehand opens a draft pull request from the run's branch, or takes the open one.",
            )
            .into_any_element(),
        kind @ (Kind::Command | Kind::Approval | Kind::StatusChecks) => {
            let needs_keep = kind == Kind::Approval;
            let title = match needs_keep {
                true => "Approves the answer of",
                false => "On failure, back to",
            };
            let choices: Vec<String> = earlier
                .iter()
                .filter(|step| step.agent && (!needs_keep || step.keeps))
                .map(|step| step.id.clone())
                .collect();
            let current = step.target.clone();
            let target = {
                let handle = handle.clone();
                crate::controls::menu_below(
                    ("step-target", i),
                    crate::controls::action(("step-target-trigger", i))
                        .outline()
                        .small()
                        .label(if current.is_empty() {
                            "Pick a step".to_string()
                        } else {
                            current.clone()
                        })
                        .icon(Icon::new(IconName::ChevronDown)),
                    move |mut menu, _, _| {
                        for id in &choices {
                            let (handle, id) = (handle.clone(), id.clone());
                            menu = menu.item(
                                crate::controls::menu_item(id.clone())
                                    .checked(id == current)
                                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                                        let id = id.clone();
                                        handle.update(cx, |page, cx| {
                                            page.edit_workflow_draft(window, cx, move |d, _, _| {
                                                d.steps[i].target = id
                                            })
                                        });
                                    }),
                            );
                        }
                        if choices.is_empty() {
                            menu = menu.label("No earlier step fits");
                        }
                        menu
                    },
                )
            };
            div()
                .v_flex()
                .gap_3()
                .when(kind == Kind::Command, |col| {
                    col.child(field(
                        "Command",
                        about("Run by onehand in the work, through sh."),
                        Input::new(&step.command),
                        cx,
                    ))
                })
                .when(kind == Kind::StatusChecks, |col| {
                    col.child(field(
                        "Wait at most",
                        about("Such as 45m or 2h; passing all, the pull request leaves draft."),
                        Input::new(&step.wait).small(),
                        cx,
                    ))
                })
                .child(field(title, None, div().h_flex().child(target), cx))
                .into_any_element()
        }
    };

    div()
        .v_flex()
        .gap_3()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(head)
        .child(names)
        .child(body)
        .into_any_element()
}
