//! A task's place: asking for it, waiting for it behind the task that holds
//! it, and giving it up to the next one waiting, in the window it was asked
//! from.

use super::{Effect, Placed, Signal, TasksDrive};
use crate::task::queue;
use crate::task::Task;
use std::path::PathBuf;

impl<W: Copy> TasksDrive<W> {
    /// Ask for task `id`'s place from `window`.
    pub fn request(&mut self, id: &str, window: W) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
        let told = |placed| Effect::Told {
            task: id.to_string(),
            placed,
        };
        let Some(task) = self.task(id) else {
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
        // it: the task reads cut off, as after a restart.
        if !window_open {
            fx.push(told(Placed::Refused("the window closed".to_string())));
            return;
        }
        if self.busy(&id) {
            fx.push(told(Placed::Refused(
                "that task is already running or waiting".to_string(),
            )));
            return;
        }
        let free = self.queue.ask(place.clone(), id.clone());
        if !free {
            self.asked_in.insert(id.clone(), window);
        }
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
    pub fn stop_queued(&mut self, id: &str) -> (bool, Vec<Effect<W>>) {
        let mut fx = Vec::new();
        if !self.queue.call_off(id) {
            return (false, fx);
        }
        // Never driven, so what was asked for its first drive goes with it: a
        // later Retry is a person's, and shows.
        self.off_screen.remove(id);
        self.asked_in.remove(id);
        let _ = self.task_mut(id).and_then(Task::take_unbegun);
        if let Some(task) = self.task(id) {
            fx.push(Effect::Save(Box::new(task.clone())));
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
