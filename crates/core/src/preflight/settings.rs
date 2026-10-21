//! Each agent step's own mode and options, checked before a run starts.
//!
//! What was learned of what the agent offers is only ever certain for a
//! mode, `model`, and any other option id the agent never offered: another
//! option's choices may depend on the model in force, so a value missing
//! from what was learned under a different model is left for the run's own
//! check at that step, the same authority it already is at runtime.

use super::{checkable_finding, finding, mode_refused, About, Change, Finding};
use crate::workflow::{option_refused, Offer, StepKind, Template};
use std::collections::BTreeMap;

/// At most this many step-setting findings are listed before the rest are
/// only counted.
const SETTINGS_SHOWN: usize = 8;

/// What the agent offers is not known for any of this workflow's steps,
/// said once for the whole workflow rather than once per step and setting.
const NOT_KNOWN_YET: &str = "The agent's settings for this workflow's steps are not known yet: \
the agent has not come up since onehand started or since it was edited, and they are checked \
when each step starts.";

/// Whether `kind` names a mode or an option of its own.
fn names_settings(kind: &StepKind) -> bool {
    matches!(
        kind,
        StepKind::Agent { mode, options, .. } if mode.is_some() || !options.is_empty()
    )
}

/// Findings for what each agent step's own mode and options the agent does
/// not offer, against `offered`; one finding for the whole workflow when
/// nothing of what it offers is known yet, unless `mode_unknown_said` already
/// said as much for the run's own starting mode. `keeps` and `change` are
/// the mode check's own, for the kind judged.
pub(super) fn step_settings(
    template: &Template,
    offered: Option<&Offer>,
    keeps: &str,
    change: Option<Change>,
    mode_unknown_said: bool,
) -> Vec<Finding> {
    let Some(offer) = offered else {
        if mode_unknown_said {
            return Vec::new();
        }
        let names_any = template.steps.iter().any(|step| names_settings(&step.kind));
        return match names_any {
            true => vec![checkable_finding(
                About::Settings,
                NOT_KNOWN_YET.to_string(),
            )],
            false => Vec::new(),
        };
    };
    let found = template
        .steps
        .iter()
        .enumerate()
        .filter_map(|(i, step)| match &step.kind {
            StepKind::Agent { mode, options, .. } => {
                Some((i + 1, step.label.as_str(), mode.as_deref(), options))
            }
            StepKind::Command { .. }
            | StepKind::Approval { .. }
            | StepKind::Push
            | StepKind::PullRequest
            | StepKind::StatusChecks { .. } => None,
        })
        .flat_map(|(n, label, mode, options)| {
            step_findings(n, label, mode, options, offer, keeps, change)
        })
        .collect();
    cap(found)
}

/// One step's own mode and options the agent does not offer, each finding
/// named `Step {n} ({label})`.
fn step_findings(
    n: usize,
    label: &str,
    mode: Option<&str>,
    options: &BTreeMap<String, String>,
    offer: &Offer,
    keeps: &str,
    change: Option<Change>,
) -> Vec<Finding> {
    let prefix = format!("Step {n} ({label})");
    let mut found = Vec::new();
    if let Some(mode) = mode {
        if let Some(why) = mode_refused(mode, &offer.modes) {
            found.push(finding(
                About::Settings,
                true,
                format!("{prefix}: {why}.{keeps}"),
                change,
            ));
        }
    }
    if let Some(model) = options.get("model") {
        // `model` never gets the other options' exception: no other
        // option's choices can tell us what it itself offers.
        if let Some((_, why)) = option_refused("model", model, offer) {
            found.push(finding(
                About::Settings,
                true,
                format!("{prefix}: {why}.{keeps}"),
                change,
            ));
        }
    }
    for (id, value) in options.iter().filter(|(id, _)| id.as_str() != "model") {
        let Some((missing_id, why)) = option_refused(id, value, offer) else {
            continue;
        };
        // What was learned was offered under whatever model was in force
        // then, the step's or one a person picked in the composer: a value
        // missing from it may well be offered under the model this step
        // runs on. Only the run's own check at this step can say, so it is
        // never a place in Settings either.
        let under = match options.get("model") {
            Some(model) => format!("under model `{model}`"),
            None => "under the model the step runs on".to_string(),
        };
        found.push(match missing_id {
            true => finding(
                About::Settings,
                true,
                format!("{prefix}: {why}.{keeps}"),
                change,
            ),
            false => finding(
                About::Settings,
                false,
                format!(
                    "{prefix}: `{value}` for `{id}` is not known yet {under}; it is checked \
                     when the step starts.{keeps}"
                ),
                None,
            ),
        });
    }
    found
}

/// At most [`SETTINGS_SHOWN`] of `found`, then one more counting the rest,
/// blocking when any of the rest does.
fn cap(mut found: Vec<Finding>) -> Vec<Finding> {
    if found.len() <= SETTINGS_SHOWN {
        return found;
    }
    let hidden = found.split_off(SETTINGS_SHOWN);
    let blocks = hidden.iter().any(|f| f.blocks);
    found.push(finding(
        About::Settings,
        blocks,
        format!(
            "… and {} more settings the agent does not offer.",
            hidden.len()
        ),
        None,
    ));
    found
}
