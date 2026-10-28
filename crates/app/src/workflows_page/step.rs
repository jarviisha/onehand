use super::draft::{Kind, StepDraft, WorkflowDraft, split_problems, step_facts};
use super::{SHIPPED, WorkflowsPage, click};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::text_area;
use crate::controls::text_input;
use crate::settings::{about, field};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use onehand_ui::Tall as _;

use crate::controls::Named as _;
use gpui_component::{ActiveTheme, Disableable as _, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::{GateKind, Place};

mod settings;

/// How tall a step's line stands, its dot centred on it: the line the rail
/// runs through from one step to the next.
const STEP_LINE_H: gpui::Rems = gpui::rems(2.25);

/// A step's dot, its number inside it.
const DOT: gpui::Rems = gpui::rems(1.375);

/// Where the rail runs: through the dots' centres.
const RAIL_X: gpui::Rems = gpui::rems(0.6875);

/// Where the rail meets a dot's centre: the line's gutter above it, then
/// half the line.
const RAIL_JOIN: gpui::Rems = gpui::rems(1.25);

/// The least a step's id or label takes before the line wraps under the kind.
const NAME_MIN: gpui::Rems = gpui::rems(10.);

/// What a later step's menus need to know of an earlier one.
pub(super) struct Earlier {
    pub(super) id: String,
    pub(super) agent: bool,
    pub(super) keeps: bool,
}

/// The steps as a run, read down a rail: a numbered dot each, joined to the
/// next, then its name, its kind and what it does on failure, read without
/// opening it; the one open is marked, its fields in the column beside.
pub(super) fn steps_box(
    handle: &Entity<WorkflowsPage>,
    draft: &WorkflowDraft,
    cx: &App,
) -> AnyElement {
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
        step_row(handle, &row, step, cx)
    });
    div().v_flex().w_full().children(rows).into_any_element()
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

/// One step's line on the rail: its dot, its name, its kind and what it does
/// on failure; the one open beside marked.
fn step_row(
    handle: &Entity<WorkflowsPage>,
    row: &StepRow,
    step: &StepDraft,
    cx: &App,
) -> AnyElement {
    let (i, last, shipped, open) = (row.i, row.last, row.shipped, row.open);
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
                .named("Move or remove this step"),
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
        .gap_3()
        .min_h(STEP_LINE_H)
        .px_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        // The step open beside is marked as a picked row is.
        .map(|row| match open {
            true => row
                .bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground),
            false => row.hover(|row| row.bg(cx.theme().list_hover)),
        })
        .on_click(click(handle, move |page, window, cx| {
            page.edit_open_step(window, cx, |d| d.toggle_step(i))
        }))
        .child(div().min_w_0().truncate().font_medium().child(name.clone()))
        .child(
            onehand_ui::badge(
                step.kind.label(),
                onehand_ui::Tint {
                    fill: cx.theme().secondary,
                    ink: cx.theme().secondary_foreground,
                },
            )
            .flex_none(),
        )
        .child(div().flex_1())
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
        // A press on the ⋯ stays on it, or the row under it would open or
        // close the step on the same click.
        .when(!shipped, |head| {
            head.child(
                div()
                    .id(("step-more-hold", i))
                    .flex_none()
                    .on_mouse_up(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(more),
            )
        });
    let rail = cx.theme().border;
    let dot = {
        let (fill, edge, ink) = match row.open {
            true => (
                cx.theme().accent,
                cx.theme().accent,
                cx.theme().accent_foreground,
            ),
            false => (cx.theme().background, rail, muted),
        };
        div()
            .flex_none()
            .h(STEP_LINE_H)
            .h_flex()
            .items_center()
            .child(
                div()
                    .size(DOT)
                    .rounded_full()
                    .border_1()
                    .border_color(edge)
                    .bg(fill)
                    .h_flex()
                    .justify_center()
                    .text_xs()
                    .text_color(ink)
                    .child(format!("{}", i + 1)),
            )
    };
    // The rail in two halves a step draws for itself, into its dot from the
    // step before and out of it to the next, so it starts and ends on a dot
    // however tall an open step stands.
    let half = || {
        div()
            .absolute()
            .left(RAIL_X)
            .w_0()
            .border_l_1()
            .border_color(rail)
    };
    let step_line = |content: AnyElement| {
        div()
            .relative()
            .h_flex()
            .items_start()
            .gap_2()
            .when(i > 0, |line| line.child(half().top_0().h(RAIL_JOIN)))
            .when(!last, |line| line.child(half().top(RAIL_JOIN).bottom_0()))
            // A gutter above and below each line, inside the step's own
            // height so the rail runs on through it: the open step's fill and
            // a hovered neighbour's never touch.
            .child(div().flex_none().py_0p5().child(dot))
            .child(div().flex_1().min_w_0().py_0p5().v_flex().child(content))
            .into_any_element()
    };
    step_line(head.into_any_element())
}

/// The column the open step's fields stand in, beside the steps: its head
/// names the step, steps to the one before or after, and closes it; then
/// the step's kind, id and label, and its kind's own fields. While no step
/// is open it says what to pick.
pub(super) fn step_panel(
    handle: &Entity<WorkflowsPage>,
    draft: &WorkflowDraft,
    place: Place,
    cwd: Option<&std::path::Path>,
    cx: &App,
) -> AnyElement {
    let Some(i) = draft.open_step.filter(|i| *i < draft.steps.len()) else {
        return onehand_ui::hint("Pick a step to change it", cx);
    };
    let step = &draft.steps[i];
    let shipped = draft.shipped;
    let last = i + 1 == draft.steps.len();
    // Every earlier agent step by id, for what a command goes back to and an
    // approval approves.
    let earlier: Vec<Earlier> = draft.steps[..i]
        .iter()
        .map(|step| Earlier {
            id: step.id.read(cx).value().trim().to_string(),
            agent: step.kind == Kind::Agent,
            keeps: step.keep_answer,
        })
        .collect();
    let earlier = &earlier[..];
    let label = step.label.read(cx).value().trim().to_string();
    let name = match label.is_empty() {
        true => step.id.read(cx).value().trim().to_string(),
        false => label,
    };
    let kind_trigger = onehand_ui::picker(
        ("step-kind-trigger", i),
        step.kind.label(),
        true,
        onehand_ui::ControlSize::Tall,
        cx,
    );
    // A built-in workflow's refuses, as its fields are disabled.
    let kind_menu = if shipped {
        kind_trigger.refuses(Some(SHIPPED)).into_any_element()
    } else {
        let handle = handle.clone();
        crate::controls::menu_below(("step-kind", i), kind_trigger, move |mut menu, _, _| {
            for kind in Kind::ALL {
                let handle = handle.clone();
                menu = menu.item(crate::controls::menu_item(kind.label()).on_click(
                    move |_, window: &mut Window, cx: &mut App| {
                        handle.update(cx, |page, cx| {
                            page.edit_workflow_draft(window, cx, |d, _, _| d.steps[i].kind = kind)
                        });
                    },
                ));
            }
            menu
        })
        .into_any_element()
    };
    // Each sets which step is open: the one before, the one after, or none.
    // A step past either end refuses, saying there is none.
    let go = |id: &'static str,
              icon: IconName,
              tip: &'static str,
              key: &'static dyn gpui::Action,
              to: Option<usize>,
              refused: Option<&'static str>| {
        let handle = handle.clone();
        crate::controls::action((id, i))
            .small()
            .quiet_icon(false, cx)
            .icon(Icon::new(icon))
            .named_with_action(tip, key, Some("WorkflowsPage"))
            .refuses(refused.map(str::to_string))
            .on_click(move |_, window: &mut Window, cx: &mut App| {
                handle.update(cx, |page, cx| {
                    page.edit_open_step(window, cx, |d| d.open_step = to)
                })
            })
    };
    let head = div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_semibold()
                .child(format!("Step {} · {name}", i + 1)),
        )
        .child(
            onehand_ui::badge(
                step.kind.label(),
                onehand_ui::Tint {
                    fill: cx.theme().secondary,
                    ink: cx.theme().secondary_foreground,
                },
            )
            .flex_none(),
        )
        .child(div().flex_1())
        .child(go(
            "step-panel-up",
            IconName::ChevronUp,
            "The step before",
            &super::WorkflowStepUp,
            i.checked_sub(1),
            (i == 0).then_some("This is the first step"),
        ))
        .child(go(
            "step-panel-down",
            IconName::ChevronDown,
            "The step after",
            &super::WorkflowStepDown,
            (!last).then_some(i + 1),
            last.then_some("This is the last step"),
        ))
        .child(go(
            "step-panel-close",
            IconName::Close,
            "Close the step",
            &super::CloseWorkflowForm,
            None,
            None,
        ));
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
            // A prompt too long for the field is written in a dialog the
            // size of a page, at the field's end where the eye leaves it.
            let open_larger = {
                let (page, prompt, name) = (handle.clone(), step.prompt.clone(), name.clone());
                crate::controls::action(("step-prompt-larger", i))
                    .tall()
                    .quiet(cx)
                    .icon(Icon::new(IconName::Maximize))
                    .label(if shipped {
                        "Read larger…"
                    } else {
                        "Write larger…"
                    })
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        super::prompt_dialog::open(
                            page.clone(),
                            name.clone(),
                            prompt.clone(),
                            shipped,
                            window,
                            cx,
                        )
                    })
            };
            let gates = GateKind::ALL
                .iter()
                .filter(|gate| gate.fits(place) || step.gates.contains(gate))
                .map(|gate| {
                    let gate = *gate;
                    let on = step.gates.contains(&gate);
                    let handle = handle.clone();
                    div()
                        .flex_none()
                        .when(!shipped, |d| d.cursor_pointer())
                        .child(
                            onehand_ui::toggle(
                                SharedString::from(format!("gate-{i}-{gate:?}")),
                                on,
                            )
                            .label(gate.label())
                            .disabled(shipped)
                            .on_click(
                                move |_: &bool, window: &mut Window, cx: &mut App| {
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
                                },
                            ),
                        )
                })
                .collect::<Vec<_>>();
            let keep = {
                let handle = handle.clone();
                div()
                    .flex_none()
                    .when(!shipped, |d| d.cursor_pointer())
                    .child(
                        onehand_ui::toggle(("keep-answer", i), step.keep_answer)
                            .label("Keep its answer")
                            .disabled(shipped)
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
                    div()
                        .v_flex()
                        .gap_2()
                        .child(text_area(&step.prompt, cx).disabled(shipped))
                        .child(div().h_flex().justify_end().child(open_larger)),
                    cx,
                ))
                .child(field(
                    "Gates",
                    about("Checked when its turn ends"),
                    div().h_flex().flex_wrap().gap_3().children(gates),
                    cx,
                ))
                .child(div().h_flex().child(keep))
                .child(settings::agent_settings(handle, i, step, shipped, cwd, cx))
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
            let target_trigger = onehand_ui::picker(
                ("step-target-trigger", i),
                if current.is_empty() {
                    "Pick a step".to_string()
                } else {
                    current.clone()
                },
                !current.is_empty(),
                onehand_ui::ControlSize::Tall,
                cx,
            );
            let target = if shipped {
                target_trigger.refuses(Some(SHIPPED)).into_any_element()
            } else {
                let handle = handle.clone();
                crate::controls::menu_below(
                    ("step-target", i),
                    target_trigger,
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
                            menu = menu.item(crate::controls::menu_label("No earlier step fits"));
                        }
                        menu
                    },
                )
                .into_any_element()
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
        .gap_4()
        .child(head)
        .child(names)
        .child(body)
        .into_any_element()
}
