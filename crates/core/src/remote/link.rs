//! Where the remote bridge meets a session's drive: what a prompt, a stop, a
//! press or a look at the pickers from outside the app does to one session,
//! and what a session's news says on a chat.
//!
//! The bridge's drive never holds a session's; its front end finds the
//! session an effect names and calls these, then reports back what they
//! answered.

use super::drive::{Handled, Stopped};
use super::press::{self, Press};
use super::types::Button;
use super::Announcement;
use crate::chat::drive::{Code, Effect, Intake, Origin, Refusal};
use crate::chat::{Away, ChatItem, Now, SessionDrive, UserAsk};

/// How much of a finished answer rides along with the turn-ended announcement.
///
/// Sized for the surface it lands on rather than for the answer: a notification
/// is read at a glance on a phone, so this is a few sentences, enough for the
/// paragraph an answer closes with. Whatever the channel itself will not carry
/// is clipped again on the way out, so this is the smaller of two bounds and the
/// one chosen for how it reads.
const ANSWER_TAIL_MAX: usize = 700;

/// What a press on a card that is no longer open is told.
///
/// One sentence for every way it can happen (answered in the window, by an
/// earlier press on the same message, or a form this build can no longer
/// read), because they are the same fact to whoever pressed it: there is
/// nothing there to answer.
const SETTLED: &str = "That's already been answered.";

/// Send `text` to the session as a prompt from outside the app.
///
/// **Straight into the session, not through a composer**, whose draft is
/// whatever the person at the keyboard was typing. The busy case queues, since
/// the sender cannot see a turn is in flight; an occupied queue is a refusal,
/// said out loud, since queueing over it would destroy a prompt already
/// acknowledged (`SessionDrive::prompt`'s table). A prompt from outside never
/// waits on an offloaded project's code: the bridge reaches no session whose
/// project runs on a host.
pub fn prompt(session: &mut SessionDrive, text: &str, now: Now) -> (Handled, Vec<Effect>) {
    let (intake, fx) = session.prompt(text, &[], Origin::Remote, Code::Here, now);
    let handled = match intake {
        Intake::Sent => Handled::Sent,
        Intake::Queued => Handled::Queued,
        Intake::Refused(Refusal::AlreadyQueued) => Handled::Refused(
            "a prompt is already waiting behind this turn — send it again once that one has gone"
                .to_string(),
        ),
        // A standing condition the sender has to hear about in its own words:
        // an adapter that is not up, a message with nothing in it.
        Intake::Refused(why) => Handled::Refused(why.hint()),
        Intake::Held | Intake::Waiting | Intake::CodeFirst => {
            Handled::Refused(Refusal::NotTaken.hint())
        }
    };
    (handled, fx)
}

/// Cancel the session's turn from outside the app.
///
/// **What waited behind the turn is taken back first**, then the cancel goes:
/// a turn's end sends what is queued, and from a chat there is no chip to see,
/// so a stop that quietly launched the next prompt would be the opposite of
/// what was asked. Taken back and quoted, not dropped: somebody typed it.
pub fn stop(session: &mut SessionDrive) -> (Stopped, Vec<Effect>) {
    if !session.busy {
        return (Stopped::Idle, Vec::new());
    }
    let taken_back = session.unqueue().map(|pending| pending.text);
    let fx = session.stop();
    (Stopped::Cancelled { taken_back }, fx)
}

/// What `uid`'s agent lets a chat change, as a message and its buttons.
///
/// An agent that offers nothing gets a sentence saying so: a blank reply reads
/// as a command that failed.
pub fn options(session: &SessionDrive, uid: u64) -> (String, Vec<Vec<Button>>) {
    let selectors = session.selectors();
    if selectors.is_empty() {
        return (
            format!("{uid}'s agent doesn't offer anything to change."),
            Vec::new(),
        );
    }
    let mut text = String::new();
    let mut buttons = Vec::new();
    for selector in &selectors {
        let (rows, dropped) = press::option_buttons(uid, selector);
        let here = selector
            .current
            .as_ref()
            .and_then(|current| {
                selector
                    .choices
                    .iter()
                    .find(|choice| choice.value == *current)
            })
            .map(|choice| choice.label.clone())
            // A picker the agent has not settled yet, or whose value this
            // build does not recognise: neither has a name to print.
            .unwrap_or_else(|| "not set".to_string());
        text.push_str(&format!("{} · {here}\n", selector.name));
        // Said rather than silently dropped: a picker missing two of its
        // choices reads as a picker that only has the rest.
        if dropped > 0 {
            text.push_str(&format!(
                "    {dropped} of its choices can't be offered here — use the app.\n"
            ));
        }
        buttons.extend(rows);
    }
    (text.trim_end().to_string(), buttons)
}

/// Carry out a press on the session it names, and what to tell the presser.
///
/// **The card is the one the button named**, by its place: a message stays
/// pressable as long as it is scrollable, and a card answered since, in the
/// window or by an earlier press, is said settled rather than have the press
/// slide onto the next one. Every way it cannot be carried out is said: a
/// press that quietly did nothing would leave its presser believing the agent
/// unblocked.
pub fn press(session: &mut SessionDrive, press: Press) -> (String, Vec<Effect>) {
    // Every press that names a card names one; the picker is the one that
    // does not, and it is the arm that never reads this.
    let item = press.item().unwrap_or_default();
    let said = |words: &str| (words.to_string(), Vec::new());
    match press {
        Press::Permission { option, .. } => {
            let Some(ChatItem::Permission(perm)) = session.items.get(item) else {
                return said(SETTLED);
            };
            if perm.resolved.is_some() {
                return said(SETTLED);
            }
            // An index that names nothing is a refusal: a press whose meaning
            // cannot be established must not grant something, nor leave the
            // agent parked in silence.
            let Some(chosen) = press::option_at(&perm.req.options, option) else {
                return said("That permission offers nothing this can answer with.");
            };
            let (id, name) = (chosen.id.clone(), chosen.name.clone());
            (format!("{name}."), session.answer_permission(item, &id))
        }
        Press::Question { choice, .. } => {
            // The guard the buttons were built behind, checked again: a message
            // outlives the form it was drawn for.
            let label = session.ask_at(item).and_then(|ask| {
                let field = ask.req.fields.first()?;
                Some(field.kind.choices().get(choice)?.label.clone())
            });
            let Some(label) = label else {
                return said(SETTLED);
            };
            // Sent whatever shape the form is: the press is the answer.
            let (settled, mut fx) = session.ask_pick(item, 0, choice);
            if !settled {
                fx.extend(session.ask_submit(item));
            }
            (format!("{label}."), fx)
        }
        Press::Skip { .. } => {
            if session.ask_at(item).is_none() {
                return said(SETTLED);
            }
            let fx = session.ask_decline(item);
            (
                "Skipped — the agent carries on without an answer.".to_string(),
                fx,
            )
        }
        // Not a card: what the agent offers is live, and the drive refuses a
        // group or a choice that has moved rather than settling for the nearest.
        Press::Option { group, choice, .. } => {
            let (chosen, fx) = session.choose(&group, choice);
            let words = chosen.unwrap_or_else(|| {
                "That isn't offered any more — ask for /options again.".to_string()
            });
            (words, fx)
        }
    }
}

/// What a chat is told of `what` on session `uid`, beyond its headline.
///
/// A finished turn carries the end of what the agent said, which is where an
/// answer says what it did. A waiting card carries the question itself and
/// the buttons to answer it, of the newest card of the kind that parked: an
/// older one is already in somebody's chat with its own buttons, and `ask`
/// decides which list is read, since a permission and a question can both
/// wait. A card settled meanwhile, or a form a row of buttons cannot express,
/// goes without buttons: knowing an agent is blocked is worth more than being
/// able to unblock it from here.
pub fn announcement(session: &SessionDrive, uid: u64, what: Away) -> Announcement {
    let (detail, buttons) = match what {
        Away::TurnEnded => (session.answer_tail(ANSWER_TAIL_MAX), Vec::new()),
        Away::Asked(UserAsk::Permission) => session
            .pending_permissions()
            .last()
            .map(|(item, perm)| {
                (
                    Some(perm.req.title.clone()),
                    press::permission_buttons(uid, *item, &perm.req.options),
                )
            })
            .unwrap_or((None, Vec::new())),
        Away::Asked(UserAsk::Question) => session
            .pending_asks()
            .last()
            .map(|(item, parked)| {
                (
                    Some(parked.req.message.clone()),
                    press::question_buttons(uid, *item, &parked.req.fields),
                )
            })
            .unwrap_or((None, Vec::new())),
        Away::LinkLost => (None, Vec::new()),
    };
    Announcement {
        away: what,
        detail,
        buttons,
    }
}

#[cfg(test)]
mod tests;
