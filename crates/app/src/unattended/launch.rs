use super::report::tell_issue;
use super::{Project, Served, label, with};
use crate::state::Shared;
use gpui::App;
use onehand_core::connector::{Connector, PrState};
use onehand_core::issues::template;
use onehand_core::preflight::{self, Check, Facts, Forge, Kind};
use onehand_core::task::Working;
use onehand_core::task::{Source, Task};
use onehand_core::unattended::{self as core, Issue, IssueSource, Tracker, TrackerRef};
use onehand_core::workflow::{self as flow, Setup, Template};
use onehand_core::worktree;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::path::PathBuf;

mod answer;
mod pick;

pub use answer::{ReviewRead, read_review, start_answer};
use pick::warn;
pub use pick::{Pickable, look_now, pickable_blocking, pickable_one_blocking, start_picked};

/// A claimed issue and the worktree made for it.
pub(super) struct Claimed {
    /// The project the issue was found in.
    pub(super) repo: PathBuf,
    /// Where the issue lives: where it was claimed and where it is told how
    /// the run ended.
    pub(super) tracker: Tracker,
    /// The forge the project's work goes to, or `None` for a project on no
    /// forge, whose work stays on the branch.
    pub(super) forge: Option<&'static dyn Connector>,
    pub(super) issue: Issue,
    /// The id of the workflow a new task on it runs: the one picked, else the
    /// one its labels choose. A review answered runs its task's own.
    workflow: String,
    /// What the claim works on.
    work: Work,
    /// The window a person picked it in, rather than the search finding it.
    /// Such a run is placed from there and its project stays when it ends —
    /// somebody asked for it — so a card it parks is theirs.
    picked_in: Option<gpui::AnyWindowHandle>,
    /// What the person who picked it added for this run, after the brief's
    /// own instructions; empty for one the search found.
    instructions: String,
    /// What the start found for the first report to say, for a run nobody
    /// was there to start.
    notes: Vec<String>,
}

impl Claimed {
    /// Whether a person picked this run rather than the search finding it.
    pub(super) fn picked_by_hand(&self) -> bool {
        self.picked_in.is_some()
    }
}

/// What a claimed issue is worked on.
#[derive(Clone)]
enum Work {
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

/// An issue task that worked on a branch of its own, as a claim looks for
/// one still working on the issue, or whose pull request it answers.
pub(super) struct Earlier {
    task: String,
    tracker: TrackerRef,
    number: u64,
    dir: PathBuf,
    branch: String,
    working: bool,
    /// Its last run's workflow can answer a review: it has a step its status
    /// checks send back to, and a forge to push the answer to.
    answers_reviews: bool,
}

/// Every issue task with a branch of its own, oldest first.
pub(super) fn earlier(cx: &App) -> Vec<Earlier> {
    crate::task::each(cx, |task, working| {
        let issue = task.issue()?;
        Some(Earlier {
            task: task.id.clone(),
            tracker: issue.tracker.clone(),
            number: issue.number,
            dir: task.setup.dir.clone(),
            branch: task.setup.branch.clone()?,
            working,
            answers_reviews: task.answers_reviews(),
        })
    })
}

/// An issue that was claimed and then could not be started, and where to say
/// so.
pub(super) struct Unstarted {
    repo: PathBuf,
    tracker: Tracker,
    number: u64,
    why: String,
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
pub(super) fn begin_blocking(
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
            match core::candidates_blocking(&tracker, &project.root, label) {
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
                                    super::facts::checked_out_blocking(&project.root)
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
    if let Err(why) = core::claim_blocking(&tracker, &repo, number, label, answering) {
        eprintln!("onehand: could not claim issue #{number}: {why}");
        return None;
    }
    let workflow = choosing.workflow_for(&row.labels, label).to_string();
    Some(
        prepare_blocking(repo, tracker, forge, row.issue, workflow, taking, None)
            .map(|claimed| Claimed { notes, ..claimed }),
    )
}

/// What a search judges each new task by, gathered before it goes off the
/// UI loop: the configuration's own facts, every workflow an issue may be
/// given, and the issue tasks there are.
pub(super) struct Judging {
    pub(super) common: Facts,
    pub(super) templates: Vec<(String, Result<Template, String>)>,
    pub(super) tasks: Vec<(Task, Option<Working>)>,
}

impl Judging {
    /// A new run of workflow `id` on `issue`, as [`super::facts::issue_run`]
    /// gathers it in a window.
    fn facts(
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
        let project = super::facts::ProjectFacts {
            has_check,
            checked_out,
            // The look before the search asked the account; a project whose
            // account failed is not searched.
            forge: forge.map(|forge| Forge {
                name: forge.name().to_string(),
                account: None,
            }),
        };
        super::facts::new_issue_run(
            self.common.clone(),
            workflow,
            tracker,
            issue,
            project,
            self.tasks.clone(),
        )
    }
}

/// A project a search may look in: the forge its work goes to, if any, and
/// whether it has a check command.
pub(super) struct Workable {
    pub(super) project: Project,
    pub(super) forge: Option<&'static dyn Connector>,
    pub(super) has_check: bool,
}

/// How a search chooses each issue's workflow: the default, the workflow
/// labels, and which of the workflows those name run the project's check
/// command.
pub(super) struct Choosing {
    pub(super) default: String,
    pub(super) by_label: BTreeMap<String, String>,
    pub(super) need_check: HashSet<String>,
}

impl Choosing {
    /// The id of the workflow an issue carrying `labels` is worked with, the
    /// trigger label being `trigger`.
    fn workflow_for(&self, labels: &[String], trigger: &str) -> &str {
        core::workflow_for(labels, &self.by_label, &self.default, trigger)
    }

    /// Whether that workflow runs the project's check command.
    fn needs_check(&self, labels: &[String], trigger: &str) -> bool {
        self.need_check.contains(self.workflow_for(labels, trigger))
    }
}

/// What claiming an issue comes to, given the tasks it had before.
enum Taking {
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
    fn answering(&self) -> Option<&str> {
        match self {
            Self::Review { pr, .. } => Some(pr),
            Self::Fresh | Self::Refused(_) => None,
        }
    }
}

/// Why an issue is not claimed now, its label left on.
enum Skip {
    /// A task is still working on it: it is never taken twice.
    Busy,
    /// The pull request on its last task's branch could not be looked up.
    Unread(String),
}

impl Skip {
    /// What a person who picked the issue `named` is told.
    fn said(&self, named: &str) -> String {
        match self {
            Self::Busy => preflight::already_working(named),
            Self::Unread(why) => format!("Could not start on issue {named}: {why}"),
        }
    }
}

/// What claiming issue `number` of `tracker` comes to, by the newest of
/// `earlier` on it and, with a forge, the pull request on its branch; one
/// merged, or none, is a fresh start. Blocking.
fn taking_blocking(
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
            Taking::Refused(core::review_unanswerable(&pr.url))
        }
        PrState::Open => Taking::Review {
            task: last.task.clone(),
            dir: last.dir.clone(),
            branch: last.branch.clone(),
            note: core::review_note(&pr.url, &forge.read_review_with(pr.number)),
            pr: pr.url,
        },
        PrState::Closed => Taking::Refused(core::review_closed(&pr.url)),
        PrState::Merged => Taking::Fresh,
    })
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
fn prepare_blocking(
    repo: PathBuf,
    tracker: Tracker,
    forge: Option<&'static dyn Connector>,
    issue: Issue,
    workflow: String,
    taking: Taking,
    picked_in: Option<gpui::AnyWindowHandle>,
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
                    return Err(core::review_diverged());
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
        let branch = core::free_branch_blocking(&top, &core::branch_for(&tracker, &issue), taken)?;
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
            picked_in,
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

/// Where a project's issues are looked for, in the order they are searched.
///
/// A project kept in step with its forge is looked for **here only**, since
/// the forge's issues are already here and searching both would find each one
/// twice. Otherwise its own issues first, then the forge's. Blocking: it reads
/// the issue file to learn whether the project is synced.
fn trackers_blocking(
    issues: Option<PathBuf>,
    forge: Option<&'static dyn Connector>,
) -> Vec<Tracker> {
    if let (Some(file), Some(forge)) = (&issues, forge)
        && onehand_core::issues::load_blocking(file)
            .is_ok_and(|kept| kept.in_step_with(forge.name()))
    {
        return vec![Tracker::Synced {
            file: file.clone(),
            forge,
        }];
    }
    issues
        .map(Tracker::Local)
        .into_iter()
        .chain(forge.map(Tracker::Forge))
        .collect()
}

/// The claim came back: start the session, or say why not.
///
/// `held` is why nothing was started where something was looked for, when
/// that is so: a search somebody asked for says it rather than that there was
/// nothing to find.
pub(super) fn landed(
    begun: Option<Result<Claimed, Unstarted>>,
    asked_from: Option<gpui::AnyWindowHandle>,
    held: Option<String>,
    cx: &mut App,
) {
    with(cx, |u| u.claiming = false);
    let unstarted = match begun {
        None => {
            // A search somebody asked for that found nothing says so; one the
            // tick ran says nothing, as a quiet half hour always has.
            if let Some(window) = asked_from {
                let label = label(cx);
                let said = held.unwrap_or_else(|| {
                    format!(
                        "No open issue of yours labelled `{label}` in the projects switched on."
                    )
                });
                warn(window, said, cx);
            }
            return;
        }
        Some(Err(unstarted)) => unstarted,
        Some(Ok(claimed)) => match start(claimed, cx) {
            Ok(()) => return,
            Err(unstarted) => unstarted,
        },
    };
    let said = core::could_not_start(&unstarted.why);
    cx.background_executor()
        .spawn(
            async move { tell_issue(&unstarted.tracker, &unstarted.repo, unstarted.number, &said) },
        )
        .detach();
}

/// Keep the claimed issue as a task of its workflow, then ask for
/// its place in the window holding its project: the one it was picked in
/// when that one does, so its session comes up in front of the person who
/// asked for it.
fn start(claimed: Claimed, cx: &mut App) -> Result<(), Unstarted> {
    let Some((mode, timeout)) = with(cx, |u| (u.mode.clone(), u.timeout.clone())) else {
        return Err(unstarted(claimed, "unattended runs are off"));
    };
    let agent = super::run_agent(cx);
    let holding: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .filter(|w| {
            w.shell
                .upgrade()
                .is_some_and(|s| s.read(cx).holds_root(&claimed.repo))
        })
        .map(|w| (w.handle, w.shell.clone()))
        .collect();
    let Some((window, shell)) = holding
        .iter()
        .find(|(handle, _)| Some(*handle) == claimed.picked_in)
        .or(holding.first())
        .cloned()
    else {
        return Err(unstarted(
            claimed,
            "the project was closed before the run could start",
        ));
    };
    let Some(shell) = shell.upgrade() else {
        return Err(unstarted(claimed, "the project's window was closed"));
    };
    let (branch, base, dir) = match claimed.work.clone() {
        Work::Cut { branch, base, dir } => (branch, base, dir),
        // A review is answered on the task's own snapshot, as any retry is:
        // the configured workflow is for new tasks only.
        Work::Review { task, note } => {
            return answer_review(claimed, task, note, window, &shell, cx);
        }
    };
    let template = match workflow(&claimed.workflow, &timeout, cx) {
        Ok(template) => template,
        Err(why) => return Err(unstarted(claimed, &why)),
    };
    let check = shell.read(cx).check_of(&claimed.repo);
    if template.needs_check() && check.is_none() {
        return Err(unstarted(
            claimed,
            &format!(
                "the workflow `{}` runs the project's check command, and the project has \
                 none; set one on the project's page",
                template.name
            ),
        ));
    }
    let picked = claimed.picked_by_hand();
    let setup = Setup {
        repo: claimed.repo.clone(),
        dir,
        branch: Some(branch),
        agent,
        check,
        // Left empty, the agent stays in the mode it starts in, which asks
        // more rather than less.
        mode: Some(mode).filter(|mode| !mode.trim().is_empty()),
        forge: claimed.forge.map(|forge| forge.name().to_string()),
    };
    let brief = core::brief_for(&claimed.tracker, &claimed.issue, &claimed.instructions);
    let task_id = onehand_core::task::new_id();
    let mut task = Task::new(task_id.clone(), template, brief, setup);
    task.source = Source::Issue(IssueSource {
        tracker: claimed.tracker.to_ref(),
        number: claimed.issue.number,
        forge_ref: claimed.issue.forge_ref().map(str::to_string),
        forge: claimed.forge.map(|forge| forge.name().to_string()),
        base,
        picked,
        unsent: Vec::new(),
        notes: claimed.notes,
    });
    // Counted against the cap until it has asked for its place.
    with(cx, |u| u.starting.insert(task_id.clone()));
    crate::task::add(task, cx);
    let asked = {
        let task_id = task_id.clone();
        window.update(cx, |_, window, cx| {
            shell.update(cx, |_, cx| crate::task::request(task_id, window, cx))
        })
    };
    if asked.is_err() {
        super::placed(&task_id, cx);
    }
    Ok(())
}

/// Retry issue task `task` to answer the review on its pull request, telling
/// its new run `note`, from the step its status checks send back to; then ask
/// for its place in `window`.
fn answer_review(
    claimed: Claimed,
    task: String,
    note: String,
    window: gpui::AnyWindowHandle,
    shell: &gpui::Entity<crate::shell::Shell>,
    cx: &mut App,
) -> Result<(), Unstarted> {
    let Some(last) = crate::task::task(&task, cx).and_then(|t| t.runs.last().cloned()) else {
        return Err(unstarted(claimed, "the task of its pull request is gone"));
    };
    let from = last.template.repair_step().map(str::to_string);
    if !crate::task::retry(&task, last.template, from.as_deref(), Some(note), cx) {
        return Err(unstarted(claimed, "a run is still working on it"));
    }
    with(cx, |u| u.starting.insert(task.clone()));
    let asked = window.update(cx, |_, window, cx| {
        shell.update(cx, |_, cx| crate::task::request(task.clone(), window, cx))
    });
    if asked.is_err() {
        super::placed(&task, cx);
    }
    Ok(())
}

/// The workflow `id` as a run of it starts: its timeout put to the config's,
/// and validated, or why it cannot run.
///
/// **Only a workflow that works on a worktree.** An issue's run is cut a
/// worktree of its own, and a workflow meant for a checkout run there leaves
/// its work uncommitted where nobody looks and reports that nothing landed.
pub(super) fn workflow(id: &str, timeout: &str, cx: &App) -> Result<Template, String> {
    let template = found(id, timeout, cx)?;
    if let Some(why) = onehand_core::preflight::unfit_for_issue(&template) {
        return Err(why);
    }
    let problems = flow::validate(&template);
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

/// The workflow `id` with its timeout put to `timeout`, not yet judged, or
/// why there is none.
pub(super) fn found(id: &str, timeout: &str, cx: &App) -> Result<Template, String> {
    let mut template = crate::workflow::templates(cx)
        .into_iter()
        .filter_map(|entry| entry.template.ok())
        .find(|template| template.id == id)
        .ok_or_else(|| format!("there is no workflow `{id}`"))?;
    template.timeout = timeout.to_string();
    Ok(template)
}

/// `claimed` could not be started, for `why`.
fn unstarted(claimed: Claimed, why: &str) -> Unstarted {
    Unstarted {
        repo: claimed.repo,
        tracker: claimed.tracker,
        number: claimed.issue.number,
        why: why.to_string(),
    }
}
