//! Room at a limited step, as a run's drive asks for it: the run waits
//! before a full step with its clock stopped, is told when it has room, and
//! says when it has left a step it held room at. Who is given room is the
//! tasks' drive's to decide.

use super::{Asked, Effect, RunDrive};
use crate::chat::Chat;
use crate::task::rooms::RoomKey;
use crate::Now;

impl RunDrive {
    /// The run arrived at a full step: stop its clock, say so, and ask for
    /// room there.
    pub(super) fn await_room(&mut self, said: bool, now: Now, fx: &mut Vec<Effect>) {
        let Some(step) = self.run.current() else {
            return;
        };
        let Some(limit) = self.run.limit_at(self.run.step) else {
            return;
        };
        self.budget.pause(now.at);
        if said {
            self.asked
                .push(Asked::Note(format!("Waiting for room at {}.", step.label)));
        }
        fx.push(Effect::AskRoom {
            key: RoomKey {
                repo: self.run.setup.repo.clone(),
                workflow: self.run.template.id.clone(),
                step: step.id.clone(),
            },
            limit,
        });
    }

    /// The run was given the room it waited for: the step goes on, and its
    /// clock with the step's prompt.
    pub(super) fn room_given(&mut self, chat: Option<&Chat>, now: Now, fx: &mut Vec<Effect>) {
        if !self.run.awaiting_room() {
            return;
        }
        self.room = Some(self.run.step);
        self.advance(chat, now, fx, move |run| run.room_given(now.secs));
    }

    /// Say the run left the step it held room at, once it is at another, is
    /// waiting for room anew, or is over.
    pub(super) fn left_room(&mut self, fx: &mut Vec<Effect>) {
        let Some(at) = self.room else {
            return;
        };
        if at != self.run.step || self.run.awaiting_room() || self.run.over() {
            self.room = None;
            fx.push(Effect::LeaveRoom);
        }
    }
}
