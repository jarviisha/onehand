//! An agent step's settings: the mode its visits run in and the agent's
//! options they set. A workflow is tied to no agent, so the choices drawn are
//! those of the agent runs take by default, as it was seen to offer them;
//! a value it does not offer stays on screen, marked, and any other agent's
//! options are typed by id. The editor judges no value against an agent, the
//! run's preflight does.

use super::super::draft::{StepDraft, run_agent_offer, set_pick};
use super::super::{SHIPPED, WorkflowsPage};
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::text_input;
use crate::settings::{about, field, section};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_ui::Tall as _;
use onehand_ui::TallIcon as _;
use std::path::Path;
use std::rc::Rc;

/// The most of an agent's options drawn as rows of choices before it counts
/// the rest, which are still set by id among the typed rows.
const OPTIONS_SHOWN: usize = 12;

/// What picking a choice does: `None` for the run's own value.
type Pick = crate::controls::SettingPick;

/// The section on step `i`: what the agent runs take offers as rows of
/// choices, *Mode* first, then the rows typed by id and *Add option*. While
/// the agent's offer is not known, *Mode* and the rows are typed, with
/// *Check the agent*. A shipped workflow's are read only, with nothing to
/// add or remove.
pub(super) fn agent_settings(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    step: &StepDraft,
    shipped: bool,
    cwd: Option<&Path>,
    cx: &App,
) -> AnyElement {
    let agent = run_agent_offer(cx);
    let offer = agent.as_ref().and_then(|(_, offer)| offer.as_ref());
    let said: SharedString = match (&agent, crate::unattended::run_agent(cx)) {
        (Some((spec, Some(_))), _) => format!("Choices {} offers", spec.name).into(),
        (Some((spec, None)), _) => format!("What {} offers is not known yet", spec.name).into(),
        (None, Some(name)) => {
            format!("{name} is not configured; the choices come from the agent runs use").into()
        }
        (None, None) => "No agent is configured, so there is nothing to pick from".into(),
    };
    // The agent comes up in a project the window has open, else at home.
    let check = agent
        .as_ref()
        .filter(|(_, offer)| offer.is_none())
        .zip(cwd.map(Path::to_path_buf).or_else(std::env::home_dir))
        .and_then(|((spec, _), cwd)| crate::agent_check::Ask::new(Some(&spec.name), &cwd, None, cx))
        .map(|ask| crate::agent_check::button(i, &ask, cx));

    let mut body = section(Some("Agent settings"), Some(said), cx)
        .children(check.map(|check| div().h_flex().child(check)));
    let typed = typed_options(handle, i, step, shipped, cx);
    let Some(offer) = offer else {
        body = body.child(field(
            "Mode",
            None,
            text_input(&step.mode, cx).tall().disabled(shipped),
            cx,
        ));
        // Picked while the agent's offer was known: each stays on screen,
        // to keep or leave to the run, with nothing judged of it.
        for (n, (id, value)) in step.picked.iter().enumerate() {
            let row = crate::controls::setting_row(
                format!("step-pick-{i}-{n}"),
                "Run's value",
                None,
                Some(value),
                false,
                shipped.then_some(SHIPPED),
                option_pick(handle, i, id),
                cx,
            );
            body = body.child(field(id.clone(), None, row, cx));
        }
        return body
            .child(field(
                "Options",
                about("By the agent's own ids and values"),
                typed,
                cx,
            ))
            .into_any_element();
    };

    let mode = step.mode.read(cx).value().trim().to_string();
    let mode = (!mode.is_empty()).then_some(mode);
    if !offer.modes.is_empty() || mode.is_some() {
        let pick: Pick = {
            let (handle, input) = (handle.clone(), step.mode.clone());
            Rc::new(move |to, window, cx| {
                let input = input.clone();
                handle.update(cx, |page, cx| {
                    page.edit_workflow_draft(window, cx, |_, window, cx| {
                        input.update(cx, |state, cx| {
                            state.set_value(to.unwrap_or_default(), window, cx)
                        })
                    })
                })
            })
        };
        let row = crate::controls::setting_row(
            format!("step-mode-{i}"),
            "Run's mode",
            Some(&offer.modes),
            mode.as_deref(),
            true,
            shipped.then_some(SHIPPED),
            pick,
            cx,
        );
        body = body.child(field("Mode", None, row, cx));
    }

    // The options offered, in the agent's order, then any picked that it
    // does not offer, so no value is hidden.
    let shown: Vec<(&str, &[String])> = offer
        .options
        .iter()
        .take(OPTIONS_SHOWN)
        .map(|(id, values)| (id.as_str(), values.as_slice()))
        .chain(
            step.picked
                .keys()
                .filter(|id| {
                    !offer
                        .options
                        .iter()
                        .take(OPTIONS_SHOWN)
                        .any(|(shown, _)| shown == *id)
                })
                .map(|id| {
                    let values = offer
                        .options
                        .iter()
                        .find(|(offered, _)| offered == id)
                        .map_or(&[][..], |(_, values)| values.as_slice());
                    (id.as_str(), values)
                }),
        )
        .collect();
    let typed_texts = step.option_texts(cx);
    let left_out = offer
        .options
        .iter()
        .filter(|(id, _)| !shown.iter().any(|(shown, _)| shown == id))
        .count();
    for (n, (id, values)) in shown.into_iter().enumerate() {
        let row = crate::controls::setting_row(
            format!("step-pick-{i}-{n}"),
            "Run's value",
            Some(values),
            taken(step, &typed_texts, id),
            // Only the model, and an option the agent offers none of, are
            // judged on what was learned; another's values depend on the
            // model.
            id == "model" || values.is_empty(),
            shipped.then_some(SHIPPED),
            option_pick(handle, i, id),
            cx,
        );
        body = body.child(field(id.to_string(), None, row, cx));
    }
    if left_out > 0 {
        body = body.child(
            div()
                .text_xs()
                .text_color(crate::theme::meta_ink(cx))
                .child(format!(
                    "{left_out} more options not shown; set them by id below"
                )),
        );
    }
    body.child(field(
        "Other options",
        about("By the agent's own ids and values"),
        typed,
        cx,
    ))
    .into_any_element()
}

/// What picking a value of option `id` on step `i` does: the step's pick
/// set, or cleared for `None`, and a typed row setting the id dropped, as a
/// pick is the option's one value.
fn option_pick(handle: &Entity<WorkflowsPage>, i: usize, id: &str) -> Pick {
    let (handle, id) = (handle.clone(), id.to_string());
    Rc::new(move |to, window, cx| {
        let id = id.clone();
        handle.update(cx, |page, cx| {
            let mut dropped = false;
            page.edit_workflow_draft(window, cx, |d, _, cx| {
                let step = &mut d.steps[i];
                set_pick(&mut step.picked, &id, to.as_deref());
                let before = step.options.len();
                step.options
                    .retain(|(row, _)| row.read(cx).value().trim() != id);
                dropped = step.options.len() != before;
            });
            // Its fields may hold the keyboard, and they leave the frame
            // with the row.
            if dropped {
                page.hold_focus(window, cx);
            }
        })
    })
}

/// The value option `id` is set to: its pick, else what a typed row says.
fn taken<'a>(step: &'a StepDraft, typed: &'a [(String, String)], id: &str) -> Option<&'a str> {
    step.picked.get(id).map(String::as_str).or_else(|| {
        typed
            .iter()
            .rev()
            .find(|(row, value)| row.trim() == id && !value.trim().is_empty())
            .map(|(_, value)| value.trim())
    })
}

/// The rows of options typed by id and value, and *Add option*.
fn typed_options(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    step: &StepDraft,
    shipped: bool,
    cx: &App,
) -> AnyElement {
    let rows = step.options.iter().enumerate().map(|(row, (id, value))| {
        let remove = {
            let handle = handle.clone();
            crate::controls::action(SharedString::from(format!("step-option-remove-{i}-{row}")))
                .tall_icon()
                .quiet_icon(false, cx)
                .icon(Icon::new(IconName::Close))
                .named("Remove this option")
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    handle.update(cx, |page, cx| {
                        page.edit_workflow_draft(window, cx, |d, _, _| {
                            let options = &mut d.steps[i].options;
                            if row < options.len() {
                                options.remove(row);
                            }
                        });
                        // Its fields may hold the keyboard, and they leave
                        // the frame with the row.
                        page.hold_focus(window, cx);
                    })
                })
        };
        div()
            .h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(text_input(id, cx).tall().disabled(shipped)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(text_input(value, cx).tall().disabled(shipped)),
            )
            .when(!shipped, |line| line.child(remove))
    });
    let add = {
        let handle = handle.clone();
        // Filled, as it starts its line.
        crate::controls::action(("step-option-add", i))
            .secondary()
            .tall()
            .icon(Icon::new(IconName::Plus))
            .label("Add option")
            .on_click(move |_, window: &mut Window, cx: &mut App| {
                handle.update(cx, |page, cx| {
                    page.edit_workflow_draft(window, cx, |d, window, cx| {
                        d.steps[i].add_option(window, cx)
                    })
                })
            })
    };
    div()
        .v_flex()
        .gap_2()
        .children(rows)
        .when(shipped && step.options.is_empty(), |col| {
            col.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("No options set"),
            )
        })
        .when(!shipped, |col| col.child(div().h_flex().child(add)))
        .into_any_element()
}
