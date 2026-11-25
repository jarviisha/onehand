//! Each agent step's own mode and options, checked before a run starts.
//!
//! What was learned of what the agent offers is only ever certain for a
//! mode, `model`, and any other option id the agent never offered: another
//! option's choices may depend on the model in force, so a value missing
//! from what was learned under a different model is left for the run's own
//! check at that step, the same authority it already is at runtime.

use super::{checkable_finding, finding, mode_refused, About, Change, Finding, StepAgent};
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
        StepKind::Agent { mode, options, .. } | StepKind::Review { mode, options, .. }
            if mode.is_some() || !options.is_empty()
    )
}

/// Findings for what each agent step's own mode and options the agent does
/// not offer: a step naming an agent is judged against that agent's offer in
/// `step_agents`, any other against `offered`, the run's. One finding for the
/// whole workflow when what an agent offers is not known yet for a step that
/// names settings, unless `mode_unknown_said` already said as much for the
/// run's own starting mode. A step whose agent is not configured is left to
/// the finding about that. `keeps` and `change` are the mode check's own, for
/// the kind judged.
pub(super) fn step_settings(
    template: &Template,
    offered: Option<&Offer>,
    step_agents: &[StepAgent],
    keeps: &str,
    change: Option<Change>,
    mode_unknown_said: bool,
) -> Vec<Finding> {
    let mut found = Vec::new();
    let (mut unknown_run, mut unknown_step) = (false, false);
    for (i, step) in template.steps.iter().enumerate() {
        let (StepKind::Agent { mode, options, .. } | StepKind::Review { mode, options, .. }) =
            &step.kind
        else {
            continue;
        };
        let own = step_agents.iter().find(|named| named.step == i);
        let offer = match own {
            Some(named) if !named.configured => continue,
            Some(named) => named.offered.as_ref(),
            None => offered,
        };
        let Some(offer) = offer else {
            match own {
                Some(_) => unknown_step |= names_settings(&step.kind),
                None => unknown_run |= names_settings(&step.kind),
            }
            continue;
        };
        found.extend(step_findings(
            i + 1,
            &step.label,
            mode.as_deref(),
            options,
            offer,
            keeps,
            change,
        ));
    }
    let mut found = cap(found);
    if unknown_step || (unknown_run && !mode_unknown_said) {
        found.push(checkable_finding(
            About::Settings,
            NOT_KNOWN_YET.to_string(),
        ));
    }
    found
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
