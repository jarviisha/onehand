//! What a run waiting for approval is judged on: the answer under review,
//! what each answer to it starts, and the work the step under review did.

use crate::workflow::{ApprovalAt, CommandResult, Run, StepKind};

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
    /// The check that speaks for what the step under review changed: the
    /// last command since it last started.
    pub check: Checked,
}

/// The last command since the step under review last started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// None ran: an approval of a plan has nothing to check.
    NotRun,
    /// It ran, and came out as it says.
    Ran(CommandResult),
    /// It ran before a command's result was kept: what it vouches for is the
    /// commit alone, as the run's work says it.
    NotKept,
}

/// What a press the run no longer waits for says, where the reader is.
pub const ANSWER_CHANGED: &str = "The answer changed since you opened it.";

impl UnderReview {
    /// What `run` waits for approval on, if it does.
    pub fn of(run: &Run) -> Option<Self> {
        let (step, answer) = run.under_review()?;
        let at = run.approval_at()?;
        let visits = run.visits();
        let reviewed = visits.iter().rposition(|visit| visit.step == step.id);
        let span = reviewed.and_then(|i| visits[i].start.clone().zip(visits[i].end.clone()));
        // The last command since the step under review last started: one a
        // command step's visit did not keep the result of ran before results
        // were kept.
        let is_command = |id: &str| {
            run.template
                .steps
                .iter()
                .any(|s| s.id == id && matches!(s.kind, StepKind::Command { .. }))
        };
        let check = reviewed
            .and_then(|i| {
                visits[i..].iter().rev().find_map(|v| match &v.command {
                    Some(ran) => Some(Checked::Ran(ran.clone())),
                    None if v.ended_at.is_some() && is_command(&v.step) => Some(Checked::NotKept),
                    None => None,
                })
            })
            .unwrap_or(Checked::NotRun);
        Some(Self {
            at,
            of: step.label.clone(),
            answer: answer.to_string(),
            starts: run
                .template
                .steps
                .get(run.step + 1)
                .map(|next| (next.label.clone(), next.kind.does())),
            span,
            check,
        })
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
