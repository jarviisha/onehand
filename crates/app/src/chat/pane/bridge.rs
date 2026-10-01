use super::{ANSWER_TAIL_MAX, ChatPane, QUOTED_PROMPT_MAX, SETTLED};
use gpui::{App, Context};
use onehand_core::chat::{Away, Chat, ChatItem, Presence, Telling};
use onehand_core::remote::types::Button;
use onehand_core::remote::{Press, press};

impl ChatPane {
    /// Send `text` to `uid` as a prompt from outside the app.
    ///
    /// `None` means this pane has no such session, which is how a caller
    /// walking every window finds the one that does.
    ///
    /// **Straight into the session, not through the composer.** One composer
    /// serves the whole pane and what it holds is whatever the person at the
    /// keyboard was in the middle of typing — putting a message from a phone
    /// into it would overwrite their draft, and sending it would send theirs.
    ///
    /// The busy case is queued rather than refused for the same reason the
    /// composer queues: the sender is not watching the transcript and cannot
    /// tell that a turn is in flight, so refusing would mean their message is
    /// simply lost to timing they had no way to see.
    ///
    /// **But the queue is one slot, and `Chat::queue` replaces what is in it.**
    /// A prompt from outside dropped into an occupied slot destroys whatever was
    /// there — the draft somebody at the keyboard queued behind this turn, or the
    /// message this same chat sent a moment earlier — and both of those were
    /// acknowledged as though they were going to be sent. So an occupied slot is
    /// a refusal, said out loud: a message the sender knows did not go can be
    /// sent again, and one they believe went cannot be recovered at all.
    pub fn remote_prompt(
        &mut self,
        uid: u64,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Option<crate::remote::Handled> {
        use crate::remote::Handled;
        let session = self.session_of(uid)?.clone();
        let handled = session.update(cx, |session, cx| {
            // Read before anything is sent: submitting clears the condition
            // that would have explained a refusal.
            let blocker = session.chat.submit_blocker(text, &[]);
            let taken = session.chat.queued.is_some();
            match blocker {
                None if session.submit(text, &[], cx) => Handled::Sent,
                Some(onehand_core::chat::SubmitBlock::Busy) if taken => Handled::Refused(
                    "a prompt is already waiting behind this turn — send it again once that one \
                     has gone"
                        .to_string(),
                ),
                Some(onehand_core::chat::SubmitBlock::Busy) if session.chat.queue(text, &[]) => {
                    cx.notify();
                    Handled::Queued
                }
                // Everything else is a standing condition the sender has to
                // hear about in its own words -- an adapter that is not up, a
                // message with nothing in it.
                blocker => Handled::Refused(
                    blocker
                        .map(|b| b.hint())
                        .unwrap_or_else(|| "The prompt was not accepted".to_string()),
                ),
            }
        });
        cx.notify();
        Some(handled)
    }

    /// The pickers `uid`'s agent offers, as a message and its buttons.
    ///
    /// `None` means this pane has no such session. An agent that offers nothing
    /// gets a sentence saying so rather than an empty message — not every
    /// adapter advertises modes or config groups, and a blank reply reads as a
    /// command that failed.
    pub fn remote_options(&self, uid: u64, cx: &App) -> Option<(String, Vec<Vec<Button>>)> {
        let selectors = self.session_of(uid)?.read(cx).chat.selectors();
        if selectors.is_empty() {
            return Some((
                format!("{uid}'s agent doesn't offer anything to change."),
                Vec::new(),
            ));
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
                // A picker the agent has not settled yet, which is a different
                // thing from one whose value this build failed to recognise --
                // but the same sentence either way, since neither has a name to
                // print.
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
        Some((text.trim_end().to_string(), buttons))
    }

    /// Cancel the turn running on `uid`, from outside the app.
    ///
    /// `None` means this pane has no such session, the same handshake the other
    /// remote paths use to find the window that does.
    ///
    /// **Anything queued is taken back rather than left to fire.** Cancelling
    /// ends the turn, and the end of a turn is precisely what sends whatever was
    /// waiting behind it — so a plain cancel would stop the work and start the
    /// next piece in the same breath. At the keyboard that is survivable,
    /// because the queued prompt is on screen as a chip and the person pressing
    /// Stop can see it; from a chat there is nothing to see, and a stop that
    /// quietly launches something else is the opposite of what was asked for.
    /// Taken back and quoted, not dropped: the words were typed by somebody and
    /// they can decide whether to send them again.
    ///
    /// The order is the whole of it — the queue is emptied *before* the cancel
    /// goes out, so there is no arrangement of replies from the adapter that can
    /// flush it on the way past.
    pub fn remote_stop(&mut self, uid: u64, cx: &mut Context<Self>) -> Option<String> {
        let session = self.session_of(uid)?.clone();
        let said = session.update(cx, |session, cx| {
            if !session.chat.busy {
                // Not "stopped": nothing was running, and saying otherwise would
                // have the reader believe they had just cut something short.
                return format!("Nothing is running on {uid}.");
            }
            let dropped = session.chat.unqueue();
            session.chat.cancel_turn();
            cx.notify();
            match dropped {
                None => format!("Stopped {uid}."),
                Some(pending) => format!(
                    "Stopped {uid}. The prompt waiting behind it was taken back, not sent:\n\n{}",
                    onehand_core::chat::first_line_trunc(&pending.text, QUOTED_PROMPT_MAX)
                ),
            }
        });
        cx.notify();
        Some(said)
    }

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
                    state: self.signal(uid, cx).map(crate::rail::signal_word),
                })
            })
            .collect()
    }

    /// A turn settled. Badge it, and say so out loud wherever
    /// [`Attention::telling`] sends it.
    ///
    /// [`Attention::telling`]: onehand_core::chat::Attention::telling
    pub(super) fn turn_ended_detached(
        &mut self,
        uid: u64,
        agent: &str,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        let say = self.telling(uid, Away::TurnEnded, cx);
        // Before any of the three payloads, each of which reads the transcript
        // to build itself -- and before the repaint, which the reader of this
        // conversation is already getting from the turn itself.
        if say.silent() {
            return;
        }
        if say.badge
            && let Some(conv) = self.conversations.get_mut(&uid)
        {
            conv.unseen = true;
        }
        if say.desktop {
            crate::chat::session::notify_turn_ended(agent.to_string(), root.to_string());
        }
        if say.chat {
            let origin = self.origin(uid, agent, root, cx);
            crate::remote::announce(
                &origin,
                crate::remote::Announcement {
                    away: Away::TurnEnded,
                    // The end of what the agent said, which is where an answer
                    // says what it did. "Finished a turn" on its own is a
                    // notification whose only content is that there is content:
                    // it costs a walk back to the machine to find out whether
                    // anything needs doing. Not put on the desktop
                    // notification, which does not need it -- the window it is
                    // about is one keystroke away, and the transcript is in it.
                    detail: self.answer_tail(uid, cx),
                    buttons: Vec::new(),
                },
                cx,
            );
        }
        cx.notify();
    }

    /// How the agent's last answer on `uid` ended, short enough to read on a
    /// phone.
    ///
    /// The rule is core's, because where an answer's summary is and how to cut
    /// to it is a fact about prose rather than about a channel.
    pub fn answer_tail(&self, uid: u64, cx: &App) -> Option<String> {
        self.session_of(uid)?
            .read(cx)
            .chat
            .answer_tail(ANSWER_TAIL_MAX)
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

    /// What is said about one piece of news on `uid`, in the three places it
    /// could go.
    fn telling(&self, uid: u64, what: Away, cx: &App) -> Telling {
        self.presence(cx).seeing(uid).telling(what)
    }

    /// Who an announcement about `uid` is from, in the words somebody who is not
    /// looking at the app would need.
    fn origin(&self, uid: u64, agent: &str, root: &str, cx: &App) -> crate::remote::Origin {
        crate::remote::Origin {
            uid,
            agent: agent.to_string(),
            project: root.to_string(),
            conversation: self.title_for(uid, cx),
        }
    }

    /// The adapter went away. Say so outside the window, if
    /// [`Attention::telling`] says anyone should hear it.
    ///
    /// [`Attention::telling`]: onehand_core::chat::Attention::telling
    pub(super) fn link_lost_detached(
        &mut self,
        uid: u64,
        agent: &str,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.telling(uid, Away::LinkLost, cx).chat {
            return;
        }
        let origin = self.origin(uid, agent, root, cx);
        crate::remote::announce(
            &origin,
            crate::remote::Announcement::plain(Away::LinkLost),
            cx,
        );
    }

    /// An agent stopped and is waiting on the user. Say so wherever
    /// [`Attention::telling`] sends it.
    ///
    /// The repaint happens either way, unlike a finished turn's: the card the
    /// agent parked is in the transcript now, and the reader of that
    /// conversation is exactly who has to see it appear.
    ///
    /// [`Attention::telling`]: onehand_core::chat::Attention::telling
    pub(super) fn awaiting_user_detached(
        &mut self,
        uid: u64,
        ask: onehand_core::chat::UserAsk,
        agent: &str,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        let say = self.telling(uid, Away::Asked(ask), cx);
        if say.desktop {
            crate::chat::session::notify_awaiting_user(ask, agent.to_string(), root.to_string());
        }
        if say.chat {
            let origin = self.origin(uid, agent, root, cx);
            // The one announcement that carries the question itself and the
            // buttons to answer it. A desktop notification cannot do better than
            // point at the window, because the card is already in it; a message
            // on a phone is the only place the answer can be given from, so it
            // has to carry what is being asked as well as who is asking.
            let (detail, buttons) = self.parked_ask(uid, ask, cx);
            crate::remote::announce(
                &origin,
                crate::remote::Announcement {
                    away: Away::Asked(ask),
                    detail,
                    buttons,
                },
                cx,
            );
        }
        cx.notify();
    }

    /// What `uid` is waiting on, and what can be answered without opening the
    /// app.
    ///
    /// **`ask` decides which card is read, and it is not a hint.** An agent can
    /// have a permission open and then ask a question, or the reverse, and the
    /// two live in separate lists — so looking for one kind before the other
    /// would take the wrong card whenever both are parked. The headline is
    /// already written from `ask` by the time this is called, and a message that
    /// says "has a question for you" over a permission's title, with that
    /// permission's Allow and Deny beneath it, is worse than one with no buttons
    /// at all: it is answerable, and answering it does something nobody asked
    /// for.
    ///
    /// Empty buttons is a real answer and not a failure: the card may have been
    /// settled in the window between the event and this, and a form with several
    /// questions is one a row of buttons cannot express. The message still goes,
    /// because knowing an agent is blocked is worth more than being able to
    /// unblock it from here.
    fn parked_ask(
        &self,
        uid: u64,
        ask: onehand_core::chat::UserAsk,
        cx: &App,
    ) -> (Option<String>, Vec<Vec<Button>>) {
        use onehand_core::chat::UserAsk;
        let Some(chat) = self.session_of(uid).map(|s| &s.read(cx).chat) else {
            return (None, Vec::new());
        };
        // The most recent of whichever kind just parked -- an older unanswered
        // card is already sitting in somebody's chat with its own buttons.
        match ask {
            UserAsk::Permission => chat.pending_permissions().last().map(|(item, perm)| {
                (
                    Some(perm.req.title.clone()),
                    press::permission_buttons(uid, *item, &perm.req.options),
                )
            }),
            UserAsk::Question => chat.pending_asks().last().map(|(item, parked)| {
                (
                    Some(parked.req.message.clone()),
                    press::question_buttons(uid, *item, &parked.req.fields),
                )
            }),
        }
        .unwrap_or((None, Vec::new()))
    }

    /// Answer a permission or a question from outside the app.
    ///
    /// `None` means this pane has no such session, which is how a caller walking
    /// every window finds the one that does. Everything else is a sentence for
    /// the person who pressed the button, including every way it could not be
    /// carried out — a press that quietly did nothing would leave them believing
    /// the agent had been unblocked.
    ///
    /// Answering twice is safe and says so: the model refuses a card that is
    /// already resolved, so a second press from a message still sitting in a
    /// chat cannot re-answer anything.
    pub fn remote_answer(&mut self, press: Press, cx: &mut Context<Self>) -> Option<String> {
        let session = self.session_of(press.uid())?.clone();
        let said = session.update(cx, |session, cx| {
            let said = Self::apply_press(&mut session.chat, press);
            cx.notify();
            said
        });
        cx.notify();
        Some(said)
    }

    /// The press, against the model, in the one order that settles a card.
    ///
    /// **The card is the one the button named**, looked up by position rather
    /// than by "whatever is pending". A message stays pressable in a chat for as
    /// long as it is scrollable, and a card that has since been answered — in
    /// the window, or by an earlier press — has to be reported as settled rather
    /// than have the press slide onto the next unanswered one.
    fn apply_press(chat: &mut Chat, press: Press) -> String {
        // Every press that names a card names one; the picker is the one that
        // does not, and it is the arm that never reads this.
        let item = press.item().unwrap_or_default();
        match press {
            Press::Permission { option, .. } => {
                let Some(ChatItem::Permission(perm)) = chat.items.get(item) else {
                    return SETTLED.to_string();
                };
                if perm.resolved.is_some() {
                    return SETTLED.to_string();
                }
                let options = perm.req.options.clone();
                // An index that names nothing lands on a refusal rather than on
                // nothing at all: a press whose meaning cannot be established
                // must not be able to grant something, and must not silently
                // leave the agent parked either.
                let Some(chosen) = press::option_at(&options, option) else {
                    return "That permission offers nothing this can answer with.".to_string();
                };
                let (id, name) = (chosen.id.clone(), chosen.name.clone());
                chat.answer_permission(item, &id);
                format!("{name}.")
            }
            Press::Question { choice, .. } => {
                // The same guard the buttons were built behind, checked again
                // here: a message outlives the form it was drawn for, and an
                // unanswerable pick would otherwise settle the card with an
                // empty answer.
                let picked = chat.ask_at_mut(item).and_then(|ask| {
                    let label = ask
                        .req
                        .fields
                        .first()?
                        .kind
                        .choices()
                        .get(choice)?
                        .label
                        .clone();
                    ask.toggle(0, choice);
                    Some(label)
                });
                let Some(label) = picked else {
                    return SETTLED.to_string();
                };
                chat.answer_ask(item, false);
                format!("{label}.")
            }
            Press::Skip { .. } => {
                if chat.ask_at_mut(item).is_none() {
                    return SETTLED.to_string();
                }
                chat.answer_ask(item, true);
                "Skipped — the agent carries on without an answer.".to_string()
            }
            // Not a card, so none of the transcript bookkeeping above applies:
            // what the agent offers is live, and the model refuses a group or a
            // choice that has moved rather than settling for the nearest.
            Press::Option { group, choice, .. } => {
                chat.choose(&group, choice).unwrap_or_else(|| {
                    "That isn't offered any more — ask for /options again.".to_string()
                })
            }
        }
    }
}
