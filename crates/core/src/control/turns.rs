//! When a lead may give its worker the next turn, or cancel the one running.
//!
//! **A send is never queued, and never sent twice.** A worker busy, still
//! connecting or lost refuses it in the composer's own words; the lead waits
//! for the turn to end. Each turn number goes once: a lead that lost the
//! answer sends the same turn and text again and hears it already went,
//! even while that turn still runs, so the retry is checked before anything
//! else. The same number with other text is a mistake, not a retry.
//!
//! **No tool answers a card.** A cancel is refused while a permission or a
//! question waits, so a lead never cuts off what a person is about to answer.

use crate::chat::Chat;

/// What the lead last sent its worker, as the relation keeps it.
pub struct Last {
    pub turn: Option<u64>,
    pub text: Option<String>,
    /// The worker's prompts sent once that turn went.
    pub sent: usize,
}

/// What a send does.
#[derive(Debug, PartialEq, Eq)]
pub enum Send {
    /// Send it: the next turn.
    Go,
    /// That turn already went, with that text: send nothing.
    AlreadySent(u64),
}

/// Whether turn `turn`, `text`, goes to the worker on `chat`.
pub fn send(chat: &Chat, last: &Last, turn: u64, text: &str) -> Result<Send, String> {
    held_by_lead(chat, last)?;
    match last.turn {
        Some(sent) if sent == turn => {
            return match last.text.as_deref() == Some(text) {
                true => Ok(Send::AlreadySent(turn)),
                false => Err(format!("turn {turn} was already sent with other text")),
            };
        }
        Some(sent) if sent > turn => {
            return Err(format!(
                "turn {turn} is below turn {sent}, the last you sent"
            ));
        }
        _ => {}
    }
    match chat.submit_blocker(text, &[]) {
        Some(why) => Err(why.hint()),
        None => Ok(Send::Go),
    }
}

/// Whether the turn running on `chat` may be cancelled: the lead's last
/// turn number when it may.
pub fn cancel(chat: &Chat, last: &Last) -> Result<u64, String> {
    held_by_lead(chat, last)?;
    if !chat.pending_permissions().is_empty() || !chat.pending_asks().is_empty() {
        return Err("a card waits on a person; only they answer it".to_string());
    }
    if !chat.busy {
        return Err("no turn is running".to_string());
    }
    Ok(last.turn.unwrap_or(0))
}

/// Refused once a prompt of the person's has gone since the lead's last.
fn held_by_lead(chat: &Chat, last: &Last) -> Result<(), String> {
    match chat.prompts_sent > last.sent {
        true => Err(
            "the person holds this worker: they have prompted it since your last turn".to_string(),
        ),
        false => Ok(()),
    }
}

#[cfg(test)]
mod tests;
