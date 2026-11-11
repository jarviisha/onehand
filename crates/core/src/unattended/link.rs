//! The link between unattended runs and the tasks: what each drive's effects
//! become for the other.
//!
//! **Routing is behaviour, so it lives here.** That an issue's task asking
//! for its place stops counting as starting, that its end tells its issue,
//! that a claim's task is kept and asks for its place held to the cap, as a
//! task started off screen asks for its own, what a run tells unattended
//! runs, and that a report that landed leaves its task, are each a rule a
//! front end
//! would otherwise restate, untested. A front end holds the drives, joins
//! them here for one call, and carries out what comes back.

use super::drive::{Effect, On, UnattendedDrive, World};
use crate::task::drive::{RunDrive, Said};
use crate::task::tasks::link::Seen;
use crate::task::tasks::{self, TasksDrive};
use std::collections::VecDeque;

/// Put `value`, shown by the settings' drive, in force on unattended runs:
/// the workflows issues are worked with are theirs, the save the settings'.
pub fn shown(unattended: &mut UnattendedDrive, value: &crate::config::drive::Value) {
    use crate::config::drive::Value;
    match value {
        Value::Workflows { default, by_label } => {
            unattended.set_workflows(default.clone(), by_label.clone())
        }
        Value::Appearance(_) | Value::Outside(_) | Value::Agents => {}
    }
}

/// What the joined drives say to do, each drive's in its own order.
pub struct Out<W> {
    pub tasks: Vec<tasks::Effect<W>>,
    pub unattended: Vec<Effect<W>>,
}

/// The drives a front end holds, joined for one call: every task, the runs
/// under way, and unattended runs once they are set up.
pub struct Joined<'a, W> {
    tasks: &'a mut TasksDrive<W>,
    runs: Vec<&'a RunDrive>,
    unattended: Option<&'a mut UnattendedDrive>,
}

impl<'a, W: Copy> Joined<'a, W> {
    pub fn new(
        tasks: &'a mut TasksDrive<W>,
        runs: impl IntoIterator<Item = &'a RunDrive>,
        unattended: Option<&'a mut UnattendedDrive>,
    ) -> Self {
        Self {
            tasks,
            runs: runs.into_iter().collect(),
            unattended,
        }
    }

    /// Give the tasks' drive `command`, and route what it says.
    pub fn tasks<R>(
        &mut self,
        command: impl FnOnce(&mut TasksDrive<W>) -> (R, Vec<tasks::Effect<W>>),
    ) -> (R, Out<W>) {
        let (answer, fx) = command(self.tasks);
        (answer, self.settle(fx, Vec::new()))
    }

    /// Give unattended runs `command`, reading the tasks as they are, and
    /// route what it says. Nothing happens before they are set up.
    pub fn unattended<R: Default>(
        &mut self,
        command: impl FnOnce(&mut UnattendedDrive, &Seen<W>) -> (R, Vec<Effect<W>>),
    ) -> (R, Out<W>) {
        let Some(u) = self.unattended.as_deref_mut() else {
            return (R::default(), self.settle(Vec::new(), Vec::new()));
        };
        let seen = Seen::new(&*self.tasks, self.runs.iter().copied());
        let (answer, fx) = command(u, &seen);
        (answer, self.settle(Vec::new(), fx))
    }

    /// What run `task` told unattended runs (`said`), handed to their drive
    /// and routed. `world` is the projects switched on and the world as they
    /// are, read for what [`Said::looks`]; without it there is nothing to
    /// look at.
    pub fn run_said(&mut self, task: &str, said: Said, world: Option<(Vec<On>, &World)>) -> Out<W>
    where
        W: PartialEq,
    {
        self.unattended(|u, seen| {
            let fx = match (said, world) {
                (Said::Waiting, Some((roots, world))) => u.waiting(task, seen, roots, world),
                (Said::Answered, Some((roots, world))) => u.answered(task, seen, roots, world),
                (Said::Waiting | Said::Answered, None) => Vec::new(),
                (Said::CameUp { session }, _) => u.came_up(task, session, seen),
                (Said::Pause { why, fix }, _) => u.pause(&why, &fix),
            };
            ((), fx)
        })
        .1
    }

    /// Ask for task `id`'s place from `window`, held to the cap on
    /// unattended runs: every start goes through here.
    pub fn request(&mut self, id: &str, window: W) -> Out<W> {
        let fx = self.ask(id, window);
        self.settle(fx, Vec::new())
    }

    fn ask(&mut self, id: &str, window: W) -> Vec<tasks::Effect<W>> {
        let seen = Seen::new(&*self.tasks, self.runs.iter().copied());
        let over_cap = self
            .unattended
            .as_deref()
            .and_then(|u| u.over_cap(id, &seen));
        self.tasks.request(id, window, over_cap)
    }

    /// Route each drive's effects meant for the other into its commands,
    /// until none is left, and hand back the rest to carry out.
    fn settle(&mut self, tfx: Vec<tasks::Effect<W>>, ufx: Vec<Effect<W>>) -> Out<W> {
        let mut out = Out {
            tasks: Vec::new(),
            unattended: Vec::new(),
        };
        let (mut tq, mut uq) = (VecDeque::from(tfx), VecDeque::from(ufx));
        loop {
            if let Some(effect) = tq.pop_front() {
                // Without unattended runs set up, what is theirs goes nowhere.
                let seen = Seen::new(&*self.tasks, self.runs.iter().copied());
                let u = self.unattended.as_deref_mut();
                match (effect, u) {
                    (tasks::Effect::Ask { task, window }, _) => tq.extend(self.ask(&task, window)),
                    (tasks::Effect::Asked(id), Some(u)) => u.placed(&id),
                    (tasks::Effect::IssueEnded(id), Some(u)) => uq.extend(u.ended(&id, &seen)),
                    (tasks::Effect::Deliver(id), Some(u)) => uq.extend(u.deliver(&id, &seen)),
                    (tasks::Effect::DeliverAll, Some(u)) => uq.extend(u.deliver_all(&seen)),
                    (
                        tasks::Effect::Asked(_)
                        | tasks::Effect::IssueEnded(_)
                        | tasks::Effect::Deliver(_)
                        | tasks::Effect::DeliverAll,
                        None,
                    ) => {}
                    (other, _) => out.tasks.push(other),
                }
                continue;
            }
            let Some(effect) = uq.pop_front() else {
                break;
            };
            match effect {
                Effect::Submit { task, window } => {
                    let id = task.id.clone();
                    tq.extend(self.tasks.add(*task));
                    tq.extend(self.ask(&id, window));
                }
                Effect::AnswerReview {
                    task,
                    run,
                    template,
                    from,
                    note,
                    window,
                } => {
                    let (made, fx) = self.tasks.retry(
                        &task,
                        run,
                        *template,
                        from.as_deref(),
                        Some(note),
                        window,
                    );
                    // It asks for its place itself; one a run took up since
                    // the claim looked no longer counts as starting.
                    tq.extend(fx);
                    if !made {
                        if let Some(u) = self.unattended.as_deref_mut() {
                            u.placed(&task);
                        }
                    }
                }
                Effect::Drain { task, landed } => {
                    tq.extend(self.tasks.change_issue(&task, |issue| {
                        issue.unsent.drain(..landed.min(issue.unsent.len()));
                    }));
                }
                other => out.unattended.push(other),
            }
        }
        out
    }
}
