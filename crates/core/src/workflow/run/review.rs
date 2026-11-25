//! A review step in a run: an agent prompted and awaited as an agent step's
//! is, with no gates, keeping what it answered.

use crate::workflow::template::{GateKind, StepKind};

/// What a step that prompts an agent is prompted with, the gates its turn
/// is checked against and whether its answer is kept; `None` for a step
/// that prompts none.
pub(super) fn prompted(kind: &StepKind) -> Option<(&str, &[GateKind], bool)> {
    match kind {
        StepKind::Agent {
            prompt,
            gates,
            keep_answer,
            ..
        } => Some((prompt, gates, *keep_answer)),
        StepKind::Review { prompt, .. } => Some((prompt, &[], true)),
        StepKind::Command { .. }
        | StepKind::Approval { .. }
        | StepKind::Push
        | StepKind::PullRequest
        | StepKind::StatusChecks { .. } => None,
    }
}
