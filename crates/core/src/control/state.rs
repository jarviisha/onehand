//! What a lead reads about its worker: the separate facts core keeps, one
//! per line, never folded into one word.
//!
//! **Nothing here says "stopped".** A turn over, no card waiting and no
//! activity reported is enough to send the next turn, never to conclude the
//! agent's processes are gone; and a lost link clears what core held, so
//! empty values then prove nothing at all and the activity reads unknown.
//! Only a closed session, which ends its agent, is proof, and that is the
//! caller's to say, not this.

use crate::acp::Role;
use crate::chat::model::Link;
use crate::chat::Chat;
use std::path::PathBuf;

/// How many background items the activity names before counting the rest.
const NAMED_BACKGROUND: usize = 5;

/// What the lead-worker relation knows beside the chat.
pub struct Held {
    /// The lead's last turn number sent, if it sent one.
    pub turn: Option<u64>,
    /// The worker's prompts sent once the lead's last went.
    pub sent: usize,
    /// The folders the worker may write besides its own.
    pub dirs: Vec<PathBuf>,
}

/// The worker's state, one fact per line.
pub fn state(chat: &Chat, held: &Held) -> String {
    let person = chat.prompted_since(held.sent);
    let lines = [
        format!("link: {}", link(chat)),
        format!("turn: {}", turn(chat)),
        format!("waiting: {}", waiting(chat)),
        format!("activity: {}", activity(chat)),
        format!("held by: {}", if person { "person" } else { "lead" }),
        format!("turns: {}", turns(chat, held, person)),
        format!("mode: {}", mode_said(chat)),
        format!("model: {}", model_said(chat)),
        format!("dirs: {}", dirs_said(&held.dirs)),
    ];
    lines.join("\n")
}

const OWN: &str = "the agent's own";

fn link(chat: &Chat) -> &'static str {
    match chat.link {
        Link::Connecting => "connecting",
        Link::Connected => "connected",
        Link::Lost => "lost",
    }
}

fn turn(chat: &Chat) -> String {
    if chat.prompts_sent == 0 {
        return "none".to_string();
    }
    if chat.busy {
        return "running".to_string();
    }
    if chat.turn_cut_off() {
        return "lost with the link".to_string();
    }
    if let Some(failure) = chat.failure() {
        return format!("failed: {failure}");
    }
    match chat.cancelled {
        true => "cancelled".to_string(),
        false => "ended".to_string(),
    }
}

fn waiting(chat: &Chat) -> String {
    let counted = |n: usize, one: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {one}s"),
    };
    let parts: Vec<String> = [
        (chat.pending_permissions().len(), "permission"),
        (chat.pending_asks().len(), "question"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, one)| counted(n, one))
    .collect();
    match parts.is_empty() {
        true => "none".to_string(),
        false => parts.join(", "),
    }
}

fn activity(chat: &Chat) -> String {
    if chat.link != Link::Connected {
        return "unknown".to_string();
    }
    let mut parts = Vec::new();
    if chat.busy {
        parts.push("the turn".to_string());
    }
    let background = chat.background();
    if !background.is_empty() {
        let mut names: Vec<&str> = background
            .iter()
            .take(NAMED_BACKGROUND)
            .map(|item| item.name.as_str())
            .collect();
        let more = background.len().saturating_sub(NAMED_BACKGROUND);
        let more = format!("and {more} more");
        if background.len() > NAMED_BACKGROUND {
            names.push(&more);
        }
        parts.push(format!(
            "{} in background: {}",
            background.len(),
            names.join(", ")
        ));
    }
    let subagents = chat.subagents_working();
    if subagents > 0 {
        parts.push(subagents_said(subagents));
    }
    if chat.working_by_itself() {
        parts.push("working by itself".to_string());
    }
    match parts.is_empty() {
        true => "none reported".to_string(),
        false => format!("running: {}", parts.join(", ")),
    }
}

fn subagents_said(n: usize) -> String {
    match n {
        1 => "1 subagent".to_string(),
        n => format!("{n} subagents"),
    }
}

fn turns(chat: &Chat, held: &Held, person: bool) -> String {
    match held.turn {
        None => "none sent".to_string(),
        // Over only as far as anyone knows: a link lost or still coming up
        // says nothing of how the turn went.
        Some(turn)
            if chat.link == Link::Connected && !chat.busy && !chat.turn_cut_off() && !person =>
        {
            format!("sent {turn}, ended {turn}")
        }
        Some(turn) => format!("sent {turn}, not yet ended"),
    }
}

/// The mode in force, as a worker's answers say it.
pub fn mode_said(chat: &Chat) -> String {
    chat.in_force().mode.unwrap_or_else(|| OWN.to_string())
}

/// The model in force, as a worker's answers say it.
pub fn model_said(chat: &Chat) -> String {
    chat.config_options
        .iter()
        .find(|option| option.role() == Some(Role::Model))
        .and_then(|option| option.current.clone())
        .unwrap_or_else(|| OWN.to_string())
}

/// The folders a worker may write, as its answers say them.
pub fn dirs_said(dirs: &[PathBuf]) -> String {
    match dirs.is_empty() {
        true => "none".to_string(),
        false => dirs
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests;
