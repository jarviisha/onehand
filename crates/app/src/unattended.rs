//! Unattended runs, as the app drives them: the tick, the live run, the
//! watching, the timeout and the teardown.
//!
//! Everything that can be decided without a window — which issue, what branch,
//! what the prompt says, what the issue is told — is `onehand_core::unattended`.
//! What is here is what needs an entity, a window or a timer.
//!
//! **One per process, on [`Shared`]**, for the reason the remote bridge is
//! there: two windows each running a tick would be two agents on one issue. The
//! tick therefore belongs to no window and asks each in turn for its projects.

use crate::chat::session::{ChatEvent, ChatSession};
use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Entity, Subscription, Task, WeakEntity};
use onehand_core::chat::ChatItem;
use onehand_core::chat::UserAsk;
use onehand_core::config::{AgentSpec, UnattendedConfig};
use onehand_core::connector::{self, Connector};
use onehand_core::unattended::{self as core, Ending, Issue, IssueRow, Tracker, Verdict};
use onehand_core::worktree;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

/// How long a cancelled turn is given to wind down before the run is settled
/// anyway. Cancelling asks the adapter to end the turn, and the turn ending is
/// what writes its transcript — closing the session straight away would lose
/// the one turn the run was about.
const WIND_DOWN: Duration = Duration::from_secs(30);

/// The unattended half of the process: its settings, the live run and the tick.
pub struct Unattended {
    label: String,
    timeout: Duration,
    mode: String,
    agent: Option<String>,
    /// A claim is on its way to the connector and the worktree is being made. A tick
    /// landing now must not start a second.
    claiming: bool,
    /// Why no run can start at all, whatever project is switched on: a config
    /// that cannot work (no label, an interval that does not parse, a mode the
    /// agent does not offer). Every run would fail the same way on a fresh
    /// issue, so the search stops, and every switched-on row says why.
    blocked: Option<String>,
    /// The agent does not offer the configured mode. Kept apart from
    /// `blocked` because it stops a run picked by hand as well, where the
    /// label and the interval do not matter — and it is only learned once an
    /// adapter has answered, which is after a claim.
    mode_refused: Option<String>,
    run: Option<Run>,
    /// What each connector last said about its account, for the lines in
    /// Settings. `None` until the first answer lands.
    accounts: Option<Accounts>,
    /// When those answers landed, so Settings can say how old they are.
    checked_at: Option<std::time::SystemTime>,
    /// How many checks started through [`check`] -- *Check again*, a project
    /// switched on, a window registering -- have not answered yet. Without it
    /// *Check again* looks like it did nothing until the answer lands.
    ///
    /// **A count and not a flag**, because checks overlap: a scheduled look or
    /// a switch turned on can land while the one asked for is still out, and a
    /// flag cleared by whichever lands first gives the button back early.
    checks_out: usize,
    /// Why the last look at a project could not go ahead, by project.
    ///
    /// **Shown on the project's row**, because the alternative is stderr: a
    /// project no connector serves, or one whose account is unusable, fails the
    /// same way on every tick, and a switch that is on while nothing can happen
    /// looks exactly like one that is working. An entry is cleared by the next
    /// look that gets through.
    problems: HashMap<PathBuf, String>,
    _tick: Task<()>,
}

/// Each connector beside what it said about its account: who it acts as, or
/// why it cannot act.
pub type Accounts = Vec<(&'static dyn Connector, Result<String, String>)>;

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
type Served = Result<Option<&'static dyn Connector>, String>;

/// The connector serving the project at `root`, or every reason none does.
///
/// Its account is not asked here: a person picking an issue by hand hears
/// about an unusable account from the connector's own refusal, one call later.
pub fn connector_for(root: &Path) -> Result<&'static dyn Connector, String> {
    let all = crate::plugins::connectors();
    connector::serving(all, root).map(|at| all[at])
}

/// One issue being worked.
struct Run {
    claimed: Claimed,
    uid: u64,
    session: WeakEntity<ChatSession>,
    window: gpui::AnyWindowHandle,
    shell: WeakEntity<crate::shell::Shell>,
    prompted: bool,
    /// The turn has been cancelled, and this is what the run ends as once it
    /// has wound down.
    ending: Option<Ending>,
    _watch: Subscription,
    /// Fires if the session goes without saying so — its window closed under
    /// it — so the run settles now rather than holding the tick until its
    /// timeout and then reporting the wrong ending.
    _release: Subscription,
    /// The run's timeout, and after a cancel the wind-down in its place.
    _clock: Task<()>,
}

/// Start the tick, or say why it cannot run.
///
/// **The state is filed either way.** A config that cannot work is not the same
/// as a feature that is off: the switches are still on screen and still
/// switchable, so the reason has to be somewhere the rows and Settings can read
/// it — left only on stderr, a bad interval read on screen as a missing label,
/// and the account line waited for an answer that was never going to come. The
/// search is what stops; looking at projects and at accounts still works.
pub fn boot(cfg: &UnattendedConfig, cx: &mut App) {
    let label = cfg.label.trim().to_string();
    let every = core::parse_every(&cfg.every);
    let timeout = core::parse_every(&cfg.timeout);
    let blocked = if label.is_empty() {
        Some("no label is set in the config (unattended.label), so nothing is picked up".into())
    } else if every.is_none() || timeout.is_none() {
        Some(format!(
            "unattended.every and unattended.timeout take a number and a unit (\"30m\", \
             \"2h\", \"90s\"); the config has {:?} and {:?}",
            cfg.every, cfg.timeout
        ))
    } else {
        None
    };
    if let Some(why) = &blocked {
        eprintln!("onehand: unattended runs cannot start: {why}");
    }
    // The tick runs whatever the config says, because it is also what keeps the
    // rows' answers current; it only searches when nothing blocks a run.
    // The defaults are core's, parsed, so there is one answer to "how often".
    let fallback = UnattendedConfig::default();
    let every = every
        .or_else(|| core::parse_every(&fallback.every))
        .unwrap_or(Duration::from_secs(1800));
    let tick = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(every).await;
            cx.update(|cx| tick(None, cx));
        }
    });
    cx.update_global::<Shared, _>(|shared, _| {
        shared.unattended = Some(Unattended {
            label,
            timeout: timeout
                .or_else(|| core::parse_every(&fallback.timeout))
                .unwrap_or(Duration::from_secs(2700)),
            mode: cfg.mode.clone(),
            agent: cfg.agent.clone(),
            claiming: false,
            blocked,
            mode_refused: None,
            run: None,
            accounts: None,
            checked_at: None,
            checks_out: 0,
            problems: HashMap::new(),
            _tick: tick,
        });
    });
}

/// Why no run can start at all, if something stops every one.
pub fn blocked(cx: &App) -> Option<String> {
    let u = Shared::global(cx).unattended.as_ref()?;
    u.blocked.clone().or_else(|| u.mode_refused.clone())
}

/// What each connector last said about its account, if they have answered yet.
pub fn accounts(cx: &App) -> Option<Accounts> {
    Shared::global(cx).unattended.as_ref()?.accounts.clone()
}

/// When the connectors last answered, if they have.
pub fn accounts_checked_at(cx: &App) -> Option<std::time::SystemTime> {
    Shared::global(cx).unattended.as_ref()?.checked_at
}

/// Whether a check asked for by hand has not answered yet.
pub fn accounts_checking(cx: &App) -> bool {
    Shared::global(cx)
        .unattended
        .as_ref()
        .is_some_and(|u| u.checks_out > 0)
}

/// File what the connectors said, and when.
fn file_accounts(accounts: Accounts, cx: &mut App) {
    with(cx, |u| {
        u.accounts = Some(accounts);
        u.checked_at = Some(std::time::SystemTime::now());
    });
}

/// Why the last look at `root` could not go ahead, if it could not.
pub fn problem(root: &std::path::Path, cx: &App) -> Option<String> {
    Shared::global(cx)
        .unattended
        .as_ref()?
        .problems
        .get(root)
        .cloned()
}

/// Ask every connector again, and look again at every project that is switched on —
/// what Settings' *Check again* does, and what a window does when it opens.
pub fn recheck(cx: &mut App) {
    let roots = opted_in_roots(cx);
    check(roots, true, cx);
}

/// Look at one project that was just switched on, so a switch that cannot work
/// says so now rather than at the next tick, half an hour away.
pub fn check_now(project: Project, cx: &mut App) {
    check(vec![project], false, cx);
}

/// Ask each connector who it acts as and look at `roots`, off the UI loop, then
/// file what was found. `prune` when `roots` is every switched-on project, so
/// what is kept for a project since switched off or closed goes with it.
fn check(roots: Vec<Project>, prune: bool, cx: &mut App) {
    with(cx, |u| u.checks_out += 1);
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let (accounts, checked) = cx
            .background_executor()
            .spawn(async move { look_blocking(roots) })
            .await;
        cx.update(|cx| {
            with(cx, |u| u.checks_out = u.checks_out.saturating_sub(1));
            file_accounts(accounts, cx);
            record(checked, prune, cx);
        });
    })
    .detach();
}

/// What every connector says about its account, and for each of `roots` what
/// it can be worked with.
///
/// A project is worked **with its forge** when a connector serves it and that
/// connector's account can be used — and refused when the account cannot,
/// since the agent would be told to push somewhere it cannot. A project **no
/// connector serves** is still worked, on its own issues, with its work left on
/// a branch; what refuses it is having nothing to work — no forge and a
/// workspace that keeps no issues — or not being a repository at all. Whether a
/// connector serves is read locally, so a project nobody serves costs no call
/// to anything.
fn look_blocking(roots: Vec<Project>) -> (Accounts, Vec<(PathBuf, Served)>) {
    let all = crate::plugins::connectors();
    let accounts: Accounts = all.iter().map(|&c| (c, c.account_blocking())).collect();
    let checked = roots
        .into_iter()
        .map(|project| {
            let served = if onehand_core::worktree::repo_top_blocking(&project.root).is_none() {
                Err("it is not a git repository".to_string())
            } else {
                // `accounts` was built from `all` just above, so a place in one
                // is the same connector's place in the other.
                match connector::serving(all, &project.root) {
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

/// File what each look found, and redraw the rows that show it. A later entry
/// for the same project wins, so a search that failed after its project passed
/// the look is what the row says.
///
/// `prune` drops what is kept for projects no longer switched on — read now,
/// when the answer lands, and not from the list the look started with. A look
/// takes up to a minute, and a project switched on in that minute has already
/// been looked at by itself; clearing everything the slower look did not cover
/// threw that answer away.
fn record(checked: Vec<(PathBuf, Served)>, prune: bool, cx: &mut App) {
    let switched_on = prune.then(|| opted_in_roots(cx));
    with(cx, |u| {
        if let Some(switched_on) = switched_on {
            u.problems
                .retain(|root, _| switched_on.iter().any(|p| &p.root == root));
        }
        for (root, served) in checked {
            match served {
                Err(why) => u.problems.insert(root, why),
                Ok(_) => u.problems.remove(&root),
            };
        }
    });
    cx.refresh_windows();
}

/// Every switched-on project across every window, in rail order, once each.
fn opted_in_roots(cx: &App) -> Vec<Project> {
    let mut roots: Vec<Project> = Vec::new();
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    for shell in shells.iter().filter_map(WeakEntity::upgrade) {
        for project in shell.read(cx).unattended_roots() {
            if !roots.iter().any(|p| p.root == project.root) {
                roots.push(project);
            }
        }
    }
    roots
}

/// The run in progress, as the project it came from and the issue it is on.
///
/// For the rail, which says on a project's row that a run is working one of its
/// issues: the run's own session is on a worktree's row of its own, and nothing
/// on the project the issue belongs to would otherwise say so.
pub fn live_run(cx: &App) -> Option<(PathBuf, u64)> {
    let run = Shared::global(cx).unattended.as_ref()?.run.as_ref()?;
    Some((run.claimed.repo.clone(), run.claimed.issue.number))
}

/// The label that asks for a run, for the places that tell the user which
/// label to put on an issue.
pub fn label(cx: &App) -> String {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| u.label.clone())
        .unwrap_or_default()
}

/// Whether `uid` is a run that cancels a card rather than leave it up: one the
/// search found. A run somebody picked by hand hands the card to them instead.
pub fn cancels_asks(uid: u64, cx: &App) -> bool {
    Shared::global(cx)
        .unattended
        .as_ref()
        .and_then(|u| u.run.as_ref())
        .is_some_and(|run| run.uid == uid && !run.claimed.picked_by_hand())
}

/// Act on the unattended state, if there is one.
fn with<R>(cx: &mut App, act: impl FnOnce(&mut Unattended) -> R) -> Option<R> {
    cx.update_global::<Shared, _>(|shared, _| shared.unattended.as_mut().map(act))
}

/// Look at every switched-on project, and search them for an issue if
/// nothing is running and nothing blocks a run.
///
/// **Every tick looks, even while a run is live.** The rows show what the last
/// look found, and a tick that only looked while it was about to search left
/// them as old as the run was long — and left an account signed in on screen after it
/// had been signed out.
fn tick(asked_from: Option<gpui::AnyWindowHandle>, cx: &mut App) {
    let roots = opted_in_roots(cx);
    // Nothing switched on is nothing to look at and nothing to search, so a
    // tick asks no connector anything at all — the feature costs nobody who has not
    // switched a project on.
    if roots.is_empty() {
        return;
    }
    let search = with(cx, |u| {
        let idle =
            !u.claiming && u.blocked.is_none() && u.mode_refused.is_none() && u.run.is_none();
        idle.then(|| {
            u.claiming = true;
            u.label.clone()
        })
    })
    .flatten();
    cx.spawn(async move |cx| {
        let searching = search.is_some();
        // A panic in there would otherwise leave `claiming` set for the life of
        // the process, and no issue would be looked for again. Said and treated
        // as nothing found.
        let (accounts, checked, begun) = cx
            .background_executor()
            .spawn(async move {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (accounts, mut checked) = look_blocking(roots.clone());
                    // Only the projects that passed the look are searched, each
                    // with the forge the look found for it, if any.
                    let workable: Vec<(Project, Option<&'static dyn Connector>)> = roots
                        .into_iter()
                        .filter_map(|project| {
                            let (_, served) =
                                checked.iter().find(|(root, _)| *root == project.root)?;
                            let forge = *served.as_ref().ok()?;
                            Some((project, forge))
                        })
                        .collect();
                    let begun =
                        search.and_then(|label| begin_blocking(&workable, &label, &mut checked));
                    (Some(accounts), checked, begun)
                }))
                .unwrap_or_else(|_| {
                    // The look, the search and the claim are all in here, and
                    // none of them has taken a label unless it finished.
                    eprintln!("onehand: looking for an unattended run panicked");
                    (None, Vec::new(), None)
                })
            })
            .await;
        cx.update(|cx| {
            if let Some(accounts) = accounts {
                file_accounts(accounts, cx);
                record(checked, true, cx);
            }
            if searching {
                landed(begun, asked_from, cx);
            }
        });
    })
    .detach();
}

/// A claimed issue and the worktree made for it.
struct Claimed {
    /// The project the issue was found in.
    repo: PathBuf,
    /// Where the issue lives: where it was claimed and where it is told how
    /// the run ended.
    tracker: Tracker,
    /// The forge the project's work goes to, or `None` for a project on no
    /// forge, whose work stays on the branch.
    forge: Option<&'static dyn Connector>,
    issue: Issue,
    branch: String,
    /// What the worktree was cut from: the remote's default branch with a
    /// forge, the branch checked out without one. Also what a run with no
    /// forge is measured against.
    base: String,
    /// The worktree the run made, and the project root it is added as.
    dir: PathBuf,
    /// The window a person picked it in, rather than the search finding it.
    /// Such a run is put on screen there as it starts and stays when it ends —
    /// somebody asked for it and is watching — so a card it parks is theirs.
    picked_in: Option<gpui::AnyWindowHandle>,
}

impl Claimed {
    /// Whether a person picked this run rather than the search finding it.
    fn picked_by_hand(&self) -> bool {
        self.picked_in.is_some()
    }
}

/// An issue that was claimed and then could not be started, and where to say
/// so.
struct Unstarted {
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
fn begin_blocking(
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
        let branch = core::free_branch_blocking(&top, &core::branch_for(&issue));
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
/// **Refused while a run is going**: one run at a time is the rule for picked
/// and found alike, and the refusal names the issue already being worked so
/// the person knows what they are waiting on. Anything that stops it before
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
        if let Some(run) = &u.run {
            return Err(format!(
                "An unattended run is already working on issue #{} — one at a time.",
                run.claimed.issue.number
            ));
        }
        if u.claiming {
            return Err("An unattended run is starting — one at a time.".to_string());
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
            if let Some(run) = &u.run {
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
fn landed(
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
    // A run that never started left nothing, whichever kind of nothing.
    let said = core::report(
        &Ending::Failed(unstarted.why),
        &Ok(Verdict::NoPullRequest),
        "",
    );
    cx.background_executor()
        .spawn(
            async move { tell_issue(&unstarted.tracker, &unstarted.repo, unstarted.number, &said) },
        )
        .detach();
}

/// Mint the run's session in the window holding its project, and start
/// watching it.
fn start(claimed: Claimed, cx: &mut App) -> Result<(), Unstarted> {
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
    let Some((uid, session)) = shell
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
    // A cancel already winding down keeps the ending it was heading for; only
    // a session that went with nothing pending is reported as closed.
    let release = cx.observe_release(&session, move |_, cx| {
        let pending = with(cx, |u| {
            u.run
                .as_ref()
                .filter(|run| run.uid == uid)
                .and_then(|run| run.ending.clone())
        })
        .flatten();
        settle(uid, pending.unwrap_or(Ending::Closed), cx)
    });
    let clock = cx.spawn(async move |cx| {
        cx.background_executor().timer(timeout).await;
        cx.update(|cx| cancel_toward(uid, Ending::TimedOut(timeout), cx));
    });
    let (by_hand, opening, shown_in) = (
        claimed.picked_by_hand(),
        start_notes(&claimed),
        shell.clone(),
    );
    with(cx, |u| {
        u.run = Some(Run {
            claimed,
            uid,
            session: session.downgrade(),
            window,
            shell,
            prompted: false,
            ending: None,
            _watch: watch,
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
            shell.update(cx, |shell, cx| shell.show_unattended(uid, window, cx))
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

/// One event from the run's session.
fn on_event(uid: u64, session: &Entity<ChatSession>, event: &ChatEvent, cx: &mut App) {
    let Some((prompted, cancelling, shell, picked)) = with(cx, |u| {
        u.run.as_ref().filter(|run| run.uid == uid).map(|run| {
            (
                run.prompted,
                run.ending.is_some(),
                run.shell.clone(),
                run.claimed.picked_by_hand(),
            )
        })
    })
    .flatten() else {
        return;
    };
    match event {
        ChatEvent::Appended if !prompted => prompt(uid, session, cx),
        ChatEvent::Appended => {
            if prompted_by_someone_else(session, true, cx) {
                settle(uid, Ending::TakenOver { asked: None }, cx);
            }
        }
        ChatEvent::TurnEnded if cancelling => settle_pending(uid, cx),
        ChatEvent::TurnEnded => {
            let tail = shell
                .upgrade()
                .and_then(|s| s.read(cx).answer_tail(uid, cx));
            settle(uid, Ending::TurnEnded { tail }, cx);
        }
        ChatEvent::AwaitingUser(ask) => {
            // **Somebody already looking is the person the card asks** — and
            // so is whoever picked the run by hand, looking or not: they asked
            // for it moments ago and are near, and a card left up for them
            // (announced like any other) costs a wait where cancelling cost
            // the run. The run never answers a card; handing it over is not
            // answering it.
            let question = question(*ask, session, cx);
            if shell.upgrade().is_some_and(|s| s.read(cx).reading(uid, cx)) {
                settle(uid, Ending::TakenOver { asked: None }, cx);
            } else if picked {
                // Not being read, so the issue is told what is waiting — the
                // person may answer it from somewhere else entirely.
                settle(
                    uid,
                    Ending::TakenOver {
                        asked: Some(question),
                    },
                    cx,
                );
            } else {
                cancel_toward(uid, Ending::Asked(question), cx);
            }
        }
        ChatEvent::Disconnected if cancelling => settle_pending(uid, cx),
        ChatEvent::Disconnected => settle(uid, Ending::LinkLost, cx),
        ChatEvent::OpenFile(_) => {}
    }
}

/// Send the run's one prompt, once the adapter is up.
///
/// Not before: setting the mode and submitting both need the request channel
/// the handshake installs. The modes arrive ahead of that same event, so by
/// the time the link is up the mode can be checked against what is offered.
fn prompt(uid: u64, session: &Entity<ChatSession>, cx: &mut App) {
    if session.read(cx).chat.link != onehand_core::chat::Link::Connected {
        return;
    }
    // Somebody got there first: a prompt typed between the adapter coming up
    // and this one going out makes the session theirs, and the run's own
    // prompt would either be refused as busy or land on top of their work.
    if session.read(cx).chat.busy || prompted_by_someone_else(session, false, cx) {
        settle(uid, Ending::TakenOver { asked: None }, cx);
        return;
    }
    let Some((mode, text)) = with(cx, |u| {
        let run = u.run.as_mut()?;
        run.prompted = true;
        Some((
            u.mode.clone(),
            core::prompt_for(
                &run.claimed.issue,
                &run.claimed.branch,
                &run.claimed.tracker,
                run.claimed.forge,
            ),
        ))
    })
    .flatten() else {
        return;
    };
    let offered: Vec<String> = session
        .read(cx)
        .chat
        .modes
        .iter()
        .map(|m| m.id.clone())
        .collect();
    if !offered.contains(&mode) {
        // Every run would fail this way, each on a fresh issue, so the tick
        // stops here rather than working through the backlog to say it.
        let why = format!(
            "the agent offers no mode `{mode}` (it offers: {}). Unattended runs are paused \
             until unattended.mode is fixed and onehand restarted.",
            offered.join(", ")
        );
        with(cx, |u| u.mode_refused = Some(why.clone()));
        eprintln!("onehand: {why}");
        settle(uid, Ending::Failed(why), cx);
        return;
    }
    let sent = session.update(cx, |session, cx| {
        session.chat.set_mode(&mode);
        session.submit(&text, &[], cx)
    });
    if sent {
        note(
            session,
            format!("Mode {mode} set; issue sent as the prompt"),
            cx,
        );
    } else {
        settle(
            uid,
            Ending::Failed("the agent did not accept the prompt".to_string()),
            cx,
        );
    }
}

/// What the run's transcript opens with: which issue, how it was chosen, and
/// where the work is happening. The transcript is the record of the run, and
/// without these lines it would start at a prompt nobody on screen typed.
///
/// **Short lines, one fact each.** A remark in the transcript is one line down
/// the middle of the column and is cut where the column ends, so a sentence
/// carrying the issue, the folder and the branch lost all but the first.
fn start_notes(claimed: &Claimed) -> Vec<String> {
    let folder = claimed
        .dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| claimed.dir.display().to_string());
    vec![
        format!(
            "Unattended run on issue #{}, {}",
            claimed.issue.number,
            if claimed.picked_by_hand() {
                "picked by hand"
            } else {
                "found by its label"
            }
        ),
        format!("Branch {} from {}", claimed.branch, claimed.base),
        format!("Working in {folder}"),
    ]
}

/// Add a line about the run to its session's transcript.
fn note(session: &Entity<ChatSession>, text: String, cx: &mut App) {
    session.update(cx, |session, cx| {
        session.chat.items.push(ChatItem::notice(text));
        cx.emit(ChatEvent::Appended);
        cx.notify();
    });
}

/// Whether somebody other than the run has put a prompt into the session,
/// given whether the run has sent its own one yet.
///
/// Any other prompt sent, or one waiting behind the turn, came from the
/// composer or the remote bridge — and either way a person is driving. Counted
/// from what was *sent*, not from the user rows in the transcript: an adapter
/// delivers user chunks of its own mid-turn, and reading those as prompts took
/// runs over that nobody had touched.
fn prompted_by_someone_else(session: &Entity<ChatSession>, run_prompted: bool, cx: &App) -> bool {
    let chat = &session.read(cx).chat;
    chat.queued.is_some() || chat.prompts_sent > usize::from(run_prompted)
}

/// What the parked card asks, in its own words.
fn question(ask: UserAsk, session: &Entity<ChatSession>, cx: &App) -> String {
    let chat = &session.read(cx).chat;
    let asked = match ask {
        UserAsk::Permission => chat
            .pending_permissions()
            .last()
            .map(|(_, p)| p.req.title.clone()),
        UserAsk::Question => chat
            .pending_asks()
            .last()
            .map(|(_, a)| a.req.message.clone()),
    };
    asked.unwrap_or_else(|| ask.headline("The agent"))
}

/// Cancel the run's turn and end as `ending` once it has wound down.
///
/// A session that is already gone — its window closed under it — has nothing
/// to wind down, and is settled on the spot.
fn cancel_toward(uid: u64, ending: Ending, cx: &mut App) {
    let Some(session) = with(cx, |u| {
        let run = u
            .run
            .as_mut()
            .filter(|run| run.uid == uid && run.ending.is_none())?;
        run.ending = Some(ending.clone());
        Some(run.session.clone())
    })
    .flatten() else {
        return;
    };
    let Some(session) = session.upgrade() else {
        settle(uid, ending, cx);
        return;
    };
    session.update(cx, |session, cx| {
        session.chat.cancel_turn();
        cx.notify();
    });
    let why = match &ending {
        Ending::TimedOut(_) => "the run timed out",
        Ending::Asked(_) => "a question nobody is here to answer",
        _ => "the run is ending",
    };
    note(&session, format!("Cancelling the turn: {why}"), cx);
    let wind_down = cx.spawn(async move |cx| {
        cx.background_executor().timer(WIND_DOWN).await;
        cx.update(|cx| settle_pending(uid, cx));
    });
    with(cx, |u| {
        if let Some(run) = u.run.as_mut().filter(|run| run.uid == uid) {
            run._clock = wind_down;
        }
    });
}

/// Settle as whatever the cancel was heading toward.
fn settle_pending(uid: u64, cx: &mut App) {
    let pending = with(cx, |u| {
        u.run
            .as_ref()
            .filter(|run| run.uid == uid)
            .and_then(|run| run.ending.clone())
    })
    .flatten();
    if let Some(ending) = pending {
        settle(uid, ending, cx);
    }
}

/// End the run: close or keep its session, then tell the issue.
///
/// **Deferred**, because this is reached from inside the session's own event
/// and closing the session drops the subscription that event is being
/// delivered through.
fn settle(uid: u64, ending: Ending, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(run) = with(cx, |u| u.run.take_if(|run| run.uid == uid)).flatten() else {
            return;
        };
        cx.refresh_windows();
        let Run {
            claimed,
            window,
            shell,
            session,
            ..
        } = run;
        // A run picked by hand stays where the person watching it can read how
        // it ended, and is kept for good like a taken-over one — somebody who
        // watched it end may well carry on in it, and a project that vanished
        // at the next launch would take their place in it too. Only a found
        // run is taken down.
        let keep = matches!(ending, Ending::TakenOver { .. }) || claimed.picked_by_hand();
        if let Some(shell) = shell.upgrade() {
            let _ = window.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| match keep {
                    true => shell.adopt_unattended(&claimed.dir, window, cx),
                    false => shell.end_unattended(&claimed.dir, window, cx),
                })
            });
        }
        cx.spawn(async move |cx| {
            let line = cx
                .background_executor()
                .spawn(async move {
                    let found = verdict_blocking(&claimed, &ending);
                    let said = core::report(&ending, &found, &claimed.branch);
                    tell_issue(&claimed.tracker, &claimed.repo, claimed.issue.number, &said);
                    core::outcome_line(&ending, &found)
                })
                .await;
            // How it ended, in the one line a remark gets, as the transcript's
            // last — where the session is still there to carry it. The whole
            // account is the comment on the issue.
            cx.update(|cx| {
                if let Some(session) = session.upgrade() {
                    note(&session, line, cx);
                }
            });
        })
        .detach();
    });
}

/// What the run left: the pull request on its branch where the project has a
/// forge, its commits past where it started where it has none. Blocking.
fn verdict_blocking(claimed: &Claimed, ending: &Ending) -> Result<Verdict, String> {
    if !ending.may_have_work() {
        return Ok(match claimed.forge {
            Some(_) => Verdict::NoPullRequest,
            None => Verdict::NoCommits,
        });
    }
    match claimed.forge {
        Some(forge) => forge
            .pull_request_for_blocking(&claimed.repo, &claimed.branch)
            .map(|pr| pr.map_or(Verdict::NoPullRequest, Verdict::PullRequest)),
        None => worktree::commits_since_blocking(&claimed.dir, &claimed.base).map(|n| match n {
            0 => Verdict::NoCommits,
            n => Verdict::Commits(n),
        }),
    }
}

/// Leave `body` on issue `number`, saying on stderr if that failed — there is
/// nowhere else left to say it.
fn tell_issue(tracker: &Tracker, repo: &std::path::Path, number: u64, body: &str) {
    if let Err(why) = tracker.comment_blocking(repo, number, body) {
        eprintln!("onehand: could not comment on issue #{number}: {why}");
    }
}
