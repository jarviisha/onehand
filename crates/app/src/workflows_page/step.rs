use super::draft::{Kind, StepDraft, WorkflowDraft, split_problems, step_facts};
use super::{WorkflowsPage, click};
use crate::controls::Quiet as _;
use crate::controls::text_area;
use crate::controls::text_input;
use crate::settings::{about, field};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use onehand_plugin_host::Tall as _;

use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::{GateKind, Place};

/// How tall a step's prompt stands before it scrolls: a paragraph in view, so
/// a step reads without opening it, and a form of several steps still fits a
/// screen.
const PROMPT_H: gpui::Rems = gpui::rems(8.);

/// The least a step's id or label takes before the line wraps under the kind.
const NAME_MIN: gpui::Rems = gpui::rems(10.);

/// What a later step's menus need to know of an earlier one.
pub(super) struct Earlier {
    pub(super) id: String,
    pub(super) agent: bool,
    pub(super) keeps: bool,
}

/// The steps, in one box, a row each: its number, its kind, its name and
/// what it does on failure, read without opening it; the one open shows its
/// fields under its row.
pub(super) fn steps_box(
    handle: &Entity<WorkflowsPage>,
    draft: &WorkflowDraft,
    place: Place,
    cx: &App,
) -> AnyElement {
    // Every earlier agent step by id, for what a command goes back to and an
    // approval approves.
    let earlier: Vec<Earlier> = draft
        .steps
        .iter()
        .map(|step| Earlier {
            id: step.id.read(cx).value().trim().to_string(),
            agent: step.kind == Kind::Agent,
            keeps: step.keep_answer,
        })
        .collect();
    let specs: Vec<_> = draft.steps.iter().map(|step| step.to_spec(cx)).collect();
    let problems = split_problems(&draft.problem_list(cx), draft.steps.len());
    let rows = draft.steps.iter().enumerate().map(|(i, step)| {
        let row = StepRow {
            i,
            last: i + 1 == draft.steps.len(),
            open: draft.open_step == Some(i),
            shipped: draft.shipped,
            facts: step_facts(i, &specs),
            problem: problems[i].clone(),
        };
        step_row(handle, &row, step, &earlier[..i], place, cx)
    });
    crate::chat::pane::page_parts::card_box(cx)
        .w_full()
        .gap_0()
        .p_0()
        .child(crate::chat::pane::page_parts::ruled(rows, cx))
        .into_any_element()
}

/// What a step's row is drawn from besides the step itself.
struct StepRow {
    i: usize,
    last: bool,
    open: bool,
    shipped: bool,
    facts: Vec<String>,
    problem: Option<String>,
}

/// One step: its row, and its fields under it while it is the one open.
fn step_row(
    handle: &Entity<WorkflowsPage>,
    row: &StepRow,
    step: &StepDraft,
    earlier: &[Earlier],
    place: Place,
    cx: &App,
) -> AnyElement {
    let (i, last, shipped) = (row.i, row.last, row.shipped);
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
                                    d.remove_step(i);
                                })
                            })
                        }),
                )
            },
        )
    };
    let muted = crate::theme::meta_ink(cx);
    let danger = crate::theme::status_ink(cx).danger;
    let label = step.label.read(cx).value().trim().to_string();
    let name = match label.is_empty() {
        true => step.id.read(cx).value().trim().to_string(),
        false => label,
    };
    let head = div()
        .id(("step-row", i))
        .h_flex()
        .items_center()
        .gap_2()
        .min_h(crate::tokens::ROW_H)
        .px_3()
        .py_1()
        .cursor_pointer()
        .hover(|row| row.bg(cx.theme().list_hover))
        .on_click(click(handle, move |page, window, cx| {
            page.edit_open_step(window, cx, |d| d.toggle_step(i))
        }))
        .child(
            div()
                .flex_none()
                .text_color(muted)
                .child(format!("{}", i + 1)),
        )
        .child(
            onehand_plugin_host::badge(
                step.kind.label(),
                onehand_plugin_host::Tint {
                    fill: cx.theme().secondary,
                    ink: cx.theme().secondary_foreground,
                },
            )
            .flex_none(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_medium()
                .child(name),
        )
        .when(!row.facts.is_empty(), |head| {
            head.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(row.facts.join(" · ")),
            )
        })
        .children(row.problem.clone().map(|problem| {
            div()
                .min_w_0()
                .h_flex()
                .gap_1()
                .text_xs()
                .text_color(danger)
                .child(Icon::new(IconName::TriangleAlert).xsmall().flex_none())
                .child(div().truncate().child(problem))
        }))
        .child(
            Icon::new(match row.open {
                true => IconName::ChevronUp,
                false => IconName::ChevronDown,
            })
            .small()
            .text_color(muted)
            .flex_none(),
        )
        .when(!shipped, |head| head.child(more));
    if !row.open {
        return head.into_any_element();
    }
    // The kind picker is the first field: it decides every field after it.
    let names = div()
        .h_flex()
        .flex_wrap()
        .gap_4()
        .child(
            div()
                .flex_none()
                .child(field("Kind", None, div().h_flex().child(kind_menu), cx)),
        )
        .child(div().flex_1().min_w(NAME_MIN).child(field(
            "Id",
            None,
            text_input(&step.id, cx).tall().disabled(shipped),
            cx,
        )))
        .child(div().flex_1().min_w(NAME_MIN).child(field(
            "Label",
            None,
            text_input(&step.label, cx).tall().disabled(shipped),
            cx,
        )));

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
                    text_area(&step.prompt, cx).h(PROMPT_H).disabled(shipped),
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
                        text_input(&step.command, cx).tall().disabled(shipped),
                        cx,
                    ))
                })
                .when(kind == Kind::StatusChecks, |col| {
                    col.child(field(
                        "Wait at most",
                        about("Such as 45m or 2h; passing all, the pull request leaves draft"),
                        text_input(&step.wait, cx).tall().disabled(shipped),
                        cx,
                    ))
                })
                .child(field(title, None, div().h_flex().child(target), cx))
                .into_any_element()
        }
    };

    div()
        .v_flex()
        .child(head)
        .child(
            div()
                .v_flex()
                .gap_4()
                .px_3()
                .pt_2()
                .pb_4()
                .child(names)
                .child(body),
        )
        .into_any_element()
}
