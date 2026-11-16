//! An offloaded project's prompt, which waits for the project's code to go
//! up to its host first.
//!
//! Whether it waits is the session's drive's; what is here is where the code
//! stands, the send itself, and the composer the prompt waits in.

use super::ChatPane;
use crate::chat::composer::Composer;
use crate::chat::session::ChatSession;
use gpui::{App, Entity, Window};
use onehand_core::chat::drive::Code;

// The send of a project's code: the offloaded projects' carrier's, and in a
// test one that test answers.
#[cfg(not(test))]
use crate::offload::send_up;
#[cfg(test)]
use tests::send_up;

impl ChatPane {
    /// Where the code of `session`'s project stands, for a person's prompt.
    pub(super) fn code_at(session: &Entity<ChatSession>, cx: &App) -> Code {
        let session = session.read(cx);
        if !session.on_host {
            return Code::Here;
        }
        let root = &session.chat().root;
        if crate::offload::sending(cx, root).is_some() {
            return Code::Going;
        }
        match crate::offload::holds_code(cx, root) {
            true => Code::MustGo {
                line: crate::offload::sending_line_for(cx, root),
            },
            false => Code::Here,
        }
    }

    /// Carry out what only the pane can of a person's prompt to `session`,
    /// whose words were `text`: the code sent first, under `send_code`.
    pub(super) fn carry(
        session: &Entity<ChatSession>,
        composer: &Entity<Composer>,
        text: String,
        send_code: Option<u64>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(token) = send_code {
            Self::send_code(session, composer, text, token, window, cx)
        }
    }

    /// Send the project's code to its host, then tell the session what came
    /// of it, and whether the composer still holds the prompt `text`: one
    /// moved to another session stays a draft there, to send again.
    fn send_code(
        session: &Entity<ChatSession>,
        composer: &Entity<Composer>,
        text: String,
        token: u64,
        window: &mut Window,
        cx: &mut App,
    ) {
        let root = session.read(cx).chat().root.clone();
        // Claimed at once, so a second press waits on this send rather than
        // starting one of its own.
        let sending = send_up(cx, &root);
        let (session, composer) = (session.clone(), composer.clone());
        window
            .spawn(cx, async move |cx| {
                let result = match sending {
                    Some(sending) => sending.await,
                    None => Err("The code is no longer this machine's to send".into()),
                };
                let _ = cx.update(|window, cx| {
                    let still = composer.read(cx).text(cx) == text;
                    let again = session.update(cx, |session, cx| {
                        session.code_sent(token, result, still, cx)
                    });
                    // The code is up and the words still wait in the
                    // composer: they go now.
                    if again {
                        Self::submit_from(&session, &composer, window, cx);
                    }
                });
            })
            .detach();
    }
}

#[cfg(test)]
mod tests;
