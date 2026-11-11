//! Unattended runs, as the app carries them: the clock, the look, the claim
//! and the reports, done here and decided in
//! `onehand_core::unattended::drive`.
//!
//! An issue is worked as a task of the configured workflow, driven like any
//! other (`crate::task`). The drive is held beside the tasks it starts, in
//! the `Tasks` global, and the two meet in `onehand_core::unattended::link`.
//! What is here is what needs a timer, a thread, a window or the person's
//! settings: each command is given the projects switched on across every
//! window and the workflows and agents on offer as they are now, and each
//! effect is carried out once.
//!
//! **One per process**, for the reason the remote bridge is: two windows each
//! running a tick would be two agents on one issue. The tick therefore
//! belongs to no window and asks each in turn for its projects.

use crate::state::Shared;
use gpui::{AnyWindowHandle, App, Global, WeakEntity};
use onehand_core::config::UnattendedConfig;
use onehand_core::connector::{self, Connector};
use onehand_core::task::drive::Said;
use onehand_core::unattended::drive::{
    Effect, Input, On, Signal, UnattendedDrive, World, blocking,
};
use onehand_core::unattended::{self as core, Accounts, Slots};
use onehand_core::workflow::Field;
use std::collections::BTreeMap;
use std::path::Path;

mod facts;
mod launch;
mod report;
pub use launch::{look_now, start_answer, start_picked};
pub use onehand_core::unattended::{Pickable, Project};
pub(crate) use report::spec_for;

/// The tick's clock, armed again at every fire; dropping it stops it.
struct Clock {
    _tick: gpui::Task<()>,
}

impl Global for Clock {}

/// The connector serving the project at `root`, or every reason none does.
///
/// Its account is not asked here: a person picking an issue by hand hears
/// about an unusable account from the connector's own refusal, one call later.
pub fn connector_for(root: &Path) -> Result<&'static dyn Connector, String> {
    let all = crate::features::connectors();
    connector::serving(all, root).map(|at| all[at])
}

/// The workflows and agents on offer now, as the drive reads them.
fn world(cx: &App) -> World {
    let shared = Shared::global(cx);
    World {
        library: crate::workflow::templates(cx)
            .into_iter()
            .filter_map(|entry| entry.template.ok())
            .collect(),
        agents: shared.settings.agents.clone(),
        offers: shared.settings.offers_seen.clone(),
    }
}

/// Every switched-on project across every window, in rail order, once each,
/// with whether it has a check command in whichever window holds it.
fn roots(cx: &App) -> Vec<On> {
    opted_in_roots(cx)
        .into_iter()
        .map(|project| {
            let has = has_check(&project.root, cx);
            (project, has)
        })
        .collect()
}

/// Whether the project at `root` has a check command, in whichever window
/// holds it.
fn has_check(root: &Path, cx: &App) -> bool {
    Shared::global(cx)
        .windows
        .iter()
        .filter_map(|w| w.shell.upgrade())
        .any(|shell| shell.read(cx).check_of(root).is_some())
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

/// The windows holding the project at `repo`, in order, each with that
/// project's check command there.
fn holding(repo: Option<&Path>, cx: &App) -> Vec<(AnyWindowHandle, Option<String>)> {
    let Some(repo) = repo else {
        return Vec::new();
    };
    Shared::global(cx)
        .windows
        .iter()
        .filter_map(|w| {
            let shell = w.shell.upgrade()?;
            let shell = shell.read(cx);
            shell
                .holds_root(repo)
                .then(|| (w.handle, shell.check_of(repo)))
        })
        .collect()
}

/// Set unattended runs up as `cfg` says and start their clock. A config
/// that cannot work is filed, and its clock still runs.
pub fn boot(cfg: &UnattendedConfig, cx: &mut App) {
    let (drive, fx) = UnattendedDrive::new(cfg, crate::features::connectors());
    crate::task::set_unattended(drive, cx);
    perform(fx, cx);
}

/// Tell the drive what the world found, with the world as it is now.
fn feed(input: Input<AnyWindowHandle>, cx: &mut App) {
    let (roots, world) = (roots(cx), world(cx));
    let now = std::time::SystemTime::now();
    crate::task::with_unattended(cx, |u, seen| {
        ((), u.handle(input, seen, roots, &world, now))
    });
}

/// Do what the drive said, in order.
pub(crate) fn perform(effects: Vec<Effect<AnyWindowHandle>>, cx: &mut App) {
    for effect in effects {
        match effect {
            Effect::Arm(after) => {
                let clock = cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| feed(Input::Elapsed, cx));
                });
                cx.set_global(Clock { _tick: clock });
            }
            Effect::Check {
                connectors,
                roots,
                prune,
            } => off_loop(cx, move || {
                let (accounts, checked) = core::look_blocking(connectors, roots);
                Input::Checked {
                    accounts,
                    checked,
                    prune,
                }
            }),
            Effect::Look(look) => {
                off_loop(cx, move || Input::Looked(Box::new(look.run_blocking())))
            }
            Effect::Claim {
                found,
                label,
                asked_from,
                held,
            } => {
                cx.spawn(async move |cx| {
                    let begun = cx
                        .background_executor()
                        .spawn(async move { blocking::claim_found(*found, &label) })
                        .await;
                    cx.update(|cx| {
                        let repo = begun
                            .as_ref()
                            .and_then(|b| b.as_ref().ok())
                            .map(|claimed| claimed.repo.as_path());
                        let input = Input::Claimed {
                            holding: holding(repo, cx),
                            begun,
                            asked_from,
                            held,
                            new_task: onehand_core::task::new_id(),
                        };
                        feed(input, cx);
                    });
                })
                .detach();
            }
            Effect::Take {
                pick,
                window,
                earlier,
                connectors,
            } => off_loop(cx, move || {
                let taken = blocking::take(&pick, &earlier, connectors);
                Input::Taken {
                    pick,
                    window,
                    taken,
                }
            }),
            Effect::ClaimPicked {
                pick,
                window,
                forge,
                taking,
                label,
            } => {
                let named = pick.tracker.named(&pick.row.issue);
                cx.spawn(async move |cx| {
                    let begun = cx
                        .background_executor()
                        .spawn(async move { blocking::claim_picked(*pick, forge, *taking, &label) })
                        .await;
                    cx.update(|cx| {
                        let repo = match &begun {
                            Ok(Ok(claimed)) => Some(claimed.repo.as_path()),
                            Ok(Err(_)) | Err(_) => None,
                        };
                        let input = Input::PickClaimed {
                            window,
                            named,
                            holding: holding(repo, cx),
                            begun,
                            new_task: onehand_core::task::new_id(),
                        };
                        feed(input, cx);
                    });
                })
                .detach();
            }
            Effect::Tell {
                tracker,
                repo,
                number,
                body,
            } => cx
                .background_executor()
                .spawn(async move { blocking::tell_blocking(&tracker, &repo, number, &body) })
                .detach(),
            Effect::Send(sending) => off_loop(cx, move || Input::Sent {
                landed: sending.send_blocking(),
                task: sending.task,
            }),
            Effect::Teardown { dir, keep } => report::teardown(&dir, keep, cx),
            Effect::NoteTakenUp {
                file,
                number,
                said,
                session,
            } => cx
                .background_executor()
                .spawn(async move {
                    if let Err(why) =
                        onehand_core::issues::taken_up_blocking(&file, number, &said, session)
                    {
                        eprintln!("onehand: could not note the session on issue: {why}");
                    }
                })
                .detach(),
            Effect::Tick => cx.defer(|cx| tick(None, cx)),
            Effect::Signal(Signal::Warn { window, said }) => launch::warn(window, said, cx),
            Effect::Signal(Signal::Log(said)) => eprintln!("onehand: {said}"),
            Effect::Refresh => cx.refresh_windows(),
            // The tasks' to act on: the link hands them over, and never on
            // to here.
            Effect::Submit { .. } | Effect::AnswerReview { .. } | Effect::Drain { .. } => {}
        }
    }
}

/// Do `work` off the UI loop, then tell the drive what came of it.
fn off_loop(cx: &mut App, work: impl FnOnce() -> Input<AnyWindowHandle> + Send + 'static) {
    cx.spawn(async move |cx| {
        let input = cx.background_executor().spawn(async move { work() }).await;
        cx.update(|cx| feed(input, cx));
    })
    .detach();
}

/// Look at every switched-on project, and search them if a slot is free;
/// `asked_from` is the window a person asked from.
pub(super) fn tick(asked_from: Option<AnyWindowHandle>, cx: &mut App) {
    let (roots, world) = (roots(cx), world(cx));
    crate::task::with_unattended(cx, |u, seen| ((), u.tick(asked_from, seen, roots, &world)));
}

/// Read unattended runs, once they are set up.
fn read<R>(cx: &App, read: impl FnOnce(&UnattendedDrive) -> R) -> Option<R> {
    crate::task::unattended(cx, |u, _| read(u))
}

/// Whether unattended runs are set up.
pub(crate) fn set_up(cx: &App) -> bool {
    read(cx, |_| ()).is_some()
}

/// A look or a pick is claiming an issue, so *Look for an issue now* waits.
pub(crate) fn claiming(cx: &App) -> bool {
    read(cx, UnattendedDrive::claiming).unwrap_or(false)
}

/// The slots as they stand, if unattended runs are set up.
pub(crate) fn slots(cx: &App) -> Option<Slots> {
    crate::task::unattended(cx, |u, seen| u.slots(seen))
}

/// Why no run can start at all, if something stops every one.
pub fn blocked(cx: &App) -> Option<String> {
    let world = world(cx);
    read(cx, |u| u.blocked(&world)).flatten()
}

/// What each connector last said about its account, if they have answered yet.
pub fn accounts(cx: &App) -> Option<Accounts> {
    read(cx, |u| u.accounts().cloned()).flatten()
}

/// When the connectors last answered, if they have.
pub fn accounts_checked_at(cx: &App) -> Option<std::time::SystemTime> {
    read(cx, UnattendedDrive::checked_at).flatten()
}

/// Whether a check asked for by hand has not answered yet.
pub fn accounts_checking(cx: &App) -> bool {
    read(cx, UnattendedDrive::checking).unwrap_or(false)
}

/// Why the last look at `root` could not go ahead, if it could not.
pub fn problem(root: &Path, cx: &App) -> Option<String> {
    read(cx, |u| u.problem(root).map(str::to_string)).flatten()
}

/// Ask every connector again, and look again at every project that is switched on —
/// what Settings' *Check again* does, and what a window does when it opens.
pub fn recheck(cx: &mut App) {
    let roots = opted_in_roots(cx);
    crate::task::with_unattended(cx, |u, _| ((), u.check(roots, true)));
}

/// Look at one project that was just switched on, so a switch that cannot work
/// says so now rather than at the next tick, half an hour away.
pub fn check_now(project: Project, cx: &mut App) {
    crate::task::with_unattended(cx, |u, _| ((), u.check(vec![project], false)));
}

/// What the run of task `id` told unattended runs, handed to them through
/// their link with the world as it is.
pub(crate) fn run_said(id: &str, said: Said, cx: &mut App) {
    let task = id.to_string();
    match said {
        // Deferred: a person's answer is pressed inside a window's update,
        // and the look it may start reads every window's shell.
        Said::Answered => cx.defer(move |cx| hand_on(&task, Said::Answered, cx)),
        Said::Waiting | Said::CameUp { .. } | Said::Pause { .. } => hand_on(&task, said, cx),
    }
}

fn hand_on(id: &str, said: Said, cx: &mut App) {
    let looked = said.looks().then(|| (roots(cx), world(cx)));
    let looked = looked.as_ref().map(|(roots, world)| (roots.clone(), world));
    crate::task::with_joined(cx, |j| ((), j.run_said(id, said, looked)));
}

/// The label that asks for a run, for the places that tell the user which
/// label to put on an issue.
pub fn label(cx: &App) -> String {
    read(cx, |u| u.label().to_string()).unwrap_or_default()
}

/// The id of the workflow an issue carrying `labels` is worked with: the one
/// its first workflow label names, else the default.
pub fn workflow_for(labels: &[String], cx: &App) -> String {
    read(cx, |u| {
        let (default, by_label) = u.workflows();
        core::workflow_for(labels, by_label, default, u.label()).to_string()
    })
    .unwrap_or_default()
}

/// What a new run of workflow `id` on `issue` in `tracker` is checked
/// against, in a window that knows whether the project `has_check`, what it
/// has `checked_out` and the `forge` serving it.
pub(crate) use facts::issue_run as issue_facts;

/// What a Resume or a Retry of a task on a workflow is checked against: its
/// last run's own setup.
pub(crate) use facts::of_task as task_facts;

/// What a start of a task that runs with a setup of its own is checked
/// against: a retry with current settings.
pub(crate) use facts::with_setup as setup_facts;

/// A forge as a preflight reads it, its account as last seen.
pub(crate) use facts::forge as forge_facts;
pub(crate) use facts::offered;

/// The mode a lead's worker on agent `agent` starts in: the mode runs use,
/// when that agent is theirs, else what it is known to need.
pub(crate) fn worker_mode(agent: &str, cx: &App) -> Option<String> {
    let agents = &Shared::global(cx).settings.agents;
    read(cx, |u| u.worker_mode(agent, agents)).unwrap_or_else(|| {
        onehand_core::config::worker_mode(None, run_agent(cx).as_deref(), agents, agent)
    })
}

/// The agent an issue's run starts, by name: the one set for runs, else the
/// first configured.
pub(crate) fn run_agent(cx: &App) -> Option<String> {
    let agents = &Shared::global(cx).settings.agents;
    read(cx, |u| u.run_agent(agents)).unwrap_or_else(|| {
        onehand_core::config::agent_or_first(agents, None).map(|spec| spec.name.clone())
    })
}

/// What a new task of `task`'s kind would be given now, for a retry with
/// current settings, as core decides it over the config's run settings and
/// the first agent configured.
pub(crate) fn now_for(
    task: &onehand_core::task::Task,
    check: Option<String>,
    cx: &App,
) -> onehand_core::workflow::Now {
    let agents = &Shared::global(cx).settings.agents;
    let first = onehand_core::config::agent_or_first(agents, None).map(|spec| spec.name.clone());
    let runs = read(cx, |u| u.run_settings(agents));
    core::now_for(task.issue().is_some(), runs.as_ref(), first, check)
}

/// Whether a retry of `task` with current settings would run with another
/// timeout than its last run did: when a run that timed out is offered that
/// second way out.
pub(crate) fn timeout_moved(task: &onehand_core::task::Task, cx: &App) -> bool {
    let Some(last) = task.runs.last() else {
        return false;
    };
    // The check command moves no timeout, so it is not asked for.
    let now = now_for(task, None, cx);
    let newest = crate::workflow::newest(&last.template, cx);
    onehand_core::workflow::Run::with_current(last, newest, &now, false).is_ok_and(|plan| {
        plan.changes
            .iter()
            .any(|change| change.field == Field::Timeout)
    })
}

/// The default workflow and the workflow labels, as the config has them.
pub fn workflows(cx: &App) -> (String, BTreeMap<String, String>) {
    read(cx, |u| {
        let (default, by_label) = u.workflows();
        (default.to_string(), by_label.clone())
    })
    .unwrap_or_default()
}

/// Every workflow an issue can be worked with, as `(id, name, shipped)`:
/// the readable ones that work on a worktree, in the library's order.
pub fn issue_workflows(cx: &App) -> Vec<(String, String, bool)> {
    crate::workflow::templates(cx)
        .into_iter()
        .filter_map(|entry| {
            let template = entry.template.ok()?;
            (template.place == onehand_core::workflow::Place::Worktree)
                .then(|| (template.id, template.name, entry.file.is_none()))
        })
        .collect()
}

/// Make `default` the workflow issues are worked with and `by_label` the
/// workflow labels, for every run from now on, and write both to the config
/// at `path`. A run already working keeps the workflow it started with.
pub fn set_workflows(
    default: String,
    by_label: BTreeMap<String, String>,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let before = workflows(cx);
    let after = (default.clone(), by_label.clone());
    apply_workflows(default.clone(), by_label.clone(), cx);
    // Shown at once and written after; a write that fails puts both back.
    crate::saves::write_config(
        "unattended-workflows",
        "Saving the unattended runs",
        move |cfg| {
            cfg.unattended.workflow = default;
            cfg.unattended.workflows = by_label;
        },
        Some(crate::saves::Change {
            before,
            after,
            put: |(default, by_label), _, cx| apply_workflows(default, by_label, cx),
        }),
        window,
        cx,
    );
}

/// Put `default` and `by_label` in force for every run from now on.
fn apply_workflows(default: String, by_label: BTreeMap<String, String>, cx: &mut App) {
    crate::task::with_unattended(cx, |u, _| {
        u.set_workflows(default, by_label);
        ((), Vec::new())
    });
}

#[cfg(test)]
mod tests;
