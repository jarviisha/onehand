use super::WorkflowsPage;
use super::draft::{Kind, StepDraft};
use crate::controls::Quiet as _;
use crate::controls::text_area;
use crate::controls::text_input;
use crate::settings::{about, field};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use onehand_plugin_host::Tall as _;

use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::{GateKind, Place};

/// How tall a step's prompt stands before it scrolls: a paragraph in view, so
/// a step reads without opening it, and a form of several steps still fits a
/// screen.
const PROMPT_H: gpui::Rems = gpui::rems(8.);

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
    last: bool,
    step: &StepDraft,
    earlier: &[Earlier],
    place: Place,
    cx: &App,
) -> AnyElement {
    let kind_menu = {
        let handle = handle.clone();
        crate::controls::menu_below(
            ("step-kind", i),
            onehand_plugin_host::picker(
                ("step-kind-trigger", i),
                step.kind.label(),
                true,
                onehand_plugin_host::ControlSize::Tall,
                cx,
            ),
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
    // Its number and one ⋯ holding what is done to the step itself: moved
    // up or down, refusing at the ends, or removed.
    let more = {
        let (up, down, remove) = (handle.clone(), handle.clone(), handle.clone());
        let danger = crate::theme::status_ink(cx).danger;
        crate::controls::menu_below(
            ("step-more", i),
            crate::controls::action(("step-more-trigger", i))
                .small()
                .quiet_icon(false, cx)
                .icon(IconName::Ellipsis)
                .tooltip("Move or remove this step"),
            move |menu, _, _| {
                let (up, down, remove) = (up.clone(), down.clone(), remove.clone());
                menu.item(
                    crate::controls::menu_item("Move up")
                        .disabled(i == 0)
                        .on_click(move |_, window: &mut Window, cx: &mut App| {
                            up.update(cx, |page, cx| {
                                page.edit_workflow_draft(window, cx, |d, _, _| d.move_step(i, true))
                            })
                        }),
                )
                .item(
                    crate::controls::menu_item("Move down")
                        .disabled(last)
                        .on_click(move |_, window: &mut Window, cx: &mut App| {
                            down.update(cx, |page, cx| {
                                page.edit_workflow_draft(window, cx, |d, _, _| {
                                    d.move_step(i, false)
                                })
                            })
                        }),
                )
                .separator()
                .item(
                    crate::controls::menu_row(move |_, _| div().text_color(danger).child("Remove"))
                        .on_click(move |_, window: &mut Window, cx: &mut App| {
                            remove.update(cx, |page, cx| {
                                page.edit_workflow_draft(window, cx, |d, _, _| {
                                    d.steps.remove(i);
                                })
                            })
                        }),
                )
            },
        )
    };
    let head = div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .font_semibold()
                .child(format!("Step {}", i + 1)),
        )
        .child(more);
    let names = div()
        .h_flex()
        .gap_4()
        .child(
            div()
                .flex_1()
                .child(field("Id", None, text_input(&step.id, cx).tall(), cx)),
        )
        .child(
            div()
                .flex_1()
                .child(field("Label", None, text_input(&step.label, cx).tall(), cx)),
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
                .gap_4()
                .child(field(
                    "Prompt",
                    None,
                    text_area(&step.prompt, cx).h(PROMPT_H),
                    cx,
                ))
                .child(field(
                    "Gates",
                    about("Checked when its turn ends"),
                    div().h_flex().flex_wrap().gap_3().children(gates),
                    cx,
                ))
                .child(div().h_flex().child(keep))
                .into_any_element()
        }
        Kind::Push => div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("onehand pushes the commit the check passed on, as the run's branch")
            .into_any_element(),
        Kind::PullRequest => div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                "onehand opens a draft pull request from the run's branch, or takes the open one",
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
                    onehand_plugin_host::picker(
                        ("step-target-trigger", i),
                        if current.is_empty() {
                            "Pick a step".to_string()
                        } else {
                            current.clone()
                        },
                        !current.is_empty(),
                        onehand_plugin_host::ControlSize::Tall,
                        cx,
                    ),
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
                .gap_4()
                .when(kind == Kind::Command, |col| {
                    col.child(field(
                        "Command",
                        about("Run by onehand in the work, through sh"),
                        text_input(&step.command, cx).tall(),
                        cx,
                    ))
                })
                .when(kind == Kind::StatusChecks, |col| {
                    col.child(field(
                        "Wait at most",
                        about("Such as 45m or 2h; passing all, the pull request leaves draft"),
                        text_input(&step.wait, cx).tall(),
                        cx,
                    ))
                })
                .child(field(title, None, div().h_flex().child(target), cx))
                .into_any_element()
        }
    };

    // The kind picker is its first field: it decides every field after it.
    onehand_plugin_host::card_box(cx)
        .gap_4()
        .child(head)
        .child(field("Kind", None, div().h_flex().child(kind_menu), cx))
        .child(names)
        .child(body)
        .into_any_element()
}
