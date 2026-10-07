//! What a run waiting for approval is judged on: the answer under review,
//! what each answer to it starts, and the work the step under review did.

use crate::workflow::{ApprovalAt, CommandResult, Run};

/// How many lines of an answer under review are drawn until a person asks
/// for all of it: its last ones, where an answer ends on what it proposes.
pub const ANSWER_LINES: usize = 60;

/// The last `lines` lines of `text`, and how many before them were left out.
pub fn last_lines(text: &str, lines: usize) -> (&str, usize) {
    let total = text.lines().count();
    let left_out = total.saturating_sub(lines);
    let from = text
        .split_inclusive('\n')
        .take(left_out)
        .map(str::len)
        .sum::<usize>();
    (&text[from..], left_out)
}

/// What a run waiting for approval is judged on, as a review draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnderReview {
    /// The visit a press drawn from this carries.
    pub at: ApprovalAt,
    /// The step under review, by label, and the answer it kept.
    pub of: String,
    pub answer: String,
    /// The step approving starts, by label, and what it does; `None` when
    /// approving ends the run.
    pub starts: Option<(String, String)>,
    /// The work as the step under review last found it and left it, as two
    /// marks: what it changed. `None` until both are pinned.
    pub span: Option<(String, String)>,
    /// How the last command since the step under review last started came
    /// out, passed or failed: the check that speaks for what it changed.
    pub ran: Option<CommandResult>,
}

impl UnderReview {
    /// What `run` waits for approval on, if it does.
    pub fn of(run: &Run) -> Option<Self> {
        under_review(run)
    }

    /// What *Continue* starts, said beside it.
    pub fn continue_said(&self) -> String {
        match &self.starts {
            Some((label, does)) => format!("Continue starts {label}: {does}"),
            None => "Continue ends the run".to_string(),
        }
    }

    /// What *Revise…* runs again, said beside it.
    pub fn revise_said(&self) -> String {
        format!("{} runs again with your note", self.of)
    }
}

/// What `run` waits for approval on, if it does.
fn under_review(run: &Run) -> Option<UnderReview> {
    let (step, answer) = run.under_review()?;
    let at = run.approval_at()?;
    let visits = run.visits();
    let reviewed = visits.iter().rposition(|visit| visit.step == step.id);
    let span = reviewed.and_then(|i| visits[i].start.clone().zip(visits[i].end.clone()));
    let ran = reviewed.and_then(|i| visits[i..].iter().rev().find_map(|v| v.command.clone()));
    Some(UnderReview {
        at,
        of: step.label.clone(),
        answer: answer.to_string(),
        starts: run
            .template
            .steps
            .get(run.step + 1)
            .map(|next| (next.label.clone(), next.kind.does())),
        span,
        ran,
    })
}
