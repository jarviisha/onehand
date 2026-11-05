//! An offloaded project's prompt, which waits for the project's code to go
//! up to its host first.

use super::ChatPane;
use crate::chat::composer::Composer;
use crate::chat::session::{ChatEvent, ChatSession};
use gpui::{App, Entity, Window};
use onehand_core::attachment::StagedAttachment;
use std::path::PathBuf;

impl ChatPane {
    /// Whether the prompt `(text, staged)` for `session` waits on its
    /// project's code: refused while that goes up, or sent once it has. A
    /// prompt that could go at once, or a session not on a host, is left to
    /// be sent as any is.
    pub(super) fn code_first(
        session: &Entity<ChatSession>,
        composer: &Entity<Composer>,
        (text, staged): (&str, &[StagedAttachment]),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if !session.read(cx).on_host {
            return false;
        }
        let root = session.read(cx).chat.root.clone();
        // Refused while it goes: the host would refuse the prompt anyway.
        if crate::offload::sending(cx, &root).is_some() {
            return true;
        }
        let blocked = session.read(cx).chat.submit_blocker(text, staged);
        if blocked.is_some() || !crate::offload::claim_send(cx, &root) {
            return false;
        }
        Self::send_code_then(session, composer, root, text.to_string(), window, cx);
        true
    }

    /// Send `root`'s code to its host, then the prompt `text` the composer
    /// holds, if it still holds it: one moved to another session stays a
    /// draft there, to send again.
    fn send_code_then(
        session: &Entity<ChatSession>,
        composer: &Entity<Composer>,
        root: PathBuf,
        text: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (session, composer) = (session.clone(), composer.clone());
        let line = crate::offload::sending_line_for(cx, &root);
        session.update(cx, |session, cx| {
            session.chat.say_host_wait(line);
            cx.emit(ChatEvent::Appended);
            cx.notify();
        });
        window
            .spawn(cx, async move |cx| {
                let went = crate::offload::send_up(cx, root).await;
                let _ = cx.update(|window, cx| {
                    session.update(cx, |session, cx| {
                        session.chat.unsay_host_wait();
                        cx.emit(ChatEvent::Appended);
                        cx.notify();
                    });
                    match went {
                        Ok(()) if composer.read(cx).text(cx) == text => {
                            Self::submit_from(&session, &composer, window, cx)
                        }
                        Ok(()) => {}
                        Err(why) => gpui_component::WindowExt::push_notification(
                            window,
                            crate::toast::failed("Sending the code to the host", why),
                            cx,
                        ),
                    }
                });
            })
            .detach();
    }
}
