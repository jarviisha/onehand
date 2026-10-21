//! An agent step's settings: the mode its visits run in and the agent's
//! options they set, as free text, since a workflow is tied to no agent.
//! What the agents were seen to offer stands under them as a reminder; the
//! editor judges no value against an agent, the run's preflight does.

use super::super::WorkflowsPage;
use super::super::draft::StepDraft;
use crate::controls::Quiet as _;
use crate::controls::text_input;
use crate::settings::{about, field, section};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::workflow::Offer;
use onehand_ui::Tall as _;

/// The most agents whose offers are listed under the settings.
const AGENTS_SHOWN: usize = 4;

/// The section on step `i`: *Mode*, the option rows and *Add option*, then
/// what each agent was seen to offer. A shipped workflow's fields are read
/// only, with nothing to add or remove.
pub(super) fn agent_settings(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    step: &StepDraft,
    shipped: bool,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
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
    let options = div()
        .v_flex()
        .gap_2()
        .children(rows)
        .when(shipped && step.options.is_empty(), |col| {
            col.child(div().text_xs().text_color(muted).child("No options set"))
        })
        .when(!shipped, |col| col.child(div().h_flex().child(add)));
    let seen: Vec<(String, Offer)> = crate::state::Shared::global(cx)
        .offers_seen
        .iter()
        .map(|(spec, offer)| (spec.name.clone(), offer.clone()))
        .collect();
    let offers = div()
        .v_flex()
        .gap_1()
        .text_xs()
        .text_color(muted)
        .children(offer_lines(&seen).into_iter().map(|line| div().child(line)));
    section(Some("Agent settings"), None, cx)
        .child(field(
            "Mode",
            None,
            text_input(&step.mode, cx).tall().disabled(shipped),
            cx,
        ))
        .child(field(
            "Options",
            about("By the agent's own ids and values"),
            options,
            cx,
        ))
        .child(offers)
        .into_any_element()
}

/// What the agents were seen to offer, a line each, newest last, an agent
/// seen under more than one spec once, by its newest; past
/// [`AGENTS_SHOWN`] a last line says how many more.
fn offer_lines(seen: &[(String, Offer)]) -> Vec<String> {
    let mut agents: Vec<&(String, Offer)> = Vec::new();
    for entry in seen {
        agents.retain(|(name, _)| *name != entry.0);
        agents.push(entry);
    }
    if agents.is_empty() {
        return vec![
            "Nothing is known yet of what the agents offer; it is learned when one comes up"
                .to_string(),
        ];
    }
    let more = agents.len().saturating_sub(AGENTS_SHOWN);
    let mut lines: Vec<String> = agents
        .iter()
        .take(AGENTS_SHOWN)
        .map(|(name, offer)| {
            let mut parts = Vec::new();
            if !offer.modes.is_empty() {
                parts.push(format!("modes {}", offer.modes.join(", ")));
            }
            for (id, values) in &offer.options {
                parts.push(format!("{id} {}", values.join(", ")));
            }
            match parts.is_empty() {
                true => format!("{name}: no modes or options"),
                false => format!("{name}: {}", parts.join(" · ")),
            }
        })
        .collect();
    if more > 0 {
        lines.push(format!("{more} more not shown"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(modes: &[&str], options: &[(&str, &[&str])]) -> Offer {
        Offer {
            modes: modes.iter().map(|m| m.to_string()).collect(),
            options: options
                .iter()
                .map(|(id, values)| {
                    (
                        id.to_string(),
                        values.iter().map(|v| v.to_string()).collect(),
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn offer_lines_say_each_agent() {
        let seen = [
            (
                "Claude".to_string(),
                offer(&["plan", "code"], &[("model", &["strong", "cheap"])]),
            ),
            ("Bare".to_string(), offer(&[], &[])),
            ("Opts".to_string(), offer(&[], &[("fast", &["on", "off"])])),
        ];
        assert_eq!(
            offer_lines(&seen),
            vec![
                "Claude: modes plan, code · model strong, cheap".to_string(),
                "Bare: no modes or options".to_string(),
                "Opts: fast on, off".to_string(),
            ]
        );
    }

    #[test]
    fn offer_lines_none_seen() {
        assert_eq!(offer_lines(&[]).len(), 1);
    }

    #[test]
    fn offer_lines_cap_and_once_per_agent() {
        let seen: Vec<_> = ["a", "b", "a", "c", "d", "e", "f"]
            .iter()
            .map(|name| (name.to_string(), offer(&[name], &[])))
            .collect();
        let lines = offer_lines(&seen);
        assert_eq!(lines.len(), AGENTS_SHOWN + 1);
        assert_eq!(lines[0], "b: modes b");
        assert_eq!(lines[1], "a: modes a");
        assert_eq!(lines[AGENTS_SHOWN], "2 more not shown");
    }
}
