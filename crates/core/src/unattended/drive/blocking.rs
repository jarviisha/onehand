//! What the drive's effects do off the UI loop: the look and the search, the
//! claims, and a task's reports sent. Blocking, each bounded by the calls it
//! makes, and each catching its own panic: left uncaught, a claim in flight
//! would hold `claiming` for the life of the process.

use super::{Connectors, On, Pick};
use crate::connector::{self, Connector};
use crate::issues::sync::OutsideSync;
use crate::issues::template;
use crate::unattended::{
    self as core, Accounts, Choosing, Claimed, Earlier, Found, Judging, PendingReport, Served,
    Taking, Tracker, Unstarted, Verdict, Workable,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// A tick's look at the projects switched on, and the search it runs when
/// nothing stops one.
pub struct Look<W> {
    pub(super) connectors: Connectors,
    pub(super) roots: Vec<On>,
    /// What the default workflow lacks on each project, said after the look,
    /// which would otherwise say the project is fine.
    pub(super) lacking: Vec<(PathBuf, String)>,
    pub(super) search: Option<Search>,
    pub(super) asked_from: Option<W>,
}

/// What a search looks for and judges with, gathered before it goes off the
/// UI loop.
pub struct Search {
    pub(super) label: String,
    pub(super) choosing: Choosing,
    pub(super) judging: Judging,
    pub(super) earlier: Vec<Earlier>,
}

/// What a look came to.
pub struct Looked<W> {
    pub(super) asked_from: Option<W>,
    pub(super) searching: bool,
    /// The syncs the search made of files kept in step, kept even when it
    /// panicked after them.
    pub(super) synced: Vec<OutsideSync>,
    /// `None` when the look panicked: said, and treated as nothing found.
    pub(super) answer: Option<Answer>,
}

/// What a look that did not panic found.
pub(super) struct Answer {
    pub(super) accounts: Accounts,
    pub(super) checked: Vec<(PathBuf, Served)>,
    pub(super) found: Option<Found>,
    /// Why nothing was started where something was looked for.
    pub(super) held: Option<String>,
}

impl<W> Look<W> {
    /// Look at every project, then search the ones that passed, each with
    /// the forge the look found for it.
    pub fn run_blocking(self) -> Looked<W> {
        let Look {
            connectors,
            roots,
            lacking,
            search,
            asked_from,
        } = self;
        let searching = search.is_some();
        let mut synced = Vec::new();
        let answer = catch_unwind(AssertUnwindSafe(|| {
            let projects = roots.iter().map(|(p, _)| p.clone()).collect();
            let (accounts, mut checked) = core::look_blocking(connectors, projects);
            let workable: Vec<Workable> = roots
                .into_iter()
                .filter_map(|(project, has_check)| {
                    let (_, served) = checked.iter().find(|(root, _)| *root == project.root)?;
                    let forge = *served.as_ref().ok()?;
                    Some(Workable {
                        project,
                        forge,
                        has_check,
                    })
                })
                .collect();
            let found = search.and_then(|s| {
                core::find_blocking(
                    &workable,
                    &s.label,
                    &s.choosing,
                    &s.earlier,
                    &s.judging,
                    &mut checked,
                    &mut synced,
                )
            });
            // Taken before the line below, which passes issues over rather
            // than stopping a project.
            let held = checked.iter().find_map(|(root, served)| {
                let why = served.as_ref().err()?;
                let name = root.file_name()?.to_string_lossy().into_owned();
                Some(format!("Nothing was picked up in {name}: {why}"))
            });
            checked.extend(lacking.into_iter().map(|(root, why)| (root, Err(why))));
            Answer {
                accounts,
                checked,
                found,
                held,
            }
        }))
        .inspect_err(|_| eprintln!("onehand: looking for an unattended run panicked"))
        .ok();
        Looked {
            asked_from,
            searching,
            synced,
            answer,
        }
    }
}

/// Claim what the search found and make what it is worked on: `None` when
/// the claim failed, or panicked before the worktree was made; with the
/// syncs the claim made of a file kept in step.
pub fn claim_found(
    found: Found,
    label: &str,
) -> (Option<Result<Claimed, Unstarted>>, Vec<OutsideSync>) {
    let mut synced = Vec::new();
    let begun = catch_unwind(AssertUnwindSafe(|| {
        core::claim_found_blocking(found, label, &mut synced)
    }))
    .unwrap_or_else(|_| {
        // The worktree's own panic is caught where it is made, so this
        // is the claim's.
        eprintln!("onehand: claiming an unattended run panicked");
        None
    });
    (begun, synced)
}

/// What claiming `pick` comes to, refused before the claim when it would
/// be worked twice, with the forge its work goes to: an issue on the forge
/// goes back to that forge, one kept here to whichever serves the project.
pub fn take(
    pick: &Pick,
    earlier: &[Earlier],
    connectors: Connectors,
) -> Result<(Option<&'static dyn Connector>, Taking), String> {
    let named = pick.tracker.named(&pick.row.issue);
    catch_unwind(AssertUnwindSafe(|| {
        let forge = match &pick.tracker {
            Tracker::Forge(forge) | Tracker::Synced { forge, .. } => Some(*forge),
            Tracker::Local(_) => connector::serving(connectors, &pick.repo)
                .ok()
                .map(|at| connectors[at]),
        };
        core::taking_blocking(
            earlier,
            &pick.tracker,
            pick.row.issue.number,
            &pick.repo,
            forge,
        )
        .map(|taking| (forge, taking))
        .map_err(|skip| skip.said(&named))
    }))
    .unwrap_or_else(|_| Err("onehand panicked while claiming the issue".to_string()))
}

/// Claim `pick` and make what it is worked on, as `taking` says: refused
/// (`Err`) when the claim itself failed, so nothing was claimed. What its
/// text lacks of its template goes to its first report, read later by
/// whoever judges why the run went wrong. With the syncs the claim made of a
/// file kept in step.
pub fn claim_picked(
    pick: Pick,
    forge: Option<&'static dyn Connector>,
    taking: Taking,
    label: &str,
) -> (Result<Result<Claimed, Unstarted>, String>, Vec<OutsideSync>) {
    let named = pick.tracker.named(&pick.row.issue);
    let mut synced = Vec::new();
    let begun = catch_unwind(AssertUnwindSafe(|| {
        let Pick {
            repo,
            tracker,
            row,
            workflow,
            instructions,
            ..
        } = pick;
        core::claim_picked_blocking(
            &tracker,
            &repo,
            &row,
            label,
            taking.answering(),
            &mut synced,
        )
        .map_err(|why| format!("Could not start on issue {named}: {why}"))?;
        let lacks = template::lacking(
            row.issue.body_text(),
            &template::for_project_blocking(&repo),
        )
        .map(|lacks| lacks.note());
        Ok(
            core::prepare_blocking(repo, tracker, forge, row.issue, workflow, taking).map(
                |claimed| Claimed {
                    instructions,
                    notes: lacks.into_iter().collect(),
                    ..claimed
                },
            ),
        )
    }))
    .unwrap_or_else(|_| Err("onehand panicked while claiming the issue".to_string()));
    (begun, synced)
}

/// Where a task's reports go, what they are, and where to look for what its
/// run left.
pub struct Sending {
    pub task: String,
    pub(super) tracker: Tracker,
    pub(super) number: u64,
    pub(super) forge: Option<&'static dyn Connector>,
    pub(super) base: String,
    pub(super) repo: PathBuf,
    pub(super) dir: PathBuf,
    pub(super) branch: String,
    pub(super) unsent: Vec<PendingReport>,
}

impl Sending {
    /// Leave each report on the issue, in order, each with what its run left
    /// as it stands now: how many landed. The first that fails stops the
    /// rest, so the issue never hears them out of order.
    pub fn send_blocking(&self) -> usize {
        let mut landed = 0;
        for pending in &self.unsent {
            let found = self.verdict_blocking();
            let body = core::report(pending, &found, &self.branch);
            match self
                .tracker
                .comment_blocking(&self.repo, self.number, &body)
            {
                Ok(()) => landed += 1,
                Err(why) => {
                    eprintln!(
                        "onehand: could not tell issue #{} how its run ended: {why}",
                        self.number
                    );
                    break;
                }
            }
        }
        landed
    }

    fn verdict_blocking(&self) -> Result<Verdict, String> {
        core::verdict_blocking(self.forge, &self.repo, &self.branch, &self.dir, &self.base)
    }
}

/// Leave `body` on issue `number`, saying on stderr if that failed: a run
/// that never started has no task to keep it in.
pub fn tell_blocking(tracker: &Tracker, repo: &std::path::Path, number: u64, body: &str) {
    if let Err(why) = tracker.comment_blocking(repo, number, body) {
        eprintln!("onehand: could not comment on issue #{number}: {why}");
    }
}
