use super::turn::{clock, note, on_event, pending_ending, resume, settle, start_notes, tell_issue};
use super::{Project, Run, Served, connector_for, label, opted_in_roots, tick, with};
use crate::chat::session::ChatEvent;
use crate::state::Shared;
use gpui::App;
use onehand_core::config::AgentSpec;
use onehand_core::connector::{Connector, PrState};
use onehand_core::unattended::{
    self as core, Budget, Ending, Issue, IssueRow, Phase, Progress, Record, Start, Tracker, Verdict,
};
use onehand_core::worktree;
use std::path::Path;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    /// The run's own file, which is what a restart carries on from.
    pub(super) file: PathBuf,
    /// A person picked it rather than the search finding it. Such a run is put
    /// on screen as it starts and stays when it ends — somebody asked for it
    /// and is watching — so a card it parks is theirs.
    by_hand: bool,
    /// The window it was picked in, while that is still the window it was
    /// picked in: a restart forgets it.
    picked_in: Option<gpui::AnyWindowHandle>,
}

impl Claimed {
    /// Whether a person picked this run rather than the search finding it.
    pub(super) fn picked_by_hand(&self) -> bool {
        self.by_hand
    }

    /// The run as its file holds it.
    pub(super) fn record(&self, phase: Phase, progress: &Progress) -> Record {
        Record {
            repo: self.repo.clone(),
            kept: self.tracker.kept(),
            forge: self.forge.map(|f| f.name().to_string()),
            issue: self.issue.clone(),
            branch: self.branch.clone(),
            base: self.base.clone(),
            dir: self.dir.clone(),
            by_hand: self.by_hand,
            phase,
            progress: progress.clone(),
        }
    }

    /// A run read back from `file`; `None` if a connector it names is not
    /// built into this onehand any more.
    pub(super) fn from_record(file: PathBuf, record: Record) -> Option<Self> {
        let named = |name: &str| {
            crate::plugins::connectors()
                .iter()
                .copied()
                .find(|c| c.name() == name)
        };
        let forge = match &record.forge {
            Some(name) => Some(named(name)?),
            None => None,
        };
        Some(Self {
            repo: record.repo,
            tracker: record.kept.tracker(named)?,
            forge,
            issue: record.issue,
            branch: record.branch,
            base: record.base,
            dir: record.dir,
            file,
            by_hand: record.by_hand,
            picked_in: None,
        })
    }
}

/// Write the run's file in the background, saying on stderr if that failed:
/// the run goes on, it only cannot be carried on after a restart.
pub(super) fn save(claimed: &Claimed, phase: Phase, progress: &Progress, cx: &App) {
    save_record(claimed.file.clone(), claimed.record(phase, progress), cx);
}

/// [`save`], for a caller that took the record while it held the state.
pub(super) fn save_record(file: PathBuf, record: Record, cx: &App) {
    cx.background_executor()
        .spawn(async move {
            if let Err(why) = core::save_record_blocking(&file, &record) {
                eprintln!("onehand: could not save an unattended run: {why}");
            }
        })
        .detach();
}

/// An issue that was claimed and then could not be started, and where to say
/// so.
pub(super) struct Unstarted {
    repo: PathBuf,
    tracker: Tracker,
    number: u64,
    why: String,
    /// The run's file, once it has one.
    file: Option<PathBuf>,
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
///
/// `busy` is every issue a run is still on, by project; none of them is taken.
pub(super) fn begin_blocking(
    roots: &[(Project, Option<&'static dyn Connector>)],
    label: &str,
    busy: &[(PathBuf, u64)],
    checked: &mut Vec<(PathBuf, Served)>,
) -> Option<Result<(Claimed, Progress), Unstarted>> {
    let (repo, tracker, forge, issue) = roots.iter().find_map(|(project, forge)| {
        let busy: Vec<u64> = busy
            .iter()
            .filter(|(repo, _)| *repo == project.root)
            .map(|(_, number)| *number)
            .collect();
        for tracker in trackers_blocking(project.issues.clone(), *forge) {
            match core::candidate_blocking(&tracker, &project.root, label, &busy) {
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

/// Cut or find the worktree for a claimed issue, and say what the run's first
/// session is for.
///
/// **An issue worked before goes back to its branch.** The newest branch
/// under the issue's prefix is looked at: with an open pull request the run
/// answers its review, with none it carries on the work there, and with a
/// merged one it starts again on a branch of its own. A pull request closed
/// without being merged refuses the run — somebody decided against it, and
/// opening another would undo that decision.
///
/// A new branch is cut off the remote's default branch, fetched first, on a
/// project with a forge; off the branch checked out on one without, since
/// there is no remote to ask.
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
) -> Result<(Claimed, Progress), Unstarted> {
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
        let earlier = worktree::newest_branch_blocking(&top, &core::issue_prefix(issue.number));
        let start = match (&earlier, forge) {
            (Some(branch), Some(forge)) => match forge.pull_request_for_blocking(&repo, branch)? {
                Some(pr) if pr.state == PrState::Open => Some(Start::Review {
                    pr: pr.url,
                    number: pr.number,
                }),
                Some(pr) if pr.state == PrState::Closed => {
                    return Err(format!(
                        "its pull request {} was closed without being merged, and onehand \
                         does not open another. Delete the branch `{branch}` to start over.",
                        pr.url
                    ));
                }
                Some(_) => None,
                None => Some(Start::Earlier),
            },
            (Some(_), None) => Some(Start::Earlier),
            (None, _) => None,
        };
        let (branch, dir, start) = match (earlier, start) {
            (Some(branch), Some(start)) => {
                let dir = match worktree::worktree_of_blocking(&top, &branch) {
                    Some(dir) => dir,
                    None => worktree::add_blocking(
                        &top,
                        &branch,
                        &worktree::worktree_dir(&top, &branch),
                    )?,
                };
                (branch, dir, start)
            }
            _ => {
                let branch = core::free_branch_blocking(&top, &core::branch_for(&issue));
                let dir = worktree::worktree_dir(&top, &branch);
                let dir = worktree::branch_off_blocking(&top, &branch, &dir, &base)?;
                (branch, dir, Start::Fresh)
            }
        };
        // An answer to a review is measured by what it adds, not by the work
        // the pull request already holds.
        let since = match start {
            Start::Review { .. } => Some(worktree::head_blocking(&dir)?),
            Start::Fresh | Start::Earlier | Start::Repair { .. } => None,
        };
        Ok::<_, String>((base, branch, dir, Progress::new(start, since)))
    }))
    .unwrap_or_else(|_| Err("onehand panicked while preparing the worktree".to_string()));
    match made {
        Ok((base, branch, dir, progress)) => {
            let claimed = Claimed {
                repo,
                tracker,
                forge,
                file: core::new_record_file(issue.number),
                issue,
                branch,
                base,
                dir,
                by_hand: picked_in.is_some(),
                picked_in,
            };
            let record = claimed.record(Phase::Working, &progress);
            if let Err(why) = core::save_record_blocking(&claimed.file, &record) {
                eprintln!("onehand: could not save an unattended run: {why}");
            }
            Ok((claimed, progress))
        }
        Err(why) => Err(Unstarted {
            repo,
            tracker,
            number: issue.number,
            why,
            file: None,
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
/// **Refused while a run is working**: one run at a time is the rule for picked
/// and found alike — a run waiting on a person does not count — and the
/// refusal names the issue already being worked so the person knows what they
/// are waiting on. Anything that stops it before
/// the claim — a claim refused where the issue lives — is said in the window it
/// was picked from; after the claim, on the issue as well.
pub fn start_picked(
    repo: PathBuf,
    tracker: Tracker,
    row: IssueRow,
    window: gpui::AnyWindowHandle,
    cx: &mut App,
) -> Result<(), String> {
    let label = with(cx, |u| {
        if let Some(run) = u.working() {
            return Err(format!(
                "An unattended run is already working on issue #{} — one at a time.",
                run.claimed.issue.number
            ));
        }
        if u.claiming {
            return Err("An unattended run is starting — one at a time.".to_string());
        }
        if u.busy().contains(&(repo.clone(), row.issue.number)) {
            return Err(format!(
                "A run on issue #{} has not ended yet: it is waiting on its pull request's \
                 checks.",
                row.issue.number
            ));
        }
        // Refused before the claim: the run would fail at its prompt, and the
        // issue would be claimed and commented on for nothing.
        if let Some(why) = &u.mode_refused {
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
        with(cx, |u| {
            if let Some(run) = u.working() {
                Some(format!(
                    "A run is already working on issue #{} — one at a time.",
                    run.claimed.issue.number
                ))
            } else if u.claiming {
                Some("A run is already starting.".to_string())
            } else {
                u.blocked
                    .clone()
                    .or_else(|| u.mode_refused.clone())
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
    begun: Option<Result<(Claimed, Progress), Unstarted>>,
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
        Some(Ok((claimed, progress))) => match start(claimed, progress, cx) {
            Ok(()) => return,
            Err(unstarted) => unstarted,
        },
    };
    tell_unstarted(unstarted, cx);
}

/// Say on the issue that its run could not start, and let go of the run's
/// file.
pub(super) fn tell_unstarted(unstarted: Unstarted, cx: &App) {
    // A run that never started left nothing, whichever kind of nothing.
    let said = core::report(
        &Ending::Failed(unstarted.why),
        &Ok(Verdict::NoPullRequest),
        "",
    );
    cx.background_executor()
        .spawn(async move {
            tell_issue(&unstarted.tracker, &unstarted.repo, unstarted.number, &said);
            if let Some(file) = unstarted.file {
                let _ = std::fs::remove_file(file);
            }
        })
        .detach();
}

/// Mint a session of the run in the window holding its project, and start
/// watching it. `progress` says what the session is for and how much of the
/// timeout earlier sessions of this attempt already spent.
pub(super) fn start(claimed: Claimed, progress: Progress, cx: &mut App) -> Result<(), Unstarted> {
    let (agent, timeout) = match with(cx, |u| (u.agent.clone(), u.timeout)) {
        Some(settings) => settings,
        None => return Err(unstarted(claimed, "unattended runs are off")),
    };
    let Some(spec) = spec_for(agent.as_deref(), cx) else {
        return Err(unstarted(claimed, "no agent is configured"));
    };
    // The window it was picked in when that one holds the project, so the
    // session comes up in front of the person who asked for it.
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
    let Some((uid, session, owns_root)) = shell
        .upgrade()
        .and_then(|s| s.update(cx, |s, cx| s.run_unattended(claimed.dir.clone(), spec, cx)))
    else {
        return Err(unstarted(
            claimed,
            "the worktree is already open as a project",
        ));
    };

    let watch = cx.subscribe(&session, move |session, event: &ChatEvent, cx| {
        on_event(uid, &session, event, cx)
    });
    // A cancel already winding down keeps the ending it was heading for, and
    // a run waiting on a card ends on the question nobody answered; only a
    // session that went with nothing pending is reported as closed.
    let release = cx.observe_release(&session, move |_, cx| {
        let pending = with(cx, |u| u.run_mut(uid).and_then(|run| pending_ending(run))).flatten();
        settle(uid, pending.unwrap_or(Ending::Closed), cx)
    });
    // Fires on every notify the session makes, a streamed chunk included, so
    // it reads rather than borrowing the global mutably, and a run that is not
    // waiting costs one lookup.
    let answered = cx.observe(&session, move |session, cx| {
        let waiting = Shared::global(cx)
            .unattended
            .as_ref()
            .and_then(|u| u.runs.iter().find(|run| run.uid == uid))
            .is_some_and(|run| run.waiting.is_some());
        if waiting && !session.read(cx).chat.awaiting_permission() {
            resume(uid, &session, cx);
        }
    });
    let budget = Budget::resumed(
        timeout,
        Duration::from_secs(progress.spent_secs),
        Instant::now(),
    );
    let clock = clock(uid, budget.left(Instant::now()), timeout, cx);
    let (by_hand, opening, shown_in) = (
        claimed.picked_by_hand(),
        start_notes(&claimed, &progress.start),
        shell.clone(),
    );
    with(cx, |u| {
        u.runs.push(Run {
            claimed,
            uid,
            session: session.downgrade(),
            window,
            shell,
            owns_root,
            sent: 0,
            turns: 0,
            progress,
            waiting: None,
            budget,
            ending: None,
            _watch: watch,
            _answered: answered,
            _release: release,
            _clock: clock,
        })
    });
    // The rail marks the project a run is working on; nothing it watches
    // changed, so it has to be told.
    cx.refresh_windows();
    for line in opening {
        note(&session, line, cx);
    }
    // Picked by hand is asked for by somebody at the window, so it is put in
    // front of them; one found by the search never moves what is on screen.
    if by_hand && let Some(shell) = shown_in.upgrade() {
        let _ = window.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| shell.show_session(uid, window, cx))
        });
    }
    Ok(())
}

/// `claimed` could not be started, for `why`.
fn unstarted(claimed: Claimed, why: &str) -> Unstarted {
    Unstarted {
        repo: claimed.repo,
        tracker: claimed.tracker,
        number: claimed.issue.number,
        why: why.to_string(),
        file: Some(claimed.file),
    }
}

/// The agent a run starts, with every build it makes pointed at one shared
/// directory.
///
/// Through `env` rather than a new field threaded down to the process spawn,
/// because the adapter is the one process a run gets to set anything on and
/// everything the agent runs inherits from it.
// ponytail: `env` is POSIX; a Windows build would need the variable threaded
// through the ACP spawn instead.
fn spec_for(agent: Option<&str>, cx: &App) -> Option<AgentSpec> {
    let agents = &Shared::global(cx).agents;
    let base = agent
        .and_then(|name| agents.iter().find(|spec| spec.name == name))
        .or_else(|| agents.first())?
        .clone();
    let Some(target) = core::target_dir() else {
        return Some(base);
    };
    let mut args = vec![
        format!("CARGO_TARGET_DIR={}", target.display()),
        base.command,
    ];
    args.extend(base.args);
    Some(AgentSpec {
        name: base.name,
        command: "env".to_string(),
        args,
    })
}
