//! An offloaded project's prompt, which waits for the project's code to go
//! up to its host first.
//!
//! Whether it waits is the session's drive's; what is here is where the code
//! stands, the send itself, and the composer the prompt waits in.

use super::ChatPane;
use crate::chat::composer::Composer;
use crate::chat::session::ChatSession;
use gpui::{App, Entity, Window};
use onehand_core::chat::drive::{Code, Effect, Input};

impl ChatPane {
    /// Where the code of `session`'s project stands, for a person's prompt.
    pub(super) fn code_at(session: &Entity<ChatSession>, cx: &App) -> Code {
        let session = session.read(cx);
        if !session.on_host {
            return Code::Here;
        }
        let root = &session.chat.root;
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
    /// whose words were `text`: the code sent first, and the prompt sent again
    /// from the composer once the code is up.
    pub(super) fn carry(
        session: &Entity<ChatSession>,
        composer: &Entity<Composer>,
        text: String,
        fx: Vec<Effect>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for effect in fx {
            match effect {
                Effect::SendCode { token } => {
                    Self::send_code(session, composer, text.clone(), token, window, cx)
                }
                Effect::PromptAgain => Self::submit_from(session, composer, window, cx),
                // The session carried these out itself.
                Effect::Signal(_)
                | Effect::Changed
                | Effect::OfferChanged
                | Effect::Save(_)
                | Effect::Arm { .. }
                | Effect::Disarm => {}
            }
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
        let root = session.read(cx).chat.root.clone();
        // Claimed at once, so a second press waits on this send rather than
        // starting one of its own.
        let claimed = crate::offload::claim_send(cx, &root);
        let (session, composer) = (session.clone(), composer.clone());
        window
            .spawn(cx, async move |cx| {
                let result = match claimed {
                    true => crate::offload::send_up(cx, root).await,
                    false => Err("The code is no longer this machine's to send".into()),
                };
                let _ = cx.update(|window, cx| {
                    let still = composer.read(cx).text(cx) == text;
                    let left = session.update(cx, |session, cx| {
                        let fx = session.chat.handle(Input::CodeSent {
                            token,
                            result,
                            still,
                        });
                        let left = session.perform(fx, cx);
                        cx.notify();
                        left
                    });
                    Self::carry(&session, &composer, text, left, window, cx);
                });
            })
            .detach();
    }
}
