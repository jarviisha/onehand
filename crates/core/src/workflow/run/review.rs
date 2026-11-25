//! A review step in a run: an agent prompted and awaited as an agent step's
//! is, with no gates, keeping what it answered, and judged by the verdict it
//! gave during its turn rather than by the work.
//!
//! **A review is not an approval.** Nobody is waited on: a send back is a
//! miss, as a failed gate is, so reviews that never agree exhaust the run
//! instead of looping, and a turn that gave no verdict is a miss too.

use super::{Action, Await, Run};
use crate::workflow::template::{GateKind, StepKind};

/// What a review's agent said of the work it reviewed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    /// Back to the step the review sends back to, with what to change.
    SendBack(String),
}

/// What a review whose turn gave no verdict is prompted with.
const NO_VERDICT: &str =
    "End your review by calling review_verdict: approve, or send back with a note.";

impl Run {
    /// The open visit of the review whose turn the run waits on, which a
    /// verdict is given at; `None` anywhere else.
    pub(crate) fn review_visit(&self) -> Option<u32> {
        if self.awaiting != Await::Turn || !self.current()?.is_review() {
            return None;
        }
        self.visits
            .last()
            .filter(|visit| visit.ended_at.is_none())
            .map(|visit| visit.id)
    }

    /// The label of the step the review at hand sends back to.
    pub(crate) fn rejects_to_label(&self) -> Option<&str> {
        let id = self.current()?.kind.rejects_to()?;
        let at = self.template.index_of(id)?;
        Some(&self.template.steps[at].label)
    }

    /// The review's turn ended answering `answer`, with the last verdict
    /// given during it, if any.
    pub(crate) fn reviewed(&mut self, verdict: Option<Verdict>, answer: &str, now: u64) -> Action {
        if self.review_visit().is_none() {
            return Action::Idle;
        }
        let Some(step) = self.current().cloned() else {
            return Action::Idle;
        };
        let answer = answer.trim().to_string();
        self.visit_output(answer.clone());
        self.outputs.insert(step.id.clone(), answer);
        let label = step.label;
        match verdict {
            Some(Verdict::Approve) => {
                self.enter(self.step + 1, &format!("approved by {label}"), now)
            }
            Some(Verdict::SendBack(note)) => {
                let why = format!("sent back by {label}");
                if let Some(action) = self.miss(&why, now) {
                    return action;
                }
                let back = step
                    .kind
                    .rejects_to()
                    .and_then(|id| self.template.index_of(id));
                let Some(back) = back else {
                    return self.fail(why, super::Failure::Other, now);
                };
                self.revise = Some(note);
                self.revise_by = Some(label);
                self.enter(back, &why, now)
            }
            None => {
                if let Some(action) = self.miss("missed: no verdict", now) {
                    return action;
                }
                self.awaiting = Await::Turn;
                Action::Prompt(NO_VERDICT.to_string())
            }
        }
    }
}

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
