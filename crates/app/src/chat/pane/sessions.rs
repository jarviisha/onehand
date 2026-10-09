use super::project_page::EmptyProject;
use super::{ChatPane, ChatPaneEvent, ProjectFacts, Restart, SessionSignal, switching_away};
use crate::chat::conversation::{Conversation, SessionPhase};
use crate::chat::session::{ChatEvent, ChatSession};
use gpui::{App, Context, Entity, Focusable, SharedString, Window};
use onehand_core::chat::{Chat, ConvMeta};
use onehand_core::config::AgentSpec;
use std::collections::hash_map::Entry;
use std::path::PathBuf;

impl ChatPane {
    /// Show `uid`'s conversation, spawning its adapter the first time.
    ///
    /// Lazy on purpose: a workspace with a dozen roots must not launch a dozen
    /// agent processes at boot.
    pub fn show(
        &mut self,
        uid: u64,
        root: PathBuf,
        spec: &AgentSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Taken before the map is touched, and taken whatever happens next: a
        // request to open onto one particular archive belongs to the one `show`
        // it was made for, and a session switched away from and back to must not
        // be dragged into that conversation a second time.
        let asked_for = self
            .pending_resume
            .take_if(|(pending, _)| *pending == uid)
            .map(|(_, archive)| archive);
        // An entry in the map *is* "already being set up", whatever phase it has
        // reached. Before there was one entry there were three maps and three
        // checks, and two quick selections of the same session could both pass
        // them and both run the history scan -- one adapter process spawned for
        // nothing, and the conversation it belonged to archiving itself on the
        // way out.
        if let Entry::Vacant(slot) = self.conversations.entry(uid) {
            slot.insert(Conversation::opening(root.clone(), spec.clone()));
            // **A new session connects; it does not ask.** Every session in this
            // app is minted by an explicit action -- the rail's *New*, a
            // project's menu, the project page -- and each of those is a request
            // for a session, not a question about which conversation to have.
            // This used to scan for past conversations first and put the resume
            // picker up whenever it found any, so *New session* landed on a page
            // asking the user to choose a conversation, immediately after they
            // had chosen not to resume one.
            //
            // Nothing went with it: the project page lists that project's
            // archives above the button that starts a session, and a session
            // already running reaches the same picker from its header menu.
            let stored = asked_for.as_deref().and_then(onehand_core::chat::load);
            self.connect(uid, stored, cx);
        }
        if switching_away(self.active, uid) {
            self.leave_shown_session(window, cx);
            self.restore_draft(uid, window, cx);
        }
        self.leave_issues_page(window, cx);
        self.page = None;
        self.active = Some(uid);
        // The header's menu is about the project, so it follows the project
        // rather than the session: switching between two sessions of one root
        // reads the same list and does not re-read it.
        self.follow_archives(&root, cx);
        if window.is_window_active()
            && let Some(conv) = self.conversations.get_mut(&uid)
        {
            conv.unseen = false;
        }

        self.composer
            .read(cx)
            .state
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    /// Start `uid` on `root` and connect it, without putting it on screen.
    ///
    /// The other half of the lazy rule [`Self::show`] keeps. A session normally
    /// connects the first time it is shown, so that a workspace of a dozen roots
    /// does not launch a dozen agents at boot; an unattended run is one agent
    /// that was asked for, and showing it would swap the conversation somebody
    /// is reading for one they did not start. Nothing here needs a window — it
    /// is showing, not connecting, that does.
    pub fn open_unshown(
        &mut self,
        uid: u64,
        root: PathBuf,
        spec: &AgentSpec,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatSession>> {
        if let Entry::Vacant(slot) = self.conversations.entry(uid) {
            slot.insert(Conversation::opening(root, spec.clone()));
            self.connect(uid, None, cx);
        }
        self.session_of(uid).cloned()
    }

    /// Put down everything that belonged to the session leaving the screen.
    ///
    /// One place, because both of these are the same rule wearing two hats:
    /// each is pane-level state whose meaning is a single conversation. Spread
    /// across the call sites they were written at some of them and not others,
    /// which is a session opening onto the previous one's half-typed prompt.
    pub(super) fn leave_shown_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The composer is emptied *unconditionally*, so "no session showing"
        // always means "nothing composed". Anything weaker leaves a prompt in
        // the box after the session it was written for was closed, and the next
        // session opens holding it.
        let draft = self
            .composer
            .update(cx, |composer, cx| composer.take_draft(window, cx));
        // A draft belonging to nobody -- typed against a session that has since
        // been closed -- has nowhere to go back to.
        if let Some(conv) = self.active_conversation_mut() {
            conv.draft = (!draft.is_empty()).then_some(draft);
        }
    }

    /// Give `uid` back whatever it had unsent. The composer was emptied by the
    /// stash, so a session with no draft correctly opens on a blank one.
    fn restore_draft(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self
            .conversations
            .get_mut(&uid)
            .and_then(|conv| conv.draft.take())
        else {
            return;
        };
        self.composer
            .update(cx, |composer, cx| composer.restore_draft(draft, window, cx));
    }

    /// Connect `uid`, optionally resuming an archived conversation.
    pub(super) fn start(&mut self, uid: u64, resume: Option<ConvMeta>, cx: &mut Context<Self>) {
        let stored = resume
            .as_ref()
            .and_then(|meta| onehand_core::chat::load(&meta.dir));
        self.connect(uid, stored, cx);
    }

    /// Spawn an adapter for `uid` and fold its events into this pane.
    ///
    /// `stored` is both the conversation to resume *and* what the transcript
    /// shows until the agent's replay arrives -- adopting it up front is what
    /// keeps a resume (or a restart) from blanking the pane while the adapter
    /// comes up.
    fn connect(
        &mut self,
        uid: u64,
        stored: Option<onehand_core::chat::ConversationSnapshot>,
        cx: &mut Context<Self>,
    ) {
        let Some(conv) = self.conversations.get_mut(&uid) else {
            return;
        };
        let (root, spec) = (conv.root.clone(), conv.spec.clone());
        // Whatever the session was on goes *before* the replacement is spawned.
        // On a restart that is the old adapter, and its pump owns the event
        // stream: dropping it afterwards would briefly leave two processes on
        // one conversation.
        conv.disconnect();
        let session = ChatSession::spawn(
            uid,
            root.clone(),
            &spec,
            stored.as_ref().map(|s| s.session_id.clone()),
            cx,
        );
        cx.emit(ChatPaneEvent::AgentStarted);
        if let Some(stored) = stored {
            // The conversation is adopted *before* the adapter's replay lands,
            // so a stale resume still shows it instead of a blank pane. Through
            // the session rather than into its model directly: adopting is what
            // parses the markdown being adopted, and a transcript whose blocks
            // are unparsed draws its own source.
            session.update(cx, |session, cx| session.adopt(stored, cx));
        }

        let agent = spec.name.clone();
        let root_label = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let watch = cx.subscribe(
            &session,
            move |pane: &mut Self, session, event: &ChatEvent, cx| {
                match event {
                    ChatEvent::TurnEnded => {
                        Self::archive_detached(uid, &session, cx);
                        pane.turn_ended_detached(uid, &agent, &root_label, cx);
                        // The turn that just ended is what wrote the archive, so
                        // this is the moment a conversation first exists on disk
                        // and the moment its summary and its age change.
                        pane.refresh_archives(cx);
                        cx.emit(ChatPaneEvent::WorkTreeTouched);
                    }
                    ChatEvent::AwaitingUser(ask) => {
                        // **An agent that has stopped outranks a list the user
                        // left open.** The popup is drawn over the pinned
                        // cards, so a card arriving while one is up would park
                        // behind it — and this is the one moment nothing else
                        // would say so: the desktop notification is withheld
                        // precisely because the user is looking at this
                        // conversation, which is exactly where the card now is
                        // and exactly where they cannot see it.
                        //
                        // Only for a card that *arrives*. One already on screen
                        // when the picker was opened is one the user saw and
                        // chose to open a picker over.
                        // **Only when the asking session is the one on
                        // screen.** This subscription is per session while the
                        // composer is one entity shared by all of them, so
                        // unguarded it let a background agent reach across and
                        // shut the list the user was reading — with no card
                        // appearing to account for it, because the card belongs
                        // to a conversation that is not being shown.
                        if pane.active == Some(uid) {
                            pane.composer
                                .update(cx, |composer, cx| composer.close_overlay(cx));
                        }
                        pane.awaiting_user_detached(uid, *ask, &agent, &root_label, cx);
                    }
                    // Re-emitted rather than acted on: the transcript says what
                    // was asked for, and where a file goes is the shell's call.
                    // Matched exhaustively so a new variant cannot be added and
                    // silently dropped here -- which is how this one was lost.
                    ChatEvent::OpenFile(path) => cx.emit(ChatPaneEvent::OpenFile(path.clone())),
                    ChatEvent::Disconnected => {
                        pane.link_lost_detached(uid, &agent, &root_label, cx);
                    }
                    ChatEvent::Appended => {}
                }
                cx.notify();
            },
        );
        self.set_phase(
            uid,
            SessionPhase::Live {
                session,
                _watch: watch,
            },
        );
        cx.notify();
    }

    /// Move `uid` to a new phase, if it still exists.
    pub(super) fn set_phase(&mut self, uid: u64, phase: SessionPhase) {
        if let Some(conv) = self.conversations.get_mut(&uid) {
            conv.phase = phase;
        }
    }

    /// Whether `uid` exists and has nothing connected yet.
    pub(super) fn is_opening(&self, uid: u64) -> bool {
        self.conversations
            .get(&uid)
            .is_some_and(Conversation::is_opening)
    }

    /// Whether `uid` is waiting on the user to pick a conversation to resume.
    pub(super) fn is_choosing(&self, uid: u64) -> bool {
        self.conversations
            .get(&uid)
            .is_some_and(|conv| conv.choices().is_some())
    }

    /// The conversation showing right now, if one is.
    pub(super) fn active_conversation(&self) -> Option<&Conversation> {
        self.conversations.get(&self.active?)
    }

    pub(super) fn active_conversation_mut(&mut self) -> Option<&mut Conversation> {
        self.conversations.get_mut(&self.active?)
    }

    /// Write a finished turn to the conversation's file, off the UI loop.
    ///
    /// The transcript is written at the end of *every* turn rather than only
    /// when the session is dropped, so a crash -- or any exit where GPUI does
    /// not run entity drops -- costs the turn in flight instead of the whole
    /// conversation.
    ///
    /// Split in two: preparing the write needs the transcript and so stays on
    /// the UI thread, while the writing itself goes to the background executor.
    /// Preparing is what moves the conversation's mark, so two of these in
    /// flight at once cannot both carry the same turn.
    ///
    /// The result is carried back rather than dropped: this is the only save in
    /// the app whose failure the user could not recover from by redoing the
    /// action, so it is the last one that should have been silent. `uid` is what
    /// the answer comes home to -- the write outlives the turn that started it,
    /// and by the time it lands the session may not even be the one on screen.
    fn archive_detached(uid: u64, session: &Entity<ChatSession>, cx: &mut Context<Self>) {
        // `None` while the session has nowhere to write, no id, or nothing new
        // -- which is what keeps a session nobody used from standing in the
        // list beside conversations that were had.
        let Some(pending) = session.update(cx, |session, _| session.chat.flush()) else {
            return;
        };
        Self::commit_detached(uid, pending, cx);
    }

    /// Write only what a conversation is *called* and what it is set to.
    ///
    /// Kept apart from the turn's own write because a rename can happen in the
    /// middle of one, and a line written mid-turn describes a tool call that has
    /// not finished -- which nothing would ever revisit, since the turn's save
    /// writes only what came after it.
    pub(super) fn archive_meta_detached(
        uid: u64,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = session.update(cx, |session, _| session.chat.flush_meta()) else {
            return;
        };
        Self::commit_detached(uid, pending, cx);
    }

    /// Hand a prepared write to the background executor and bring the answer
    /// back to the session it belongs to.
    fn commit_detached(
        uid: u64,
        pending: onehand_core::chat::PendingWrite,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |pane, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { onehand_core::chat::commit(&pending) })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // A session closed while its own write was in flight has
                // nowhere to file the answer, and nothing left to tell.
                let Some(conv) = pane.conversations.get_mut(&uid) else {
                    return;
                };
                match written {
                    Ok(()) => conv.archive_failed = false,
                    Err(e) => {
                        // Only the edge. A directory that has gone read-only
                        // fails at the end of every turn, and one message per
                        // turn is how the first one gets lost.
                        let first = !conv.archive_failed;
                        conv.archive_failed = true;
                        if first {
                            cx.emit(ChatPaneEvent::ArchiveFailed(e.to_string()));
                        }
                    }
                }
            });
        })
        .detach();
    }

    /// Restart session `uid`'s adapter on the same conversation.
    ///
    /// Dropping the old session is what kills the old adapter (its pump task
    /// owns the event stream), so this cannot leave two processes replaying
    /// into one transcript. The live transcript is snapshotted and handed to
    /// the new session as history, so the pane keeps reading as itself while
    /// the agent replays -- the model comes back empty, and a restart that
    /// blanks the conversation looks like data loss even though it is not.
    ///
    /// Mid-turn this throws away the turn the user is waiting on; asking
    /// first is the caller's, through [`Self::turn_in_flight`].
    pub fn restart(&mut self, uid: u64, cx: &mut Context<Self>) -> Restart {
        // The session is looked up twice rather than held across the whole
        // function, and neither look-up is a clone. A cloned handle is a second
        // strong reference to the old session, and the old adapter only dies
        // when the last one goes -- so holding one here would keep the process
        // alive right through the spawn of its replacement, which is the one
        // thing this is careful about.
        if self.session_of(uid).is_none() {
            return Restart::Nothing;
        }

        // The conversation is *moved* out of the old session rather than copied
        // from it. The old one is about to be dropped, and a drop still holding
        // these items would write them to the file a second time -- while the
        // mark riding along is what lets the replacement carry on the same file
        // instead of starting the conversation again inside it.
        let stored = self
            .session_of(uid)
            .and_then(|s| s.update(cx, |s, _| s.chat.take_snapshot()));
        // `connect` drops the old adapter before spawning the new one, so
        // nothing here has to take the session apart first.
        self.connect(uid, stored, cx);
        Restart::Restarted
    }

    /// Drop a session and, with it, its agent process.
    ///
    /// One `remove`. Everything the session had -- its adapter, its unseen
    /// badge, its scroll position, its unsent draft -- goes with the entry,
    /// because all of it lives on the entry. This used to be six removals, and
    /// the way that fails is silent: forget one and a closed session keeps a
    /// dot on a rail row that no longer exists.
    pub fn close(&mut self, uid: u64, cx: &mut Context<Self>) {
        self.conversations.remove(&uid);
        if self.active == Some(uid) {
            // What the composer still holds is dropped by the next `show`,
            // which treats an unaddressed draft as unaddressed.
            self.active = None;
        }
        // The conversation just closed is the one most likely to be wanted back,
        // and until this read lands the menu still lists it as open.
        self.refresh_archives(cx);
        cx.notify();
    }

    /// Stop showing any session, without closing one.
    ///
    /// A project root can have no sessions at all -- a freshly added one always
    /// does. The shell points the Workbench and the Terminal at that root
    /// regardless, so the pane has to let go too: leaving the previous root's
    /// transcript up means the composer keeps sending prompts to *that* root's
    /// agent while every other panel says the user is somewhere else, and the
    /// agent writes files into the wrong project.
    ///
    /// Not `close`: the old session stays alive and connected in `sessions`,
    /// exactly as it does when switching between two roots that both have one.
    ///
    /// `root` is the project the pane then stands in -- its label, and the path
    /// its past conversations are keyed by. It is recorded *before* the early
    /// return, because moving between two sessionless projects changes nothing
    /// about the pane except which one it is offering.
    pub fn clear_active(
        &mut self,
        root: Option<(SharedString, PathBuf)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let standing_in = self.empty.as_ref().map(|project| &project.path);
        let unchanged = self.active.is_none()
            && self.page.is_none()
            && standing_in == root.as_ref().map(|(_, path)| path);
        if unchanged {
            return;
        }
        self.leave_issues_page(window, cx);
        self.page = None;
        self.empty = root.map(|(label, path)| EmptyProject {
            label,
            path,
            history: None,
            // They arrive from the shell a moment later, on the same switch
            // that brought this page up. Defaulting to "not pinned, not a
            // repository, no switch offered" is what a menu drawn in that
            // moment can honestly say.
            facts: ProjectFacts::default(),
            check_input: None,
        });
        self.scan_project_history(cx);
        // Going to no session at all is still leaving the one that was showing,
        // and the draft has to be put down here too: a prompt left in the box
        // while passing through an empty project would otherwise be sitting
        // there, addressed to nobody, when the next session opens.
        self.leave_shown_session(window, cx);
        self.active = None;
        cx.notify();
    }

    /// Open the next showing of `uid` straight onto an archived conversation,
    /// with no picker in between.
    ///
    /// Told to the pane before the session is shown, because the session does
    /// not exist yet when the row is clicked: the shell mints it, and the pane
    /// only ever hears about it through [`Self::show`].
    pub fn resume_next(&mut self, uid: u64, archive: PathBuf) {
        self.pending_resume = Some((uid, archive));
    }

    pub(super) fn active_chat<'a>(&self, cx: &'a App) -> Option<&'a Chat> {
        Some(&self.active_conversation()?.session()?.read(cx).chat)
    }

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
        let (text, staged) = {
            let composer = self.composer.read(cx);
            (composer.text(cx), composer.attachments.clone())
        };
        // Clear only on a prompt that went somewhere -- sent, or queued behind
        // the turn that is running. A refused one (no agent yet, an unreadable
        // attachment) must not silently eat what the user typed or staged.
        let taken = session.update(cx, |session, cx| {
            session.submit(&text, &staged, cx) || {
                let queued = session.chat.queue(&text, &staged);
                if queued {
                    cx.notify();
                }
                queued
            }
        });
        if taken {
            self.composer
                .update(cx, |composer, cx| composer.clear(window, cx));
        }
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

    fn cancel(session: &Entity<ChatSession>, cx: &mut Context<Self>) {
        session.update(cx, |session, cx| {
            session.chat.cancel_turn();
            cx.notify();
        });
    }

    /// Send session `uid`'s last prompt again, after a turn that failed. Its
    /// text only: the files staged with it were a snapshot and are not resent.
    pub fn resend_last_prompt(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        session.update(cx, |session, cx| {
            let last = session.chat.items.iter().rev().find_map(|item| match item {
                onehand_core::chat::ChatItem::User(prompt) => Some(prompt.text.clone()),
                _ => None,
            });
            if let Some(text) = last {
                session.submit(&text, &[], cx);
            }
        });
    }

    pub(super) fn session(&self) -> Option<Entity<ChatSession>> {
        self.active_conversation()?.session().cloned()
    }

    /// What the rail should show for `uid`, if anything.
    ///
    /// A *query*, not a mirrored field: the truth lives on the conversation,
    /// and the previous design -- a `Session::runtime` one level up in the
    /// workspace tree -- had nothing that could keep it current, so it stayed
    /// at its default forever.
    ///
    /// One dot, priority-ordered, rather than one per condition. Two adjacent
    /// coloured dots on a rail row are a code nobody learns, and when a session
    /// is both lost *and* unseen, "lost" is the half that needs acting on.
    pub fn signal(&self, uid: u64, cx: &App) -> Option<SessionSignal> {
        // A session still on its resume picker has no conversation yet -- and
        // nothing has been asked of the user, so it is not a signal.
        let conv = self.conversations.get(&uid)?;
        let chat = &conv.session()?.read(cx).chat;
        SessionSignal::pick(
            chat.link,
            chat.failed,
            chat.awaiting_permission(),
            chat.busy,
            conv.unseen,
        )
    }

    /// What the conversation on `uid` is called, once it has earned a name.
    ///
    /// A *query* for the same reason [`Self::signal`] is one: the title is
    /// derived from the first prompt, so it arrives mid-conversation, and a
    /// copy of it one level up in the workspace tree would have nothing that
    /// could keep it current.
    ///
    /// `None` until a prompt exists — an unnamed conversation is the caller's
    /// to label, and for the rail that means falling back to the agent's name.
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
        if !session.update(cx, |session, _| session.chat.rename(title)) {
            return false;
        }
        Self::archive_meta_detached(uid, &session, cx);
        cx.notify();
        true
    }

    /// Drop a custom name and go back to the title derived from the first
    /// prompt.
    pub fn reset_title(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        session.update(cx, |session, _| session.chat.reset_title());
        Self::archive_meta_detached(uid, &session, cx);
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

    /// Whether a turn is running on `uid`: streaming, or parked on a permission
    /// or a question nobody has answered.
    ///
    /// Asked of the conversation rather than read off [`Self::signal`], which
    /// collapses four facts to the one worth drawing: a session that is both
    /// lost and mid-turn reports `Lost` there, and a caller guarding against
    /// throwing a turn away needs the fact, not the priority.
    pub fn turn_in_flight(&self, uid: u64, cx: &App) -> bool {
        let Some(session) = self.session_of(uid) else {
            return false;
        };
        let chat = &session.read(cx).chat;
        chat.busy || chat.awaiting_permission()
    }

    /// Forget the badge on the session being looked at.
    ///
    /// Called when this window becomes the active one: `show` only clears on a
    /// session *switch*, so returning to a window whose session is already on
    /// screen would otherwise leave the badge up.
    pub fn mark_active_seen(&mut self, cx: &mut Context<Self>) {
        if let Some(conv) = self.active_conversation_mut()
            && conv.unseen
        {
            conv.unseen = false;
            cx.notify();
        }
    }

    /// Write the whole conversation to a Markdown file.
    pub fn export(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat(cx) else {
            return;
        };
        let markdown = onehand_core::chat::export_markdown(chat);
        let suggested = chat
            .conversation_title()
            .unwrap_or_else(|| "conversation".to_string());

        cx.spawn(async move |pane, cx| {
            // Picker *and* write both go to the background executor: the dialog
            // blocks until the user is done, and the write can be megabytes.
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let path = rfd::FileDialog::new()
                        .set_file_name(format!("{suggested}.md"))
                        .save_file()?;
                    std::fs::write(&path, markdown).err().map(|e| e.to_string())
                })
                .await;
            if let Some(error) = saved {
                let _ = pane.update(cx, |_: &mut Self, cx| {
                    eprintln!("onehand: export failed: {error}");
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
