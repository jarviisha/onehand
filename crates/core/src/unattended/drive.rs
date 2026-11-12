//! Unattended runs' drive: what the tick, a pick and a run's end do next.
//!
//! **Decided here, carried out elsewhere.** An [`UnattendedDrive`] takes the
//! front end's commands as methods (a tick, a search asked for now, an issue
//! picked by hand, a run waiting or answered, a run's end) and is told what
//! the world found ([`Input`]); it answers with what to do ([`Effect`]). It
//! reads no disk, no network and no clock: the look, the search, the claim
//! and the report's delivery are effects whose bodies run off the UI loop
//! (`blocking`), and the time a look landed is told it.
//!
//! **One per process.** Two ticks would be two agents on one issue, so a
//! front end holds one, beside the tasks it starts ([`link`](super::link)).
//!
//! **What it reads and does not own** comes in with each call: the tasks and
//! the runs under way ([`Seen`]), the projects switched on, in rail order,
//! with whether each has a check command, and the workflows and agents on
//! offer ([`World`]). A new task is made here and handed to the tasks' drive
//! through the link, never kept here.
//!
//! The flows:
//! - **The tick**, every `every`: send the reports not yet delivered, then
//!   look at every switched-on project and, while a slot is free and nothing
//!   stops every run, search them ([`Effect::Look`]). What the search found
//!   is checked again against the tasks as they are when it lands, then
//!   claimed ([`Effect::Claim`]); the claim kept as a task asks for its place
//!   ([`Effect::Submit`]), and the tick looks again at once for the next slot.
//! - **A pick**: refused before the claim for what would refuse any run;
//!   otherwise what claiming the issue comes to is read ([`Effect::Take`]),
//!   checked again, claimed ([`Effect::ClaimPicked`]) and started as a found
//!   one is, in the window it was picked in.
//! - **A run's end**: its project is dropped unless a person has it, and its
//!   reports are sent, oldest first ([`Effect::Send`]); each is dropped from
//!   its task only once it landed ([`Effect::Drain`]).

use super::{
    could_not_start, Accounts, Claimed, Earlier, Found, Holder, IssueRow, IssueSource,
    PendingReport, Project, Served, Slots, Taking, Tracker, Unstarted,
};
use crate::config::{AgentSpec, UnattendedConfig};
use crate::connector::Connector;
use crate::task::tasks::link::Seen;
use crate::task::{Task, Working};
use crate::workflow::{Offer, Setup, Template};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub mod blocking;
mod claiming;
mod reports;
mod settings;

pub use blocking::{Look, Looked, Search, Sending};
pub use reports::opening;
pub use settings::{offered, run_spec};

/// The connectors this process is built with.
pub type Connectors = &'static [&'static dyn Connector];

/// What the front end holds that a decision reads: the workflows on offer
/// that could be read, the agents configured, and what each agent offered
/// when it last came up in this process.
#[derive(Default)]
pub struct World {
    pub library: Vec<Template>,
    pub agents: Vec<AgentSpec>,
    pub offers: Vec<(AgentSpec, Offer)>,
}

/// A project switched on for runs, and whether it has a check command.
pub type On = (Project, bool);

/// An issue a person picked from a project's open issues, to work now.
pub struct Pick {
    pub repo: PathBuf,
    pub tracker: Tracker,
    pub row: IssueRow,
    /// The workflow picked, by id.
    pub workflow: String,
    /// What the person added for this run.
    pub instructions: String,
    /// Whether the project has a check command, told by the window it was
    /// picked in.
    pub has_check: bool,
}

/// What the world found, as the drive is told it.
pub enum Input<W> {
    /// The tick's clock came round.
    Elapsed,
    /// What every connector said of its account, and each project's look.
    /// `prune` when the look was of every switched-on project.
    Checked {
        accounts: Accounts,
        checked: Vec<(PathBuf, Served)>,
        prune: bool,
    },
    /// A tick's look, and its search when it searched.
    Looked(Box<Looked<W>>),
    /// What claiming what the search found came to: `None` when the claim
    /// itself failed or panicked. `held` is why nothing was started where
    /// something was looked for, said when nothing was claimed.
    Claimed {
        begun: Option<Result<Claimed, Unstarted>>,
        asked_from: Option<W>,
        held: Option<String>,
        /// The windows holding the claimed issue's project, in order, each
        /// with that project's check command there.
        holding: Vec<(W, Option<String>)>,
        /// The id a task made for the claim takes, or the new run of one
        /// that answers a review.
        new_task: String,
    },
    /// What claiming a picked issue would come to, with the forge its work
    /// goes to, or why it is refused before the claim.
    Taken {
        pick: Box<Pick>,
        window: W,
        taken: Result<(Option<&'static dyn Connector>, Taking), String>,
    },
    /// What claiming a picked issue came to: refused before anything was
    /// claimed (`Err`), or claimed and started or not.
    PickClaimed {
        window: W,
        named: String,
        begun: Result<Result<Claimed, Unstarted>, String>,
        holding: Vec<(W, Option<String>)>,
        new_task: String,
    },
    /// How many of task `task`'s unsent reports landed, in order.
    Sent { task: String, landed: usize },
}

/// What a person, or the log, is told; the words are the drive's, where it
/// goes the front end's.
#[derive(Debug, Clone, PartialEq)]
pub enum Signal<W> {
    /// Said in `window`, as a passing notice: why a pick or a search asked
    /// for now did nothing.
    Warn { window: W, said: String },
    /// For the log only.
    Log(String),
}

/// What to do, in the order given.
pub enum Effect<W> {
    /// Report [`Input::Elapsed`] after this long. One clock for the life of
    /// the process, armed again at each fire, so nothing supersedes it.
    Arm(Duration),
    /// Ask every connector who it acts as and look at `roots`, then report
    /// [`Input::Checked`]. Recorded: what was found is filed whenever it
    /// lands.
    Check {
        connectors: Connectors,
        roots: Vec<Project>,
        prune: bool,
    },
    /// Look, and search when the look says so ([`Look::run_blocking`]), then
    /// report [`Input::Looked`]. Awaited: no other claim starts until it
    /// lands.
    Look(Box<Look<W>>),
    /// Claim what the search found ([`blocking::claim_found`]), then report
    /// [`Input::Claimed`]. Awaited, as the look.
    Claim {
        found: Box<Found>,
        label: String,
        asked_from: Option<W>,
        held: Option<String>,
    },
    /// Read what claiming a picked issue comes to ([`blocking::take`]),
    /// then report [`Input::Taken`]. Awaited, as the look.
    Take {
        pick: Box<Pick>,
        window: W,
        earlier: Vec<Earlier>,
        connectors: Connectors,
    },
    /// Claim a picked issue and make what it is worked on
    /// ([`blocking::claim_picked`]), then report [`Input::PickClaimed`].
    /// Awaited, as the look.
    ClaimPicked {
        pick: Box<Pick>,
        window: W,
        forge: Option<&'static dyn Connector>,
        taking: Box<Taking>,
        label: String,
    },
    /// Leave `body` on issue `number`, best effort: nothing is kept to
    /// tell it again. Not answered.
    Tell {
        tracker: Tracker,
        repo: PathBuf,
        number: u64,
        body: String,
    },
    /// Keep `task`, new, and ask for its place in `window`. Carried by the
    /// link, which hands it to the tasks' drive.
    Submit { task: Box<Task>, window: W },
    /// Give task `task` a new run `run` of `template` from step `from`,
    /// telling it `note`, and ask for its place in `window`. Carried by the
    /// link.
    AnswerReview {
        task: String,
        run: String,
        template: Box<Template>,
        from: Option<String>,
        note: String,
        window: W,
    },
    /// Send task `task`'s unsent reports ([`Sending::send_blocking`]), then
    /// report [`Input::Sent`]. Recorded: what landed is dropped whatever
    /// happened since.
    Send(Box<Sending>),
    /// The first `landed` of task `task`'s unsent reports reached its issue:
    /// drop them. Carried by the link.
    Drain { task: String, landed: usize },
    /// Keep the project at `dir` a run worked in for good (`keep`), or drop
    /// it, in every window holding it: only one the run added is dropped.
    Teardown { dir: PathBuf, keep: bool },
    /// Note on issue `number` kept in `file`, of the project at `root`, that
    /// `session` works it, saying `said`: a change to the file through the
    /// drive every view of it shares, so each shows it as it lands. Never
    /// answered to this drive: what the write came to goes to the carrier,
    /// which logs a failure, so there is no late answer here to handle.
    NoteTakenUp {
        root: PathBuf,
        file: PathBuf,
        number: u64,
        said: String,
        session: String,
    },
    /// Tick again once the call being handled is over, with the world as it
    /// is then.
    Tick,
    /// Tell this.
    Signal(Signal<W>),
    /// What unattended runs show changed: draw them again.
    Refresh,
}

/// Unattended runs in this process: their settings and what is under way.
/// The runs themselves are tasks, kept by the tasks' drive.
pub struct UnattendedDrive {
    connectors: Connectors,
    label: String,
    /// How long a run may work, put over the workflow's own timeout.
    timeout: String,
    /// The mode configured; absent takes the one the runs' agent needs.
    mode: Option<String>,
    agent: Option<String>,
    /// The id of the workflow an issue is worked with, unless a workflow
    /// label on it names another.
    workflow: String,
    /// Workflow label → workflow id.
    workflows: BTreeMap<String, String>,
    at_once: u32,
    /// How many runs may wait on a person at once; `None` for no cap.
    waiting: Option<u32>,
    every: Duration,
    /// A claim is on its way: a tick landing now must not start a second.
    claiming: bool,
    /// Why no run can start at all: a config that cannot work. Every run
    /// would fail the same way, so the search stops and every switched-on
    /// row says why.
    blocked: Option<String>,
    /// A run failed on a setting every later one would fail on too. Kept
    /// apart from `blocked` because it stops a pick as well, where the label
    /// and the interval do not matter, and it is only learned after a claim.
    paused: Option<String>,
    /// Tasks whose reports are on their way, so a second delivery does not
    /// send them twice.
    delivering: HashSet<String>,
    /// Issue tasks kept but not yet waiting for or holding a place: counted
    /// against the cap meanwhile, so a tick landing in between cannot start
    /// one more.
    starting: HashSet<String>,
    /// What each connector last said about its account; `None` until the
    /// first answer lands.
    accounts: Option<Accounts>,
    checked_at: Option<SystemTime>,
    /// How many checks have not answered yet. A count, not a flag: checks
    /// overlap, and a flag cleared by whichever lands first gives the button
    /// back early.
    checks_out: usize,
    /// Why the last look at a project could not go ahead, by project, for
    /// its row: a switch that is on while nothing can happen must not look
    /// exactly like one that works.
    problems: HashMap<PathBuf, String>,
}

impl UnattendedDrive {
    /// Unattended runs as `cfg` sets them up, with `connectors`, and its
    /// clock armed.
    ///
    /// **A config that cannot work is filed, not only logged**: its reason
    /// stops the search and is on every switched-on row, while the projects
    /// and the accounts are still looked at, so the clock runs whatever the
    /// config says.
    pub fn new<W>(cfg: &UnattendedConfig, connectors: Connectors) -> (Self, Vec<Effect<W>>) {
        let label = cfg.label.trim().to_string();
        let every = super::parse_every(&cfg.every);
        let timeout = super::parse_every(&cfg.timeout).map(|_| cfg.timeout.clone());
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
        let mut fx = Vec::new();
        if let Some(why) = &blocked {
            fx.push(Effect::Signal(Signal::Log(format!(
                "unattended runs cannot start: {why}"
            ))));
        }
        let fallback = UnattendedConfig::default();
        let every = every
            .or_else(|| super::parse_every(&fallback.every))
            .unwrap_or(Duration::from_secs(1800));
        fx.push(Effect::Arm(every));
        let drive = Self {
            connectors,
            label,
            timeout: timeout.unwrap_or(fallback.timeout),
            mode: cfg.mode.clone(),
            agent: cfg.agent.clone(),
            workflow: cfg.workflow.clone(),
            workflows: cfg.workflows.clone(),
            at_once: cfg.at_once,
            waiting: cfg.waiting,
            every,
            claiming: false,
            blocked,
            paused: None,
            delivering: HashSet::new(),
            starting: HashSet::new(),
            accounts: None,
            checked_at: None,
            checks_out: 0,
            problems: HashMap::new(),
        };
        (drive, fx)
    }

    /// The label that asks for a run.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// How long a run may work, put over the workflow's own timeout.
    pub fn timeout(&self) -> &str {
        &self.timeout
    }

    /// The default workflow and the workflow labels.
    pub fn workflows(&self) -> (&str, &BTreeMap<String, String>) {
        (&self.workflow, &self.workflows)
    }

    /// Whether a look or a pick is claiming an issue.
    pub fn claiming(&self) -> bool {
        self.claiming
    }

    /// What each connector last said about its account, if they answered.
    pub fn accounts(&self) -> Option<&Accounts> {
        self.accounts.as_ref()
    }

    /// When the connectors last answered.
    pub fn checked_at(&self) -> Option<SystemTime> {
        self.checked_at
    }

    /// Whether a check has not answered yet.
    pub fn checking(&self) -> bool {
        self.checks_out > 0
    }

    /// Why the last look at `root` could not go ahead, if it could not.
    pub fn problem(&self, root: &std::path::Path) -> Option<&str> {
        self.problems.get(root).map(String::as_str)
    }

    /// Why no run can start at all, if something stops every one.
    pub fn blocked(&self, world: &World) -> Option<String> {
        self.blocked.clone().or_else(|| self.cannot_start(world))
    }

    /// The slots as they stand, across every window.
    pub fn slots<W: Copy>(&self, seen: &Seen<W>) -> Slots {
        Slots {
            holders: slot_holders(seen),
            starting: self.starting.len(),
            at_once: self.at_once,
            waiting: seen
                .all()
                .iter()
                .filter(|task| task.issue().is_some())
                .filter(|task| seen.working(&task.id) == Some(Working::Waiting))
                .count(),
            waiting_cap: self.waiting,
        }
    }

    /// Whether issue task `id` is counted against the cap already: kept and
    /// not yet placed, or queued, running or waiting.
    pub fn counted<W: Copy>(&self, id: &str, seen: &Seen<W>) -> bool {
        self.starting.contains(id) || seen.busy(id)
    }

    /// Why issue task `id` may not start now: the cap is reached. One counted
    /// already never is, and a task of no issue never is.
    pub fn over_cap<W: Copy>(&self, id: &str, seen: &Seen<W>) -> Option<String> {
        let issue = seen.task(id).is_some_and(|task| task.issue().is_some());
        if !issue || self.counted(id, seen) {
            return None;
        }
        self.at_cap(seen)
    }

    /// Make `default` the workflow issues are worked with and `by_label` the
    /// workflow labels, for every run from now on.
    pub fn set_workflows(&mut self, default: String, by_label: BTreeMap<String, String>) {
        self.workflow = default;
        self.workflows = by_label;
    }

    /// Ask every connector again and look at `roots`: *Check again*, a
    /// window opening (`prune`, `roots` being every switched-on project), or
    /// one project just switched on.
    pub fn check<W>(&mut self, roots: Vec<Project>, prune: bool) -> Vec<Effect<W>> {
        self.checks_out += 1;
        vec![
            Effect::Refresh,
            Effect::Check {
                connectors: self.connectors,
                roots,
                prune,
            },
        ]
    }

    /// Issue task `id` now waits for its place or holds it, and counts
    /// against the cap as such.
    pub fn placed(&mut self, id: &str) {
        self.starting.remove(id);
    }

    /// A run of task `id` now waits on a person, or on its status checks,
    /// and gives its slot up: an issue's looks for the next issue at once
    /// rather than at the next tick.
    pub fn waiting<W: Copy + PartialEq>(
        &mut self,
        id: &str,
        seen: &Seen<W>,
        roots: Vec<On>,
        world: &World,
    ) -> Vec<Effect<W>> {
        match seen.task(id).is_some_and(|task| task.issue().is_some()) {
            true => self.tick(None, seen, roots, world),
            false => Vec::new(),
        }
    }

    /// A person answered the run of task `id` that waited on them: under a
    /// cap on waiting runs that is room made, so an issue's looks for the
    /// next issue at once.
    pub fn answered<W: Copy + PartialEq>(
        &mut self,
        id: &str,
        seen: &Seen<W>,
        roots: Vec<On>,
        world: &World,
    ) -> Vec<Effect<W>> {
        let issue = seen.task(id).is_some_and(|task| task.issue().is_some());
        match self.waiting.is_some() && issue {
            true => self.tick(None, seen, roots, world),
            false => Vec::new(),
        }
    }

    /// A run failed on a setting, `why`: every later run would fail the same
    /// way, each on a fresh issue, so runs stop until what `fix` names is
    /// fixed and onehand restarted, which is the only thing that lifts it.
    pub fn pause<W>(&mut self, why: &str, fix: &str) -> Vec<Effect<W>> {
        let why = format!("{why}. Unattended runs are paused until {fix} and onehand restarted.");
        let fx = vec![Effect::Signal(Signal::Log(why.clone())), Effect::Refresh];
        self.paused = Some(why);
        fx
    }

    /// Something the world found: what to do about it. `roots` and `world`
    /// are as they are now; `now` is when it landed.
    pub fn handle<W: Copy + PartialEq>(
        &mut self,
        input: Input<W>,
        seen: &Seen<W>,
        roots: Vec<On>,
        world: &World,
        now: SystemTime,
    ) -> Vec<Effect<W>> {
        match input {
            Input::Elapsed => {
                let mut fx = self.tick(None, seen, roots, world);
                fx.push(Effect::Arm(self.every));
                fx
            }
            Input::Checked {
                accounts,
                checked,
                prune,
            } => {
                self.checks_out = self.checks_out.saturating_sub(1);
                self.file_accounts(accounts, now);
                self.record(checked, prune.then_some(&roots));
                vec![Effect::Refresh]
            }
            Input::Looked(looked) => self.looked(*looked, seen, &roots, now),
            Input::Claimed {
                begun,
                asked_from,
                held,
                holding,
                new_task,
            } => self.landed(
                begun, asked_from, None, held, &holding, new_task, seen, world,
            ),
            Input::Taken {
                pick,
                window,
                taken,
            } => self.taken(*pick, window, taken, seen),
            Input::PickClaimed {
                window,
                named,
                begun,
                holding,
                new_task,
            } => self.pick_claimed(window, &named, begun, &holding, new_task, seen, world),
            Input::Sent { task, landed } => {
                self.delivering.remove(&task);
                vec![Effect::Drain { task, landed }]
            }
        }
    }

    /// Why no more runs may start while these work: every slot is taken,
    /// said with who holds each, or as many runs wait on a person as may.
    fn at_cap<W: Copy>(&self, seen: &Seen<W>) -> Option<String> {
        self.slots(seen).full()
    }

    fn file_accounts(&mut self, accounts: Accounts, now: SystemTime) {
        self.accounts = Some(accounts);
        self.checked_at = Some(now);
    }

    /// File what each look found. A later entry for the same project wins,
    /// so a search that failed after its project passed the look is what the
    /// row says. `switched_on`, when given, drops what is kept for projects
    /// no longer switched on, as they are when the answer lands: a project
    /// switched on during a slow look was looked at by itself already.
    fn record(&mut self, checked: Vec<(PathBuf, Served)>, switched_on: Option<&Vec<On>>) {
        if let Some(on) = switched_on {
            self.problems
                .retain(|root, _| on.iter().any(|(p, _)| &p.root == root));
        }
        for (root, served) in checked {
            match served {
                Err(why) => self.problems.insert(root, why),
                Ok(_) => self.problems.remove(&root),
            };
        }
    }
}

/// Every issue task with a branch of its own, oldest first.
pub fn earlier<W: Copy>(seen: &Seen<W>) -> Vec<Earlier> {
    seen.all()
        .iter()
        .filter_map(|task| {
            let issue = task.issue()?;
            Some(Earlier {
                task: task.id.clone(),
                tracker: issue.tracker.clone(),
                number: issue.number,
                dir: task.setup.dir.clone(),
                branch: task.setup.branch.clone()?,
                working: seen.busy(&task.id),
                answers_reviews: task.answers_reviews(),
            })
        })
        .collect()
}

/// Each issue task holding a slot, oldest first, with its issue, its
/// workflow and the step it is at.
fn slot_holders<W: Copy>(seen: &Seen<W>) -> Vec<Holder> {
    seen.all()
        .iter()
        .filter(|task| seen.holds_slot(&task.id))
        .filter_map(|task| {
            let named = task.issue()?.named(&task.brief.title);
            let shown = match task.runs.last() {
                Some(run) => match run.current() {
                    Some(step) => format!("{named} · {} ({})", run.template.name, step.label),
                    None => format!("{named} · {}", run.template.name),
                },
                None => named.clone(),
            };
            Some(Holder {
                task: task.id.clone(),
                named,
                shown,
            })
        })
        .collect()
}

/// What the claim told an issue that could not be started.
fn tell<W>(unstarted: Unstarted) -> Effect<W> {
    Effect::Tell {
        body: could_not_start(&unstarted.why),
        tracker: unstarted.tracker,
        repo: unstarted.repo,
        number: unstarted.number,
    }
}

/// The task a claim on a new branch is kept as.
fn issue_task(
    id: String,
    claimed: Claimed,
    template: Template,
    setup: Setup,
    base: String,
    picked: bool,
) -> Task {
    let brief = super::brief_for(&claimed.tracker, &claimed.issue, &claimed.instructions);
    let issue = IssueSource {
        tracker: claimed.tracker.to_ref(),
        number: claimed.issue.number,
        forge_ref: claimed.issue.forge_ref().map(str::to_string),
        forge: claimed.forge.map(|forge| forge.name().to_string()),
        base,
        picked,
        unsent: Vec::<PendingReport>::new(),
        notes: claimed.notes,
    };
    Task::of_issue(id, template, brief, setup, issue)
}

#[cfg(test)]
mod tests;
