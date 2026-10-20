//! The search, the claim and what is made for it: everything a tick or a
//! pick decides once it is off the UI loop, over the projects, the workflows
//! and the issue tasks the app read for it.

use super::{Issue, Tracker, TrackerRef};
use crate::connector::{self, Connector, PrState};
use crate::issues::template;
use crate::preflight::{self, Check, Facts, Forge, Kind};
use crate::task::{Task, Working};
use crate::workflow::{Now, Template};
use crate::worktree;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// A project switched on for runs: its root, and the file its own issues are
/// kept in, if its workspace keeps any.
#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub issues: Option<PathBuf>,
}

/// What a look at one project found: the forge its work goes to, or `None` for
/// a project on no forge, whose work stays on a branch — or why it cannot be
/// worked at all.
pub type Served = Result<Option<&'static dyn Connector>, String>;

/// Each connector beside what it said about its account: who it acts as, or
/// why it cannot act.
pub type Accounts = Vec<(&'static dyn Connector, Result<String, String>)>;

/// What every one of `connectors` says about its account, and for each of
/// `roots` what it can be worked with.
///
/// A project is worked **with its forge** when a connector serves it and that
/// connector's account can be used — and refused when the account cannot,
/// since the agent would be told to push somewhere it cannot. A project **no
/// connector serves** is still worked, on its own issues, with its work left on
/// a branch; what refuses it is having nothing to work — no forge and a
/// workspace that keeps no issues — or not being a repository at all. Whether a
/// connector serves is read locally, so a project nobody serves costs no call
/// to anything.
pub fn look_blocking(
    connectors: &[&'static dyn Connector],
    roots: Vec<Project>,
) -> (Accounts, Vec<(PathBuf, Served)>) {
    let accounts: Accounts = connectors
        .iter()
        .map(|&c| (c, c.account_blocking()))
        .collect();
    let checked = roots
        .into_iter()
        .map(|project| {
            let served = if worktree::repo_top_blocking(&project.root).is_none() {
                Err("it is not a git repository".to_string())
            } else {
                // `accounts` was built from `connectors` just above, so a
                // place in one is the same connector's place in the other.
                match connector::serving(connectors, &project.root) {
                    Ok(at) => match &accounts[at] {
                        (c, Ok(_)) => Ok(Some(*c)),
                        (_, Err(why)) => Err(why.clone()),
                    },
                    Err(_) if project.issues.is_some() => Ok(None),
                    Err(why) => Err(format!(
                        "{why}; and its workspace keeps no issues of its own to work instead"
                    )),
                }
            };
            (project.root, served)
        })
        .collect();
    (accounts, checked)
}

/// A project a search may look in: the forge its work goes to, if any, and
/// whether it has a check command.
pub struct Workable {
    pub project: Project,
    pub forge: Option<&'static dyn Connector>,
    pub has_check: bool,
}

/// A claimed issue and the worktree made for it.
pub struct Claimed {
    /// The project the issue was found in.
    pub repo: PathBuf,
    /// Where the issue lives: where it was claimed and where it is told how
    /// the run ended.
    pub tracker: Tracker,
    /// The forge the project's work goes to, or `None` for a project on no
    /// forge, whose work stays on the branch.
    pub forge: Option<&'static dyn Connector>,
    pub issue: Issue,
    /// The id of the workflow a new task on it runs: the one picked, else the
    /// one its labels choose. A review answered runs its task's own.
    pub workflow: String,
    /// What the claim works on.
    pub work: Work,
    /// What the person who picked it added for this run, after the brief's
    /// own instructions; empty for one the search found.
    pub instructions: String,
    /// What the start found for the first report to say, for a run nobody
    /// was there to start.
    pub notes: Vec<String>,
}

/// What a claimed issue is worked on.
#[derive(Clone)]
pub enum Work {
    /// A worktree cut for a new task.
    Cut {
        branch: String,
        /// What the worktree was cut from: the remote's default branch with a
        /// forge, the branch checked out without one. Also what a run with no
        /// forge is measured against.
        base: String,
        /// The worktree the run made, and the project root it is added as.
        dir: PathBuf,
    },
    /// The task whose open pull request the claim answers the review on,
    /// brought up to the branch on the forge, and what its next run is told.
    Review { task: String, note: String },
}

/// An issue that was claimed and then could not be started, and where to say
/// so.
pub struct Unstarted {
    pub repo: PathBuf,
    pub tracker: Tracker,
    pub number: u64,
    pub why: String,
}

impl Unstarted {
    /// `claimed` could not be started, for `why`.
    pub fn of(claimed: Claimed, why: &str) -> Self {
        Self {
            repo: claimed.repo,
            tracker: claimed.tracker,
            number: claimed.issue.number,
            why: why.to_string(),
        }
    }
}

/// An issue task that worked on a branch of its own, as a claim looks for
/// one still working on the issue, or whose pull request it answers.
pub struct Earlier {
    pub task: String,
    pub tracker: TrackerRef,
    pub number: u64,
    pub dir: PathBuf,
    pub branch: String,
    pub working: bool,
    /// Its last run's workflow can answer a review: it has a step its status
    /// checks send back to, and a forge to push the answer to.
    pub answers_reviews: bool,
}

/// What a search judges each new task by, gathered before it goes off the
/// UI loop: the configuration's own facts, every workflow an issue may be
/// given, and the issue tasks there are.
pub struct Judging {
    pub common: Facts,
    pub templates: Vec<(String, Result<Template, String>)>,
    pub tasks: Vec<(Task, Option<Working>)>,
}

impl Judging {
    /// A new run of workflow `id` on `issue`, as a window gathers it for a
    /// pick.
    pub(crate) fn facts(
        &self,
        id: &str,
        tracker: &Tracker,
        issue: &Issue,
        has_check: bool,
        checked_out: Option<String>,
        forge: Option<&'static dyn Connector>,
    ) -> Facts {
        let workflow = self.templates.iter().find(|(at, _)| at == id).map_or_else(
            || Err(format!("there is no workflow `{id}`")),
            |(_, t)| t.clone(),
        );
        let project = preflight::ProjectFacts {
            has_check,
            checked_out,
            // The look before the search asked the account; a project whose
            // account failed is not searched.
            forge: forge.map(|forge| Forge {
                name: forge.name().to_string(),
                account: None,
            }),
        };
        preflight::new_issue_run(
            self.common.clone(),
            workflow,
            tracker,
            issue,
            project,
            self.tasks.clone(),
        )
    }
}

/// How a search chooses each issue's workflow: the default, the workflow
/// labels, and which of the workflows those name run the project's check
/// command.
pub struct Choosing {
    pub default: String,
    pub by_label: BTreeMap<String, String>,
    pub need_check: HashSet<String>,
}

impl Choosing {
    /// Choose with `default` and `by_label`, knowing from `validated`, each
    /// workflow as a run would take it, which of them run the project's
    /// check command; one that cannot run is not counted as needing one.
    pub fn new(
        default: String,
        by_label: BTreeMap<String, String>,
        validated: &[(String, Result<Template, String>)],
    ) -> Self {
        let need_check = std::iter::once(&default)
            .chain(by_label.values())
            .filter(|id| {
                validated
                    .iter()
                    .any(|(at, t)| at == *id && t.as_ref().is_ok_and(Template::needs_check))
            })
            .cloned()
            .collect();
        Self {
            default,
            by_label,
            need_check,
        }
    }

    /// The id of the workflow an issue carrying `labels` is worked with, the
    /// trigger label being `trigger`.
    pub fn workflow_for(&self, labels: &[String], trigger: &str) -> &str {
        super::workflow_for(labels, &self.by_label, &self.default, trigger)
    }

    /// Whether that workflow runs the project's check command.
    pub fn needs_check(&self, labels: &[String], trigger: &str) -> bool {
        self.need_check.contains(self.workflow_for(labels, trigger))
    }
}

/// What claiming an issue comes to, given the tasks it had before.
pub enum Taking {
    /// A new task, on a branch of its own.
    Fresh,
    /// The last task's pull request is open: the label put back is a reviewer
    /// asking for changes, and that task answers them on its own branch.
    Review {
        task: String,
        /// Its worktree and branch.
        dir: PathBuf,
        branch: String,
        pr: String,
        note: String,
    },
    /// The issue is claimed only to be told why nothing follows: its last
    /// pull request was closed without being merged, so a second is never
    /// opened beside the one turned down, or its workflow cannot answer a
    /// review on the one open.
    Refused(String),
}

impl Taking {
    /// The pull request whose review the claim answers, if it does.
    pub fn answering(&self) -> Option<&str> {
        match self {
            Self::Review { pr, .. } => Some(pr),
            Self::Fresh | Self::Refused(_) => None,
        }
    }
}

/// Why an issue is not claimed now, its label left on.
pub enum Skip {
    /// A task is still working on it: it is never taken twice.
    Busy,
    /// The pull request on its last task's branch could not be looked up.
    Unread(String),
}

impl Skip {
    /// What a person who picked the issue `named` is told.
    pub fn said(&self, named: &str) -> String {
        match self {
            Self::Busy => preflight::already_working(named),
            Self::Unread(why) => format!("Could not start on issue {named}: {why}"),
        }
    }
}

/// What claiming issue `number` of `tracker` comes to, by the newest of
/// `earlier` on it and, with a forge, the pull request on its branch; one
/// merged, or none, is a fresh start. Blocking.
pub fn taking_blocking(
    earlier: &[Earlier],
    tracker: &Tracker,
    number: u64,
    repo: &Path,
    forge: Option<&'static dyn Connector>,
) -> Result<Taking, Skip> {
    let at = tracker.to_ref();
    let Some(last) = earlier
        .iter()
        .rfind(|e| e.tracker == at && e.number == number)
    else {
        return Ok(Taking::Fresh);
    };
    if last.working {
        return Err(Skip::Busy);
    }
    let Some(forge) = forge else {
        return Ok(Taking::Fresh);
    };
    let pr = match forge.pull_request_for_blocking(repo, &last.branch) {
        Ok(Some(pr)) => pr,
        Ok(None) => return Ok(Taking::Fresh),
        Err(why) => {
            return Err(Skip::Unread(format!(
                "the pull request on `{}` could not be looked up: {why}",
                last.branch
            )));
        }
    };
    Ok(match pr.state {
        PrState::Open if !last.answers_reviews => {
            Taking::Refused(super::review_unanswerable(&pr.url))
        }
        PrState::Open => Taking::Review {
            task: last.task.clone(),
            dir: last.dir.clone(),
            branch: last.branch.clone(),
            note: super::review_note(&pr.url, &forge.read_review_with(pr.number)),
            pr: pr.url,
        },
        PrState::Closed => Taking::Refused(super::review_closed(&pr.url)),
        PrState::Merged => Taking::Fresh,
    })
}

/// Where a project's issues are looked for, in the order they are searched.
///
/// A project kept in step with its forge is looked for **here only**, since
/// the forge's issues are already here and searching both would find each one
/// twice. Otherwise its own issues first, then the forge's. Blocking: it reads
/// the issue file to learn whether the project is synced.
pub fn trackers_blocking(
    issues: Option<PathBuf>,
    forge: Option<&'static dyn Connector>,
) -> Vec<Tracker> {
    if let (Some(file), Some(forge)) = (&issues, forge) {
        if crate::issues::load_blocking(file).is_ok_and(|kept| kept.in_step_with(forge.name())) {
            return vec![Tracker::Synced {
                file: file.clone(),
                forge,
            }];
        }
    }
    issues
        .map(Tracker::Local)
        .into_iter()
        .chain(forge.map(Tracker::Forge))
        .collect()
}

/// Find an issue, claim it and make its worktree. Blocking.
///
/// `None` when there is nothing to do — or when the claim itself failed, in
/// which case there is nobody to tell but stderr, since an issue the app could
/// not edit is one it cannot comment on either. After the claim every failure is
/// the issue's to hear about.
///
/// A project whose search failed has the reason added to `checked`, so its
/// row says why rather than showing it as workable.
/// Within a project the issues it keeps itself are searched before its forge's:
/// they are the ones written for onehand to work. Each issue's workflow is
/// chosen as `choosing` says, and on a project with no check command an issue
/// whose workflow runs one is passed over, its label left on, while the next
/// may still be taken.
///
/// **A new task is preflighted before its claim**, as a pick is: what blocks
/// it is said on the project's row, and the issue keeps its label for when
/// it is fixed. A review answered is judged by its own refusals instead.
pub fn begin_blocking(
    roots: &[Workable],
    label: &str,
    choosing: &Choosing,
    earlier: &[Earlier],
    judging: &Judging,
    checked: &mut Vec<(PathBuf, Served)>,
) -> Option<Result<Claimed, Unstarted>> {
    // The oldest labelled issue no task is still working on: one being
    // worked is passed over, never claimed twice, and never in the way of
    // the next.
    let mut notes = Vec::new();
    let (repo, tracker, forge, row, taking) = roots.iter().find_map(|w| {
        let (project, forge) = (&w.project, &w.forge);
        let mut checked_out = None;
        for tracker in trackers_blocking(project.issues.clone(), *forge) {
            match super::candidates_blocking(&tracker, &project.root, label) {
                Ok(found) => {
                    for row in found {
                        if !w.has_check && choosing.needs_check(&row.labels, label) {
                            continue;
                        }
                        match taking_blocking(
                            earlier,
                            &tracker,
                            row.issue.number,
                            &project.root,
                            *forge,
                        ) {
                            Err(Skip::Busy) => continue,
                            // Looked at again at the next tick, its label
                            // still on: a blip must not spend the request.
                            Err(Skip::Unread(why)) => {
                                eprintln!(
                                    "onehand: passed over issue #{} for now: {why}",
                                    row.issue.number
                                );
                                continue;
                            }
                            Ok(Taking::Fresh) => {
                                let at = checked_out.get_or_insert_with(|| {
                                    preflight::checked_out_blocking(&project.root)
                                });
                                let facts = judging.facts(
                                    choosing.workflow_for(&row.labels, label),
                                    &tracker,
                                    &row.issue,
                                    w.has_check,
                                    at.clone(),
                                    *forge,
                                );
                                let found = preflight::preflight(Kind::NewIssueRun, &facts);
                                match found.iter().find(|f| f.blocks) {
                                    // What blocks this issue alone (another
                                    // run on it, its workflow, a check
                                    // command its workflow needs) passes it
                                    // over, its label left on, as above; the
                                    // next may still be taken.
                                    Some(f)
                                        if matches!(
                                            f.check,
                                            Check::Issue | Check::Workflow | Check::CheckCommand
                                        ) =>
                                    {
                                        continue;
                                    }
                                    // Anything else would block every issue
                                    // here: said on the project's row.
                                    Some(f) => {
                                        checked.push((project.root.clone(), Err(f.text.clone())));
                                        return None;
                                    }
                                    None => {}
                                }
                                // Nobody is there to read these before the
                                // run: its first report says them instead.
                                let lacks = template::lacking(
                                    row.issue.body_text(),
                                    &template::for_project_blocking(&project.root),
                                );
                                notes = facts
                                    .issue
                                    .as_ref()
                                    .and_then(|issue| preflight::earlier_note(&issue.tasks))
                                    .into_iter()
                                    .chain(lacks.map(|lacks| lacks.note()))
                                    .collect();
                                return Some((
                                    project.root.clone(),
                                    tracker,
                                    *forge,
                                    row,
                                    Taking::Fresh,
                                ));
                            }
                            Ok(taking) => {
                                return Some((project.root.clone(), tracker, *forge, row, taking));
                            }
                        }
                    }
                }
                Err(why) => {
                    checked.push((project.root.clone(), Err(why)));
                    return None;
                }
            }
        }
        None
    })?;
    let answering = taking.answering();
    let number = row.issue.number;
    if let Err(why) = super::claim_blocking(&tracker, &repo, number, label, answering) {
        eprintln!("onehand: could not claim issue #{number}: {why}");
        return None;
    }
    let workflow = choosing.workflow_for(&row.labels, label).to_string();
    Some(
        prepare_blocking(repo, tracker, forge, row.issue, workflow, taking)
            .map(|claimed| Claimed { notes, ..claimed }),
    )
}

/// Make what a claimed issue is worked on, as `taking` says. For a new task,
/// a worktree off the remote's default branch, fetched first, on a project
/// with a forge; off the branch checked out on one without, since there is no
/// remote to ask. For a review, the last task's worktree brought up to its
/// branch on the forge, which a reviewer may have pushed to: a branch that
/// went its own way there is refused rather than pushed over.
///
/// Caught as well as everything around it: by now the issue has been claimed,
/// so a panic has to become a comment on the issue, or it is left claimed with
/// nothing saying what happened.
pub fn prepare_blocking(
    repo: PathBuf,
    tracker: Tracker,
    forge: Option<&'static dyn Connector>,
    issue: Issue,
    workflow: String,
    taking: Taking,
) -> Result<Claimed, Unstarted> {
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match taking {
            Taking::Refused(why) => return Err(why),
            Taking::Review {
                task,
                dir,
                branch,
                note,
                ..
            } => {
                if let Some(forge) = forge {
                    forge.fetch_blocking(&repo, &branch)?;
                }
                let theirs = format!("origin/{branch}");
                if worktree::went_its_own_way_blocking(&dir, &theirs)? {
                    return Err(super::review_diverged());
                }
                worktree::fast_forward_blocking(&dir, &theirs)?;
                return Ok(Work::Review { task, note });
            }
            Taking::Fresh => {}
        }
        let base = match forge {
            Some(forge) => {
                let default = forge.default_branch_blocking(&repo)?;
                forge.fetch_blocking(&repo, &default)?;
                format!("origin/{default}")
            }
            None => worktree::current_branch_blocking(&repo)?,
        };
        let top = worktree::repo_top_blocking(&repo).unwrap_or_else(|| repo.clone());
        // A branch the forge still has a pull request on belongs to work
        // before this, whatever is left of it here.
        let taken = |name: &str| match forge {
            Some(forge) => forge
                .pull_request_for_blocking(&repo, name)
                .map(|pr| pr.is_some())
                .map_err(|why| format!("could not ask the forge about branch {name}: {why}")),
            None => Ok(false),
        };
        let branch =
            super::free_branch_blocking(&top, &super::branch_for(&tracker, &issue), taken)?;
        let dir = worktree::worktree_dir(&top, &branch);
        let dir = worktree::branch_off_blocking(&top, &branch, &dir, &base)?;
        Ok::<_, String>(Work::Cut { branch, base, dir })
    }))
    .unwrap_or_else(|_| Err("onehand panicked while preparing the worktree".to_string()));
    match made {
        Ok(work) => Ok(Claimed {
            repo,
            tracker,
            forge,
            issue,
            workflow,
            work,
            instructions: String::new(),
            notes: Vec::new(),
        }),
        Err(why) => Err(Unstarted {
            repo,
            tracker,
            number: issue.number,
            why,
        }),
    }
}

/// What a person can pick from in a project: its open issues from where each
/// lives, the ones it keeps itself first, and whether the list was cut — plus
/// why the forge's could not be read, when its own could.
pub type Pickable = (Vec<(Tracker, super::IssueRow)>, bool, Option<String>);

/// Read what a person can pick from in `root`, whose own issues are in
/// `issues` if its workspace keeps any, served by whichever of `connectors`
/// takes it. Blocking.
///
/// A forge that cannot be read is an error only when there is nothing else to
/// show; beside issues of the project's own it is said under the list, so one
/// half being down does not hide the other.
pub fn pickable_blocking(
    connectors: &[&'static dyn Connector],
    root: &Path,
    issues: Option<PathBuf>,
) -> Result<Pickable, String> {
    let (mut rows, mut cut, mut unread) = (Vec::new(), false, None);
    let forge = connector::serving(connectors, root).map(|at| connectors[at]);
    for tracker in trackers_blocking(issues, forge.as_ref().ok().copied()) {
        match super::open_issues_blocking(&tracker, root) {
            Ok((found, more)) => {
                cut |= more;
                rows.extend(found.into_iter().map(|row| (tracker.clone(), row)));
            }
            // The forge's half being down is said beside the rest; the
            // project's own issues failing to read is the whole answer.
            Err(why) => match tracker {
                Tracker::Forge(_) => unread = Some(why),
                Tracker::Local(_) | Tracker::Synced { .. } => return Err(why),
            },
        }
    }
    // A project no connector serves says why only when there is nothing else
    // to list: beside its own issues, the forge it does not have is no news.
    match (rows.is_empty(), unread, forge) {
        (true, Some(why), _) | (true, None, Err(why)) => Err(why),
        (_, unread, _) => Ok((rows, cut, unread)),
    }
}

/// Issue `number` of the ones `root` keeps itself, in `issues`, to pick, or
/// why it cannot be. Blocking.
///
/// Only the project's own issues are read, never its forge's: the Issues tab
/// numbers the issues it keeps, and a forge's numbers are its own. So the list
/// being cut is that list's, and an issue missing from it is said to be older
/// than the newest it holds, or else one a run may not take.
pub fn pickable_one_blocking(
    connectors: &[&'static dyn Connector],
    root: &Path,
    issues: Option<PathBuf>,
    number: u64,
) -> Result<Pickable, String> {
    let forge = connector::serving(connectors, root)
        .ok()
        .map(|at| connectors[at]);
    let tracker = trackers_blocking(issues, forge)
        .into_iter()
        .find(|tracker| !matches!(tracker, Tracker::Forge(_)))
        .ok_or("this workspace keeps no issues of its own")?;
    let (found, cut) = super::open_issues_blocking(&tracker, root)?;
    let rows: Vec<_> = found
        .into_iter()
        .filter(|row| row.issue.number == number)
        .map(|row| (tracker.clone(), row))
        .collect();
    match (rows.is_empty(), cut) {
        (false, _) => Ok((rows, false, None)),
        (true, true) => Err(format!(
            "issue {number} is not among the newest {} open issues a pick reads; close \
             some of the newer ones to reach it",
            super::ISSUES_SHOWN
        )),
        (true, false) => Err(format!(
            "issue {number} cannot be worked by a run: it is closed, or it was brought in \
             from a forge the project is no longer kept in step with"
        )),
    }
}

/// `template` as an issue's run may take it, or why it cannot run.
///
/// **Only a workflow that works on a worktree.** An issue's run is cut a
/// worktree of its own, and a workflow meant for a checkout run there leaves
/// its work uncommitted where nobody looks and reports that nothing landed.
pub fn fit_for_issue(template: Template) -> Result<Template, String> {
    if let Some(why) = preflight::unfit_for_issue(&template) {
        return Err(why);
    }
    let problems = crate::workflow::validate(&template);
    if !problems.is_empty() {
        let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
        return Err(format!(
            "the workflow `{}` cannot run: {}",
            template.name,
            said.join("; ")
        ));
    }
    Ok(template)
}

/// Why `template` cannot run on a project that `has_check` or not, and where
/// to set one: an issue's run works the project the issue is in, whose page
/// is where the check command lives.
pub fn check_missing(template: &Template, has_check: bool) -> Option<String> {
    template
        .lacks_check(has_check)
        .map(|why| format!("{why}; set one on the project's page"))
}

/// Why nothing at all may start, picked or found: the agent does not offer
/// the mode runs start in (`mode_refused`), or a workflow runs may be given —
/// `default`, or one a workflow label names in `by_label` — cannot run, or no
/// agent is configured.
///
/// **A label's workflow stops every run**, not only its issues': it is the
/// same broken config either way, and said before any claim it costs no issue
/// a comment.
pub fn cannot_start(
    mode_refused: Option<&str>,
    default: &Result<Template, String>,
    by_label: &[(String, Result<Template, String>)],
    agent_configured: bool,
) -> Option<String> {
    mode_refused
        .map(str::to_string)
        .or_else(|| {
            default
                .as_ref()
                .err()
                .map(|why| format!("{why} (unattended.workflow)"))
        })
        .or_else(|| {
            by_label.iter().find_map(|(label, template)| {
                let why = template.as_ref().err()?;
                Some(format!("{why} (unattended.workflows, label `{label}`)"))
            })
        })
        .or_else(|| (!agent_configured).then(|| "no agent is configured to run it".to_string()))
}

/// What runs start with, as the config has it.
pub struct RunSettings {
    pub agent: Option<String>,
    pub mode: String,
    /// Put over the workflow's own timeout.
    pub timeout: String,
}

/// What a new task would be given now, for a retry with current settings: an
/// `issue`'s task takes `runs` (its agent, its mode and the timeout put over
/// the workflow's) when runs are set up; any other task the first agent
/// configured and the workflow's own timeout. Both take the project's
/// `check`. A blank mode leaves the agent in the mode it starts in.
pub fn now_for(
    issue: bool,
    runs: Option<&RunSettings>,
    first_agent: Option<String>,
    check: Option<String>,
) -> Now {
    match runs.filter(|_| issue) {
        Some(runs) => Now {
            agent: runs.agent.clone().or(first_agent),
            mode: Some(runs.mode.clone()).filter(|mode| !mode.trim().is_empty()),
            check,
            timeout: Some(runs.timeout.clone()),
        },
        None => Now {
            agent: first_agent,
            mode: None,
            check,
            timeout: None,
        },
    }
}

#[cfg(test)]
mod tests;
