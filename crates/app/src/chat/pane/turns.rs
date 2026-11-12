//! A session's turns from the composer: what Enter sends or queues, a
//! queued prompt taken back, a turn stopped, and a failed one sent again.

use super::ChatPane;
use crate::chat::composer::Composer;
use crate::chat::session::ChatSession;
use gpui::{App, Context, Entity, Window};
use onehand_core::chat::drive::{Intake, Origin};

impl ChatPane {
    /// What Enter does, which depends on whether the popup is open.
    pub(super) fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session() else {
            return;
        };
        let accepted = self
            .composer
            .update(cx, |composer, cx| composer.commit(&session, window, cx));
        if !accepted {
            self.submit(window, cx);
        }
    }

    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session() else {
            return;
        };
        Self::submit_from(&session, &self.composer.clone(), window, cx);
    }

    /// Send what `composer` holds to `session`, or queue it behind the turn
    /// that runs.
    pub(super) fn submit_from(
        session: &Entity<ChatSession>,
        composer: &Entity<Composer>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (text, staged) = {
            let composer = composer.read(cx);
            (composer.text(cx), composer.attachments.clone())
        };
        // An offloaded project's code goes up before its prompt, and the
        // prompt waits in the composer meanwhile: sent once the host holds
        // the code, kept there when it could not get it.
        let code = Self::code_at(session, cx);
        let (intake, left) = session.update(cx, |session, cx| {
            session.prompt(&text, &staged, Origin::Person, code, cx)
        });
        // Clear only on a prompt that went somewhere -- sent, or queued behind
        // the turn that is running. A refused one (no agent yet, an unreadable
        // attachment) must not silently eat what the user typed or staged.
        match intake {
            Intake::Sent | Intake::Queued => {
                composer.update(cx, |composer, cx| composer.clear(window, cx))
            }
            Intake::Held | Intake::Waiting | Intake::CodeFirst | Intake::Refused(_) => {}
        }
        Self::carry(session, composer, text, left, window, cx);
    }

    /// Put the queued prompt back in the composer.
    ///
    /// Taking it back rather than discarding it: the user wrote it, and a
    /// cancel that throws the words away is a worse answer than one that hands
    /// them back to be edited.
    pub(super) fn unqueue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session() else {
            return;
        };
        let Some(pending) = session.update(cx, |session, cx| {
            let pending = session.chat.unqueue();
            if pending.is_some() {
                cx.notify();
            }
            pending
        }) else {
            return;
        };
        self.composer.update(cx, |composer, cx| {
            composer.restore_queued(pending, window, cx);
        });
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.session() {
            Self::cancel(&session, cx);
        }
    }

    /// Stop session `uid`'s turn, from its row in the rail.
    pub fn stop_turn(&mut self, uid: u64, cx: &mut Context<Self>) {
        if let Some(session) = self.session_of(uid).cloned() {
            Self::cancel(&session, cx);
        }
    }

    pub(super) fn cancel(session: &Entity<ChatSession>, cx: &mut App) {
        session.update(cx, |session, cx| session.stop(cx));
    }

    /// Whether session `uid` has a failed turn whose prompt can go again.
    pub fn can_resend(&self, uid: u64, cx: &App) -> bool {
        self.session_of(uid)
            .is_some_and(|session| session.read(cx).chat.resend_text().is_some())
    }

    /// Send session `uid`'s last prompt again, after a turn that failed.
    pub fn resend_last_prompt(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        session.update(cx, |session, cx| session.resend(cx));
    }
}
