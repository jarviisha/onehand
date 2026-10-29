//! Opening a lead's worker: what the open does next against the worker's
//! live session, and the handle it answers with.
//!
//! **A worker is ready only on what it was asked to run.** The mode and the
//! model go in before the first prompt, one pick at a time, each answered
//! before the next. An agent that refuses one puts its old value back, so
//! "every pick answered" is not enough: the open is ready only when what is
//! in force is what was wanted, and refused otherwise. A worker never runs
//! silently on another model.

use crate::acp::Role;
use crate::chat::model::Link;
use crate::chat::{Chat, ChatItem};
use crate::workflow::{next, Next, Pick, Settings};

/// What an open does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The agent is starting, or a pick is still unanswered.
    Wait,
    /// Send this pick.
    Pick(Pick),
    /// The open fails, and why.
    Refused(String),
    /// The worker runs in the mode and on the model asked for.
    Ready,
}

/// The next step of an open wanting `mode` and `model` on `chat`, the groups
/// in `picked` already sent.
pub fn step(chat: &Chat, mode: Option<&str>, model: Option<&str>, picked: &[String]) -> Step {
    match chat.link {
        Link::Connecting => return Step::Wait,
        Link::Lost => return Step::Refused(format!("the agent did not start: {}", lost(chat))),
        Link::Connected => {}
    }
    if chat.picks_waiting() {
        return Step::Wait;
    }
    let mut want = Settings {
        mode: mode.map(str::to_string),
        options: Default::default(),
    };
    if let Some(model) = model {
        let Some(option) = chat
            .config_options
            .iter()
            .find(|option| option.role() == Some(Role::Model))
        else {
            return Step::Refused("the agent offers no models".to_string());
        };
        want.options.insert(option.id.clone(), model.to_string());
    }
    let named: Vec<String> = want
        .mode
        .iter()
        .map(|_| crate::chat::MODE_SELECTOR.to_string())
        .chain(want.options.keys().cloned())
        .collect();
    match next(&want, &named, &chat.offer(), &chat.in_force(), picked) {
        Next::Pick(pick) => Step::Pick(pick),
        Next::Refused(why) => Step::Refused(why),
        Next::Done => match missed(&want, &chat.in_force()) {
            Some(missed) => Step::Refused(format!("the agent did not take {missed}")),
            None => Step::Ready,
        },
    }
}

/// What of `want` is not in force, as a refusal names it.
fn missed(want: &Settings, in_force: &Settings) -> Option<String> {
    if let Some(mode) = want
        .mode
        .as_ref()
        .filter(|mode| in_force.mode.as_ref() != Some(*mode))
    {
        return Some(format!("mode `{mode}`"));
    }
    want.options
        .iter()
        .find(|(id, value)| in_force.options.get(*id) != Some(*value))
        .map(|(id, value)| format!("`{value}` for `{id}`"))
}

/// Why the link was lost, in the agent's own words where it gave any.
fn lost(chat: &Chat) -> String {
    chat.failure()
        .map(str::to_string)
        .or_else(|| {
            chat.items.iter().rev().find_map(|item| match item {
                ChatItem::Notice { text, .. } => Some(text.clone()),
                _ => None,
            })
        })
        .unwrap_or_else(|| "its link was lost".to_string())
}

/// The handle a tool names a worker by: the run of the app and its uid, since
/// uids count from 1 again in every run.
pub fn handle(run: &str, uid: u64) -> String {
    format!("{run}-{uid}")
}

#[cfg(test)]
mod tests;
