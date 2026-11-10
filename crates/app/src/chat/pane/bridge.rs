use super::ChatPane;
use gpui::{App, Context};
use onehand_core::chat::drive::{Effect, Input};
use onehand_core::chat::{Away, Presence, SessionSignal};

impl ChatPane {
    /// Every session in this pane, as somebody reading about them from outside
    /// the app would need them.
    ///
    /// Ordered by uid, which is also the number each one is listed under. A
    /// position in a list is the wrong handle for a chat to hold: the list is
    /// read, then a message is typed, and in between a session can be closed —
    /// so a number that means "the second one" would quietly come to mean a
    /// different conversation. A uid is minted once and never reused, so the
    /// number printed and the number typed back name the same session or name
    /// nothing at all.
    ///
    /// A session that has not reached a live adapter is skipped rather than
    /// listed as unavailable: it is still on its resume picker, so there is no
    /// conversation to name and nothing that could be sent to it.
    pub fn remote_sessions(&self, cx: &App) -> Vec<crate::remote::RemoteSession> {
        let mut uids: Vec<u64> = self.conversations.keys().copied().collect();
        uids.sort_unstable();
        uids.iter()
            .filter_map(|&uid| {
                let conv = self.conversations.get(&uid)?;
                let session = conv.session()?;
                Some(crate::remote::RemoteSession {
                    uid,
                    project: conv
                        .root
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| conv.root.display().to_string()),
                    conversation: session.read(cx).chat.conversation_title(),
                    agent: conv.spec.name.clone(),
                    // The rail's own word for the same condition, so a row on
                    // screen and a line on a phone cannot end up calling one
                    // state two things.
                    state: self.signal(uid, cx).map(SessionSignal::word),
                    here: conv.host.is_none(),
                })
            })
            .collect()
    }

    /// Answer the news `uid`'s drive asked about with where the person is,
    /// and carry what the drive says to tell.
    pub(super) fn look(
        &mut self,
        uid: u64,
        what: Away,
        agent: &str,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        let attention = self.presence(cx).seeing(uid);
        let left = session.update(cx, |session, cx| {
            let fx = session
                .chat
                .handle(Input::Seen { what, attention }, crate::chat::session::now());
            session.perform(fx, cx)
        });
        for effect in left {
            match effect {
                Effect::Tell {
                    what,
                    desktop,
                    chat,
                } => self.tell(uid, what, desktop, chat, agent, root, cx),
                // Only a tell answers a look; the rest the session carried.
                Effect::Signal(_)
                | Effect::Changed
                | Effect::OfferChanged
                | Effect::Save(_)
                | Effect::Arm { .. }
                | Effect::Disarm
                | Effect::SendCode { .. }
                | Effect::PromptAgain
                | Effect::Request(_)
                | Effect::SaveTitle { .. }
                | Effect::Look(_) => {}
            }
        }
        cx.notify();
    }

    /// Say `what` on `uid` outside the window: on the desktop, and to the
    /// chats following it.
    #[allow(clippy::too_many_arguments)]
    fn tell(
        &self,
        uid: u64,
        what: Away,
        desktop: bool,
        chat: bool,
        agent: &str,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        if desktop {
            match what {
                Away::TurnEnded => {
                    crate::chat::session::notify_turn_ended(agent.to_string(), root.to_string())
                }
                Away::Asked(ask) => crate::chat::session::notify_awaiting_user(
                    ask,
                    agent.to_string(),
                    root.to_string(),
                ),
                // Never put on the desktop: the drive asks for the chat only.
                Away::LinkLost => {}
            }
        }
        if chat && let Some(session) = self.session_of(uid) {
            let origin = self.origin(uid, agent, root, cx);
            let what = onehand_core::remote::link::announcement(&session.read(cx).chat, uid, what);
            crate::remote::announce(&origin, what, cx);
        }
    }

    /// The three facts about where the user is that the silence rules are
    /// decided from.
    ///
    /// Gathered here and handed over, because only this side can see them and
    /// only the other side should weigh them. The away flag goes through the
    /// bridge's own reader rather than straight to the global: it has one
    /// setter for the same reason, and a second way of reading it is a second
    /// place for the two to disagree.
    fn presence(&self, cx: &App) -> Presence {
        Presence {
            away: crate::remote::is_away(cx),
            window_active: cx.active_window() == Some(self.window),
            shown: self.active,
        }
    }

    /// Who an announcement about `uid` is from, in the words somebody who is not
    /// looking at the app would need.
    fn origin(&self, uid: u64, agent: &str, root: &str, cx: &App) -> crate::remote::Origin {
        crate::remote::Origin {
            uid,
            agent: agent.to_string(),
            project: root.to_string(),
            conversation: self.title_for(uid, cx),
            here: self
                .conversations
                .get(&uid)
                .is_some_and(|conv| conv.host.is_none()),
        }
    }
}
