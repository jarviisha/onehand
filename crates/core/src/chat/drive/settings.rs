//! What the agent is set to: the mode, its config options and the picks a
//! run puts in force, and the work it runs beside its turns.
//!
//! Each goes to the agent and shows at once: the agent confirms in its reply
//! rather than as an update, so waiting for an echo would leave the picker on
//! the old value. A refusal comes back as an event, and the model puts the
//! choice back.

use super::{Effect, SessionDrive};
use crate::workflow::Pick;

impl SessionDrive {
    /// Put the agent in mode `id`.
    pub fn set_mode(&mut self, id: &str) -> Vec<Effect> {
        self.chat.set_mode(id);
        self.sent(Vec::new())
    }

    /// Step to the mode after the one in force, wrapping at the end: the
    /// mode now in force, or `None` when the agent offers fewer than two.
    pub fn cycle_mode(&mut self) -> (Option<String>, Vec<Effect>) {
        let now = self.chat.cycle_mode().map(str::to_string);
        (now, self.sent(Vec::new()))
    }

    /// Set the agent's config option `id` (a model, an effort) to `value`.
    pub fn set_config_option(&mut self, id: &str, value: &str) -> Vec<Effect> {
        self.chat.set_config_option(id, value);
        self.sent(Vec::new())
    }

    /// Send one setting pick, as a run's step or onehand's tools put it in
    /// force.
    pub fn pick(&mut self, pick: &Pick) -> Vec<Effect> {
        self.chat.pick(pick);
        self.sent(Vec::new())
    }

    /// Choose `choice` of picker `group`, by position, as the remote bridge's
    /// buttons do: what was chosen, in the picker's words, or `None` when the
    /// group or the choice has moved since the buttons were drawn.
    pub fn choose(&mut self, group: &str, choice: usize) -> (Option<String>, Vec<Effect>) {
        let chosen = self.chat.choose(group, choice);
        (chosen, self.sent(Vec::new()))
    }

    /// Ask the agent to stop background item `id`. The list follows the
    /// agent's report that it ended, not this request.
    pub fn stop_background(&mut self, id: &str) -> Vec<Effect> {
        self.chat.stop_background(id);
        self.sent(Vec::new())
    }
}
