//! A conversation's name: the one derived from its first prompt, and the
//! one a person gives it, written down as soon as it changes.

use super::{ChatPane, ChatPaneEvent};
use crate::chat::session::ChatSession;
use gpui::{App, Context, Entity};

impl ChatPane {
    /// What the conversation on `uid` is called, once it has earned a name.
    ///
    /// A *query* for the same reason [`Self::signal`] is one: the title is
    /// derived from the first prompt, so it arrives mid-conversation, and a
    /// copy of it one level up in the workspace tree would have nothing that
    /// could keep it current.
    ///
    /// `None` until a prompt exists, unless a worker's folder stands in — an
    /// unnamed conversation is the caller's to label, and for the rail that
    /// means falling back to the agent's name.
    pub fn title_for(&self, uid: u64, cx: &App) -> Option<String> {
        self.session_of(uid)?.read(cx).chat.conversation_title()
    }

    /// Give a conversation a name of the user's choosing.
    ///
    /// Written down immediately rather than at the end of the next turn. A
    /// rename is often the last thing done to a finished conversation, and a
    /// title that survives only until the next prompt is a title that is usually
    /// lost.
    ///
    /// The name only, not the transcript: a rename can land in the middle of a
    /// turn, and this must not commit a half-finished turn to the file.
    ///
    /// Returns whether anything changed — a blank name is not a rename, and
    /// `Chat::rename` is where that rule lives.
    pub fn rename(&mut self, uid: u64, title: &str, cx: &mut Context<Self>) -> bool {
        let Some(session) = self.session_of(uid).cloned() else {
            return false;
        };
        let before = session.read(cx).chat.custom_title.clone();
        if !session.update(cx, |session, _| session.chat.rename(title)) {
            return false;
        }
        self.save_title(uid, before, &session, cx);
        cx.notify();
        true
    }

    /// Write a new name for `uid` in order after every rename before it, and
    /// put the last saved name back if this one, as the newest, fails, said
    /// once. `before` is the name the change replaced.
    fn save_title(
        &mut self,
        uid: u64,
        before: Option<String>,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) {
        let now = session.read(cx).chat.custom_title.clone();
        let Some(conv) = self.conversations.get_mut(&uid) else {
            return;
        };
        let this = conv
            .title
            .get_or_insert_with(|| onehand_core::saving::Saving::new(before))
            .change(now.clone());
        let Some(pending) = session.update(cx, |session, _| session.chat.flush_meta()) else {
            return;
        };
        let written = crate::state::Shared::global(cx)
            .writer
            .write(move || onehand_core::chat::commit(&pending).map_err(|why| why.to_string()));
        cx.spawn(async move |pane, cx| {
            let result = written.await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                let Some(conv) = pane.conversations.get_mut(&uid) else {
                    return;
                };
                let Some(title) = conv.title.as_mut() else {
                    return;
                };
                let Some(why) = title.landed(this, now, result) else {
                    return;
                };
                let shown = title.shown().clone();
                if let Some(session) = conv.session() {
                    session.update(cx, |session, _| match shown {
                        Some(name) => {
                            session.chat.rename(&name);
                        }
                        None => session.chat.reset_title(),
                    });
                }
                cx.emit(ChatPaneEvent::RenameFailed(why));
                cx.notify();
            });
        })
        .detach();
    }

    /// Drop a custom name and go back to the automatic title: a worker's
    /// folder, else the one derived from the first prompt.
    pub fn reset_title(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        let before = session.read(cx).chat.custom_title.clone();
        session.update(cx, |session, _| session.chat.reset_title());
        self.save_title(uid, before, &session, cx);
        cx.notify();
    }

    /// The name the user typed, if they have typed one.
    ///
    /// Distinct from [`Self::title_for`], which falls back to the derived
    /// title: a rename field must open on what the user set, not on the
    /// summary the app guessed, or accepting the prefilled value would silently
    /// freeze that guess in place forever.
    pub fn custom_title(&self, uid: u64, cx: &App) -> Option<String> {
        self.session_of(uid)?.read(cx).chat.custom_title.clone()
    }
}
