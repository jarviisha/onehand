//! A task's place: asking for it, waiting for it behind the task that holds
//! it, and giving it up to the next one waiting, in the window it was asked
//! from.

use super::{Effect, Placed, Signal, TasksDrive};
use crate::task::queue;
use crate::task::Task;
use crate::workflow::Stop;
use crate::Now;
use std::path::PathBuf;

impl<W: Copy> TasksDrive<W> {
    /// Ask for task `id`'s place from `window`. `over_cap` is why the cap on
    /// unattended runs refuses it now, if it does.
    pub fn request(&mut self, id: &str, window: W, over_cap: Option<String>) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
        let told = |placed| Effect::Told {
            task: id.to_string(),
            placed,
        };
        if let Some(why) = over_cap {
            fx.push(told(Placed::Refused(why.clone())));
            // A retry's new run never began: dropped, the task is as it was.
            if let Some(task) = self.task_mut(id) {
                if task.drop_unbegun_retry() {
                    fx.push(Effect::Save(Box::new(task.clone())));
                }
            }
            fx.push(Effect::Signal(Signal::Refused { window, why }));
            fx.push(Effect::Refresh);
            return fx;
        }
        let Some(task) = self.task(id) else {
            fx.push(Effect::Asked(id.to_string()));
            fx.push(told(Placed::Refused("that task is gone".to_string())));
            return fx;
        };
        fx.push(Effect::FindPlace {
            task: id.to_string(),
            dir: task.setup.dir.clone(),
            window,
        });
        fx
    }

    pub(super) fn place_found(
        &mut self,
        id: String,
        place: PathBuf,
        window: W,
        window_open: bool,
        fx: &mut Vec<Effect<W>>,
    ) {
        let told = |placed| Effect::Told {
            task: id.clone(),
            placed,
        };
        // The window went while the place was looked up, so nothing asks for
        // it: the cap lets go here, and the task reads cut off, as after a
        // restart, rather than holding a slot until one.
        if !window_open {
            fx.push(Effect::Asked(id.clone()));
            fx.push(told(Placed::Refused("the window closed".to_string())));
            return;
        }
        if self.busy(&id) {
            fx.push(Effect::Asked(id.clone()));
            fx.push(told(Placed::Refused(
                "that task is already running or waiting".to_string(),
            )));
            return;
        }
        let free = self.queue.ask(place.clone(), id.clone());
        if !free {
            self.asked_in.insert(id.clone(), window);
        }
        // Asked for: an issue's run counts against the cap from here as
        // queued or running.
        fx.push(Effect::Asked(id.clone()));
        if free {
            return fx.push(Effect::Drive { task: id, window });
        }
        let title = self
            .task(&id)
            .map_or_else(String::new, |task| task.brief.title.clone());
        let at = place.file_name().map_or_else(
            || place.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let holder = self
            .queue
            .holder_of(&id)
            .and_then(|holder| self.task(holder));
        let said = queue::queued_said(
            &title,
            holder.map(|holder| holder.brief.title.as_str()),
            &at,
        );
        fx.push(Effect::Signal(Signal::Queued {
            window,
            said: said.clone(),
        }));
        fx.push(Effect::Refresh);
        fx.push(told(Placed::Queued(said)));
    }

    /// A person pressed Stop on queued task `id`: it no longer waits. One that
    /// never ran a step is over, stopped by a person, with no empty run kept;
    /// one resumed goes back to how it was. Whether it was queued.
    pub fn stop_queued(&mut self, id: &str, now: Now) -> (bool, Vec<Effect<W>>) {
        let mut fx = Vec::new();
        if !self.queue.call_off(id) {
            return (false, fx);
        }
        // Never driven, so what was asked for its first drive goes with it: a
        // later Retry is a person's, and shows.
        self.off_screen.remove(id);
        self.asked_in.remove(id);
        let dropped = self.task_mut(id).and_then(Task::take_unbegun);
        // An issue whose run never began is told it was stopped, or it would
        // stay claimed with nothing after the claim.
        let issue = self.task(id).is_some_and(|task| task.issue().is_some());
        let told = issue && dropped.is_some();
        if let Some(mut run) = dropped.filter(|_| issue) {
            // Through the engine, so the stop is in the run's history as any is.
            run.stopped(Stop::ByPerson, now.secs);
            self.keep_report(id, &run, None);
        }
        if let Some(task) = self.task(id) {
            fx.push(Effect::Save(Box::new(task.clone())));
        }
        if told {
            fx.push(Effect::Deliver(id.to_string()));
        }
        fx.extend(self.enforce_cap());
        fx.push(Effect::Refresh);
        (true, fx)
    }

    /// Give task `id`'s place up, and start the next task waiting for it in
    /// the window it was asked from. One with no window to start in stays
    /// cut off, and the place passes on.
    pub(super) fn release(&mut self, id: &str, fx: &mut Vec<Effect<W>>) {
        let next = self.queue.release(id).map(|next| {
            let asked = self.asked_in.remove(&next);
            (next, asked)
        });
        fx.extend(self.enforce_cap());
        fx.push(Effect::Refresh);
        match next {
            Some((next, Some(window))) => fx.push(Effect::Drive { task: next, window }),
            Some((next, None)) => self.release(&next, fx),
            None => {}
        }
    }
}
