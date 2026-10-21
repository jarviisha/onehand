//! What an agent step runs with, and the planner that sends its mode and
//! config options one pick at a time: picks go in order (mode, then
//! `model`, then the rest by id), each checked against what the session
//! offers and has in force after the ones before it were answered.

use super::template::StepKind;
use crate::preflight::mode_refused;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A mode and config option values, by the adapter's own ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub mode: Option<String>,
    pub options: BTreeMap<String, String>,
}

/// What an agent offers: its modes, and each config option's choice values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offer {
    pub modes: Vec<String>,
    pub options: Vec<(String, Vec<String>)>,
}

/// One setting a visit sends to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Mode(String),
    Option { id: String, value: String },
}

/// What a visit does next for its settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Pick(Pick),
    Refused(String),
    Done,
}

impl Settings {
    /// What a visit to `step` runs with: the step's own over `start`.
    pub fn for_step(step: &StepKind, start: &Settings) -> Settings {
        match step {
            StepKind::Agent { mode, options, .. } => {
                let mut merged = start.options.clone();
                merged.extend(options.clone());
                Settings {
                    mode: mode.clone().or_else(|| start.mode.clone()),
                    options: merged,
                }
            }
            StepKind::Command { .. }
            | StepKind::Approval { .. }
            | StepKind::Push
            | StepKind::PullRequest
            | StepKind::StatusChecks { .. } => start.clone(),
        }
    }
}

impl Pick {
    /// The group the chat counts it under: `MODE_SELECTOR` or the option id.
    pub fn group(&self) -> &str {
        match self {
            Pick::Mode(_) => crate::chat::MODE_SELECTOR,
            Pick::Option { id, .. } => id,
        }
    }
}

/// Values joined by `, `, or `none` when there are none, as `mode_refused`
/// reads.
fn joined_or_none(values: &[String]) -> String {
    match values.is_empty() {
        true => "none".to_string(),
        false => values.join(", "),
    }
}

/// What a visit wanting `mode` does next, skipping it when `picked` already
/// holds it this visit.
fn mode_pick(mode: &str, offer: &Offer, in_force: &Settings, picked: &[String]) -> Option<Next> {
    if picked
        .iter()
        .any(|group| group == crate::chat::MODE_SELECTOR)
    {
        return None;
    }
    if let Some(why) = mode_refused(mode, &offer.modes) {
        return Some(Next::Refused(why));
    }
    if in_force.mode.as_deref() == Some(mode) {
        return None;
    }
    Some(Next::Pick(Pick::Mode(mode.to_string())))
}

/// Why the agent does not offer `value` for option `id`, if it does not: it
/// offers no such id at all (`true`), or it offers the id but not that
/// value among what it offers for it (`false`).
pub(crate) fn option_refused(id: &str, value: &str, offer: &Offer) -> Option<(bool, String)> {
    let Some((_, values)) = offer.options.iter().find(|(offered, _)| offered == id) else {
        let ids: Vec<String> = offer
            .options
            .iter()
            .map(|(offered, _)| offered.clone())
            .collect();
        return Some((
            true,
            format!(
                "the agent offers no option `{id}` (it offers: {})",
                joined_or_none(&ids)
            ),
        ));
    };
    (!values.iter().any(|offered| offered == value)).then(|| {
        (
            false,
            format!(
                "the agent offers no `{value}` for `{id}` (it offers: {})",
                joined_or_none(values)
            ),
        )
    })
}

/// What a visit wanting `value` for option `id` does next, skipping it when
/// `picked` already holds `id`.
fn option_pick(
    id: &str,
    value: &str,
    offer: &Offer,
    in_force: &Settings,
    picked: &[String],
) -> Option<Next> {
    if picked.iter().any(|group| group == id) {
        return None;
    }
    if let Some((_, why)) = option_refused(id, value, offer) {
        return Some(Next::Refused(why));
    }
    if in_force.options.get(id).map(String::as_str) == Some(value) {
        return None;
    }
    Some(Next::Pick(Pick::Option {
        id: id.to_string(),
        value: value.to_string(),
    }))
}

/// The next pick a visit wanting `want` sends, given what the session offers
/// and has in force now (`in_force`), skipping groups in `picked` (already
/// answered this visit).
pub fn next(want: &Settings, offer: &Offer, in_force: &Settings, picked: &[String]) -> Next {
    if let Some(mode) = &want.mode {
        if let Some(outcome) = mode_pick(mode, offer, in_force, picked) {
            return outcome;
        }
    }
    if let Some(value) = want.options.get("model") {
        if let Some(outcome) = option_pick("model", value, offer, in_force, picked) {
            return outcome;
        }
    }
    for (id, value) in want.options.iter().filter(|(id, _)| id.as_str() != "model") {
        if let Some(outcome) = option_pick(id, value, offer, in_force, picked) {
            return outcome;
        }
    }
    Next::Done
}

#[cfg(test)]
mod tests;
