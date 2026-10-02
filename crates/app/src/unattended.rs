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

use crate::chat::session::ChatSession;
use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Subscription, Task, WeakEntity};
use onehand_core::config::UnattendedConfig;
use onehand_core::connector::PullRequest;
use onehand_core::connector::{self, Connector};
use onehand_core::unattended::{
    self as core, Budget, Checked, Ending, Failure, Phase, Progress, Spent, Start, Step, Tracker,
};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

pub use onehand_plugin_host::IssueRun;

mod launch;
mod turn;
use launch::{Claimed, begin_blocking, landed, save_record, tell_unstarted};
pub use launch::{Pickable, look_now, pickable_blocking, start_picked};

/// How long a cancelled turn is given to wind down before the run is settled
/// anyway. Cancelling asks the adapter to end the turn, and the turn ending is
/// what writes its transcript — closing the session straight away would lose
/// the one turn the run was about.
const WIND_DOWN: Duration = Duration::from_secs(30);

/// The unattended half of the process: its settings, the live runs and the tick.
pub struct Unattended {
    label: String,
    timeout: Duration,
    mode: String,
    agent: Option<String>,
    /// How many turns of one session may fail their step's gate.
    turns: u32,
    /// How many repairs one attempt may start.
    repairs: u32,
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
    /// Every run that has not ended. A run waiting on a person to answer a
    /// card holds its issue and its session but not the slot, so one question
    /// nobody has answered yet does not stop every other issue from being
    /// worked. **A run is only started while none is working**, but one whose
    /// card is answered carries on beside whatever started meanwhile: its turn
    /// is already under way, and the protocol has no way to hold an agent
    /// mid-turn — refusing would mean cancelling the work the answer was for.
    // ponytail: waiting runs are not capped; each keeps an adapter alive. Cap
    // them when a pile of unanswered runs is seen to cost something.
    runs: Vec<Run>,
    /// Runs with no session: waiting on their pull request's checks, or due a
    /// session — a repair, or one carried on after a restart — once the slot
    /// is free.
    parked: Vec<Parked>,
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
    /// Its runs wait for a person to approve their plan.
    pub approve_plans: bool,
    /// The command its runs' work must pass, run by onehand.
    pub check: Option<String>,
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
    /// The run added the project the session is on, and drops it again.
    owns_root: bool,
    /// How many prompts the run itself has sent the session: any more, and a
    /// person is driving.
    sent: usize,
    /// Turns of the session that failed their step's gate.
    missed: u32,
    /// Where the worktree stood when the plan's session began, as its head
    /// and whether it was dirty, so the plan's gate measures only the plan.
    plan_from: Option<(String, bool)>,
    /// What the session is for and what the attempt has used.
    progress: Progress,
    /// The question a parked card is asking, while the run waits for a person
    /// to answer it. The run never answers a card itself.
    waiting: Option<String>,
    /// How much of the timeout is left; it does not run while waiting.
    budget: Budget,
    /// The turn has been cancelled, and this is what the run ends as once it
    /// has wound down.
    ending: Option<Ending>,
    _watch: Subscription,
    /// Watches for the run's cards being answered. An answer changes the
    /// transcript and says nothing else, so waiting for the agent's next event
    /// instead would leave a run whose adapter went quiet waiting forever,
    /// with no clock to end it.
    _answered: Subscription,
    /// Fires if the session goes without saying so — its window closed under
    /// it — so the run settles now rather than holding the tick until its
    /// timeout and then reporting the wrong ending.
    _release: Subscription,
    /// The run's timeout, and after a cancel the wind-down in its place.
    /// Empty while the run waits on a person.
    _clock: Task<()>,
}

/// A run with no session.
struct Parked {
    claimed: Claimed,
    phase: Phase,
    progress: Progress,
}

impl Unattended {
    /// Every issue a run has not ended on, by project.
    fn busy(&self) -> Vec<(PathBuf, u64)> {
        let runs = self.runs.iter().map(|run| &run.claimed);
        let parked = self.parked.iter().map(|p| &p.claimed);
        runs.chain(parked)
            .map(|c| (c.repo.clone(), c.issue.number))
            .collect()
    }

    /// The run on session `uid`, if it has not ended.
    fn run_mut(&mut self, uid: u64) -> Option<&mut Run> {
        self.runs.iter_mut().find(|run| run.uid == uid)
    }

    /// A run holding the slot: working rather than waiting.
    fn working(&self) -> Option<&Run> {
        self.runs.iter().find(|run| run.waiting.is_none())
    }
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
            turns: cfg.turns.max(1),
            repairs: cfg.repairs,
            claiming: false,
            blocked,
            mode_refused: None,
            runs: Vec::new(),
            parked: Vec::new(),
            accounts: None,
            checked_at: None,
            checks_out: 0,
            problems: HashMap::new(),
            _tick: tick,
        });
    });
    carry_on_runs(cx);
}

/// Read back every run a previous onehand left unfinished, and park them:
/// those waiting on checks go on waiting, those that were working get a
/// session once a window holding their project is open.
///
/// **A run is never read back twice**: its file is the run, and is only
/// removed once the issue has been told how it ended.
fn carry_on_runs(cx: &mut App) {
    cx.spawn(async move |cx| {
        let found = cx
            .background_executor()
            .spawn(async move { core::load_records_blocking(&core::runs_dir()) })
            .await;
        cx.update(|cx| {
            for (file, read) in found {
                let record = match read {
                    Ok(record) => record,
                    Err(why) => {
                        eprintln!(
                            "onehand: an unattended run's file {} is unreadable: {why}",
                            file.display()
                        );
                        continue;
                    }
                };
                let (phase, mut progress) = (record.phase, record.progress.clone());
                let Some(claimed) = Claimed::from_record(file, record) else {
                    eprintln!("onehand: an unattended run names a connector this onehand lacks");
                    continue;
                };
                // A session cut short leaves whatever it did on the branch,
                // perhaps uncommitted; the next one is told to look first.
                if progress.start == Start::Fresh {
                    progress.start = Start::Earlier;
                }
                with(cx, |u| {
                    u.parked.push(Parked {
                        claimed,
                        phase,
                        progress,
                    })
                });
            }
            resume_parked(cx);
        });
    })
    .detach();
}

/// Give the next parked run that is due a session one, if the slot is free and
/// a window holds its project. One that cannot start is told so on its issue.
pub fn resume_parked(cx: &mut App) {
    let held: Vec<PathBuf> = with(cx, |u| {
        u.parked
            .iter()
            .filter(|p| p.phase == Phase::Working)
            .map(|p| p.claimed.repo.clone())
            .collect()
    })
    .unwrap_or_default();
    let held: Vec<PathBuf> = held
        .into_iter()
        .filter(|repo| {
            Shared::global(cx).windows.iter().any(|w| {
                w.shell
                    .upgrade()
                    .is_some_and(|s| s.read(cx).holds_root(repo))
            })
        })
        .collect();
    let next = with(cx, |u| {
        let free =
            !u.claiming && u.blocked.is_none() && u.mode_refused.is_none() && u.working().is_none();
        let at = u
            .parked
            .iter()
            .position(|p| p.phase == Phase::Working && held.contains(&p.claimed.repo))
            .filter(|_| free)?;
        Some(u.parked.remove(at))
    })
    .flatten();
    if let Some(Parked {
        claimed, progress, ..
    }) = next
        && let Err(unstarted) = launch::start(claimed, progress, cx)
    {
        tell_unstarted(unstarted, cx);
    }
}

/// A run waiting on its pull request's checks, as the tick looks at it.
struct Awaiting {
    file: PathBuf,
    repo: PathBuf,
    branch: String,
    forge: &'static dyn Connector,
    since: u64,
    repairs: u32,
    failed_before: Vec<String>,
}

/// What a look at a waiting run's pull request found.
enum Polled {
    Wait,
    End(Ending),
    /// Start a repair session for this, failing these checks.
    Repair {
        start: Start,
        failing: Vec<String>,
    },
}

/// How many failed checks' logs a repair is handed. The rest are named.
const LOGS_MAX: usize = 3;

/// Look at each waiting run's pull request and decide. Blocking.
///
/// A run whose pull request could not be read waits for the next tick: one
/// failed call to the forge is not a verdict on the run.
fn poll_blocking(
    awaiting: Vec<Awaiting>,
    timeout: Duration,
    repairs: u32,
) -> Vec<(PathBuf, Polled)> {
    awaiting
        .into_iter()
        .map(|a| {
            let pr = match a.forge.pull_request_for_blocking(&a.repo, &a.branch) {
                Ok(pr) => pr,
                Err(why) => {
                    eprintln!(
                        "onehand: could not look at {}'s pull request: {why}",
                        a.branch
                    );
                    return (a.file, Polled::Wait);
                }
            };
            let waited =
                Duration::from_secs(onehand_core::chat::store::now_secs().saturating_sub(a.since));
            let checked = core::after_checks(
                pr.as_ref(),
                waited,
                timeout,
                repairs.saturating_sub(a.repairs),
                a.repairs,
                &a.failed_before,
            );
            let polled = match (checked, pr) {
                (Checked::Wait, _) => Polled::Wait,
                (Checked::Gone, _) => Polled::End(Ending::PullRequestGone),
                (Checked::Exhausted(spent), _) => Polled::End(Ending::Exhausted(spent)),
                (Checked::Ready { ran }, pr) => ready_blocking(&a, pr.as_ref(), ran),
                (
                    Checked::Repair {
                        failing,
                        conflicting,
                    },
                    Some(pr),
                ) => Polled::Repair {
                    start: Start::Repair {
                        failing: logs_blocking(&a, &pr, &failing),
                        pr: pr.url,
                        conflicting,
                    },
                    failing,
                },
                (Checked::Repair { .. }, None) => Polled::Wait,
            };
            (a.file, polled)
        })
        .collect()
}

/// Take a pull request whose checks passed out of draft. One the forge would
/// not take out waits for the next look rather than being called ready.
fn ready_blocking(a: &Awaiting, pr: Option<&PullRequest>, ran: bool) -> Polled {
    if let Some(pr) = pr.filter(|pr| pr.draft)
        && let Err(why) = a.forge.mark_ready_blocking(&a.repo, pr.number)
    {
        eprintln!("onehand: could not mark {} ready for review: {why}", pr.url);
        return Polled::Wait;
    }
    Polled::End(Ending::Ready { checks_ran: ran })
}

/// The end of each failing check's log, or why there is none.
fn logs_blocking(a: &Awaiting, pr: &PullRequest, failing: &[String]) -> Vec<Failure> {
    pr.checks
        .iter()
        .filter(|c| failing.contains(&c.name))
        .take(LOGS_MAX)
        .map(|c| Failure {
            check: c.name.clone(),
            log: a
                .forge
                .check_log_blocking(&a.repo, c)
                .unwrap_or_else(|why| format!("(no log: {why})")),
        })
        .collect()
}

/// Act on what the looks found: end the runs that are over, and mark the ones
/// due a repair as wanting a session.
fn apply_polls(polled: Vec<(PathBuf, Polled)>, cx: &mut App) {
    for (file, polled) in polled {
        match polled {
            Polled::Wait => {}
            Polled::End(ending) => {
                let parked = with(cx, |u| {
                    let at = u.parked.iter().position(|p| p.claimed.file == file)?;
                    Some(u.parked.remove(at))
                })
                .flatten();
                if let Some(parked) = parked {
                    turn::conclude(parked.claimed, ending, None, cx);
                }
            }
            Polled::Repair { start, failing } => {
                let record = with(cx, |u| {
                    let p = u.parked.iter_mut().find(|p| p.claimed.file == file)?;
                    p.phase = Phase::Working;
                    p.progress.start = start;
                    // What to fix is in the failing checks, so a repair has
                    // no plan to make or follow.
                    p.progress.step = Step::Implement;
                    p.progress.plan = None;
                    p.progress.repairs += 1;
                    p.progress.failed_before = failing;
                    Some(p.claimed.record(p.phase, &p.progress))
                })
                .flatten();
                if let Some(record) = record {
                    save_record(file, record, cx);
                }
            }
        }
    }
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

/// A run that has not ended, as the rail and the workspace page read it.
pub struct LiveRun {
    /// The project the issue was found in.
    pub repo: PathBuf,
    /// How its issue is shown: the forge's number, or *Draft*.
    pub name: String,
    pub title: String,
    /// The run's own session.
    pub uid: u64,
    /// The window the session is in.
    pub window: gpui::AnyWindowHandle,
    /// The question a parked card is asking, while the run waits on it.
    pub waiting: Option<String>,
    /// The step the run is in.
    pub step: Step,
}

/// Every run that has not ended, oldest first.
///
/// The rail says on a project's row that a run is working one of its issues:
/// the run's own session is on a worktree's row of its own, and nothing on the
/// project the issue belongs to would otherwise say so. The workspace page
/// lists them, the waiting ones apart from the working.
pub fn live_runs(cx: &App) -> Vec<LiveRun> {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| {
            u.runs
                .iter()
                .map(|run| LiveRun {
                    repo: run.claimed.repo.clone(),
                    name: run.claimed.tracker.shown(&run.claimed.issue),
                    title: run.claimed.issue.title_text().to_string(),
                    uid: run.uid,
                    window: run.window,
                    waiting: run.waiting.clone(),
                    step: run.progress.step,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A run whose plan waits for a person to approve it, as the rail and the
/// workspace page read it. It has no session, so [`live_runs`] does not list
/// it.
pub struct Approval {
    /// The project the issue was found in.
    pub repo: PathBuf,
    /// How its issue is shown: the forge's number, or *Draft*.
    pub name: String,
    pub title: String,
    /// The number its issue is filed under where it lives.
    pub number: u64,
    /// Its issue is kept in onehand, so the Issues panel lists it and can
    /// approve it.
    pub kept_here: bool,
}

/// Every run waiting for its plan to be approved, oldest first.
pub fn awaiting_approval(cx: &App) -> Vec<Approval> {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| {
            u.parked
                .iter()
                .filter(|p| matches_approval(p.phase))
                .map(|p| {
                    let c = &p.claimed;
                    Approval {
                        repo: c.repo.clone(),
                        name: c.tracker.shown(&c.issue),
                        title: c.issue.title_text().to_string(),
                        number: c.issue.number,
                        kept_here: match c.tracker {
                            Tracker::Local(_) | Tracker::Synced { .. } => true,
                            Tracker::Forge(_) => false,
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a run in `phase` waits for its plan to be approved.
fn matches_approval(phase: Phase) -> bool {
    match phase {
        Phase::AwaitingApproval { .. } => true,
        Phase::Working | Phase::AwaitingChecks { .. } => false,
    }
}

/// Every run that has not ended on an issue kept here, for the Issues panel:
/// working, waiting on a card, on its checks or on approval.
pub fn runs_by_issue(cx: &App) -> Vec<IssueRun> {
    let Some(u) = Shared::global(cx).unattended.as_ref() else {
        return Vec::new();
    };
    let live = u
        .runs
        .iter()
        .map(|run| (&run.claimed, run.progress.step, false));
    let parked = u
        .parked
        .iter()
        .map(|p| (&p.claimed, p.progress.step, matches_approval(p.phase)));
    // Only issues kept here: the Issues panel lists those, by the number
    // onehand files them under, which a forge's issue does not share.
    live.chain(parked)
        .filter(|(c, ..)| match c.tracker {
            Tracker::Local(_) | Tracker::Synced { .. } => true,
            Tracker::Forge(_) => false,
        })
        .map(|(c, step, awaiting)| IssueRun {
            repo: c.repo.clone(),
            number: c.issue.number,
            step,
            awaiting,
        })
        .collect()
}

/// Approve the plan of the run on issue `number` in `repo`: it goes on to the
/// change in a session of its own, carrying the plan.
pub fn approve(repo: &Path, number: u64, cx: &mut App) {
    unpark_plan(repo, number, Step::Implement, None, cx);
}

/// Send the plan of the run on issue `number` in `repo` back to be written
/// again, with what a person asked to change in it.
pub fn revise(repo: &Path, number: u64, note: String, cx: &mut App) {
    unpark_plan(repo, number, Step::Plan, Some(note), cx);
}

/// Make the run on issue `number` waiting for approval due a session again, at
/// `step`, and give it one if the slot is free.
fn unpark_plan(repo: &Path, number: u64, step: Step, revise: Option<String>, cx: &mut App) {
    let found = with(cx, |u| {
        let p = u.parked.iter_mut().find(|p| {
            p.claimed.repo == repo && p.claimed.issue.number == number && matches_approval(p.phase)
        })?;
        p.phase = Phase::Working;
        p.progress.step = step;
        p.progress.revise = revise;
        Some((
            p.claimed.file.clone(),
            p.claimed.record(p.phase, &p.progress),
            p.claimed.tracker.clone(),
        ))
    })
    .flatten();
    let Some((file, record, tracker)) = found else {
        return;
    };
    save_record(file, record, cx);
    let said = format!("Step: {}", step.label());
    cx.background_executor()
        .spawn(async move {
            if let Err(why) = tracker.note_blocking(number, &said) {
                eprintln!("onehand: could not note a step on issue #{number}: {why}");
            }
        })
        .detach();
    cx.refresh_windows();
    resume_parked(cx);
}

/// End every run whose plan has waited for approval longer than the timeout.
fn expire_approvals(cx: &mut App) {
    let now = onehand_core::chat::store::now_secs();
    let expired = with(cx, |u| {
        let timeout = u.timeout;
        let (gone, kept): (Vec<Parked>, Vec<Parked>) =
            std::mem::take(&mut u.parked).into_iter().partition(|p| {
                let Phase::AwaitingApproval { since } = p.phase else {
                    return false;
                };
                Duration::from_secs(now.saturating_sub(since)) >= timeout
            });
        u.parked = kept;
        (gone, timeout)
    });
    let Some((gone, timeout)) = expired else {
        return;
    };
    for parked in gone {
        turn::conclude(
            parked.claimed,
            Ending::Exhausted(Spent::Unapproved(timeout)),
            None,
            cx,
        );
    }
}

/// The session mode an issue's sessions start in: a run's, and one started on
/// an issue from its Issues tab.
pub fn mode(cx: &App) -> String {
    Shared::global(cx)
        .unattended
        .as_ref()
        .map(|u| u.mode.clone())
        .unwrap_or_default()
}

/// Start the next issue's sessions in `mode`. A mode the agent refused no
/// longer stops the runs: the next one finds out whether this one is offered.
pub fn set_mode(mode: String, cx: &mut App) {
    with(cx, |u| {
        u.mode = mode;
        u.mode_refused = None;
    });
    cx.refresh_windows();
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
    expire_approvals(cx);
    let roots = opted_in_roots(cx);
    let (awaiting, timeout, repairs) = with(cx, |u| {
        let awaiting: Vec<Awaiting> = u
            .parked
            .iter()
            .filter_map(|p| {
                let Phase::AwaitingChecks { since } = p.phase else {
                    return None;
                };
                Some(Awaiting {
                    file: p.claimed.file.clone(),
                    repo: p.claimed.repo.clone(),
                    branch: p.claimed.branch.clone(),
                    forge: p.claimed.forge?,
                    since,
                    repairs: p.progress.repairs,
                    failed_before: p.progress.failed_before.clone(),
                })
            })
            .collect();
        (awaiting, u.timeout, u.repairs)
    })
    .unwrap_or_default();
    // Nothing switched on and nothing waiting is nothing to look at, so a tick
    // asks no connector anything at all — the feature costs nobody who has not
    // switched a project on.
    if roots.is_empty() && awaiting.is_empty() {
        resume_parked(cx);
        return;
    }
    // A run due a session goes before any new issue.
    let search = with(cx, |u| {
        let idle = !u.claiming
            && u.blocked.is_none()
            && u.mode_refused.is_none()
            && u.working().is_none()
            && u.parked.iter().all(|p| p.phase != Phase::Working);
        idle.then(|| {
            u.claiming = true;
            (u.label.clone(), u.busy())
        })
    })
    .flatten();
    cx.spawn(async move |cx| {
        let searching = search.is_some();
        // A panic in there would otherwise leave `claiming` set for the life of
        // the process, and no issue would be looked for again. Said and treated
        // as nothing found.
        let (accounts, checked, polled, begun) = cx
            .background_executor()
            .spawn(async move {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let polled = poll_blocking(awaiting, timeout, repairs);
                    let repairing = polled.iter().any(|(_, p)| match p {
                        Polled::Repair { .. } => true,
                        Polled::Wait | Polled::End(_) => false,
                    });
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
                    // A repair due now takes the slot this tick, not a new issue.
                    let begun = search.filter(|_| !repairing).and_then(|(label, busy)| {
                        begin_blocking(&workable, &label, &busy, &mut checked)
                    });
                    (Some(accounts), checked, polled, begun)
                }))
                .unwrap_or_else(|_| {
                    // The look, the search and the claim are all in here, and
                    // none of them has taken a label unless it finished.
                    eprintln!("onehand: looking for an unattended run panicked");
                    (None, Vec::new(), Vec::new(), None)
                })
            })
            .await;
        cx.update(|cx| {
            if let Some(accounts) = accounts {
                file_accounts(accounts, cx);
                record(checked, true, cx);
            }
            apply_polls(polled, cx);
            if searching {
                landed(begun, asked_from, cx);
            }
            resume_parked(cx);
        });
    })
    .detach();
}
