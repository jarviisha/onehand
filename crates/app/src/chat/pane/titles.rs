//! A conversation's name: the one derived from its first prompt, and the
//! one a person gives it, written down as soon as it changes.

use super::ChatPane;
use gpui::{App, Context};

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

    /// Give a conversation a name of the user's choosing, written down at
    /// once; see [`onehand_core::chat::SessionDrive::rename`]. Whether
    /// anything changed: a blank name is not a rename.
    pub fn rename(&mut self, uid: u64, title: &str, cx: &mut Context<Self>) -> bool {
        let Some(session) = self.session_of(uid).cloned() else {
            return false;
        };
        let renamed = session.update(cx, |session, cx| session.rename(title, cx));
        cx.notify();
        renamed
    }

    /// Drop a custom name and go back to the automatic title: a worker's
    /// folder, else the one derived from the first prompt.
    pub fn reset_title(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        session.update(cx, |session, cx| session.reset_title(cx));
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
