//! Unattended runs, as the app drives them: the tick, the claim, the cap, and
//! what happens once a run's task ends.
//!
//! An issue is worked as a task of the configured workflow, driven like any
//! other (`crate::task`). Everything that can be decided without a window —
//! which issue, what branch, what the run is asked, what the issue is told — is
//! `onehand_core::unattended`. What is here is what needs an entity, a window
//! or a timer.
//!
//! **One per process, on [`Shared`]**, for the reason the remote bridge is
//! there: two windows each running a tick would be two agents on one issue. The
//! tick therefore belongs to no window and asks each in turn for its projects.

use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Task, WeakEntity};
use onehand_core::config::UnattendedConfig;
use onehand_core::connector::{self, Connector};
use onehand_core::unattended as core;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

mod launch;
mod report;
pub use launch::{Pickable, look_now, pickable_blocking, start_picked};
use launch::{begin_blocking, landed};
pub(crate) use report::{
    card_question, deliver, deliver_all, ended, keep, opening, refuse_mode, spec_for, started,
};

/// The unattended half of the process: its settings, the claim in flight and
/// the tick. The runs themselves are tasks, kept with every other.
pub struct Unattended {
    label: String,
    /// How long a run may work, put over the workflow's own timeout.
    timeout: String,
    mode: String,
    agent: Option<String>,
    /// The id of the workflow an issue is worked with.
    workflow: String,
    /// How many runs may work at once.
    at_once: u32,
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
    /// Tasks whose reports are on their way to their issue, so a second
    /// delivery does not send them twice.
    delivering: HashSet<String>,
    /// Issue tasks kept but not yet waiting for or holding a place: counted
    /// against the cap meanwhile, so a tick landing in between cannot start
    /// one more.
    starting: HashSet<String>,
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

/// Why no more runs may start while these work: the cap is reached, said
/// with the issues it is waiting on. A run waiting on a person is not
/// counted, so one question nobody has answered yet does not stop every other
/// issue from being worked; one whose card is answered carries on beside
/// whatever started meanwhile, since the protocol cannot hold an agent
/// mid-turn.
// ponytail: waiting runs are not capped; each keeps an adapter alive. Cap
// them when a pile of unanswered runs is seen to cost something.
fn at_cap(u: &Unattended, cx: &App) -> Option<String> {
    let working = crate::task::issues_working(cx);
    if core::room(working.len() + u.starting.len(), u.at_once) {
        return None;
    }
    Some(match working.as_slice() {
        [] if u.at_once == 0 => {
            "Unattended runs are capped at none at once (unattended.at_once).".to_string()
        }
        [] => "An unattended run is starting.".to_string(),
        [one] => format!(
            "An unattended run is already working on issue {one} — {} at a time.",
            u.at_once
        ),
        many => format!(
            "Unattended runs are already working on issues {} — {} at a time.",
            many.join(", "),
            u.at_once
        ),
    })
}

/// Why nothing at all may start, picked or found: the agent does not offer
/// the mode runs start in, or the workflow they run is missing or cannot run.
fn cannot_start(u: &Unattended, cx: &App) -> Option<String> {
    u.mode_refused
        .clone()
        .or_else(|| launch::workflow(&u.workflow, &u.timeout, cx).err())
        .or_else(|| {
            spec_for(u.agent.as_deref(), cx)
                .is_none()
                .then(|| "no agent is configured to run it".to_string())
        })
}

/// Why the project at `root` cannot be worked by the workflow runs use, when
/// that workflow runs the project's check command and the project has none.
/// Asked before a claim, so no issue is claimed for a run that cannot start.
fn lacks_check(root: &Path, cx: &App) -> Option<String> {
    let u = Shared::global(cx).unattended.as_ref()?;
    let template = launch::workflow(&u.workflow, &u.timeout, cx).ok()?;
    let has = Shared::global(cx)
        .windows
        .iter()
        .filter_map(|w| w.shell.upgrade())
        .any(|shell| shell.read(cx).check_of(root).is_some());
    (template.needs_check() && !has).then(|| {
        format!(
            "the workflow `{}` runs the project's check command, and it has none; set one \
             under Settings ▸ Workflows",
            template.name
        )
    })
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
    let timeout = core::parse_every(&cfg.timeout).map(|_| cfg.timeout.clone());
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
            timeout: timeout.unwrap_or(fallback.timeout),
            mode: cfg.mode.clone(),
            agent: cfg.agent.clone(),
            workflow: cfg.workflow.clone(),
            at_once: cfg.at_once,
            claiming: false,
            blocked,
            mode_refused: None,
            delivering: HashSet::new(),
            starting: HashSet::new(),
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
    u.blocked.clone().or_else(|| cannot_start(u, cx))
}

/// What stops a run being picked now, if anything: the cap reached, then
/// anything that stops every run.
fn why_not(cx: &App) -> (Option<String>, Option<String>) {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| (at_cap(u, cx), cannot_start(u, cx)))
        .unwrap_or_default()
}

/// Why issue task `id` may not start now: the cap is reached. A task the
/// tick or a pick has just kept is counted already, and one already queued or
/// running holds its slot; every other start of an issue's task, a Resume or
/// a Retry, is held to the cap like the tick.
pub(crate) fn over_cap(id: &str, cx: &App) -> Option<String> {
    let u = Shared::global(cx).unattended.as_ref()?;
    let issue = crate::task::task(id, cx).is_some_and(|task| task.issue().is_some());
    if !issue || u.starting.contains(id) || crate::task::is_working(id, cx) {
        return None;
    }
    at_cap(u, cx)
}

/// Issue task `id` now waits for its place or holds it, and counts against
/// the cap as such.
pub(crate) fn placed(id: &str, cx: &mut App) {
    with(cx, |u| u.starting.remove(id));
}

/// A run of issue task `id` now waits on a person and gives its place under
/// the cap up: look for the next issue at once rather than at the next tick.
pub(crate) fn waiting(id: &str, cx: &mut App) {
    if crate::task::task(id, cx).is_some_and(|task| task.issue().is_some()) {
        tick(None, cx);
    }
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

/// The label that asks for a run, for the places that tell the user which
/// label to put on an issue.
pub fn label(cx: &App) -> String {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| u.label.clone())
        .unwrap_or_default()
}

/// Act on the unattended state, if there is one.
fn with<R>(cx: &mut App, act: impl FnOnce(&mut Unattended) -> R) -> Option<R> {
    cx.update_global::<Shared, _>(|shared, _| shared.unattended.as_mut().map(act))
}

/// Look at every switched-on project, and search them for an issue if no run
/// is working and nothing blocks a run.
///
/// **Every tick looks, even while a run is live.** The rows show what the last
/// look found, and a tick that only looked while it was about to search left
/// them as old as the run was long — and left an account signed in on screen after it
/// had been signed out.
fn tick(asked_from: Option<gpui::AnyWindowHandle>, cx: &mut App) {
    // A report that could not reach its issue is tried again at every tick,
    // switched on or not: an issue picked by hand needs no switch, and its
    // report must not wait for one.
    deliver_all(cx);
    let roots = opted_in_roots(cx);
    // Nothing switched on is nothing to look at and nothing to search, so a
    // tick asks no connector anything at all — the feature costs nobody who has not
    // switched a project on.
    if roots.is_empty() {
        return;
    }
    let stopped = Shared::global(cx)
        .unattended
        .as_ref()
        .and_then(|u| at_cap(u, cx).or_else(|| cannot_start(u, cx)));
    let lacking: Vec<(PathBuf, String)> = roots
        .iter()
        .filter_map(|p| Some((p.root.clone(), lacks_check(&p.root, cx)?)))
        .collect();
    let earlier = launch::earlier(cx);
    let search = with(cx, |u| {
        let idle = !u.claiming && u.blocked.is_none() && stopped.is_none();
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
                            if let Some((root, why)) =
                                lacking.iter().find(|(root, _)| *root == project.root)
                            {
                                checked.push((root.clone(), Err(why.clone())));
                                return None;
                            }
                            let (_, served) =
                                checked.iter().find(|(root, _)| *root == project.root)?;
                            let forge = *served.as_ref().ok()?;
                            Some((project, forge))
                        })
                        .collect();
                    let begun = search.and_then(|label| {
                        begin_blocking(&workable, &label, &earlier, &mut checked)
                    });
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
