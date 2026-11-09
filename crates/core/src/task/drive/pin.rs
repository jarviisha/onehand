//! What a run holds while a mark is being pinned: a call that came in
//! meanwhile waits for the pin and for the action it holds back, and runs
//! after it, or, when it ends the run, instead of it.

use super::steps::ends_run;
use super::{Effect, RunDrive, Signal};
use crate::chat::Chat;
use crate::task::marks;
use crate::workflow::{Action, ApprovalAt, Stop};
use std::time::Instant;

/// A call held until a mark is pinned.
#[derive(Debug, Clone)]
pub(super) enum Held {
    Answer(Answer),
    Cut(Stop),
    End(Stop),
}

/// A person's answer to an approval.
#[derive(Debug, Clone)]
pub(super) enum Answer {
    Approved(ApprovalAt),
    Revised(ApprovalAt, String),
}

impl RunDrive {
    /// Hold `held`, which ends the run when `ends`, while a mark is being
    /// pinned: whether it was held.
    pub(super) fn hold(&mut self, ends: bool, held: Held) -> bool {
        match &mut self.held {
            Some(list) => {
                list.push((ends, held));
                true
            }
            None => false,
        }
    }

    /// Do what the engine said, once the work is pinned at every boundary the
    /// run has crossed since the last pin: the mark of a step's start lands
    /// before its prompt or command touches the work. A mark that cannot be
    /// pinned is said and left out; it never stops the run.
    pub(super) fn act(
        &mut self,
        action: Action,
        chat: Option<&Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        if ends_run(&action) || action == Action::Idle {
            return self.carry_out(action, chat, now, fx);
        }
        let refs = marks::refs_from(&self.task, &self.run, self.pinned);
        if refs.is_empty() {
            return self.carry_out(action, chat, now, fx);
        }
        let from = self.pinned;
        self.held = Some(Vec::new());
        self.waiting = Some((action, from, from + refs.len()));
        fx.push(Effect::Pin {
            dir: self.run.setup.dir.clone(),
            refs,
        });
    }

    /// The marks landed, or did not: do the action that waited on them, and
    /// then what was asked meanwhile.
    pub(super) fn landed(
        &mut self,
        pinned: Result<String, String>,
        chat: Option<&Chat>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        let Some((action, from, total)) = self.waiting.take() else {
            return;
        };
        match &pinned {
            Ok(commit) => self.run.pinned(commit, from),
            Err(why) => fx.push(Effect::Signal(Signal::PinFailed(why.clone()))),
        }
        self.pinned = total;
        let mut held = self.held.take().unwrap_or_default();
        // An ending asked meanwhile wins over the action, which would only
        // start work on a run that is over; the first one asked is its
        // reason, and the rest find nothing left to do.
        if let Some(at) = held.iter().position(|(ends, _)| *ends) {
            let (_, ending) = held.swap_remove(at);
            return self.replay(ending, chat, now, fx);
        }
        self.carry_out(action, chat, now, fx);
        // In the order asked; one that starts another pin holds the rest.
        for (_, then) in held {
            self.replay(then, chat, now, fx);
        }
    }

    fn replay(&mut self, held: Held, chat: Option<&Chat>, now: Instant, fx: &mut Vec<Effect>) {
        match held {
            Held::Answer(answer) => self.answer(answer, chat, now, fx),
            Held::Cut(stop) => self.cut(stop, chat, now, fx),
            Held::End(stop) => self.end(stop, chat, now, fx),
        }
    }
}
