use super::fork::StagedFork;
use super::project_page::EmptyProject;
use super::{ChatPane, ChatPaneEvent, ProjectFacts, Restart, SessionSignal, switching_away};
use crate::chat::composer::Composer;
use crate::chat::conversation::{Conversation, SessionPhase};
use crate::chat::conversation::{HostAt, OnHost};
use crate::chat::session::{BeginOnHost, Carried, ChatEvent, ChatSession, HostBegin};
use gpui::{App, Context, Entity, Focusable, SharedString, Window};
use onehand_core::acp::Opening;
use onehand_core::chat::drive::{Closing, Input, Intake, Origin, Restarting, Signal};
use onehand_core::chat::{Chat, ConvMeta};
use onehand_core::config::AgentSpec;
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;
use std::collections::hash_map::Entry;
use std::path::PathBuf;

impl ChatPane {
    /// Show `uid`'s conversation, spawning its adapter the first time.
    ///
    /// Lazy on purpose: a workspace with a dozen roots must not launch a dozen
    /// agent processes at boot. `host` is the host the project's sessions run
    /// on, and the folder they run in there.
    pub fn show(
        &mut self,
        uid: u64,
        root: PathBuf,
        host: Option<HostAt>,
        spec: &AgentSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A host project keeps none of its conversations here; an offloaded
        // one keeps those it had before it went.
        let kept_here = host.as_ref().is_none_or(|(_, cwd)| *cwd != root);
        // Taken before the map is touched, and taken whatever happens next: a
        // request to open onto one particular archive belongs to the one `show`
        // it was made for, and a session switched away from and back to must not
        // be dragged into that conversation a second time.
        let asked_for = self
            .pending_resume
            .take_if(|(pending, _)| *pending == uid)
            .map(|(_, archive)| archive);
        let fork = self.take_fork(uid);
        let watch = self
            .pending_watch
            .take_if(|(pending, _)| *pending == uid)
            .map(|(_, id)| id);
        // An entry in the map *is* "already being set up", whatever phase it has
        // reached. Before there was one entry there were three maps and three
        // checks, and two quick selections of the same session could both pass
        // them and both run the history scan -- one adapter process spawned for
        // nothing, and the conversation it belonged to archiving itself on the
        // way out.
        if let Entry::Vacant(slot) = self.conversations.entry(uid) {
            slot.insert(Conversation {
                host: opening_host(host, watch.as_ref(), fork.is_some()),
                ..Conversation::opening(root.clone(), spec.clone())
            });
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
            match (fork, watch) {
                (Some(staged), _) => self.connect_as(uid, Start::Fork(staged), cx),
                (None, Some(id)) => self.connect_as(uid, Start::Watch(id), cx),
                (None, None) => {
                    let stored = asked_for.as_deref().and_then(onehand_core::chat::load);
                    self.connect(uid, stored, cx);
                }
            }
        }
        if switching_away(self.active, uid) {
            self.leave_shown_session(window, cx);
            self.restore_draft(uid, window, cx);
        }
        self.leave_page(window, cx);
        self.page = None;
        // The project's page is made again when the pane next stands on it,
        // so it reads its lists anew: a session closed meanwhile is among its
        // past conversations, or its host's sessions.
        self.empty = None;
        self.active = Some(uid);
        // The header's menu is about the project, so it follows the project
        // rather than the session: switching between two sessions of one root
        // reads the same list and does not re-read it. A host's session lists
        // nothing of this machine's.
        if kept_here {
            self.follow_archives(&root, cx);
        }
        if window.is_window_active() {
            self.shown(uid, cx);
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
        dirs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatSession>> {
        if let Entry::Vacant(slot) = self.conversations.entry(uid) {
            slot.insert(Conversation {
                dirs,
                ..Conversation::opening(root, spec.clone())
            });
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
        let start = match stored {
            Some(stored) => Start::Resume(stored),
            None => Start::Fresh,
        };
        self.connect_as(uid, start, cx);
    }

    /// [`Self::connect`], starting the conversation as `start` says.
    fn connect_as(&mut self, uid: u64, start: Start, cx: &mut Context<Self>) {
        let Some(conv) = self.conversations.get_mut(&uid) else {
            return;
        };
        let (root, spec, dirs) = (conv.root.clone(), conv.spec.clone(), conv.dirs.clone());
        // What the old session had sent, counted on: who prompted a worker is
        // read from it, and the new chat would start it from nothing.
        let mut sent_before = conv
            .session()
            .map_or(0, |session| session.read(cx).chat.prompts_sent);
        let (on_host, start) = match &conv.host {
            None => (None, start),
            Some(OnHost { host, cwd, session }) => {
                let (begin, start) = host_begin(start, session.as_ref(), sent_before);
                // A re-watch counts the prompts again as its journal replays
                // them; what it carries is for an open in its place.
                if matches!(begin, HostBegin::Rewatch(..)) {
                    sent_before = 0;
                }
                let (host, cwd) = (host.clone(), cwd.clone());
                (Some(BeginOnHost { host, cwd, begin }), start)
            }
        };
        // Its names too: a worker's stand-in is never saved, and a name its
        // lead gave is not written down until a turn ends.
        let titles = conv.session().map(|session| {
            let chat = &session.read(cx).chat;
            (chat.custom_title.clone(), chat.stand_in_title.clone())
        });
        // Whatever the session was on goes *before* the replacement is spawned.
        // On a restart that is the old adapter, and its pump owns the event
        // stream: dropping it afterwards would briefly leave two processes on
        // one conversation.
        conv.disconnect();
        let session = ChatSession::spawn(
            uid,
            root.clone(),
            &spec,
            match &start {
                Start::Fresh | Start::Watch(_) | Start::Restart(None, _) => Opening::Fresh,
                Start::Resume(stored) | Start::Restart(Some(stored), _) => {
                    Opening::Resume(stored.session_id.clone())
                }
                Start::Fork(staged) => Opening::Fork {
                    from: staged.from.clone(),
                    message: staged.message.clone(),
                },
            },
            dirs,
            on_host,
            cx,
        );
        if sent_before > 0 {
            session.update(cx, |session, _| session.chat.carry_sent(sent_before));
        }
        if let Some((custom, stand_in)) = titles {
            session.update(cx, |session, _| session.chat.carry_titles(custom, stand_in));
        }
        cx.emit(ChatPaneEvent::AgentStarted);
        // The conversation is adopted *before* the adapter's replay lands,
        // so a stale resume still shows it instead of a blank pane. Through
        // the session rather than into its model directly: adopting is what
        // parses the markdown being adopted, and a transcript whose blocks
        // are unparsed draws its own source.
        match start {
            Start::Fresh | Start::Watch(_) | Start::Restart(None, _) => {}
            Start::Resume(stored) | Start::Restart(Some(stored), _) => {
                session.update(cx, |session, cx| session.adopt(stored, cx))
            }
            Start::Fork(staged) => {
                session.update(cx, |session, cx| session.adopt_fork(staged.seed, cx));
                if let Some(conv) = self.conversations.get_mut(&uid) {
                    conv.draft = Some(staged.draft);
                }
            }
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
                    // Work the agent did between turns is said as a turn's
                    // is; the drive has saved both.
                    ChatEvent::Session(said @ (Signal::CycleEnded | Signal::TurnEnded)) => {
                        let (root, on_host) = {
                            let session = session.read(cx);
                            (session.chat.root.clone(), session.on_host)
                        };
                        crate::offload::session_said(cx, &root, on_host, said);
                        // The turn that just ended is what wrote the archive, so
                        // this is the moment a conversation first exists on disk
                        // and the moment its summary and its age change.
                        pane.refresh_archives(cx);
                        cx.emit(ChatPaneEvent::WorkTreeTouched);
                    }
                    ChatEvent::Session(Signal::AwaitingUser(_)) => {
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
                        // The card the agent parked is in the transcript now,
                        // and the reader of that conversation has to see it.
                        cx.notify();
                    }
                    ChatEvent::Look(what) => pane.look(uid, *what, &agent, &root_label, cx),
                    // Re-emitted rather than acted on: the transcript says what
                    // was asked for, and where a file goes is the shell's call.
                    // Matched exhaustively so a new variant cannot be added and
                    // silently dropped here -- which is how this one was lost.
                    ChatEvent::OpenFile(path) => cx.emit(ChatPaneEvent::OpenFile(path.clone())),
                    // A fork that never opened goes, and says why, rather than
                    // stand on the rail as a session holding someone else's
                    // history with no agent behind it. A host session read
                    // from this machine's copy is out of reach, not lost:
                    // nothing is announced, and it is restarted once the
                    // host is back.
                    ChatEvent::Session(Signal::Disconnected) => {
                        let session = session.read(cx);
                        let (failed, offline) = (
                            session.chat.fork_failure(),
                            session.chat.host_away().cloned(),
                        );
                        match (failed, offline) {
                            (Some(why), _) => cx.emit(ChatPaneEvent::ForkFailed {
                                uid: Some(uid),
                                why,
                            }),
                            (None, Some(host)) => cx.emit(ChatPaneEvent::HostLost {
                                uid: Some(uid),
                                host,
                            }),
                            // A lost agent is the drive's news (`ChatEvent::Look`).
                            (None, None) => {}
                        }
                    }
                    ChatEvent::Fork(target) => pane.begin_fork(uid, *target, cx),
                    ChatEvent::Session(Signal::Appended) => {}
                    ChatEvent::Session(Signal::PickRefused(why)) => {
                        cx.emit(ChatPaneEvent::PickRefused(why.clone()));
                        cx.notify();
                    }
                    ChatEvent::Session(Signal::StopRefused(why)) => {
                        cx.emit(ChatPaneEvent::StopRefused(why.clone()));
                    }
                    ChatEvent::Session(Signal::SaveFailed(why)) => {
                        cx.emit(ChatPaneEvent::ArchiveFailed(why.clone()));
                    }
                    ChatEvent::Session(Signal::CodeFailed(why)) => {
                        cx.emit(ChatPaneEvent::CodeFailed(why.clone()));
                    }
                    ChatEvent::Session(Signal::RenameFailed(why)) => {
                        cx.emit(ChatPaneEvent::RenameFailed(why.clone()));
                        cx.notify();
                    }
                    ChatEvent::HostSession(id) => {
                        if let Some(on) = pane
                            .conversations
                            .get_mut(&uid)
                            .and_then(|conv| conv.host.as_mut())
                        {
                            on.session = Some(id.clone());
                        }
                    }
                    ChatEvent::HostBusy => {
                        if let Some(on) = pane.conversations.get(&uid).and_then(|c| c.host.as_ref())
                        {
                            cx.emit(ChatPaneEvent::HostBusy {
                                uid: Some(uid),
                                host: on.host.clone(),
                            });
                        }
                    }
                    ChatEvent::RestartHost(host) => {
                        cx.emit(ChatPaneEvent::RestartHost(host.clone()));
                    }
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

    /// Restart session `uid`'s adapter on the same conversation;
    /// `after_service` when its host's service has just been restarted.
    ///
    /// Dropping the old session is what kills the old adapter (its pump task
    /// owns the event stream), so this cannot leave two processes replaying
    /// into one transcript. The live transcript is snapshotted and handed to
    /// the new session as history, so the pane keeps reading as itself while
    /// the agent replays -- the model comes back empty, and a restart that
    /// blanks the conversation looks like data loss even though it is not.
    ///
    /// Mid-turn this throws away the turn the user is waiting on; asking
    /// first is the caller's, through [`Self::restarting`].
    pub fn restart(&mut self, uid: u64, after_service: bool, cx: &mut Context<Self>) -> Restart {
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
        let stored = self.session_of(uid).and_then(|s| {
            s.update(cx, |s, cx| {
                // Its line about waiting, or about being out of reach, is
                // over with the restart.
                s.forget_host_wait(cx);
                s.forget_host_offline(cx);
                s.chat.take_snapshot(crate::chat::session::now())
            })
        });
        // `connect_as` drops the old adapter before spawning the new one, so
        // nothing here has to take the session apart first.
        self.connect_as(uid, Start::Restart(stored, after_service), cx);
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
        crate::control::forget(uid, cx);
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
        root: Option<(SharedString, PathBuf, Option<HostAt>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let standing_in = self
            .empty
            .as_ref()
            .map(|project| (&project.path, project.on_host()));
        let unchanged = self.active.is_none()
            && self.page.is_none()
            && standing_in
                == root
                    .as_ref()
                    .map(|(_, path, host)| (path, host.as_ref().map(|(h, c)| (h, c.as_path()))));
        if unchanged {
            return;
        }
        self.leave_page(window, cx);
        self.page = None;
        self.empty = root.map(|(label, path, on)| {
            // An offloaded project's sessions run in its mirror there.
            let mirror = on
                .as_ref()
                .filter(|(_, cwd)| *cwd != path)
                .map(|(_, cwd)| cwd.clone());
            let host = on.map(|(host, _)| host);
            EmptyProject {
                label,
                // A folder on a host keeps none of its conversations here; the
                // host lists its sessions instead. An offloaded one keeps those
                // it had before it went.
                history: (host.is_some() && mirror.is_none()).then(Vec::new),
                path,
                mirror,
                listed: None,
                listing: false,
                bypass: None,
                asked: 0,
                host,
                // They arrive from the shell a moment later, on the same switch
                // that brought this page up. Defaulting to "not pinned, not a
                // repository, no switch offered" is what a menu drawn in that
                // moment can honestly say.
                facts: ProjectFacts::default(),
                check_input: None,
                rows: super::project_page::home_rows(cx),
            }
        });
        self.scan_project_history(cx);
        self.list_host_sessions(cx);
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

    /// Open the next showing of `uid`, a session in a host project, onto the
    /// host session `id`, read as the host keeps it.
    pub fn watch_next(&mut self, uid: u64, id: HostSession) {
        self.pending_watch = Some((uid, id));
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
        let session = conv.session()?.read(cx);
        let chat = &session.chat;
        SessionSignal::pick(
            session.chat.host_waiting().is_some(),
            session.chat.host_away().is_some(),
            chat.link,
            chat.failed(),
            chat.awaiting_permission() || crate::task::waits_on_person(uid, cx),
            chat.running(),
            session.chat.unseen(),
        )
    }

    /// The wait session `uid` was on ended on `why`, its host out of reach:
    /// it ends as a lost agent does, which Restart tries again.
    pub(crate) fn host_wait_failed(&mut self, uid: u64, why: &str, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid) else {
            return;
        };
        session.update(cx, |session, cx| {
            if session.forget_host_wait(cx) {
                let lost = onehand_core::acp::AcpEvent::Disconnected(why.to_string());
                session.fold(lost, cx);
            }
        });
    }

    /// The host session `uid` is out of reach of, until it is back.
    pub(crate) fn host_lost(&self, uid: u64, cx: &App) -> Option<HostName> {
        let conv = self.conversations.get(&uid)?;
        conv.session()?.read(cx).chat.host_away().cloned()
    }

    /// The wait for `uid`'s host to be back ended on `why`, the host taken
    /// by another machine: it ends as a lost agent does, which Restart tries
    /// again.
    pub(crate) fn host_back_failed(&mut self, uid: u64, why: &str, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid) else {
            return;
        };
        session.update(cx, |session, cx| {
            if session.forget_host_offline(cx) {
                let lost = onehand_core::acp::AcpEvent::Disconnected(why.to_string());
                session.fold(lost, cx);
            }
        });
    }

    /// The host session `uid` waits on, its busy sessions to finish.
    pub(crate) fn host_wait(&self, uid: u64, cx: &App) -> Option<HostName> {
        let conv = self.conversations.get(&uid)?;
        conv.session()?.read(cx).chat.host_waiting().cloned()
    }

    /// How many background items `uid`'s agent has alive, how many subagents
    /// work, and whether a turn is open -- the half of running that Stop ends.
    pub fn activity(&self, uid: u64, cx: &App) -> (usize, usize, bool) {
        self.conversations
            .get(&uid)
            .and_then(|conv| conv.session())
            .map_or((0, 0, false), |session| {
                let chat = &session.read(cx).chat;
                (chat.background().len(), chat.subagents_working(), chat.busy)
            })
    }

    /// Whether a turn is running on `uid`: streaming, or parked on a permission
    /// or a question nobody has answered.
    ///
    /// Asked of the conversation rather than read off [`Self::signal`], which
    /// collapses four facts to the one worth drawing: a session that is both
    /// lost and mid-turn reports `Lost` there, and a caller guarding against
    /// throwing a turn away needs the fact, not the priority.
    pub fn turn_in_flight(&self, uid: u64, cx: &App) -> bool {
        self.closing(uid, cx) == Closing::Ask
    }

    /// What closing session `uid` asks first; one with nothing live closes
    /// at once.
    pub fn closing(&self, uid: u64, cx: &App) -> Closing {
        self.session_of(uid)
            .map_or(Closing::Now, |session| session.read(cx).chat.closing())
    }

    /// What restarting session `uid`'s agent asks first.
    pub fn restarting(&self, uid: u64, cx: &App) -> Restarting {
        self.session_of(uid).map_or(Restarting::Now, |session| {
            session.read(cx).chat.restarting()
        })
    }

    /// Forget the badge on the session being looked at.
    ///
    /// Called when this window becomes the active one: `show` only clears on a
    /// session *switch*, so returning to a window whose session is already on
    /// screen would otherwise leave the badge up.
    pub fn mark_active_seen(&mut self, cx: &mut Context<Self>) {
        if let Some(uid) = self.active {
            self.shown(uid, cx);
        }
    }

    /// The person is looking at `uid`'s conversation: tell its drive, and draw
    /// its row again.
    fn shown(&mut self, uid: u64, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        session.update(cx, |session, cx| {
            let fx = session
                .chat
                .handle(Input::Shown, crate::chat::session::now());
            session.perform(fx, cx);
        });
        cx.notify();
    }

    /// Write the whole conversation to a Markdown file, saying so in a toast
    /// when the write fails: the picker is gone by then, and nothing else on
    /// screen would.
    pub fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat(cx) else {
            return;
        };
        let markdown = onehand_core::chat::export_markdown(chat);
        let suggested = chat
            .conversation_title()
            .unwrap_or_else(|| "conversation".to_string());

        cx.spawn_in(window, async move |pane, cx| {
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
                let _ = pane.update_in(cx, |_: &mut Self, window, cx| {
                    gpui_component::WindowExt::push_notification(
                        window,
                        crate::toast::failed("Exporting the conversation", error),
                        cx,
                    );
                });
            }
        })
        .detach();
    }
}

/// How a session in a host project begins on the host, given how its
/// conversation starts, the host session it was on and the prompts it sent;
/// with the start that is left for the pane to adopt at once.
///
/// Only a restart reads the host session again. What it took from the old
/// session rides with the re-watch, adopted only if the host has ended the
/// session and a new one opens in its place: a re-watch replays the whole
/// journal, prompts and all, and a copy adopted beside it is shown twice.
pub(super) fn host_begin(
    start: Start,
    known: Option<&HostSession>,
    sent: usize,
) -> (HostBegin, Start) {
    match (start, known) {
        (Start::Watch(id), _) => (HostBegin::Watch(id), Start::Fresh),
        (Start::Restart(stored, after_service), Some(id)) => (
            HostBegin::Rewatch(
                id.clone(),
                Carried {
                    stored,
                    sent,
                    after_service,
                },
            ),
            Start::Fresh,
        ),
        // Adopted as a local fork is, so the refusal ends it as a failed one.
        (start @ Start::Fork(_), _) => (HostBegin::Refuse("Not on a host yet"), start),
        (start @ (Start::Fresh | Start::Resume(_) | Start::Restart(..)), _) => {
            (HostBegin::Open, start)
        }
    }
}

/// The host a session in a host project opens on, with the host session it
/// watches when that is known before it starts: so a second press on the
/// same listed session finds this one at once. A fork opens its own.
pub(super) fn opening_host(
    host: Option<HostAt>,
    watch: Option<&HostSession>,
    forking: bool,
) -> Option<OnHost> {
    host.map(|(host, cwd)| OnHost {
        host,
        cwd,
        session: watch.filter(|_| !forking).cloned(),
    })
}

/// How a session's conversation starts.
pub(super) enum Start {
    Fresh,
    /// On a saved conversation, resumed.
    Resume(onehand_core::chat::ConversationSnapshot),
    /// As a fork of another.
    Fork(StagedFork),
    /// On a host session the host already keeps.
    Watch(HostSession),
    /// Again on what it was on, the conversation taken from the session
    /// being replaced: a restart; `true` when it follows the host service's
    /// own restart.
    Restart(Option<onehand_core::chat::ConversationSnapshot>, bool),
}
