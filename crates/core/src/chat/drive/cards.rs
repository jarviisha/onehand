//! The answers a session's cards take: a permission's option, and a
//! question's picks, typed words, skips and sending.
//!
//! Where a reader is on a question card (which question shows, the
//! keyboard's row) is the front end's, in `AskPlace`; what the answers are,
//! and when they settle the form, is here.

use super::{Effect, SessionDrive};

impl SessionDrive {
    /// Answer the permission card at `idx` with `option_id`. A card already
    /// answered stays as it was: its request is resolved, and a second answer
    /// would echo an id the agent no longer holds.
    pub fn answer_permission(&mut self, idx: usize, option_id: &str) -> Vec<Effect> {
        self.chat.answer_permission(idx, option_id);
        Vec::new()
    }

    /// Pick `option` of `field` on the open question at `idx`, toggling it on
    /// a multi-select; the field's typed answer goes, as an answer is one or
    /// the other. A form of one single-select question has nothing left to
    /// decide, so the pick answers it: whether it did.
    pub fn ask_pick(&mut self, idx: usize, field: usize, option: usize) -> (bool, Vec<Effect>) {
        let Some(ask) = self.chat.ask_at_mut(idx) else {
            return (false, Vec::new());
        };
        ask.toggle(field, option);
        let quick = ask.is_quick();
        if quick {
            self.chat.answer_ask(idx, false);
        }
        (quick, Vec::new())
    }

    /// Type `text` as the answer to `field` of the open question at `idx`; a
    /// non-blank one drops the field's picks.
    pub fn ask_type(&mut self, idx: usize, field: usize, text: String) -> Vec<Effect> {
        if let Some(ask) = self.chat.ask_at_mut(idx) {
            ask.set_custom(field, text);
        }
        Vec::new()
    }

    /// Pass on `field` of the open question at `idx`, dropping what it held.
    /// Answers the question to show next, or `None` once that was the last and
    /// the form is settled: sent with what the other questions hold, or
    /// declined when none holds anything, as work already done above the
    /// skipped question is not thrown away at the last press.
    pub fn ask_skip(&mut self, idx: usize, field: usize) -> (Option<usize>, Vec<Effect>) {
        let last = self
            .chat
            .ask_at_mut(idx)
            .is_none_or(|ask| ask.skip_field(field));
        if !last {
            return (Some(field + 1), Vec::new());
        }
        let answered = self.chat.ask_at(idx).is_some_and(|ask| ask.has_answer());
        self.chat.answer_ask(idx, !answered);
        (None, Vec::new())
    }

    /// Move the open question at `idx` on from `field`, a multi-select left
    /// empty counting as answered with none. Answers the question to show
    /// next, or `None` once that was the last and the form is sent.
    pub fn ask_advance(&mut self, idx: usize, field: usize) -> (Option<usize>, Vec<Effect>) {
        let last = self
            .chat
            .ask_at_mut(idx)
            .is_none_or(|ask| ask.advance(field));
        if !last {
            return (Some(field + 1), Vec::new());
        }
        self.chat.answer_ask(idx, false);
        (None, Vec::new())
    }

    /// Send the open question at `idx` as it stands, once any of it holds an
    /// answer.
    pub fn ask_submit(&mut self, idx: usize) -> Vec<Effect> {
        if self.chat.ask_at(idx).is_some_and(|ask| ask.has_answer()) {
            self.chat.answer_ask(idx, false);
        }
        Vec::new()
    }

    /// Decline the open question at `idx`: the agent goes on with no answer.
    pub fn ask_decline(&mut self, idx: usize) -> Vec<Effect> {
        self.chat.answer_ask(idx, true);
        Vec::new()
    }
}
