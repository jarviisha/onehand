use super::report::tell_issue;
use super::{Project, Served, connector_for, label, opted_in_roots, tick, why_not, with};
use crate::state::Shared;
use gpui::App;
use onehand_core::connector::Connector;
use onehand_core::task::{Source, Task};
use onehand_core::unattended::{self as core, Issue, IssueRow, IssueSource, Tracker};
use onehand_core::workflow::{self as flow, Setup, Template};
use onehand_core::worktree;
use std::path::Path;
use std::path::PathBuf;

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
    pub(super) branch: String,
    /// What the worktree was cut from: the remote's default branch with a
    /// forge, the branch checked out without one. Also what a run with no
    /// forge is measured against.
    pub(super) base: String,
    /// The worktree the run made, and the project root it is added as.
    pub(super) dir: PathBuf,
    /// The window a person picked it in, rather than the search finding it.
    /// Such a run is put on screen there as it starts and stays when it ends —
    /// somebody asked for it and is watching — so a card it parks is theirs.
    picked_in: Option<gpui::AnyWindowHandle>,
}

impl Claimed {
    /// Whether a person picked this run rather than the search finding it.
    pub(super) fn picked_by_hand(&self) -> bool {
        self.picked_in.is_some()
    }
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
/// they are the ones written for onehand to work.
pub(super) fn begin_blocking(
    roots: &[(Project, Option<&'static dyn Connector>)],
    label: &str,
    checked: &mut Vec<(PathBuf, Served)>,
) -> Option<Result<Claimed, Unstarted>> {
    let (repo, tracker, forge, issue) = roots.iter().find_map(|(project, forge)| {
        for tracker in trackers_blocking(project.issues.clone(), *forge) {
            match core::candidate_blocking(&tracker, &project.root, label) {
                Ok(Some(issue)) => return Some((project.root.clone(), tracker, *forge, issue)),
                Ok(None) => {}
                Err(why) => {
                    checked.push((project.root.clone(), Err(why)));
                    return None;
                }
            }
        }
        None
    })?;
    if let Err(why) = core::claim_blocking(&tracker, &repo, issue.number, label) {
        eprintln!("onehand: could not claim issue #{}: {why}", issue.number);
        return None;
    }
    Some(prepare_blocking(repo, tracker, forge, issue, None))
}

/// Cut a worktree for a claimed issue: off the remote's default branch, fetched
/// first, on a project with a forge; off the branch checked out on one without,
/// since there is no remote to ask.
///
/// Caught as well as everything around it: by now the issue has been claimed,
/// so a panic has to become a comment on the issue, or it is left claimed with
/// nothing saying what happened.
fn prepare_blocking(
    repo: PathBuf,
    tracker: Tracker,
    forge: Option<&'static dyn Connector>,
    issue: Issue,
    picked_in: Option<gpui::AnyWindowHandle>,
) -> Result<Claimed, Unstarted> {
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let base = match forge {
            Some(forge) => {
                let default = forge.default_branch_blocking(&repo)?;
                forge.fetch_blocking(&repo, &default)?;
                format!("origin/{default}")
            }
            None => worktree::current_branch_blocking(&repo)?,
        };
        let top = worktree::repo_top_blocking(&repo).unwrap_or_else(|| repo.clone());
        let branch = core::free_branch_blocking(&top, &core::branch_for(&tracker, &issue));
        let dir = worktree::worktree_dir(&top, &branch);
        let dir = worktree::branch_off_blocking(&top, &branch, &dir, &base)?;
        Ok::<_, String>((base, branch, dir))
    }))
    .unwrap_or_else(|_| Err("onehand panicked while preparing the worktree".to_string()));
    match made {
        Ok((base, branch, dir)) => Ok(Claimed {
            repo,
            tracker,
            forge,
            issue,
            branch,
            base,
            dir,
            picked_in,
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
pub type Pickable = (Vec<(Tracker, IssueRow)>, bool, Option<String>);

/// Read what a person can pick from in `root`, whose own issues are in
/// `issues` if its workspace keeps any. Blocking.
///
/// A forge that cannot be read is an error only when there is nothing else to
/// show; beside issues of the project's own it is said under the list, so one
/// half being down does not hide the other.
pub fn pickable_blocking(root: &Path, issues: Option<PathBuf>) -> Result<Pickable, String> {
    let (mut rows, mut cut, mut unread) = (Vec::new(), false, None);
    let forge = connector_for(root);
    for tracker in trackers_blocking(issues, forge.as_ref().ok().copied()) {
        match core::open_issues_blocking(&tracker, root) {
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

/// Work `row`, picked by hand from a project's open issues, now.
///
/// **Refused while the cap is reached**, the rule for picked and found alike —
/// a run waiting on a person does not count — and the refusal names the
/// issues being worked so the person knows what they are waiting on. Anything that stops it before
/// the claim — a claim refused where the issue lives — is said in the window it
/// was picked from; after the claim, on the issue as well.
pub fn start_picked(
    repo: PathBuf,
    tracker: Tracker,
    row: IssueRow,
    window: gpui::AnyWindowHandle,
    cx: &mut App,
) -> Result<(), String> {
    let (full, refused) = why_not(cx);
    let label = with(cx, |u| {
        if let Some(why) = full {
            return Err(why);
        }
        if u.claiming {
            return Err("An unattended run is starting — one at a time.".to_string());
        }
        // Refused before the claim: the run would fail, and the issue would
        // be claimed and commented on for nothing.
        if let Some(why) = refused {
            return Err(format!("Nothing can be started: {why}"));
        }
        u.claiming = true;
        Ok(u.label.clone())
    })
    .ok_or("Unattended runs are not set up.")??;
    cx.spawn(async move |cx| {
        let number = row.issue.number;
        let begun = cx
            .background_executor()
            .spawn(async move {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // An issue on the forge goes back to that forge; one kept
                    // here goes to whichever forge serves the project, if any.
                    let forge = match &tracker {
                        Tracker::Forge(forge) | Tracker::Synced { forge, .. } => Some(*forge),
                        Tracker::Local(_) => connector_for(&repo).ok(),
                    };
                    core::claim_picked_blocking(&tracker, &repo, &row, &label)
                        .map_err(|why| format!("Could not start on issue #{number}: {why}"))?;
                    Ok(prepare_blocking(
                        repo,
                        tracker,
                        forge,
                        row.issue,
                        Some(window),
                    ))
                }))
                .unwrap_or_else(|_| Err("onehand panicked while claiming the issue".to_string()))
            })
            .await;
        cx.update(|cx| {
            let unstarted = match begun {
                // Nothing was claimed, so the issue has nothing to be told; the
                // window it was picked from is where the person is.
                Err(why) => {
                    with(cx, |u| u.claiming = false);
                    warn(window, why, cx);
                    return;
                }
                Ok(begun) => begun,
            };
            if let Err(Unstarted { why, .. }) = &unstarted {
                warn(
                    window,
                    format!("Could not start on issue #{number}: {why}"),
                    cx,
                );
            }
            landed(Some(unstarted), None, cx);
        });
    })
    .detach();
    Ok(())
}

/// Say `why` in `window`, as the transient notice it is.
fn warn(window: gpui::AnyWindowHandle, why: String, cx: &mut App) {
    use gpui_component::WindowExt as _;
    let _ = window.update(cx, |_, window, cx| {
        window.push_notification(gpui_component::notification::Notification::warning(why), cx);
    });
}

/// Look for a labelled issue now, rather than at the next tick, and say what
/// came of it in `window` — the person pressed a button and is waiting to hear.
/// Every way it can do nothing is said rather than left to look like nothing
/// happened.
pub fn look_now(window: gpui::AnyWindowHandle, cx: &mut App) {
    let why_not = if opted_in_roots(cx).is_empty() {
        Some("No project is switched on for unattended runs.".to_string())
    } else {
        let (full, refused) = why_not(cx);
        with(cx, |u| {
            if full.is_some() {
                full
            } else if u.claiming {
                Some("A run is already starting.".to_string())
            } else {
                u.blocked
                    .clone()
                    .or(refused)
                    .map(|why| format!("Nothing can be picked up: {why}"))
            }
        })
        .flatten()
    };
    match why_not {
        Some(why) => warn(window, why, cx),
        None => tick(Some(window), cx),
    }
}

/// The claim came back: start the session, or say why not.
pub(super) fn landed(
    begun: Option<Result<Claimed, Unstarted>>,
    asked_from: Option<gpui::AnyWindowHandle>,
    cx: &mut App,
) {
    with(cx, |u| u.claiming = false);
    let unstarted = match begun {
        None => {
            // A search somebody asked for that found nothing says so; one the
            // tick ran says nothing, as a quiet half hour always has.
            if let Some(window) = asked_from {
                let label = label(cx);
                warn(
                    window,
                    format!(
                        "No open issue of yours labelled `{label}` in the projects switched on."
                    ),
                    cx,
                );
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

/// Keep the claimed issue as a task of the configured workflow, then ask for
/// its place in the window holding its project: the one it was picked in
/// when that one does, so its session comes up in front of the person who
/// asked for it.
fn start(claimed: Claimed, cx: &mut App) -> Result<(), Unstarted> {
    let Some((id, mode, agent, timeout)) = with(cx, |u| {
        (
            u.workflow.clone(),
            u.mode.clone(),
            u.agent.clone(),
            u.timeout.clone(),
        )
    }) else {
        return Err(unstarted(claimed, "unattended runs are off"));
    };
    let template = match workflow(&id, &timeout, cx) {
        Ok(template) => template,
        Err(why) => return Err(unstarted(claimed, &why)),
    };
    let agent = agent.or_else(|| {
        Shared::global(cx)
            .agents
            .first()
            .map(|spec| spec.name.clone())
    });
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
    let check = shell.read(cx).check_of(&claimed.repo);
    if template.needs_check() && check.is_none() {
        return Err(unstarted(
            claimed,
            &format!(
                "the workflow `{}` runs the project's check command, and the project has \
                 none; set one under Settings ▸ Workflows",
                template.name
            ),
        ));
    }
    let picked = claimed.picked_by_hand();
    let setup = Setup {
        repo: claimed.repo.clone(),
        dir: claimed.dir.clone(),
        branch: Some(claimed.branch.clone()),
        agent,
        check,
        // Left empty, the agent stays in the mode it starts in, which asks
        // more rather than less.
        mode: Some(mode).filter(|mode| !mode.trim().is_empty()),
    };
    let brief = core::brief_for(&claimed.tracker, &claimed.issue);
    let task_id = onehand_core::task::new_id();
    let mut task = Task::new(task_id.clone(), template, brief, setup);
    task.source = Source::Issue(IssueSource {
        tracker: claimed.tracker.to_ref(),
        number: claimed.issue.number,
        forge_ref: claimed.issue.forge_ref().map(str::to_string),
        forge: claimed.forge.map(|forge| forge.name().to_string()),
        base: claimed.base.clone(),
        picked,
        unsent: Vec::new(),
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
    // ponytail: a window closed while the place is looked up also leaves the
    // task counted; it goes at the next start of onehand.
    if asked.is_err() {
        super::placed(&task_id, cx);
    }
    Ok(())
}

/// The workflow `id` as a run of it starts: its timeout put to the config's,
/// and validated, or why it cannot run.
pub(super) fn workflow(id: &str, timeout: &str, cx: &App) -> Result<Template, String> {
    let mut template = crate::workflow::templates(cx)
        .into_iter()
        .filter_map(|entry| entry.template.ok())
        .find(|template| template.id == id)
        .ok_or_else(|| format!("there is no workflow `{id}` (unattended.workflow)"))?;
    template.timeout = timeout.to_string();
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

/// `claimed` could not be started, for `why`.
fn unstarted(claimed: Claimed, why: &str) -> Unstarted {
    Unstarted {
        repo: claimed.repo,
        tracker: claimed.tracker,
        number: claimed.issue.number,
        why: why.to_string(),
    }
}
