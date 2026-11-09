//! The link between the tasks and the runs under way: what a run's write
//! and its end become for the task it is of.
//!
//! **Routing is behaviour, so it lives here.** That a run's save writes its
//! whole task, that its place is held while its session's turn still goes,
//! and that the question a parked card asked goes into an issue's report are
//! each a rule a front end would otherwise restate, untested. A front end
//! calls these with the drives it holds and carries out what comes back.

use super::{Effect, TasksDrive};
use crate::chat::Chat;
use crate::task::drive::RunDrive;
use crate::task::Task;

/// `run` asked to be saved: the task it is of, keeping the run as it now
/// stands, to write.
pub fn save_run<W: Copy>(tasks: &mut TasksDrive<W>, run: &RunDrive) -> Option<Task> {
    tasks.keep_run(run.task(), run.run().clone())
}

/// `run` finished, on `session` (`uid`, in `window`) if it had one: what its
/// task does about it.
pub fn finished<W: Copy>(
    tasks: &mut TasksDrive<W>,
    run: &RunDrive,
    session: Option<(&Chat, u64)>,
    window: W,
) -> Vec<Effect<W>> {
    let chat = session.map(|(chat, _)| chat);
    let asked = chat.filter(|_| run.card()).and_then(card_question);
    tasks.run_ended(
        run.task(),
        run.run().clone(),
        run.pinned(),
        chat.is_some_and(|chat| chat.busy),
        asked,
        session.map(|(_, uid)| uid),
        window,
    )
}

/// What the card `chat` parked last asks a person: a question's message, or
/// a permission's title.
pub fn card_question(chat: &Chat) -> Option<String> {
    chat.pending_asks()
        .last()
        .map(|(_, ask)| ask.req.message.clone())
        .or_else(|| {
            chat.pending_permissions()
                .last()
                .map(|(_, permission)| permission.req.title.clone())
        })
}
