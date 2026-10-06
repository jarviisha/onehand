//! What a start checks before it claims an issue, cuts a worktree or starts
//! an agent, and what it found.
//!
//! **It judges the configuration that will actually run.** A new start runs
//! what Settings says now; a Resume or a Retry runs the run's own setup, so a
//! setting changed since does not clear what blocks it. The app gathers the
//! facts (nothing here reads the disk or the network), and draws what comes
//! back where the start is asked for; the callers that claim and cut still do
//! that themselves, after this.

use crate::task::{Task, Working};
use crate::workflow::{Place, StepKind, Template};

/// What kind of start is checked: each has its own configuration and place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// From the launcher: the workflow picked, cut off `HEAD` when it works
    /// on a worktree.
    NewRun,
    /// An issue picked by hand or found by its label, on a new worktree.
    NewIssueRun,
    /// A run cut off, carried on where it stopped, on its own snapshot.
    Resume,
    /// A task's next run; `newer` when it runs a newer version of the
    /// workflow than its last run did, which is checked as a new one is.
    Retry { newer: bool },
}

/// What a finding is about, in the order findings are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    Workflow,
    Agent,
    Mode,
    CheckCommand,
    Place,
    Base,
    Forge,
    Issue,
    EarlierTask,
    Slot,
    PlaceTaken,
}

/// One thing the preflight found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub check: Check,
    /// It stops the start; otherwise it is said and the start goes on.
    pub blocks: bool,
    pub text: String,
    /// Where it is changed, when that is somewhere else.
    pub change: Option<&'static str>,
    /// The task it offers instead, for an earlier task worth retrying.
    pub task: Option<String>,
}

/// The forge serving the project, and what its account said when last seen:
/// `None` until it has been asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forge {
    pub name: String,
    pub account: Option<Result<String, String>>,
}

/// The issue a start works, as a sentence names it, and its tasks with what
/// each is doing.
#[derive(Debug, Clone)]
pub struct IssueFacts {
    pub named: String,
    pub tasks: Vec<(Task, Option<Working>)>,
}

/// How many issue runs may work at once, and who holds the slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slots {
    /// The issues being worked, as they are shown.
    pub working: Vec<String>,
    /// Issue tasks kept and not yet placed, which count too.
    pub starting: usize,
    pub at_once: u32,
}

/// The branch a detached `HEAD` reads as.
pub const DETACHED: &str = "(detached)";

/// What the app knows when a start is asked for.
#[derive(Debug, Clone)]
pub struct Facts {
    /// The workflow that will run, or why there is none.
    pub workflow: Result<Template, String>,
    /// The agent the setup names; `None` when nothing is configured at all.
    pub agent: Option<String>,
    /// That agent is among the configured ones.
    pub agent_configured: bool,
    /// The mode the run starts in, if one is set.
    pub mode: Option<String>,
    /// The modes the agent offers, when learned in this process from the
    /// spec it is configured with now; `None` when not known yet.
    pub offered: Option<Vec<String>>,
    /// The run has a check command to run.
    pub has_check: bool,
    /// The project is in git.
    pub in_git: bool,
    /// The branch checked out, [`DETACHED`] when there is none.
    pub checked_out: Option<String>,
    /// The forge the work goes to, `None` on a project no forge serves.
    pub forge: Option<Forge>,
    /// The issue it works, for an issue's start.
    pub issue: Option<IssueFacts>,
    /// The slots, for an issue's start.
    pub slots: Option<Slots>,
    /// The task whose place this start would queue behind.
    pub queued_behind: Option<String>,
}

/// Everything `facts` says about a start of `kind`: blocks first, the
/// workflow's own problems first among them, then what only informs.
pub fn preflight(kind: Kind, facts: &Facts) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut say = |check, blocks, text: String, change| {
        found.push(Finding {
            check,
            blocks,
            text,
            change,
            task: None,
        })
    };
    let own = matches!(kind, Kind::Resume | Kind::Retry { .. });
    let keeps = match own {
        true => " The run keeps its own setup, so changing Settings does not change it.",
        false => "",
    };
    let issue_kind = kind == Kind::NewIssueRun;

    // The workflow.
    let template = match &facts.workflow {
        Ok(template) => Some(template),
        Err(why) => {
            say(Check::Workflow, true, capital(why), None);
            None
        }
    };
    let judged = matches!(
        kind,
        Kind::NewRun | Kind::NewIssueRun | Kind::Retry { newer: true }
    );
    if let Some(template) = template.filter(|_| judged) {
        if let Some(why) = unfit_for_issue(template).filter(|_| issue_kind) {
            say(Check::Workflow, true, capital(&why), None);
        }
        for problem in crate::workflow::validate(template) {
            say(
                Check::Workflow,
                true,
                format!("The workflow cannot run: {problem}"),
                Some("Settings ▸ Workflows"),
            );
        }
    }

    // The agent and its mode.
    match (&facts.agent, facts.agent_configured) {
        (None, _) => say(
            Check::Agent,
            true,
            "No agent is configured to run it.".to_string(),
            Some("Settings ▸ Agents"),
        ),
        (Some(name), false) => say(
            Check::Agent,
            true,
            format!("The agent `{name}` is no longer configured.{keeps}"),
            Some("Settings ▸ Agents"),
        ),
        (Some(_), true) => {}
    }
    if let Some(mode) = facts.mode.as_deref().filter(|m| !m.trim().is_empty()) {
        let change = match own {
            true => None,
            false => Some("unattended.mode in onehand.toml"),
        };
        match &facts.offered {
            Some(offered) => {
                if let Some(why) = mode_refused(mode, offered) {
                    say(
                        Check::Mode,
                        true,
                        format!("{}.{keeps}", capital(&why)),
                        change,
                    );
                }
            }
            None => say(
                Check::Mode,
                false,
                format!(
                    "Mode `{mode}` is not known yet: the agent has not come up since onehand \
                     started or since it was edited, and it is checked when it does."
                ),
                None,
            ),
        }
    }

    // The check command.
    if let Some(template) = template {
        let commands = template
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::Command { .. }));
        if template.needs_check() && !facts.has_check {
            say(
                Check::CheckCommand,
                true,
                format!(
                    "The workflow `{}` runs the project's check command, and there is none.",
                    template.name
                ),
                Some("Settings ▸ Workflows"),
            );
        } else if !commands {
            say(
                Check::CheckCommand,
                false,
                "The workflow runs no command: nothing verifies the work.".to_string(),
                None,
            );
        }
    }

    // Where it works, and what the branch is cut off.
    let detached = facts.checked_out.as_deref() == Some(DETACHED);
    let worktree = template.is_some_and(|t| t.place == Place::Worktree);
    match kind {
        Kind::NewRun if worktree && !facts.in_git => say(
            Check::Place,
            true,
            "The project is not in git, so no worktree can be cut for it.".to_string(),
            None,
        ),
        Kind::NewRun if worktree => say(
            Check::Base,
            false,
            "The branch is cut off `HEAD`.".to_string(),
            None,
        ),
        Kind::NewIssueRun => match (&facts.forge, detached, &facts.checked_out) {
            (Some(_), _, _) => say(
                Check::Base,
                false,
                "The branch is cut off the default branch on `origin`, fetched first.".to_string(),
                None,
            ),
            (None, true, _) => say(
                Check::Place,
                true,
                "HEAD is detached and no forge serves the project, so there is no branch to \
                 cut the worktree from."
                    .to_string(),
                None,
            ),
            (None, false, Some(branch)) => say(
                Check::Base,
                false,
                format!("The branch is cut off `{branch}`, the branch checked out."),
                None,
            ),
            (None, false, None) => {}
        },
        Kind::NewRun | Kind::Resume | Kind::Retry { .. } => {}
    }

    // The forge: an issue's run asks it for the default branch whatever its
    // steps; any other run needs it only for forge steps.
    let forge_steps = template.is_some_and(|t| {
        t.steps.iter().any(|step| match step.kind {
            StepKind::Push | StepKind::PullRequest | StepKind::StatusChecks { .. } => true,
            StepKind::Agent { .. } | StepKind::Command { .. } | StepKind::Approval { .. } => false,
        })
    });
    let refused = match &facts.forge {
        Some(Forge {
            name,
            account: Some(Err(why)),
        }) if issue_kind || forge_steps => Some((name, why)),
        _ => None,
    };
    if let Some((name, why)) = refused {
        say(
            Check::Forge,
            true,
            format!("{name} cannot be used: {why}"),
            Some("Settings ▸ Connections"),
        );
    }

    // The issue and its earlier tasks.
    if let Some(issue) = facts.issue.as_ref().filter(|_| issue_kind) {
        if issue.tasks.iter().any(|(_, working)| working.is_some()) {
            say(Check::Issue, true, already_working(&issue.named), None);
        } else if let Some((task, said)) = earlier_ended(&issue.tasks) {
            found.push(Finding {
                check: Check::EarlierTask,
                blocks: false,
                text: format!(
                    "The issue's last task ended: {said}. Starting makes a second task \
                     beside it; retry that one to carry its work on."
                ),
                change: None,
                task: Some(task),
            });
        }
    }

    // The slots and the place.
    let mut say = |check, blocks, text: String| {
        found.push(Finding {
            check,
            blocks,
            text,
            change: None,
            task: None,
        })
    };
    if let Some(why) = facts
        .slots
        .as_ref()
        .and_then(|s| crate::unattended::full(&s.working, s.starting, s.at_once))
    {
        say(Check::Slot, true, why);
    }
    if let Some(ahead) = &facts.queued_behind {
        say(
            Check::PlaceTaken,
            false,
            format!("The place is taken: the run will queue behind {ahead}."),
        );
    }

    found.sort_by_key(|f| (!f.blocks, f.check));
    found
}

/// Why `template` cannot work an issue: it works in the checkout, and an
/// issue's run is cut a worktree of its own, where a checkout workflow leaves
/// its work uncommitted where nobody looks.
pub fn unfit_for_issue(template: &Template) -> Option<String> {
    (template.place != Place::Worktree).then(|| {
        format!(
            "the workflow `{}` works in the checkout, and an issue is worked on a worktree \
             of its own",
            template.name
        )
    })
}

/// Why the issue `named` is not started again: a run already works on it.
pub fn already_working(named: &str) -> String {
    format!("A run is already working on issue {named}.")
}

/// Why an agent offering `offered` cannot start in `mode`, if it cannot.
pub fn mode_refused(mode: &str, offered: &[String]) -> Option<String> {
    (!offered.iter().any(|m| m == mode)).then(|| {
        let offers = match offered.is_empty() {
            true => "none".to_string(),
            false => offered.join(", "),
        };
        format!("the agent offers no mode `{mode}` (it offers: {offers})")
    })
}

/// The issue's newest task, when it ended on something nobody chose and
/// nothing works on it now: its id, and how it ended.
fn earlier_ended(tasks: &[(Task, Option<Working>)]) -> Option<(String, String)> {
    let (task, working) = tasks.iter().max_by_key(|(task, _)| task.recency())?;
    if working.is_some() || !task.group(*working).needs_attention() {
        return None;
    }
    let said = crate::task::work::Work::of(task, None, None).said;
    Some((task.id.clone(), lower(&said)))
}

/// What a new task's report says of the issue's last one, when it ended on
/// something nobody chose: the run started anyway, beside it.
pub fn earlier_note(tasks: &[(Task, Option<Working>)]) -> Option<String> {
    let (_, said) = earlier_ended(tasks)?;
    Some(format!(
        "An earlier task on this issue ended: {said}. Its worktree is kept."
    ))
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn lower(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests;
