//! An agent step's settings: the mode its visits run in and the agent's
//! options they set. A workflow is tied to no agent, so the choices drawn are
//! those of the agent runs take by default, as it was seen to offer them;
//! a value it does not offer stays on screen, marked, and any other agent's
//! options are typed by id. The editor judges no value against an agent, the
//! run's preflight does.

use super::super::WorkflowsPage;
use super::super::draft::{StepDraft, run_agent_offer, set_pick};
use crate::controls::text_input;
use crate::controls::{Quiet as _, Refuses as _};
use crate::settings::{about, field, section};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_ui::Tall as _;
use std::path::Path;
use std::rc::Rc;

/// The most of an agent's values a setting's row draws before it counts
/// the rest.
const CHOICES_SHOWN: usize = 12;

/// The most of an agent's options drawn as rows of choices before it counts
/// the rest, which are still set by id among the typed rows.
const OPTIONS_SHOWN: usize = 12;

/// What a refused pick says on a shipped workflow.
const SHIPPED: &str = "Built in: duplicate it to change it";

/// What picking a choice does: `None` for the run's own value.
type Pick = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

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
            let row = pick_row(
                format!("step-pick-{i}-{n}"),
                "Run's value",
                None,
                Some(value),
                shipped,
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
        let row = pick_row(
            format!("step-mode-{i}"),
            "Run's mode",
            Some(&offer.modes),
            mode.as_deref(),
            shipped,
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
        let row = pick_row(
            format!("step-pick-{i}-{n}"),
            "Run's value",
            Some(values),
            taken(step, &typed_texts, id),
            shipped,
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

/// A setting's row of choices: `run` (the step names none) first, then the
/// values offered, the one taken filled; a value taken that the agent does
/// not offer is marked in the warning ink. `offered` is `None` while what
/// the agent offers is not known, and nothing is marked.
/// Wraps on a narrow panel; past [`CHOICES_SHOWN`] a line says how many more.
fn pick_row(
    id: String,
    run: &'static str,
    offered: Option<&[String]>,
    current: Option<&str>,
    shipped: bool,
    pick: Pick,
    cx: &App,
) -> AnyElement {
    let known = offered.is_some();
    let (shown, more) = chips(offered.unwrap_or_default(), current);
    let warning = crate::theme::status_ink(cx).warning;
    let chip = |n: usize, label: SharedString, to: Option<String>, on: bool| {
        let pick = pick.clone();
        onehand_ui::choice(
            SharedString::from(format!("{id}-{n}")),
            label,
            on,
            onehand_ui::ControlSize::Short,
            cx,
        )
        .refuses(shipped.then_some(SHIPPED))
        .on_click(move |_, window: &mut Window, cx: &mut App| pick(to.clone(), window, cx))
    };
    let chips = shown.into_iter().enumerate().map(|(n, (value, offered))| {
        let on = current == Some(value.as_str());
        match offered || !known {
            true => chip(n + 1, value.clone().into(), Some(value), on),
            false => chip(
                n + 1,
                format!("{value} · not offered").into(),
                Some(value),
                on,
            )
            .text_color(warning)
            // A shipped workflow's refusal keeps its own tooltip.
            .when(!shipped, |chip| {
                chip.tooltip("The agent does not offer it: a run fails at this step")
            }),
        }
    });
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .h_flex()
                .flex_wrap()
                .gap_1()
                .child(chip(0, run.into(), None, current.is_none()))
                .children(chips),
        )
        .when(more > 0, |col| {
            col.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::meta_ink(cx))
                    .child(format!("{more} more")),
            )
        })
        .into_any_element()
}

/// The values a row draws after its first choice, each with whether the
/// agent offers it: the first [`CHOICES_SHOWN`] offered, the one taken
/// even past them, and a value taken that is not offered last; and how many
/// offered values are left out.
fn chips(offered: &[String], current: Option<&str>) -> (Vec<(String, bool)>, usize) {
    let mut shown: Vec<(String, bool)> = offered
        .iter()
        .take(CHOICES_SHOWN)
        .map(|value| (value.clone(), true))
        .collect();
    if let Some(current) = current.filter(|current| !shown.iter().any(|(v, _)| v == current)) {
        shown.push((current.to_string(), offered.iter().any(|v| v == current)));
    }
    let drawn = shown.iter().filter(|(_, known)| *known).count();
    (shown, offered.len() - drawn)
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
                .tall()
                .quiet_icon(false, cx)
                .icon(Icon::new(IconName::Close))
                .tooltip("Remove this option")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn values(n: usize) -> Vec<String> {
        (0..n).map(|v| format!("v{v}")).collect()
    }

    #[test]
    fn chips_show_what_is_offered() {
        let (shown, more) = chips(&values(2), Some("v1"));
        assert_eq!(
            shown,
            vec![("v0".to_string(), true), ("v1".to_string(), true)]
        );
        assert_eq!(more, 0);
    }

    #[test]
    fn a_value_not_offered_stays_marked() {
        let (shown, more) = chips(&values(1), Some("opus"));
        assert_eq!(
            shown,
            vec![("v0".to_string(), true), ("opus".to_string(), false)]
        );
        assert_eq!(more, 0);
        assert_eq!(
            chips(&[], Some("plan")).0,
            vec![("plan".to_string(), false)]
        );
    }

    #[test]
    fn chips_are_capped_and_the_one_taken_kept() {
        let offered = values(CHOICES_SHOWN + 3);
        let (shown, more) = chips(&offered, None);
        assert_eq!((shown.len(), more), (CHOICES_SHOWN, 3));
        let last = offered.last().unwrap();
        let (shown, more) = chips(&offered, Some(last));
        assert_eq!(shown.last(), Some(&(last.clone(), true)));
        assert_eq!((shown.len(), more), (CHOICES_SHOWN + 1, 2));
    }
}
