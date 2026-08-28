//! The dock's centre panel.
//!
//! Holds one [`Conversation`] per session the user has opened and renders the
//! active one. The pane is a *coordinator*: it decides which conversation is
//! showing and draws it, and what belongs to a single session lives on that
//! session rather than here. Switching is a lookup, not a save/restore.
//!
//! What is left at this level is chrome — the composer widget, the find bar,
//! the zoom, the window handle — plus the one question the pane alone can
//! answer, which is which conversation the user is looking at.

use super::composer::{Composer, ComposerEvent};
use super::conversation::{Conversation, SessionPhase};
use super::session::{ChatEvent, ChatSession};
use super::transcript;
use super::viewport::{self, FindState, RunKind};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ListState, ParentElement, Rems, Render, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, list, px, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::{Chat, ConvMeta, Link, TranscriptItemId};
use onehand_core::config::AgentSpec;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::PathBuf;

/// The rest the transcript comes to above the composer.
///
/// **Inside the scroll, not around the card.** The overlay itself must stay
/// transparent outside its surfaces so the transcript can remain visible
/// around the floating composer. Held here, this is still real scrollable
/// space: the last row can rest clear of the card without an opaque footer.
///
/// A turn's worth of air rather than a hairline. The composer is a surface of
/// its own floating over the conversation, and a transcript that stops just
/// short of it reads as one still trying to fit — the last line of an answer
/// and the box it is answered in run together into a single block.
const COMPOSER_REST: Rems = rems(1.5);
/// Safe first-frame clearance before the overlay has reported its real height.
/// The resting composer is about this tall; using zero until prepaint is what
/// lets the initial transcript tail land behind it.
const COMPOSER_MIN_H: Rems = rems(6.5);
/// The shared reading column for transcript rows and the composer surfaces.
/// `w_full` lets it shrink with a narrow panel; this cap keeps prose and machine
/// output from stretching across the whole window on a wide one.
const CONTENT_COLUMN: Rems = rems(52.);
/// The space at a turn boundary — above a prompt, and between a prompt and the
/// answer replying to it.
///
/// A turn is the unit the eye scrolls looking for. Given the same gap as the
/// blocks *within* a turn, a long conversation is one undifferentiated column
/// with nothing saying where the last question was asked.
const TURN_GAP: Rems = rems(1.5);
/// The space between two ordinary blocks of one turn.
const BLOCK_GAP: Rems = rems(0.75);
/// The space between two collapsed history rows, which are an index and are
/// read as one.
const COMPACT_GAP: Rems = rems(0.25);
/// The transcript's own head start, inside the scroll rather than around it.
///
/// Named because it is read twice: it is the padding the list draws with, and
/// it is where the top of a question held at the top of the panel comes to rest
/// — so the rule that decides when to stop holding one has to be measured from
/// the same number the row is actually drawn at.
const LIST_HEAD: Rems = rems(1.);

/// How many past conversations the project page lists.
///
/// The page is an entrance, not an archive browser: the newest handful is what
/// "where was I" needs, and a project worked in for months would otherwise draw
/// an unbounded column into a container that does not scroll. What the cap cut
/// off is said on screen rather than silently dropped.
const HOME_ROWS: usize = 8;

/// The project the pane is standing in while no conversation is showing.
///
/// Pushed by the shell rather than looked up: the workspace tree is the
/// shell's, and a copy of it here would be one more thing to keep in step.
struct EmptyProject {
    /// What the project is called, for the page's own title.
    label: SharedString,
    /// Where it is, which is what its archived conversations are keyed by --
    /// and what tells one scan's answer from another's.
    path: PathBuf,
    /// Its past conversations, across every agent. `None` while the scan is
    /// still running, which is a different thing from a project that has none:
    /// one is a wait and the other is an answer.
    history: Option<Vec<ConvMeta>>,
    /// Whether it is pinned to the top of the rail, and whether it is a git
    /// repository.
    ///
    /// Two facts the page's own menu needs and cannot work out: one lives in the
    /// workspace tree and the other in a `git status` sweep, and both are the
    /// shell's. Pushed rather than asked for, like everything else the pane
    /// knows about the window, and pushed again whenever either changes — a menu
    /// still offering *Pin to top* on a project pinned a second ago is worse
    /// than one that does not offer it at all.
    pinned: bool,
    is_repo: bool,
}

pub struct ChatPane {
    focus_handle: FocusHandle,
    /// Every session the user has opened, in whatever phase it has reached.
    ///
    /// One map, not six. What each session's phase means, and why the parallel
    /// maps this replaced could disagree with one another, is on
    /// [`Conversation`].
    conversations: HashMap<u64, Conversation>,
    active: Option<u64>,
    composer: Entity<Composer>,
    /// The transcript find bar's query, and where in the hits it is.
    ///
    /// Per pane rather than per session: the bar is chrome over whichever
    /// transcript is showing, and carrying a stale query across a session
    /// switch would show hit counts for a conversation nobody is reading --
    /// which is why every path that changes what is showing drops it.
    find: Option<FindState>,
    /// This pane's window, so a turn ending can ask whether *this* window is
    /// the active one.
    ///
    /// The pump that reports a finished turn has no `&Window` in hand, and
    /// `cx.active_window()` alone answers a different question -- "is any
    /// onehand window active" -- which in a two-window setup marks a background
    /// window's turn as seen.
    window: gpui::AnyWindowHandle,
    /// This pane's reading size. The whole pane scales, composer included:
    /// zooming the transcript and leaving the box you answer in at its old
    /// size is not a posture anyone wants.
    zoom: crate::zoom::Zoom,
    /// The session a restart was asked for while a turn was in flight, so the
    /// second press is the confirmation. A restart mid-turn throws away work
    /// the user is waiting on, which is exactly when a stray keystroke is most
    /// likely.
    ///
    /// **The session is the point, not just the fact.** A bare flag armed on
    /// one conversation was still raised after switching to another, so the
    /// next press there skipped its own confirmation and threw away a turn
    /// nobody had been warned about.
    restart_armed: Option<u64>,
    /// The conversation a delete was asked for, so the second press is the
    /// confirmation.
    ///
    /// The *conversation*, for the same reason a restart arms a session rather
    /// than the pane: an arming press made on one row says nothing about the
    /// row below it, and a bare flag would let one press on one conversation
    /// delete a different one.
    ///
    /// Deleting is the only thing this app does that cannot be undone by doing
    /// it again -- a closed session respawns, a removed project is added back,
    /// a deleted conversation is gone -- so it is guarded whether or not
    /// anything is running.
    delete_armed: Option<PathBuf>,
    /// A handle to this pane, for the callbacks the list builds outside the
    /// `render` that owns `Context<Self>`.
    handle: gpui::WeakEntity<Self>,
    /// The project the pane offers to start something in, when there is one.
    ///
    /// Only ever read while no session is showing.
    empty: Option<EmptyProject>,
    /// The archived conversation the next [`Self::show`] of a session must open
    /// on.
    ///
    /// Recorded against the session before it is shown rather than passed to
    /// `show`: the shell points the pane at whatever the workspace's active
    /// session is, and every other route into that call has no archive to name.
    /// Taken on use, so a conversation picked once cannot be resumed again by a
    /// later switch back to that session.
    pending_resume: Option<(u64, PathBuf)>,
    /// Whether the window's rail is hidden, so this pane can offer the way
    /// back.
    ///
    /// Pushed for the same reason, and read in the panel's toolbar: the rail is
    /// the window's chrome and a dock panel has no handle on the window.
    rail_hidden: bool,
    /// Whether the active project has a shell alive in the terminal dock.
    ///
    /// Pushed by the shell, like the flag above and for the same reason: the
    /// dock is the window's and this panel has no handle on it. It is not the
    /// dock's *open* state — it is whether a child process is running behind a
    /// dock that may well be closed, which is the one thing the terminal button
    /// cannot say by being a button.
    terminal_live: bool,
    /// How tall the composer overlay measured, last time it was drawn.
    ///
    /// The composer floats over the transcript, so the transcript has to end
    /// above it or its last line is permanently behind the box the user types
    /// in — and how much room that takes is not knowable in advance: the field
    /// grows with what is typed, the attachment tray appears and goes, and a
    /// parked permission pins another card on top. So the overlay is measured
    /// where it is drawn and the list is padded by what it measured.
    ///
    /// A `Cell` rather than a plain field written through the entity: this is
    /// set during *prepaint*. A changed measurement schedules exactly one more
    /// frame so non-typing changes (initial mount, a permission arriving, an
    /// attachment disappearing) also update the list's bottom padding.
    composer_h: std::rc::Rc<std::cell::Cell<gpui::Pixels>>,
    /// Whether the last frame this pane drew had a composer in it.
    ///
    /// Recorded by the renderer rather than worked out again by whoever asks,
    /// because the composer is the last of six things the body can be and the
    /// five ahead of it are early returns. A second reading of those
    /// conditions would be a copy that drifts, and the cost of it being wrong
    /// is silent: focus handed to an input that is not on screen leaves the
    /// window with nothing focused at all.
    composer_drawn: bool,
}

impl ChatPane {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let composer = cx.new(|cx| Composer::new(window, cx));
            let input = composer.read(cx).state.clone();

            cx.subscribe_in(
                &input,
                window,
                |pane: &mut Self, _, event: &InputEvent, window, cx| {
                    // Shift+Enter is a newline. A bare Enter takes the
                    // completion popup's selection when one is open, and only
                    // sends otherwise -- accepting a candidate and firing the
                    // prompt off the same key would send half-typed paths.
                    if let InputEvent::PressEnter { shift: false, .. } = event {
                        pane.enter(window, cx);
                    }
                },
            )
            .detach();

            // Send and Stop are one button, and which of the two it was is a
            // question about the turn rather than about the press: a click
            // landing a frame after the turn ended must not cancel the next
            // one. So the composer reports the press and the pane, which can
            // still see the conversation, decides.
            cx.subscribe_in(
                &composer,
                window,
                |pane: &mut Self, _, event: &ComposerEvent, window, cx| match event {
                    ComposerEvent::SendPressed => {
                        if pane.busy(cx) {
                            pane.stop(cx);
                        } else {
                            pane.submit(window, cx);
                        }
                    }
                },
            )
            .detach();

            Self {
                focus_handle: cx.focus_handle(),
                conversations: HashMap::new(),
                active: None,
                composer,
                find: None,
                zoom: crate::zoom::Zoom::default(),
                restart_armed: None,
                delete_armed: None,
                window: window.window_handle(),
                handle: cx.entity().downgrade(),
                empty: None,
                pending_resume: None,
                rail_hidden: false,
                terminal_live: false,
                composer_h: Default::default(),
                composer_drawn: false,
            }
        })
    }

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
            // app is minted by an explicit action -- the rail's *New session*, a
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
        // A conversation is coming on screen, so the page that arming press was
        // made on is not what the user is looking at any more. An arm that
        // outlived a trip into a session and back would be waiting on a row
        // nobody had just pressed.
        self.delete_armed = None;
        if switching_away(self.active, uid) {
            self.leave_shown_session(window, cx);
            self.restore_draft(uid, window, cx);
        }
        self.active = Some(uid);
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

    /// Put down everything that belonged to the session leaving the screen.
    ///
    /// One place, because these three are the same rule wearing three hats:
    /// each is pane-level state whose meaning is a single conversation. Spread
    /// across the call sites, the find bar's reset was written once and the
    /// other two not at all.
    fn leave_shown_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The query is chrome over whichever transcript is showing; a hit count
        // for a conversation nobody is reading is worse than no bar.
        self.find = None;
        // An arming press only speaks for the conversation it was made on.
        self.restart_armed = None;
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
    fn start(&mut self, uid: u64, resume: Option<ConvMeta>, cx: &mut Context<Self>) {
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
                        cx.emit(ChatPaneEvent::WorkTreeTouched);
                    }
                    // Re-emitted rather than acted on: the transcript says what
                    // was asked for, and where a file goes is the shell's call.
                    // Matched exhaustively so a new variant cannot be added and
                    // silently dropped here -- which is how this one was lost.
                    ChatEvent::OpenFile(path) => cx.emit(ChatPaneEvent::OpenFile(path.clone())),
                    ChatEvent::Appended | ChatEvent::Disconnected => {}
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
    fn set_phase(&mut self, uid: u64, phase: SessionPhase) {
        if let Some(conv) = self.conversations.get_mut(&uid) {
            conv.phase = phase;
        }
    }

    /// Whether `uid` exists and has nothing connected yet.
    fn is_opening(&self, uid: u64) -> bool {
        self.conversations
            .get(&uid)
            .is_some_and(Conversation::is_opening)
    }

    /// Whether `uid` is waiting on the user to pick a conversation to resume.
    fn is_choosing(&self, uid: u64) -> bool {
        self.conversations
            .get(&uid)
            .is_some_and(|conv| conv.choices().is_some())
    }

    /// The conversation showing right now, if one is.
    fn active_conversation(&self) -> Option<&Conversation> {
        self.conversations.get(&self.active?)
    }

    fn active_conversation_mut(&mut self) -> Option<&mut Conversation> {
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
    fn archive_meta_detached(uid: u64, session: &Entity<ChatSession>, cx: &mut Context<Self>) {
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

    /// This pane's zoom, for the shell to step.
    pub fn zoom_mut(&mut self) -> &mut crate::zoom::Zoom {
        &mut self.zoom
    }

    /// This pane's zoom, for the status bar to report.
    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    /// Put the caret in the composer.
    pub fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .read(cx)
            .state
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    /// Take focus back from a panel that is being unmounted.
    ///
    /// **A window with nothing focused answers no shortcut at all.** GPUI
    /// resolves a key against the path from the root of the frame's dispatch
    /// tree down to the focused node; with no focused node the path is the root
    /// alone, and every handler the app hung on the window's own frame sits
    /// below it, unreachable. That is what a closing panel leaves behind if the
    /// caret was inside it -- the focused element is simply not in the next
    /// frame, so the key that closed the panel cannot reopen it, and neither
    /// can any other.
    ///
    /// The conversation is where focus belongs on the way out, since it is what
    /// the panel was covering. The composer takes it when there is one on
    /// screen, so typing resumes where the user left it; otherwise the pane's
    /// own handle does, which is drawn unconditionally and is all the keymap
    /// needs.
    pub fn reclaim_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer_drawn {
            self.focus_composer(window, cx);
            return;
        }
        self.focus_handle.clone().focus(window, cx);
        cx.notify();
    }

    /// Restart the active session's adapter on the same conversation.
    ///
    /// Dropping the old session is what kills the old adapter (its pump task
    /// owns the event stream), so this cannot leave two processes replaying
    /// into one transcript. The live transcript is snapshotted and handed to
    /// the new session as history, so the pane keeps reading as itself while
    /// the agent replays -- the model comes back empty, and a restart that
    /// blanks the conversation looks like data loss even though it is not.
    pub fn restart_active(&mut self, cx: &mut Context<Self>) -> Restart {
        let Some(uid) = self.active else {
            return Restart::Nothing;
        };
        // The session is looked up twice rather than held across the whole
        // function, and neither look-up is a clone. A cloned handle is a second
        // strong reference to the old session, and the old adapter only dies
        // when the last one goes -- so holding one here would keep the process
        // alive right through the spawn of its replacement, which is the one
        // thing this is careful about.
        let Some(busy) = self.session_of(uid).map(|s| s.read(cx).chat.busy) else {
            return Restart::Nothing;
        };
        if restart_needs_arming(busy, self.restart_armed, uid) {
            self.restart_armed = Some(uid);
            return Restart::Armed;
        }
        self.restart_armed = None;

        // Taken only now: an arming press does not need it.
        //
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
        if self.restart_armed == Some(uid) {
            self.restart_armed = None;
        }
        if self.active == Some(uid) {
            self.active = None;
            // Nothing is showing for the bar to be searching. What the composer
            // still holds is dropped by the next `show`, which treats an
            // unaddressed draft as unaddressed.
            self.find = None;
        }
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
        let unchanged = self.active.is_none() && standing_in == root.as_ref().map(|(_, path)| path);
        if unchanged {
            return;
        }
        self.empty = root.map(|(label, path)| EmptyProject {
            label,
            path,
            history: None,
            // Both arrive from the shell a moment later, on the same switch
            // that brought this page up. Defaulting to "not pinned, not a
            // repository" is what a menu drawn in that moment can honestly say.
            pinned: false,
            is_repo: false,
        });
        // The page being replaced takes its arming press with it: a press made
        // against a row on one project's page speaks for nothing on another's.
        self.delete_armed = None;
        self.scan_project_history(cx);
        // Going to no session at all is still leaving the one that was showing,
        // and the draft has to be put down here too: a prompt left in the box
        // while passing through an empty project would otherwise be sitting
        // there, addressed to nobody, when the next session opens.
        self.leave_shown_session(window, cx);
        self.active = None;
        cx.notify();
    }

    /// Read the project page's list of past conversations, off the UI loop.
    ///
    /// Across every agent, not just the configured default: what the user is
    /// looking for is a conversation they had in this project, and which agent
    /// ran it is a detail of that conversation rather than a filter on the
    /// question. The resume picker inside a session is the narrower one -- there
    /// the agent is already chosen, because the session it belongs to has one.
    fn scan_project_history(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.empty.as_ref().map(|project| project.path.clone()) else {
            return;
        };
        let scan = path.clone();
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &scan,
                        None,
                    )
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // Reading a directory of archives takes long enough that the
                // user can have moved on twice over. An answer about a project
                // the pane has left is not this page's list, and adopting it
                // would put one project's conversations under another's name.
                let Some(project) = pane.empty.as_mut().filter(|project| project.path == path)
                else {
                    return;
                };
                project.history = Some(past);
                cx.notify();
            });
        })
        .detach();
    }

    /// Delete an archived conversation, on the second press.
    ///
    /// Offered on the project page and nowhere else, and that is the guard
    /// doing most of the work rather than a rule anybody has to remember: the
    /// page is what shows when the selected project has **no session on it**,
    /// so the conversations listed there are the ones nothing is writing to.
    /// A live conversation deleted underneath its own session would not even
    /// stay deleted -- the session's next turn writes the file again, holding
    /// only what came after, because its mark says the rest is already on disk.
    /// A session in another *window* is the case the page's own shape does not
    /// cover, so the check below covers it.
    fn delete_conversation(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if delete_needs_arming(self.delete_armed.as_deref(), &dir) {
            self.delete_armed = Some(dir);
            cx.notify();
            return;
        }
        self.delete_armed = None;

        let store = onehand_core::chat::conversations_dir();
        let live = self
            .conversations
            .values()
            .filter_map(Conversation::session)
            .any(|session| {
                session
                    .read(cx)
                    .chat
                    .session_id
                    .as_deref()
                    .is_some_and(|sid| onehand_core::chat::conv_dir(&store, sid) == dir)
            });
        if live {
            cx.emit(ChatPaneEvent::ConversationNotDeleted(
                "it is open in a session".to_string(),
            ));
            cx.notify();
            return;
        }

        let removing = dir.clone();
        cx.spawn(async move |pane, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { onehand_core::chat::delete(&removing) })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                match done {
                    // Taken off the page here rather than by scanning the
                    // directory again: the row is gone because the thing it
                    // named is gone, and a second read of the store to
                    // discover that is a read that can also answer late.
                    Ok(()) => {
                        if let Some(project) = pane.empty.as_mut()
                            && let Some(history) = project.history.as_mut()
                        {
                            history.retain(|conv| conv.dir != dir);
                        }
                    }
                    Err(e) => cx.emit(ChatPaneEvent::ConversationNotDeleted(e.to_string())),
                }
                cx.notify();
            });
        })
        .detach();
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

    /// Told by the shell when the rail comes and goes.
    pub fn set_rail_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        self.rail_hidden = hidden;
        cx.notify();
    }

    /// Told by the shell when a shell starts or dies on the active project.
    ///
    /// **Guarded**, unlike the rail's flag: a terminal notifies once per chunk
    /// of whatever is printing into it, and the shell's own guard upstream is
    /// about its own repaint. Repainting the whole conversation on every line a
    /// build prints, to redraw a dot that has not moved, is the one thing this
    /// must not cost.
    /// Told by the shell what the selected project is, beyond its name.
    ///
    /// Guarded for the same reason the flag below is: this is pushed from the
    /// same places a git sweep lands, and a sweep lands on every finished turn.
    pub fn set_project_facts(&mut self, pinned: bool, is_repo: bool, cx: &mut Context<Self>) {
        let Some(project) = self.empty.as_mut() else {
            return;
        };
        if project.pinned == pinned && project.is_repo == is_repo {
            return;
        }
        project.pinned = pinned;
        project.is_repo = is_repo;
        cx.notify();
    }

    pub fn set_terminal_live(&mut self, live: bool, cx: &mut Context<Self>) {
        if self.terminal_live == live {
            return;
        }
        self.terminal_live = live;
        cx.notify();
    }

    /// `uid`'s live session, if it has reached one.
    fn session_of(&self, uid: u64) -> Option<&Entity<ChatSession>> {
        self.conversations.get(&uid)?.session()
    }

    fn active_chat<'a>(&self, cx: &'a App) -> Option<&'a Chat> {
        Some(&self.active_conversation()?.session()?.read(cx).chat)
    }

    /// What Enter does, which depends on whether the popup is open.
    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn unqueue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session() else {
            return;
        };
        session.update(cx, |session, cx| {
            session.chat.cancel_turn();
            cx.notify();
        });
    }

    fn session(&self) -> Option<Entity<ChatSession>> {
        self.active_conversation()?.session().cloned()
    }

    /// A turn settled. Badge it, and say so out loud if the window is not even
    /// on screen.
    fn turn_ended_detached(&mut self, uid: u64, agent: &str, root: &str, cx: &mut Context<Self>) {
        // "Is *this* window the active one" -- not "is any onehand window
        // active", which is what a bare `active_window().is_some()` asks and
        // which marks a background window's turn as already seen.
        let here = cx.active_window() == Some(self.window);
        if self.active == Some(uid) && here {
            return;
        }
        if let Some(conv) = self.conversations.get_mut(&uid) {
            conv.unseen = true;
        }
        // Only when the window itself is away: a badge is enough for a
        // background session whose rail row the user can already see.
        if !here {
            super::session::notify_turn_ended(agent.to_string(), root.to_string());
        }
        cx.notify();
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

    /// Open the find bar, or close it if it is already open.
    pub fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.find.take() {
            Some(_) => {}
            None => {
                let query =
                    cx.new(|cx| InputState::new(window, cx).placeholder("Find in transcript…"));
                cx.subscribe(&query, |pane: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        // A new query invalidates where we were in the old one.
                        if let Some(find) = &mut pane.find {
                            find.current = 0;
                        }
                        cx.notify();
                    }
                })
                .detach();
                query.focus_handle(cx).focus(window, cx);
                self.find = Some(FindState::new(query));
            }
        }
        cx.notify();
    }

    /// Step through the hits, wrapping, and scroll the new one into view.
    /// `delta` is +1 / -1.
    ///
    /// Scrolling happens here and **not** while the query is being typed. Every
    /// keystroke changes the hit list, so revealing on each one would drag the
    /// transcript around under a user who is still deciding what to search for;
    /// Next and Previous are the presses that mean "take me there".
    fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        let hits = self.matches(cx);
        if hits.is_empty() {
            return;
        }
        let Some(find) = &mut self.find else {
            return;
        };
        let next = find.current as isize + delta;
        find.current = next.rem_euclid(hits.len() as isize) as usize;
        let target = hits[find.current].target;

        // A hit inside a collapsed activity strip is one the user is told about
        // and cannot see, so the strip that holds it opens. The run's position
        // does not move: folding decides what a run draws, never how many runs
        // there are.
        if let Some(anchor) = self
            .active_conversation()
            .and_then(|conv| conv.viewport.reveal(target))
            && let Some(session) = self.session()
        {
            session.update(cx, |session, cx| {
                session.toggle_activity(anchor);
                cx.notify();
            });
        }
        cx.notify();
    }

    fn matches(&mut self, cx: &App) -> Vec<onehand_core::chat::TranscriptMatch> {
        let Some(chat) = self.active_chat(cx) else {
            return Vec::new();
        };
        let Some(find) = &mut self.find else {
            return Vec::new();
        };
        find.matches(chat, cx)
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

    /// The past conversations `uid` is choosing between, or nothing if it is
    /// not choosing.
    fn choices_of(&self, uid: u64) -> Vec<ConvMeta> {
        self.conversations
            .get(&uid)
            .and_then(Conversation::choices)
            .unwrap_or_default()
            .to_vec()
    }

    /// The resume picker: past conversations for this root + agent, newest
    /// first, plus the option to start fresh.
    fn resume_picker(&mut self, uid: u64, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let past = self.choices_of(uid);
        let now = onehand_core::chat::now_secs();

        div()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .w_full()
                    .max_w(px(560.))
                    .child(div().font_semibold().child("Resume a conversation"))
                    .children(past.into_iter().enumerate().map(|(i, meta)| {
                        // The agent is not named here: this picker belongs to a
                        // session that already has one, and every row in it was
                        // run by that same agent.
                        let subtitle = format!(
                            "{} · {} items",
                            rel_time(now, meta.updated),
                            meta.item_count
                        );
                        conversation_card(
                            ("resume", i),
                            meta.title.clone().into(),
                            subtitle.into(),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |pane: &mut Self, _, _, cx| {
                                let meta = pane.choices_of(uid).get(i).cloned();
                                pane.start(uid, meta, cx);
                            },
                        ))
                    }))
                    .child(
                        crate::controls::action("resume-fresh")
                            .primary()
                            .label("Start a new conversation")
                            .on_click(cx.listener(move |pane: &mut Self, _, _, cx| {
                                pane.start(uid, None, cx);
                            })),
                    ),
            )
    }

    /// The project page: what the centre of the window shows while the selected
    /// project has no conversation on it.
    ///
    /// This is the state every freshly added project starts in, and the state a
    /// project returns to when its last session is closed -- so it is the first
    /// thing a new user sees, and it used to be one line of grey text saying
    /// *Start a session in X* with nothing to press. Everything a project can be
    /// entered by is here instead: the conversations already had in it, newest
    /// first and across every agent, and the button that starts a fresh one.
    ///
    /// The history is drawn from the same card the resume picker uses, because
    /// it is the same question -- which past conversation -- and one of the two
    /// looking unclickable is how a list stops being read as a list.
    fn project_home(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(project) = self.empty.as_ref() else {
            // Not a project with nothing in it -- no project at all. Naming
            // what has to happen first beats an offer that cannot be taken:
            // every session belongs to a root, and there is no root to bind one
            // to.
            return div()
                .size_full()
                .v_flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Add a project to start a session")
                .into_any_element();
        };
        let now = onehand_core::chat::now_secs();
        let muted = cx.theme().muted_foreground;
        let danger = crate::theme::status_ink(cx).danger;
        // Read out here rather than inside the rows: the rows borrow `cx` to
        // build their own callbacks, and this is one answer for all of them.
        let armed = self.delete_armed.clone();
        // Bounded, and the bound says so below. A project worked in for months
        // has more archives than this page is for, and none of this scrolls.
        let shown: Vec<ConvMeta> = project
            .history
            .iter()
            .flatten()
            .take(HOME_ROWS)
            .cloned()
            .collect();
        let hidden = project
            .history
            .as_ref()
            .map_or(0, |all| all.len().saturating_sub(HOME_ROWS));
        // `None` is the scan still running, `Some([])` is a project that has
        // never been prompted. Both draw a line, and they must not draw the
        // same one: telling a user with a hundred conversations that they have
        // none, for the half-second a directory read takes, is worse than
        // saying nothing.
        let note = match &project.history {
            None => Some("Looking for past conversations…"),
            Some(all) if all.is_empty() => Some("No conversations in this project yet."),
            Some(_) => None,
        };

        div()
            .size_full()
            .v_flex()
            // The header stays. It is the panel's only chrome, and everything on
            // it that this page can still answer is about the *project* rather
            // than about a conversation: the file tree, a shell, the way back to
            // a hidden rail. Dropping it here took all three away at exactly the
            // moment there is no conversation to reach them from instead -- and
            // a panel that loses its own chrome between one click and the next
            // reads as one that broke.
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .child(
                        div()
                            .v_flex()
                            .gap_3()
                            .w_full()
                            .max_w(px(560.))
                            // The project's name is *not* repeated here. The
                            // header above says it now, in the same place it
                            // says a conversation's name, and printing it again
                            // two rows lower was one word twice on a page whose
                            // whole job is to offer the few things there are.
                            .child(
                                crate::controls::action("project-new-session")
                                    .primary()
                                    .icon(Icon::new(IconName::Plus))
                                    .label("New session")
                                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                                        cx.emit(ChatPaneEvent::StartSession {
                                            agent: None,
                                            resume: None,
                                        });
                                    })),
                            )
                            .children(
                                note.map(|note| div().text_xs().text_color(muted).child(note)),
                            )
                            .children((!shown.is_empty()).then(|| {
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("Past conversations")
                            }))
                            .children(shown.into_iter().enumerate().map(|(i, meta)| {
                                // The agent *is* named here, unlike in a session's own
                                // picker: this list crosses every agent that has worked
                                // in the project, and resuming a row starts a session on
                                // the one that held it.
                                let subtitle = format!(
                                    "{} · {} items · {}",
                                    rel_time(now, meta.updated),
                                    meta.item_count,
                                    meta.agent
                                );
                                let (agent, archive) =
                                    (SharedString::from(meta.agent.clone()), meta.dir.clone());
                                let armed = armed.as_deref() == Some(meta.dir.as_path());
                                let dir = meta.dir.clone();
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .w_full()
                                    .items_center()
                                    .child(
                                        conversation_card(
                                            ("home", i),
                                            meta.title.clone().into(),
                                            subtitle.into(),
                                            cx,
                                        )
                                        .flex_1()
                                        .on_click(
                                            cx.listener(move |_: &mut Self, _, _, cx| {
                                                cx.emit(ChatPaneEvent::StartSession {
                                                    agent: Some(agent.clone()),
                                                    resume: Some(archive.clone()),
                                                });
                                            }),
                                        ),
                                    )
                                    // A word rather than a glyph, and this is the one
                                    // control in the app that earns the distinction:
                                    // everything else it offers can be done again --
                                    // a closed session respawns, a removed project is
                                    // added back -- and a deleted conversation cannot.
                                    // The confirming state has to be a word anyway, so
                                    // a picture would only be half the control.
                                    .child(
                                        crate::controls::action(("home-delete", i))
                                            .ghost()
                                            .small()
                                            .text_color(danger)
                                            .label(if armed { "Delete?" } else { "Delete" })
                                            .tooltip(if armed {
                                                "Press again to delete this conversation for good"
                                            } else {
                                                "Delete this conversation"
                                            })
                                            .on_click(cx.listener(
                                                move |pane: &mut Self, _, _, cx| {
                                                    pane.delete_conversation(dir.clone(), cx);
                                                },
                                            )),
                                    )
                            }))
                            .children((hidden > 0).then(|| {
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{hidden} older not shown"))
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The blocking cards the agent is parked on, drawn just above the composer.
    ///
    /// Pinned rather than left in the transcript because the transcript scrolls
    /// and this does not: a permission that arrived four screens ago is still
    /// the only reason nothing is happening, and hunting for it is not a thing
    /// to ask of someone who is waiting. Once answered the card leaves here and
    /// takes its place in the transcript, where it reads as a record of what
    /// was decided rather than as a control.
    ///
    /// The transcript is what leaves it out — see the projection — so the card
    /// is never drawn twice.
    fn pinned(
        &self,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let Some(chat) = self.active_chat(cx) else {
            return Vec::new();
        };
        let mut out: Vec<(usize, gpui::AnyElement)> = chat
            .pending_permissions()
            .into_iter()
            .map(|(idx, p)| {
                (
                    idx,
                    transcript::permission(session, p, TranscriptItemId::Live(idx), cx)
                        .into_any_element(),
                )
            })
            .chain(chat.pending_asks().into_iter().map(|(idx, a)| {
                (
                    idx,
                    transcript::ask(session, a, TranscriptItemId::Live(idx), cx).into_any_element(),
                )
            }))
            .collect();
        // Two lists merged back into transcript order: the agent can park on a
        // permission and a question at once, and the order they were asked in
        // is the only order that makes sense of them.
        out.sort_by_key(|(idx, _)| *idx);
        let mut pinned: Vec<gpui::AnyElement> =
            out.into_iter().map(|(_, element)| element).collect();
        // Under the blocking cards and directly over the composer, because that
        // is where the prompt it holds was written and where it will reappear
        // if the queue is cancelled.
        pinned.extend(self.connecting_strip(cx).map(IntoElement::into_any_element));
        pinned.extend(self.queued_strip(cx).map(IntoElement::into_any_element));
        pinned
    }

    /// Shown while the adapter is still coming up.
    ///
    /// **A resumed conversation is on screen before it is live.** The archive
    /// is adopted the moment one is picked, deliberately -- blanking the pane
    /// for the seconds an adapter takes to spawn would be worse. But that
    /// leaves a transcript, a header and a composer that all look ready while
    /// nothing can be sent yet, and the only thing that said so was a Send
    /// button that refused once pressed.
    ///
    /// Over the composer rather than in the transcript, because it is a fact
    /// about the *session* and not a thing the conversation said -- and because
    /// this is the corner the user is looking at when they go to type.
    ///
    /// Only a **re**connect ever sees this. A conversation coming up for the
    /// first time draws nothing but the wait, so there is no composer for a
    /// strip to sit over; what is left here is the case where the transcript
    /// stays -- a restart, or an adapter respawned after it died.
    fn connecting_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let chat = self.active_chat(cx)?;
        if chat.link != Link::Connecting {
            return None;
        }
        let status = SharedString::from(chat.activity_status()?);
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .px_3()
                .py_2()
                .rounded(cx.theme().radius * 2.)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover.alpha(1.))
                .shadow_lg()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Spinner::new().xsmall())
                .child(status),
        )
    }

    /// What is waiting for this turn to end, and the way to take it back.
    ///
    /// A prompt that left the composer and is not in the transcript is a prompt
    /// nothing on screen accounts for -- which is indistinguishable from one
    /// the app dropped.
    fn queued_strip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let queued = self.active_chat(cx)?.queued.as_ref()?;
        let line = SharedString::from(onehand_core::chat::first_line_trunc(&queued.text, 80));
        let count = queued.attachments.len();
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .px_3()
                .py_2()
                .rounded(cx.theme().radius * 2.)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover.alpha(1.))
                .shadow_lg()
                .text_sm()
                .child(Icon::new(IconName::Calendar).size_3())
                .child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground)
                        .child("Queued"),
                )
                .child(div().flex_1().min_w_0().truncate().child(line))
                .children((count > 0).then(|| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(match count {
                            1 => "1 attachment".to_string(),
                            n => format!("{n} attachments"),
                        })
                }))
                .child(
                    crate::controls::action("unqueue")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Close))
                        .tooltip("Put it back in the composer")
                        .on_click(cx.listener(|pane: &mut Self, _, window, cx| {
                            pane.unqueue(window, cx);
                        })),
                ),
        )
    }

    /// Go back to choosing which past conversation this session should run.
    ///
    /// The live one has its name and settings written on the way out: the
    /// transcript is written at the end of every turn, so an idle conversation
    /// is already on disk, but the title and the selector picks are metadata
    /// and leaving without writing them would lose them.
    ///
    /// The adapter stays up until a choice is made. Dropping it here would gain
    /// nothing -- the choice is what decides which conversation to connect to,
    /// and `connect` drops it before spawning the replacement anyway.
    fn show_history(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.active else {
            return;
        };
        let Some(conv) = self.conversations.get(&uid) else {
            return;
        };
        if let Some(session) = conv.session() {
            Self::archive_meta_detached(uid, &session.clone(), cx);
        }
        let (root, agent) = (conv.root.clone(), conv.spec.name.clone());
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &root,
                        Some(&agent),
                    )
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // An empty list is not a picker with nothing in it -- it is a
                // session that has never been anywhere else, and putting up a
                // page whose only option is the one already on screen would be
                // a dead end.
                if !past.is_empty() {
                    pane.set_phase(uid, SessionPhase::ChoosingHistory(past));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The session header: what conversation this is, what it is doing, and the
    /// things you do *to* it.
    ///
    /// Separate from the composer's row because the two answer different
    /// questions. The composer's controls are about the message being written —
    /// what to attach, which mode to send it in, whether to send it at all. Find,
    /// Export, Restart and Close are about the conversation as a whole, and
    /// mixing them into one row of seven buttons made every one of them equally
    /// easy to hit by accident.
    ///
    /// It is also **the only chrome this panel has**. The dock draws the
    /// conversation as a bare panel with no tab bar, so the two ways back to
    /// something the window has put away — the rail and the Workbench — have
    /// nowhere else to be offered from, and a route that exists only as a
    /// keystroke is a route only someone who already knows it can take.
    ///
    /// **The name carries the conversation's own menu**, and the right-hand end
    /// carries only what is about the *window*: find, and the way back to the
    /// Workbench. That split is why there is no ••• here any more — a menu button
    /// beside the name it acts on says nothing the name could not say itself, and
    /// the things in it were all things done to the conversation the name is.
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chat = self.active_chat(cx);
        let title = chat.and_then(Chat::conversation_title).unwrap_or_else(|| {
            self.empty
                .as_ref()
                .map(|project| project.label.to_string())
                .unwrap_or_default()
        });
        // Nothing while a live thought or a running tool is already saying it:
        // the status line answers "is anything happening", and repeating what
        // the block above says is noise, not reassurance.
        let status = chat.and_then(Chat::activity_status);
        let busy = chat.is_some_and(|chat| chat.busy);
        let signal = self.active.and_then(|uid| self.signal(uid, cx));
        // What the badge says, or nothing at all.
        //
        // **Two sources, in this order.** The activity status is the specific
        // sentence -- which agent is being connected to, that approval is what
        // is being waited on -- so it wins wherever there is one. Where there is
        // not, a signal that is *not* busy still has something to say, and
        // saying it here is new: a dead adapter used to leave this header
        // silent, with only the rail's small triangle to notice. Busy with no
        // status is the case that stays silent on purpose, because it means the
        // transcript's own last block is already spelling out what is running.
        let badge = match (status, signal) {
            (Some(text), signal) => Some((signal, SharedString::from(text))),
            (None, Some(signal)) if !matches!(signal, SessionSignal::Busy) => Some((
                Some(signal),
                SharedString::from(crate::rail::signal_word(signal)),
            )),
            _ => None,
        };
        // A conversation the agent has not named yet has no directory to remove:
        // nothing is written until the first turn ends. The menu says so by
        // refusing rather than by hiding the entry, which would make the whole
        // menu change shape between one turn and the next.
        let archive = chat
            .and_then(|chat| chat.session_id.as_deref())
            .map(|sid| onehand_core::chat::conv_dir(&onehand_core::chat::conversations_dir(), sid));
        // The title is a menu only where there *is* a conversation. Standing on
        // a project with no session the same line names the project, and every
        // entry behind it would be about something that does not exist yet.
        let live = chat.is_some();

        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(self.title_control(title, busy, archive, cx))
            .children(badge.map(|(signal, text)| status_badge(signal, text, cx)))
            .child(div().flex_1())
            // Hiding the rail must not be a one-way door: with it gone there is
            // no workspace name, no project list and no session list, and the
            // way back would be a keystroke the user would have had to already
            // know. So the route rides in the header of the panel that took the
            // space -- and only while the rail is actually gone, because a
            // button that unhides what is already on screen does nothing.
            .when(self.rail_hidden, |header| {
                header.child(
                    header_control("show-rail", IconName::PanelLeft, cx)
                        .tooltip("Show the navigation rail")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::ShowRail);
                        })),
                )
            })
            // Only where there is a transcript to search. On the project page
            // this would open a bar over a list of past conversations and report
            // no matches for every word in them, which is a control that can
            // only fail.
            .when(live, |header| {
                header.child(
                    header_control("find", IconName::Search, cx)
                        .tooltip("Find in this conversation")
                        .on_click(cx.listener(|pane: &mut Self, _, window, cx| {
                            pane.toggle_find(window, cx);
                        })),
                )
            })
            // Beside the Workbench button rather than in the status bar, where
            // it used to be: both are docks this panel is sitting between, and
            // a closed one leaves nothing on screen at all -- no edge, no strip,
            // no name -- so the route to it belongs with the panel that took the
            // space. Which mode it opens on and whether a second press closes it
            // are the shell's rules.
            .child(self.terminal_control(cx))
            // The Workbench closed leaves nothing on screen at all -- no strip,
            // no edge, no name -- so without this the file tree and the editor
            // exist only for someone who remembers two keystrokes. Offered from
            // here rather than done here: which mode it opens on and whether a
            // second press closes it are the shell's rules, and the chat has no
            // business knowing a dock is where the Workbench lives.
            .child(
                header_control("workbench", IconName::PanelRight, cx)
                    .tooltip("Show the Workbench")
                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                        cx.emit(ChatPaneEvent::ToggleWorkbench);
                    })),
            )
            // Last, and only while there is a session to end. It keeps the
            // conversation -- the transcript is written at the end of every turn
            // and closing costs nothing that is not already on disk -- which is
            // why it can be a control on the row while deleting stays behind the
            // name, two presses and a warning away.
            .when(live, |header| {
                header.child(
                    header_control("close-session", IconName::Close, cx)
                        .tooltip("Close this session and its agent")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::CloseSession);
                        })),
                )
            })
    }

    /// The way to the terminal, and whether a shell is already running in it.
    ///
    /// **The dot is the whole reason this is not one more plain button.** A
    /// shell outliving a closed dock is the one fact the icon cannot carry: the
    /// child is still running, it is still holding whatever it was doing, and
    /// closing the window is what would end it. It rides at the corner rather
    /// than inside the button so the button keeps the square metrics its
    /// neighbours have — a child in the content row would make this one control
    /// wider than the three beside it, which reads as a mistake.
    ///
    /// Success ink, the same colour the app uses for a turn that finished
    /// unseen: both mean "something of yours is there and you are not looking at
    /// it".
    fn terminal_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let live = self.terminal_live;
        let success = crate::theme::status_ink(cx).success;

        div()
            .relative()
            .flex_none()
            .child(
                header_control("terminal", IconName::SquareTerminal, cx)
                    .tooltip(if live {
                        "A shell is running here — show the terminal"
                    } else {
                        "Open a shell in this project"
                    })
                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                        cx.emit(ChatPaneEvent::ToggleTerminal);
                    })),
            )
            .when(live, |control| {
                control.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .size(rems(0.375))
                        .rounded_full()
                        .bg(success),
                )
            })
    }

    /// The name of the conversation on screen, and everything done *to* it.
    ///
    /// **The name is the control.** It is the loudest thing in the header --
    /// full-strength ink and semibold against a row that is otherwise muted --
    /// because it is the one thing there that answers "which conversation is
    /// this", and it was drawn in the same grey as the status beside it. What
    /// says it can be pressed is the hover: the background arrives and a chevron
    /// appears at its end. The chevron's space is held whether or not it is
    /// drawn, so the name does not move under the pointer that is about to
    /// press it.
    ///
    /// **The project page gets the same control**, naming the project instead
    /// and holding what is done to a project. Same shape on purpose: on that
    /// page this line is still "what you are looking at", and a name that is a
    /// menu in one state and inert in the other teaches the user it is neither.
    /// Where there is no project at all it *is* inert — there is nothing behind
    /// it to act on, and an empty menu is worse than none.
    ///
    /// **Closing the session is not in here.** It is a control at the right-hand
    /// end of the header, with the rest of what is about the window — it keeps
    /// every word of the conversation, so it does not belong beside the entry
    /// that throws the conversation away.
    fn title_control(
        &self,
        title: String,
        busy: bool,
        archive: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let live = self.active_chat(cx).is_some();
        let project = (!live).then_some(self.empty.as_ref()).flatten();
        let name = div()
            .truncate()
            .text_color(cx.theme().foreground)
            .font_semibold()
            .child(title);
        if !live && project.is_none() {
            return div().flex_none().min_w_0().child(name).into_any_element();
        }
        let project = project.map(|project| (project.pinned, project.is_repo));

        let (muted, radius) = (cx.theme().muted_foreground, cx.theme().radius);
        // One colour for hover and for the open menu: while the menu is up the
        // row it came from has to keep saying so, or the pointer moving down
        // into the menu leaves nothing on screen pointing back at what it acts
        // on.
        let lit = cx.theme().secondary;
        let this = cx.entity();

        let row = div()
            .id("conversation-title")
            .group("conversation-title")
            .h_flex()
            .items_center()
            .gap_1()
            .flex_none()
            .min_w_0()
            .px_1p5()
            .py_0p5()
            .rounded(radius)
            .cursor_pointer()
            .hover(move |row| row.bg(lit))
            .child(name)
            .child(
                div()
                    .flex_none()
                    .invisible()
                    .group_hover("conversation-title", |chevron| chevron.visible())
                    .child(Icon::new(IconName::ChevronDown).size_3().text_color(muted)),
            );

        if let Some((pinned, is_repo)) = project {
            let target = this.clone();
            return crate::controls::MenuTrigger::new(row, lit)
                .dropdown_menu_with_anchor(
                    gpui::Anchor::TopLeft,
                    project_menu(pinned, is_repo, target),
                )
                .into_any_element();
        }

        crate::controls::MenuTrigger::new(row, lit)
            .dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |menu, _, cx| {
                let danger = crate::theme::status_ink(cx).danger;
                let (rename, export, history) = (this.clone(), this.clone(), this.clone());
                let (restart, remove) = (this.clone(), this.clone());
                let archive = archive.clone();
                menu.item(
                    PopupMenuItem::new("Rename…")
                        .icon(Icon::new(crate::icons::Icon::SquarePen))
                        .on_click(move |_, _, cx: &mut App| {
                            rename.update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Rename));
                        }),
                )
                .item(
                    PopupMenuItem::new("Export as Markdown…")
                        .icon(Icon::new(IconName::ExternalLink))
                        .on_click(move |_, _, cx: &mut App| {
                            export.update(cx, |pane: &mut Self, cx| pane.export(cx));
                        }),
                )
                // Named and refusing rather than absent. The transcript is held
                // in a shape JSON can carry and this is the format another tool
                // reads; leaving it out entirely would say the opposite.
                .item(
                    PopupMenuItem::new("Export as JSON… (not yet)")
                        .icon(Icon::new(IconName::File))
                        .disabled(true),
                )
                .separator()
                .item(
                    // Disabled mid-turn rather than guarded by a second click:
                    // going back to the picker throws the running turn away
                    // exactly as a restart does, and a menu that has to be
                    // opened twice to be believed is a worse warning than an
                    // item that will not go.
                    PopupMenuItem::new("Resume another conversation…")
                        .icon(Icon::new(IconName::Undo))
                        .disabled(busy)
                        .on_click(move |_, _, cx: &mut App| {
                            history.update(cx, |pane: &mut Self, cx| pane.show_history(cx));
                        }),
                )
                .item(
                    PopupMenuItem::new("Restart the agent")
                        .icon(Icon::new(IconName::Redo))
                        .on_click(move |_, _, cx: &mut App| {
                            restart.update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Restart));
                        }),
                )
                .separator()
                .item(
                    // The only entry here that ends something for good.
                    // Closing the session -- which keeps every word of this on
                    // disk -- is a control of its own at the other end of the
                    // header, so the two are never one press apart.
                    PopupMenuItem::element(move |_, _| {
                        div().text_color(danger).child("Delete conversation")
                    })
                    .icon(Icon::new(IconName::Delete).text_color(danger))
                    // Nothing on disk to remove until the first turn has ended,
                    // and an entry that can only report that is one the eye has
                    // to learn to skip.
                    .disabled(archive.is_none())
                    .on_click(move |_, _, cx: &mut App| {
                        let Some(dir) = archive.clone() else {
                            return;
                        };
                        remove.update(cx, |_: &mut Self, cx| {
                            cx.emit(ChatPaneEvent::DeleteConversation(dir))
                        });
                    }),
                )
            })
            .into_any_element()
    }

    /// The find bar, when it is open.
    fn find_bar(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let hits = self.matches(cx).len();
        let find = self.find.as_mut()?;
        // The transcript grows under an open bar, so the cursor is clamped
        // against the live hit list rather than trusted from last frame.
        if find.current >= hits {
            find.current = 0;
        }
        let position = if hits == 0 {
            "no matches".to_string()
        } else {
            format!("{} of {hits}", find.current + 1)
        };
        let query = find.query.clone();

        Some(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().flex_1().child(Input::new(&query)))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(position),
                )
                .child(
                    crate::controls::action("find-prev")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::ChevronUp))
                        .on_click(cx.listener(|pane: &mut Self, _, _, cx| {
                            pane.step_find(-1, cx);
                        })),
                )
                .child(
                    crate::controls::action("find-next")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::ChevronDown))
                        .on_click(cx.listener(|pane: &mut Self, _, _, cx| {
                            pane.step_find(1, cx);
                        })),
                )
                .child(
                    crate::controls::action("find-close")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Close))
                        .on_click(cx.listener(|pane: &mut Self, _, window, cx| {
                            pane.toggle_find(window, cx);
                        })),
                ),
        )
    }

    fn busy(&self, cx: &App) -> bool {
        self.active_chat(cx).is_some_and(|chat| chat.busy)
    }

    /// Rebuild the active session's run layout, and hand back the list state
    /// that draws it.
    ///
    /// One call, because the two have to agree: the list's item count is the
    /// plan's length, and a list told about a different number of runs than the
    /// plan holds draws blanks or drops the tail.
    fn reproject(&mut self, room: viewport::TopRoom, cx: &App) -> Option<ListState> {
        let session = self.active_conversation()?.session()?.clone();
        let session = session.read(cx);
        let handle = self.handle.clone();
        let conv = self.active_conversation_mut()?;
        conv.viewport
            .replan(&session.chat, session.folds_revision(), |anchor| {
                session.activity_is_open(anchor)
            });
        let state = conv.viewport.list_state(session.chat.busy, room);
        // Asked for once the state exists, and only then: the list is what
        // knows it has been scrolled, and the pane is what draws the control
        // that depends on it. Without this the pill waited for whatever
        // happened to redraw the pane next, which on a finished conversation
        // is nothing at all.
        conv.viewport.hook_scroll(move |_, _, cx| {
            let _ = handle.update(cx, |_: &mut Self, cx| cx.notify());
        });
        Some(state)
    }

    /// Take the reader back to where the latest activity is arriving.
    ///
    /// Through the viewport rather than straight at the list, because the list
    /// cannot answer where that is: while a question is held at the top, the
    /// activity is arriving in the room under it and the place to return to is
    /// the question, which only the layout knows the row of.
    fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        if let Some(conv) = self.active_conversation_mut() {
            conv.viewport.jump_to_latest();
        }
        cx.notify();
    }

    /// Draw run `ix`. Called by the list, after `render` has returned.
    /// `window` is here for one thing the renderer cannot get any other way:
    /// the rem size in force for this subtree, which is what per-panel zoom
    /// overrides. The markdown renderer scales its headings off a base given in
    /// pixels, so without the live rem size an answer's headings are the one
    /// part of it that stays put while everything around them grows.
    fn run_element(
        &self,
        ix: usize,
        session: &Entity<ChatSession>,
        window: &Window,
        cx: &App,
    ) -> gpui::AnyElement {
        let Some(plan) = self
            .active_conversation()
            .and_then(|conv| conv.viewport.run(ix))
        else {
            return div().into_any_element();
        };
        let Some(chat) = self.active_chat(cx) else {
            return div().into_any_element();
        };

        let body = |targets: &[TranscriptItemId]| -> Vec<gpui::AnyElement> {
            targets
                .iter()
                .filter_map(|&target| {
                    viewport::item(chat, target).map(|item| {
                        transcript::item(session, item, target, window, cx).into_any_element()
                    })
                })
                .collect()
        };

        // The run layout already classified every run's cadence, so the space
        // above this one is decided by the pair it forms with the run before it
        // rather than by this run's opinion of itself.
        let lead = lead_gap(
            self.active_conversation()
                .and_then(|conv| conv.viewport.kind_before(ix)),
            plan.head_kind(),
        );
        // Transcript rows are wider than the composer, but keep the same
        // inside inset as its visible curve so the two surfaces retain one
        // spacing rhythm. Derive it from the exact composer radius so a theme
        // change cannot make them drift apart.
        let side_padding = cx.theme().radius * 2.;

        let Some(strip) = plan.strip.clone() else {
            return column(lead, side_padding, body(&plan.members)).into_any_element();
        };

        let anchor = plan.members[0];
        let this = self.handle.clone();
        let folded = session.clone();

        column(
            lead,
            side_padding,
            vec![
                div()
                    .v_flex()
                    .gap(COMPACT_GAP)
                    .w_full()
                    .min_w_0()
                    .child(transcript::activity_strip(
                        strip.group,
                        strip.summary,
                        plan.open,
                        move |_, _, cx: &mut App| {
                            folded.update(cx, |session, cx| {
                                session.toggle_activity(anchor);
                                cx.notify();
                            });
                            // The pane owns the run layout the list reads back,
                            // so it is the half that has to be told to draw
                            // again -- the session's own notify redraws the
                            // session, not the plan.
                            let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                        },
                        ("activity", anchor.index()).into(),
                        cx,
                    ))
                    .when(plan.open, |strip| {
                        // Clear the group header's icon column so expanded
                        // members read as its children. A leaf's own detail
                        // then adds the same inset again beneath that leaf's
                        // label, preserving the hierarchy at both levels.
                        //
                        // At the cadence index rows keep everywhere else, not a
                        // looser one: what a group opens into is the same kind
                        // of quiet row that sits a quarter-rem from its
                        // neighbours out in the transcript, and one list drawn
                        // at two rhythms depending on whether it is inside a
                        // group is the group deciding something that is not
                        // its to decide.
                        strip.child(
                            div()
                                .v_flex()
                                .gap(COMPACT_GAP)
                                .pl_6()
                                .children(body(&plan.members)),
                        )
                    })
                    .into_any_element(),
            ],
        )
        .into_any_element()
    }
}

impl Panel for ChatPane {
    fn panel_name(&self) -> &'static str {
        "AgentPane"
    }

    /// Not zoomable, because the dock's zoom is a button on a tab bar this
    /// panel no longer has.
    ///
    /// Little is lost: the dock zoom fills the frame *right of the rail*, and
    /// the conversation is already the whole of that whenever both docks are
    /// closed -- which is how the window opens. What it was actually for,
    /// putting the docks away for a moment, is what the docks' own toggles do,
    /// and the app-wide direction still has `Ctrl+Shift+K`.
    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    /// The panel's name to the dock, which no longer draws it.
    ///
    /// Kept because the trait needs an answer and because the dock uses it for
    /// drag payloads and menus; the title the user reads is the header's.
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(
            self.active_chat(cx)
                .and_then(|chat| chat.conversation_title())
                .unwrap_or_else(|| "Agent".to_string()),
        )
    }
}

/// What a session is doing, when that is something the rail should say.
///
/// Only states that want the user's eye. A session that is connected, idle and
/// already read carries **no** signal — that is what keeps a rail full of
/// healthy sessions a clean list of names rather than a wall of dots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionSignal {
    /// The adapter went away. `Ctrl+Shift+R` brings it back.
    Lost,
    /// A parked permission or question. The only signal that is about *the
    /// user*: nothing moves until they answer.
    AwaitingUser,
    /// A turn is in flight.
    Busy,
    /// A turn finished while this session was not being looked at.
    UnseenTurn,
}

impl SessionSignal {
    /// How badly each state wants the user's eye, lowest first.
    ///
    /// **The one place the order lives.** Two rows read it — a session row
    /// reducing its own four facts, and a project row reducing its sessions'
    /// signals — and an order written out twice is an order that will disagree
    /// with itself the first time someone edits one copy.
    fn rank(self) -> u8 {
        match self {
            // A dead adapter outranks a question nobody can answer any more.
            Self::Lost => 0,
            // A parked question outranks busy: only one of them moves on its
            // own.
            Self::AwaitingUser => 1,
            // What is happening now outranks what happened last turn.
            Self::Busy => 2,
            // The calmest of the four, and the only one about the past rather
            // than about now.
            Self::UnseenTurn => 3,
        }
    }

    /// Reduce a session's four independent facts to the one thing the rail
    /// draws.
    ///
    /// One mark, not one per condition: two marks side by side on a rail row
    /// are a code nobody learns. Which one survives is [`Self::rank`] — a
    /// judgement about which fact the user needs when several are true at once.
    ///
    /// Pure, and separate from the lookup, so the order is testable without a
    /// window: it is a rule about attention, and rules about attention are
    /// exactly what regresses silently.
    pub fn pick(link: Link, awaiting_user: bool, busy: bool, unseen: bool) -> Option<Self> {
        Self::most_urgent(
            [
                (link == Link::Lost).then_some(Self::Lost),
                awaiting_user.then_some(Self::AwaitingUser),
                busy.then_some(Self::Busy),
                unseen.then_some(Self::UnseenTurn),
            ]
            .into_iter()
            .flatten(),
        )
    }

    /// The signal a *group* of sessions carries — a project row's roll-up.
    ///
    /// Without it a collapsed project was silent about everything inside it:
    /// an agent could be waiting on an answer, or dead, with nothing on screen
    /// saying so until the user thought to expand that project. The same rank
    /// decides, so the mark on a project means exactly what the same mark means
    /// on the session it came from.
    pub fn most_urgent(signals: impl IntoIterator<Item = Self>) -> Option<Self> {
        signals.into_iter().min_by_key(|signal| signal.rank())
    }
}

/// What [`ChatPane::restart_active`] did, so the shell can say so.
pub enum Restart {
    /// The adapter is coming back up on the same conversation.
    Restarted,
    /// A turn is in flight; the press armed the guard instead of restarting.
    Armed,
    /// Nothing to restart -- no session, or one that never connected.
    Nothing,
}

/// The project-page menu's entries.
///
/// **Not a copy of the rail's project menu.** Two of that menu's entries are
/// missing here on purpose: *New session* is the primary button in the middle of
/// this very page, and *Open terminal* is a button at the end of the row the
/// menu hangs off. Repeating either would be the page offering the same thing
/// twice within an inch of itself.
#[derive(Clone, Copy)]
pub enum ProjectAction {
    /// Pin to the top of the rail, or take the pin off.
    TogglePin,
    /// Split it into a second checkout. Offered on repositories only.
    Worktree,
    CopyPath,
    RefreshGit,
    /// Drop it from the workspace. The shell still guards this behind a second
    /// press while anything is running in it.
    Remove,
}

/// What the pane asks the shell for. Kept tiny on purpose: the chat's job is
/// the conversation, and routing a file into the Workbench is the shell's.
pub enum ChatPaneEvent {
    OpenFile(PathBuf),
    /// A turn finished, so the agent has probably touched the working tree.
    ///
    /// Announced rather than acted on: the chat has no idea a file tree or a
    /// git status exists, and the two things that go stale here belong to two
    /// other panels. Which session it was does not matter — an agent writes to
    /// the root, and the panels are per root.
    WorkTreeTouched,
    /// The rail is hidden and the user asked for it back.
    ///
    /// Announced rather than acted on for the usual reason: the rail is the
    /// window's chrome, and a dock panel has no business reaching outside the
    /// dock to draw it.
    ShowRail,
    /// The Workbench is closed and the user asked for it.
    ///
    /// Announced rather than acted on for the same reason as the rail: the
    /// Workbench is a dock, the dock is the window's arrangement, and the panel
    /// sitting in the middle of it does not get to rearrange the window. It
    /// also does not know the three-state rule the keystroke follows — which
    /// mode to open on, and that a press while it is open and focused closes
    /// it — and two places deciding that would drift apart.
    ToggleWorkbench,
    /// The terminal dock is closed and the user asked for it.
    ///
    /// Announced rather than acted on for exactly the reasons above: the dock is
    /// the window's arrangement, and whether it opens, focuses or closes on this
    /// press is the same three-state rule `Ctrl+Shift+\`` follows — one place
    /// decides it or the two drift apart.
    ToggleTerminal,
    /// Restart the agent on the conversation showing.
    ///
    /// Announced rather than done here even though the pane owns the adapter:
    /// a restart mid-turn has to be confirmed, and the confirmation is a
    /// notification the shell raises. Doing half of it here would mean two
    /// places deciding when a turn may be thrown away.
    Restart,
    /// Close the session showing, and with it its agent.
    ///
    /// The pane can drop a conversation but not the workspace row that names
    /// it, and the mid-turn guard belongs with the same one that guards the
    /// rail's ✕.
    CloseSession,
    /// Something done to the project the pane is standing on with no session.
    ///
    /// One variant carrying an action rather than five of its own, because they
    /// are one sentence with a word swapped: do this to the *selected* project.
    /// The shell answers every one of them by reaching for the same root, and
    /// the page that offers them is only ever drawn for that root — it is what
    /// shows when the selected project has nothing running in it.
    Project(ProjectAction),
    /// Rename the conversation showing.
    ///
    /// Announced rather than done here because the rename is a dialog, and a
    /// dialog belongs to the window: the shell already owns the field, the
    /// trigger-less dialog it lives in and the reset-to-derived-title rule, and
    /// the rail's own *Rename…* goes to the same place.
    Rename,
    /// Delete the conversation showing — the directory on disk, and with it the
    /// session that is writing to it.
    ///
    /// The pane cannot do this alone and must not try. While the session is
    /// alive its mark says the transcript so far is already on disk, so the very
    /// next turn would write the file back holding only what came after: a
    /// delete that does not stay deleted. Ending the session is what settles
    /// that, and the session is a row in the workspace tree — the shell's.
    DeleteConversation(PathBuf),
    /// Start a session on the project the pane is standing in — the project
    /// page's *New session*, and every past conversation listed under it.
    ///
    /// `agent` names the agent to run it, which is the archive's own when a
    /// past conversation was picked and nobody's when the button was; `resume`
    /// is the archive to open it on.
    ///
    /// Announced rather than done here because a session is a row in the
    /// workspace tree: the pane holds conversations, the shell holds the tree,
    /// and a pane that minted its own sessions would be the second place
    /// deciding which project one belongs to.
    StartSession {
        agent: Option<SharedString>,
        resume: Option<PathBuf>,
    },
    /// A conversation could not be written to disk.
    ///
    /// Announced rather than shown here because the pane has no window to show
    /// it on, and because this is the one layer of the app's state the user
    /// cannot produce again: a workspace or a config that fails to save can be
    /// re-entered, a conversation that fails to save is gone. Every other write
    /// in the app says so when it fails; this one used to fail in silence.
    ArchiveFailed(String),
    /// A conversation the user asked to delete is still there, and why.
    ///
    /// Its own variant rather than a second use of the one above, because the
    /// two need opposite sentences: one says work was not kept, this one says
    /// work was not thrown away. Reporting a refusal as a failure to save would
    /// send the reader looking for the wrong thing entirely.
    ConversationNotDeleted(String),
}

impl EventEmitter<ChatPaneEvent> for ChatPane {}
impl EventEmitter<PanelEvent> for ChatPane {}

impl Focusable for ChatPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChatPane {
    /// The pane's own key context, so chat commands resolve while the
    /// conversation has focus and nowhere else. The body is wrapped rather
    /// than repeated: the pane has three shapes (picker, empty hint, live
    /// conversation) and all three answer to the same keys.
    ///
    /// **The focus handle is tracked here.** A panel inside a tab group is
    /// tracked by the group, which is why the Workbench and the terminal do not
    /// do this; the conversation is mounted as a bare panel, so nothing above
    /// it puts its handle in the dispatch tree, and without an entry there
    /// `contains_focused` answers "no" however deep inside the pane the caret
    /// actually is. Everything that asks which panel a command belongs to reads
    /// that answer, so the whole three-state panel keymap would quietly address
    /// the wrong panel. Clicking a blank part of the pane focusing the pane
    /// itself is the same behaviour the tab group had, for the same reason: an
    /// inner focusable takes the click first and stops it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = self.zoom;
        let body = self.body(window, cx);
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context("Chat")
            // Esc dismisses whichever list the composer has open.
            //
            // Handled here, on the action, rather than bound to the key: the
            // input already claims `escape` at a deeper point of the focus
            // stack, so a binding of ours would lose to it whatever context it
            // named. What the input does with an escape it has no use for is
            // let it keep travelling outward, and this is where it arrives.
            // With nothing open the same thing happens again, so anything
            // above the pane still gets its turn.
            .on_action(cx.listener(
                |pane: &mut Self, _: &gpui_component::input::Escape, _, cx| {
                    if pane.composer.read(cx).overlay_open() {
                        pane.composer
                            .update(cx, |composer, cx| composer.close_overlay(cx));
                    } else {
                        cx.propagate();
                    }
                },
            ))
            .child(zoom.scale(window, body))
    }
}

impl ChatPane {
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Cleared here and set only on the one path that mounts a composer, so
        // every early return below leaves it false without having to say so.
        self.composer_drawn = false;
        // A session choosing which conversation to resume has no transcript and
        // no composer yet: nothing is connected until the choice is made.
        if let Some(uid) = self.active.filter(|uid| self.is_choosing(*uid)) {
            return self.resume_picker(uid, cx).into_any_element();
        }
        // Opening is a wait, not an absence: the scan for past conversations is
        // running, or a restart has just dropped one adapter and is a line away
        // from spawning the next. The hint for "no session here" told the user
        // to start one they had already started.
        if self.active.is_some_and(|uid| self.is_opening(uid)) {
            return waiting_hint("Opening the session…".into(), cx).into_any_element();
        }
        let Some(session) = self.session() else {
            return self.project_home(cx);
        };
        // Copied out rather than borrowed, so recording the connect and drawing
        // the header below are not held up by a borrow of the conversation.
        let Some((link, status)) = self
            .active_chat(cx)
            .map(|chat| (chat.link, chat.activity_status()))
        else {
            return self.project_home(cx);
        };
        // Set the first time an adapter is actually up and never unset: it is
        // what tells a first connect from a reconnect.
        if link == Link::Connected
            && let Some(conv) = self.active_conversation_mut()
        {
            conv.was_live = true;
        }
        // A first connect draws nothing but the wait.
        //
        // A resumed conversation is adopted from its archive the moment it is
        // picked, so otherwise the whole of it is on screen -- transcript,
        // header, composer -- seconds before a word can be sent to it, with
        // nothing but a refused Send to say so. A *re*connect is the opposite
        // case, which is what the flag is for: on a restart the conversation is
        // already being read, and taking it away for the seconds a spawn costs
        // reads as data loss.
        //
        // The header stays. It names the conversation being opened, and a pane
        // that drops its own chrome while it waits reads as one that lost it.
        if waits_alone(
            link,
            self.active_conversation().is_some_and(|conv| conv.was_live),
        ) {
            let waiting = status.unwrap_or_else(|| "Connecting…".to_string());
            return div()
                .size_full()
                .v_flex()
                .child(self.header(cx))
                .child(waiting_hint(waiting.into(), cx))
                .into_any_element();
        }
        let Some(chat) = self.active_chat(cx) else {
            return self.project_home(cx);
        };

        // Asked of the conversation, on the composer's current contents, so
        // Send can refuse out loud instead of swallowing the press. Computed
        // here because this is the last point both are borrowed at once.
        let blocked = {
            let composer = self.composer.read(cx);
            chat.submit_blocker(&composer.text(cx), &composer.attachments)
        };
        // Mode first, then whatever config options the agent advertised
        // (model, effort, sub-agent). The list is the agent's, not ours.
        // `None` addresses the session mode, `Some(i)` the i-th config option --
        // the same key the composer's overlay uses, so a chip and its open list
        // cannot disagree about which selector they belong to.
        let specs: Vec<(Option<usize>, String, Option<String>)> = std::iter::once((
            None,
            "Mode".to_string(),
            chat.modes
                .iter()
                .find(|m| Some(&m.id) == chat.current_mode.as_ref())
                .map(|m| m.name.clone()),
        ))
        .filter(|_| !chat.modes.is_empty())
        .chain(chat.config_options.iter().enumerate().map(|(i, opt)| {
            let current = opt
                .current
                .as_ref()
                .and_then(|value| opt.choices.iter().find(|c| &c.value == value))
                .map(|c| c.name.clone());
            (Some(i), opt.name.clone(), current)
        }))
        .collect();
        // Measured last frame. Read once, and turned into one number, because
        // it is the line three separate things rest on -- the last row of the
        // transcript, the jump-to-the-latest pill, and the point a question
        // held at the top of the panel stops being held -- and two of them read
        // a frame apart is the pill floating off the conversation's floor.
        let measure = self.composer_h.clone();
        let measured = self.composer_h.get();
        let minimum = COMPOSER_MIN_H.to_pixels(window.rem_size());
        let overlay_h = if measured > minimum {
            measured
        } else {
            minimum
        };
        let floor = overlay_h + COMPOSER_REST.to_pixels(window.rem_size());
        // The two edges a newly asked question is held between: the list's own
        // top padding below, which is where it comes to rest, and the composer
        // above whatever is left of the panel.
        let room = viewport::TopRoom {
            head: LIST_HEAD.to_pixels(window.rem_size()),
            floor,
        };

        // History and live items are two collections, and a fold or a
        // permission answer has to reach the right one -- hence the typed id
        // rather than a render position.
        let Some(list_state) = self.reproject(room, cx) else {
            return self.project_home(cx);
        };
        // Asked after the layout, because holding a question at the top is what
        // decides both: the room under the last run, and whether being parked
        // above the tail is news worth a control to undo. It is not -- the
        // reader did not scroll anywhere, the transcript came to them, and the
        // answer they are waiting for is arriving in the space below.
        let holding = self
            .active_conversation()
            .is_some_and(|conv| conv.viewport.holding());
        let tail_room = self
            .active_conversation()
            .map_or(floor, |conv| conv.viewport.tail_room(floor));
        let this = cx.entity();
        let for_render = session.clone();
        let scrolled_up = away_from_tail(&list_state) && !holding;
        // The field draws no ring of its own once the card is its border, so
        // the card has to answer "does typing go here" -- with an app keymap
        // that reaches over the terminal and a rail that can take focus, an
        // input with no focused state is one the user has to test by typing.
        let typing_here = self
            .composer
            .read(cx)
            .state
            .focus_handle(cx)
            .contains_focused(window, cx);
        self.composer_drawn = true;

        div()
            .size_full()
            .v_flex()
            .child(self.header(cx))
            .children(self.find_bar(cx).map(|bar| bar.into_any_element()))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        // The list fills the well, so it clips exactly at the
                        // header's rule rather than at an inset below it -- a
                        // band of blank surface above a line of text sliced
                        // in half reads as a rendering fault, not as a margin.
                        // Its breathing room is *inside* the scroll instead:
                        // padding on the list is part of what scrolls, which is
                        // also how the transcript ends above the composer
                        // floating over it.
                        list(list_state, move |ix, window: &mut Window, cx: &mut App| {
                            this.read(cx).run_element(ix, &for_render, window, cx)
                        })
                        .size_full()
                        .pt(LIST_HEAD)
                        .pb(tail_room),
                    )
                    // Over the transcript rather than in a row of its own: a
                    // control that appears and disappears cannot own layout, or
                    // the whole conversation shifts by its height every time the
                    // reader scrolls up and back down.
                    .when(scrolled_up, |well| {
                        well.child(
                            div()
                                .absolute()
                                .bottom(floor)
                                .left_0()
                                .right_0()
                                .h_flex()
                                .justify_center()
                                .child(
                                    // The outline button's own fill is partly
                                    // transparent. Give the floating control an
                                    // opaque raised surface so transcript text
                                    // scrolling beneath cannot show through it.
                                    div()
                                        .rounded(px(9999.))
                                        .bg(cx.theme().popover.alpha(1.))
                                        .shadow_lg()
                                        .child(
                                            crate::controls::action("to-bottom")
                                                .outline()
                                                // Fully round, which is what a
                                                // radius past any plausible
                                                // half-height means here -- not
                                                // a measured size.
                                                .rounded(px(9999.))
                                                .icon(Icon::new(IconName::ChevronDown))
                                                .label("New activity")
                                                .tooltip("Jump to the latest activity")
                                                .on_click(cx.listener(
                                                    |pane: &mut Self, _, _, cx| {
                                                        pane.jump_to_latest(cx);
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    })
                    .child(self.overlay(&session, measure, typing_here, blocked, specs, cx)),
            )
            .into_any_element()
    }

    /// The composer and everything stacked on it, floating over the transcript.
    ///
    /// **A real overlay.** It takes no height out of the conversation, so the
    /// transcript never jumps when the field grows. Only the interactive cards
    /// are opaque; the full-width wrapper stays transparent around the shared
    /// reading column. The measured list padding still lets the final row rest
    /// above the card instead of becoming unreachable behind it.
    ///
    /// The popups belong here for the same reason, but *outside* the box that
    /// is measured: they are transient chrome that may cover the conversation
    /// and must not move it.
    fn overlay(
        &mut self,
        session: &Entity<ChatSession>,
        measure: std::rc::Rc<std::cell::Cell<gpui::Pixels>>,
        typing_here: bool,
        blocked: Option<onehand_core::chat::SubmitBlock>,
        specs: Vec<(Option<usize>, String, Option<String>)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let pinned = self.pinned(session, cx);
        let pane = cx.entity();

        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .v_flex()
            .gap_2()
            .w_full()
            // Reading the conversation is a way of saying the popup is done
            // with. It hangs off this whole block rather than off the list, so
            // a click on a chip, a row or the field -- every one of which is a
            // click *outside* the list -- still reaches the control it was
            // aimed at.
            .on_mouse_down_out(cx.listener(|pane: &mut Self, _, _, cx| {
                if pane.composer.read(cx).overlay_open() {
                    pane.composer
                        .update(cx, |composer, cx| composer.close_overlay(cx));
                }
            }))
            // The popup sits *above* the input, so a long candidate list grows
            // away from the text being typed rather than over it -- and it sits
            // outside the measured box below, which is the whole point.
            //
            // Measured, the transcript's bottom padding would grow by the
            // popup's height the moment one opened and shrink again when it
            // closed, so every `@` typed shoved the conversation up and every
            // completion dropped it back. The popup is transient chrome; it may
            // cover the transcript, but it must not move it.
            .children(
                self.composer
                    .update(cx, |composer, cx| composer.popup(session, cx))
                    .map(|popup| {
                        div()
                            .w_full()
                            .px_4()
                            .child(div().w_full().max_w(CONTENT_COLUMN).mx_auto().child(popup))
                    }),
            )
            .child(
                // What the transcript has to clear: the pinned cards and the
                // composer, and the transparent space under them.
                div()
                    .relative()
                    .w_full()
                    // Measures this whole box, padding included, which is why
                    // the padding sits on the child rather than here: an
                    // absolutely positioned `size_full` resolves against the
                    // padding box, so a padded parent would report itself short
                    // by exactly the margin the transcript most needs to clear.
                    .child(
                        gpui::canvas(
                            move |bounds, _, cx| {
                                let height = bounds.size.height;
                                if measure.replace(height) != height {
                                    // Prepaint has finished rendering the
                                    // entity, so defer the notification instead
                                    // of updating it re-entrantly from its own
                                    // element tree.
                                    cx.defer(move |cx| {
                                        pane.update(cx, |_: &mut Self, cx| cx.notify());
                                    });
                                }
                            },
                            |_, _: (), _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .px_4()
                            // Transparent spacing around the cards is what makes
                            // this read as an overlay rather than a footer. The
                            // transcript keeps painting through it; only the
                            // surfaces below cover what sits directly behind
                            // them.
                            .pb_4()
                            .children((!pinned.is_empty()).then(|| {
                                div()
                                    .v_flex()
                                    .gap_2()
                                    .w_full()
                                    .max_w(CONTENT_COLUMN)
                                    .mx_auto()
                                    // The same size the transcript is set at: a
                                    // permission card is pinned here while it
                                    // waits and drawn down there once answered,
                                    // and one card that changes size on being
                                    // answered reads as two different cards.
                                    .text_size(transcript::TEXT)
                                    .children(pinned)
                            }))
                            .child(div().w_full().max_w(CONTENT_COLUMN).mx_auto().child(
                                self.composer.update(cx, |composer, cx| {
                                    composer.card(session, blocked, &specs, typing_here, cx)
                                }),
                            )),
                    ),
            )
    }
}

/// The space between the run above and the run below it.
///
/// **One number per boundary, and it hangs off the lower run.** Owned by the
/// upper run instead -- which is where it used to live -- a block could only say
/// how much room it wanted *after* itself, so what it got above depended on
/// whatever happened to precede it. Two consequences, both visible: a prompt sat
/// 2.25rem below prose and 1.75rem below a folded strip while always giving
/// 1.5rem to the answer under it, so the two sides of one space differed by a
/// step and the turn read as sitting slightly low; and a folded strip pulled the
/// answer *after* it up to 0.25rem, gluing prose to an index row it has nothing
/// to do with.
fn lead_gap(previous: Option<RunKind>, this: RunKind) -> Rems {
    // The first run rests on the list's own top padding.
    let Some(previous) = previous else {
        return rems(0.);
    };
    match (previous, this) {
        // A turn boundary, taken from either side: above the prompt it opens
        // the turn, below it separates the question from its answer. At the gap
        // blocks within a turn take, the first row under a prompt reads as one
        // more line of the question.
        (RunKind::Prompt, _) | (_, RunKind::Prompt) => TURN_GAP,
        // Index entries close ranks with each other and with nothing else.
        (RunKind::Compact, RunKind::Compact) => COMPACT_GAP,
        _ => BLOCK_GAP,
    }
}

/// The frame one run of the transcript is drawn in: a centred reading column
/// that shrinks with its panel. Width lives here rather than around each item
/// because a run is what the virtual list draws; activity summaries drawn by
/// the pane and their steps must share the same two edges.
fn column(lead: Rems, side_padding: gpui::Pixels, content: Vec<gpui::AnyElement>) -> gpui::Div {
    div()
        .h_flex()
        .w_full()
        // The reading size is set here, on the frame every run is drawn in, so
        // one place decides it for prose, cards, wells and rows alike. Set per
        // block instead, the blocks that never asked would keep the app's own
        // base and the transcript would be two sizes.
        .text_size(transcript::TEXT)
        // The side margin rides on the run, not on the box that clips the
        // transcript: padding there would inset the clip too, cutting the text
        // short of the header's rule and leaving a band of blank surface above
        // whatever line the scroll happened to stop on.
        .px_4()
        .pt(lead)
        .child(
            div()
                .w_full()
                .min_w_0()
                .max_w(CONTENT_COLUMN)
                .mx_auto()
                // Equal to the composer's visible corner radius. Even though
                // the transcript and composer now share an outer cap, their
                // content still keeps the same inset rhythm.
                .px(side_padding)
                .children(content),
        )
}

/// `3m ago` / `2h ago` / `5d ago`, for the resume picker's subtitle.
pub fn rel_time(now: u64, then: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// Shown while no session is on screen.
///
/// Named by the project it would start in. "Pick a session in the rail" was a
/// wrong instruction in the state it appeared in most: a project with no
/// sessions has none to pick, which is what every freshly added project looks
/// like, so the window's centre was telling the user to do something the rail
/// gave them no way to do.
fn waiting_hint(what: SharedString, cx: &App) -> impl IntoElement + use<> {
    div()
        .size_full()
        .flex_1()
        .min_h_0()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(cx.theme().muted_foreground)
        .child(Spinner::new().small())
        .child(what)
}

/// One archived conversation, as a card that can be picked.
///
/// Shared by a session's resume picker and the project page, because they ask
/// the same question from two places: the caller supplies the subtitle, which
/// is the only part that differs, and hangs its own click on the result. Two
/// hand-written card styles is how one of them ends up not looking clickable.
fn conversation_card(
    id: impl Into<ElementId>,
    title: SharedString,
    subtitle: SharedString,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .v_flex()
        .gap_0p5()
        .w_full()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .cursor_pointer()
        // Hover is the fill alone, and the row's hairline stays what it was.
        // The tint these rows carried before was under a twentieth of a step
        // off the card in the light palette -- no feedback at all on a list
        // whose whole purpose is picking one row out of several -- but that was
        // the palette's fault, not the fill's, and the ramp answers it.
        .hover(|row| row.bg(cx.theme().list_hover))
        .child(div().truncate().child(title))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(subtitle),
        )
}

/// Whether the reader is parked above the conversation's tail.
///
/// **Not `is_scrolled_to_end`**, which is the obvious answer and the wrong one:
/// it needs the height of every run, and a list that measures rows lazily has
/// no reason to know most of them. On any transcript long enough for the
/// question to matter it returned "don't know", which read as "not scrolled
/// away" — so the way back to the latest appeared on short conversations and
/// went missing on exactly the long ones it exists for.
///
/// Follow-tail state answers it without measuring anything: the list stops
/// following the moment the reader scrolls up and starts again when they come
/// back down. The second half of the test is for the transcript that fits on
/// screen — a wheel event there stops following without moving anything, and a
/// conversation with no bottom to be away from must not offer a way back to it.
fn away_from_tail(list: &ListState) -> bool {
    !list.is_following_tail() && list.logical_scroll_top().item_ix < list.item_count()
}

/// Whether the pane shows nothing but the wait.
///
/// The rule in one place because it is a rule about *hiding a conversation*,
/// and the two cases it stands between want opposite things. Coming up for the
/// first time, a resumed transcript is adopted from its archive before a word
/// of it can be sent, so showing it is the pane claiming to be ready. Coming
/// back up -- a restart, an adapter respawned after it died -- the same
/// transcript is already being read, and taking it away for the seconds a spawn
/// costs reads as data loss.
fn waits_alone(link: Link, was_live: bool) -> bool {
    link == Link::Connecting && !was_live
}

/// Whether showing `next` means putting down whatever the pane was showing.
///
/// Pure so the rule has one statement rather than one per call site: the pane
/// stops showing a session from two directions -- picking a different one, and
/// landing on a project with no sessions at all -- and for a while only the
/// second of those put anything down.
///
/// `None` counts as a switch. It does not mean "the composer is empty"; it
/// means the composer is addressed to nobody, which is the state a closed
/// session leaves behind.
fn switching_away(current: Option<u64>, next: u64) -> bool {
    current != Some(next)
}

/// Whether a restart of `uid` still needs its confirming press.
///
/// A restart mid-turn throws away work the user is waiting on, so the first
/// press only arms it. What is armed is the *conversation*, not the pane: an
/// arming press made on one session says nothing about another, and treating it
/// as though it did let a second press throw away a turn on a session the user
/// had never been warned about.
fn restart_needs_arming(busy: bool, armed: Option<u64>, uid: u64) -> bool {
    busy && armed != Some(uid)
}

/// What the project page's own name opens.
///
/// The same set the rail offers on a project row, minus the two this page
/// already answers with a control of its own, and reached the same way the
/// conversation's menu is — so the header's leftmost thing is always "what you
/// are looking at, and what can be done to it", whichever of the two it is.
///
/// Every entry is announced, not done: a project is a row in the workspace tree,
/// and the pane holds conversations. The builder rather than the element, so the
/// row it hangs off stays the header's to draw.
fn project_menu(
    pinned: bool,
    is_repo: bool,
    pane: Entity<ChatPane>,
) -> impl Fn(
    gpui_component::menu::PopupMenu,
    &mut Window,
    &mut Context<gpui_component::menu::PopupMenu>,
) -> gpui_component::menu::PopupMenu
+ 'static {
    move |menu, _, cx| {
        let danger = crate::theme::status_ink(cx).danger;
        let act = |action: ProjectAction, pane: Entity<ChatPane>| {
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                pane.update(cx, |_: &mut ChatPane, cx| {
                    cx.emit(ChatPaneEvent::Project(action))
                });
            }
        };
        menu.item(
            // The label is the state readout as well as the action: with no pin
            // marker anywhere on this page, a project would otherwise only say
            // it is pinned by where it sits in a rail that may be hidden.
            PopupMenuItem::new(if pinned { "Unpin" } else { "Pin to top" })
                .icon(Icon::new(IconName::Star))
                .on_click(act(ProjectAction::TogglePin, pane.clone())),
        )
        // Only where there is a repository to split. On a plain folder this
        // could do nothing but report that git said no, and an entry whose whole
        // job is to fail is one the eye has to learn to skip.
        .when(is_repo, |menu| {
            menu.item(
                PopupMenuItem::new("New worktree…")
                    .icon(Icon::new(crate::icons::Icon::GitBranch))
                    .on_click(act(ProjectAction::Worktree, pane.clone())),
            )
        })
        .item(
            PopupMenuItem::new("Copy project path")
                .icon(Icon::new(IconName::Copy))
                .on_click(act(ProjectAction::CopyPath, pane.clone())),
        )
        .item(
            PopupMenuItem::new("Refresh Git status")
                .icon(Icon::new(IconName::Redo))
                .on_click(act(ProjectAction::RefreshGit, pane.clone())),
        )
        .separator()
        .item(
            PopupMenuItem::element(move |_, _| {
                div().text_color(danger).child("Remove from workspace")
            })
            .icon(Icon::new(IconName::Delete).text_color(danger))
            .on_click(act(ProjectAction::Remove, pane.clone())),
        )
    }
}

/// One of the header's right-hand controls.
///
/// **Bigger and quieter than the library's default.** Two changes that pull in
/// opposite directions and are one decision: at the smallest size these were
/// three glyphs the pointer had to be aimed at, and at full-strength ink four
/// icons in a row out-shouted the conversation's own name two inches to their
/// left. A step up in size makes them easy to hit; a step down in tone puts them
/// behind the name, which is what the header is for. What brings the ink back is
/// hovering one — the fill arrives and says which is about to be pressed.
///
/// Built in one place because the alternative is four call sites that each have
/// to remember two things, and the one that forgets is the one that looks wrong.
fn header_control(id: &'static str, icon: IconName, cx: &App) -> gpui_component::button::Button {
    crate::controls::action(id)
        .ghost()
        .small()
        .icon(Icon::new(icon))
        .text_color(cx.theme().muted_foreground)
}

/// How wide the header's status badge may get.
///
/// In rems, like every other size here, so it scales with the panel's own zoom.
/// The badge sits between the conversation's name and the row's controls and is
/// the least important of the three: what it says is either already visible in
/// the transcript or is a state the rail is marking too, so it truncates rather
/// than pushing either of its neighbours around.
const BADGE_MAX_W: f32 = 14.;

/// What the session is doing, beside the name of the conversation doing it.
///
/// **A pill, not a line of grey text.** It used to be exactly that -- the same
/// muted ink as the header around it, at the same weight, so "Connecting to
/// Claude Code…" read as part of the title rather than as a state that would go
/// away. A filled shape with an edge is what separates the two: the name is ink
/// on the surface, this is a thing sitting on it.
///
/// **The mark is the rail's own** ([`crate::rail::signal_mark`]), so one
/// condition keeps one shape everywhere it appears -- a spinner for a turn in
/// flight, a triangle for a lost adapter, a dot for a parked question -- and it
/// brings its own tooltip with it. The colour lives in the mark and the words
/// stay muted: tinting the whole badge would make a routine "Working…" as loud
/// as a dead agent.
fn status_badge(
    signal: Option<SessionSignal>,
    text: SharedString,
    cx: &App,
) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .h_flex()
        .items_center()
        .gap_1p5()
        .max_w(rems(BADGE_MAX_W))
        .px_2()
        .py_0p5()
        .rounded_full()
        .bg(cx.theme().muted)
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .children(signal.map(|signal| crate::rail::signal_mark(signal, cx)))
        .child(div().min_w_0().truncate().child(text))
}

/// Whether deleting `dir` still needs its confirming press.
///
/// Unlike a restart there is no condition that makes this safe to do in one
/// press: a restart is only guarded while a turn is in flight, because that is
/// the only time it throws anything away. A delete always does, and it is the
/// one thing here that doing again does not undo.
///
/// What is armed is the conversation, not the page. Arming on one row and
/// pressing the row below it must ask again -- otherwise the second press
/// deletes something the first one never named.
fn delete_needs_arming(armed: Option<&std::path::Path>, dir: &std::path::Path) -> bool {
    armed != Some(dir)
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_GAP, COMPACT_GAP, RunKind, SessionSignal, TURN_GAP, TranscriptItemId, away_from_tail,
        delete_needs_arming, lead_gap, restart_needs_arming, switching_away, viewport, waits_alone,
    };
    use onehand_core::chat::Link;
    use std::path::Path;

    /// The two sides of a prompt are one space, so they are one number —
    /// whatever sits above the prompt and whatever follows it.
    ///
    /// This is the property the old layout could not hold. Spacing hung off the
    /// *upper* run, so the gap above a prompt was that run's own bottom padding
    /// plus the prompt's, and it therefore changed with what happened to
    /// precede it while the gap below never did.
    #[test]
    fn a_prompt_is_spaced_the_same_above_and_below() {
        for above in [RunKind::Block, RunKind::Compact, RunKind::Prompt] {
            assert_eq!(
                lead_gap(Some(above), RunKind::Prompt),
                TURN_GAP,
                "{above:?} above a prompt"
            );
        }
        for below in [RunKind::Block, RunKind::Compact, RunKind::Prompt] {
            assert_eq!(
                lead_gap(Some(RunKind::Prompt), below),
                TURN_GAP,
                "{below:?} below a prompt"
            );
        }
    }

    /// Index rows close ranks with each other and with nothing else. The old
    /// layout gave the *answer* after a folded strip the strip's own compact
    /// cadence, gluing prose to a row it has nothing to do with.
    #[test]
    fn only_two_index_rows_close_ranks() {
        assert_eq!(
            lead_gap(Some(RunKind::Compact), RunKind::Compact),
            COMPACT_GAP
        );
        assert_eq!(
            lead_gap(Some(RunKind::Compact), RunKind::Block),
            BLOCK_GAP,
            "an answer under a folded strip is a block boundary"
        );
        assert_eq!(lead_gap(Some(RunKind::Block), RunKind::Compact), BLOCK_GAP);
        assert_eq!(lead_gap(Some(RunKind::Block), RunKind::Block), BLOCK_GAP);
    }

    /// Opening a group must not move the row that was clicked.
    ///
    /// The gap above a run is a property of the boundary, and opening a group
    /// changes nothing about the boundary above its own header — only about
    /// what hangs below it. Decided from the run as a whole, that gap tripled
    /// the moment the group opened: the header slid down under the pointer
    /// that had just clicked it, and every row above appeared to shift.
    #[test]
    fn opening_a_group_leaves_the_space_above_it_alone() {
        let strip = |open: bool| viewport::RunPlan {
            members: vec![TranscriptItemId::Live(0)],
            strip: Some(viewport::ActivityPlan {
                group: onehand_core::chat::ActivityGroup::Explored,
                summary: "Inspected 3 files".to_string(),
            }),
            open,
            // What the layout classifies an opened group as: a block's worth
            // of reading, which is what the run *after* it has to answer to.
            kind: if open {
                RunKind::Block
            } else {
                RunKind::Compact
            },
        };

        assert_eq!(
            lead_gap(Some(RunKind::Compact), strip(true).head_kind()),
            lead_gap(Some(RunKind::Compact), strip(false).head_kind()),
            "the space over a group's own header changed when it opened"
        );
        assert_eq!(
            lead_gap(Some(RunKind::Compact), strip(true).head_kind()),
            COMPACT_GAP
        );
        // What *does* change is the space under it: an opened group is a block,
        // and the index row after it no longer closes ranks with a header it
        // can no longer see the bottom of.
        assert_eq!(strip(true).tail_kind(), RunKind::Block);
        assert_eq!(
            lead_gap(Some(strip(true).tail_kind()), RunKind::Compact),
            BLOCK_GAP
        );
    }

    /// The way back to the latest shows when the reader is parked above the
    /// tail, and only then.
    ///
    /// Both halves have failed. Asked of the list's measured height it was
    /// backwards on any long transcript — the rows above the viewport have
    /// never been measured, the answer came back "don't know", and the control
    /// stayed hidden on exactly the conversations that need it. And a
    /// transcript that fits on screen has no bottom to be away from, so a wheel
    /// event that moves nothing must not summon a way back to where the reader
    /// already is.
    #[test]
    fn the_way_back_appears_only_when_there_is_somewhere_to_go_back_to() {
        use gpui::{ListAlignment, ListOffset, ListState, px};

        let list = ListState::new(8, ListAlignment::Bottom, px(512.));
        list.set_follow_mode(gpui::FollowMode::Tail);
        assert!(
            !away_from_tail(&list),
            "a list at its tail is not away from it"
        );

        // Parked above the end: `scroll_to` stops the list following.
        list.scroll_to(ListOffset {
            item_ix: 3,
            offset_in_item: px(0.),
        });
        assert!(away_from_tail(&list));

        // Back at the end.
        list.scroll_to_end();
        list.set_follow_mode(gpui::FollowMode::Tail);
        assert!(!away_from_tail(&list));

        // Nothing to scroll: following stopped by hand, but the offset never
        // left the tail.
        let short = ListState::new(2, ListAlignment::Bottom, px(512.));
        short.set_follow_mode(gpui::FollowMode::Tail);
        short.pause_following_tail();
        assert!(
            !away_from_tail(&short),
            "a transcript that fits on screen has no bottom to be away from"
        );
    }

    /// A conversation is hidden while it comes up for the first time, and never
    /// again after that.
    ///
    /// The second half is the one worth a test: a restart drops the adapter and
    /// spawns another, so the link goes back to connecting on a transcript the
    /// user is in the middle of reading. Blanking it there looks exactly like
    /// the restart having thrown the conversation away.
    #[test]
    fn only_a_conversation_that_was_never_live_is_hidden_while_it_connects() {
        assert!(waits_alone(Link::Connecting, false));
        assert!(
            !waits_alone(Link::Connecting, true),
            "a restart must not blank a transcript being read"
        );
        for link in [Link::Connected, Link::Lost] {
            assert!(!waits_alone(link, false));
            assert!(!waits_alone(link, true));
        }
    }

    /// The first run rests on the list's own top padding; a gap there would be
    /// space between the header's rule and nothing.
    #[test]
    fn the_top_of_the_transcript_leads_with_nothing() {
        for kind in [RunKind::Block, RunKind::Compact, RunKind::Prompt] {
            assert_eq!(lead_gap(None, kind), gpui::rems(0.));
        }
    }

    /// Re-selecting the session already on screen is not a switch. It happens
    /// on every rail click and on every window activation, so treating it as
    /// one would throw the find bar away while the user was typing in it.
    #[test]
    fn reselecting_the_shown_session_changes_nothing() {
        assert!(!switching_away(Some(7), 7));
    }

    /// Both of the ways the pane stops showing a conversation count.
    ///
    /// The second one is the case that was missed: after the pane has been
    /// cleared, the composer still holds what was typed for the session that
    /// was showing, and opening any session at all has to take it away first.
    #[test]
    fn every_other_move_is_a_switch() {
        assert!(switching_away(Some(7), 8), "one session to another");
        assert!(switching_away(None, 8), "from nothing showing to a session");
    }

    /// A turn in flight is what makes a restart worth confirming.
    #[test]
    fn an_idle_session_restarts_on_the_first_press() {
        assert!(!restart_needs_arming(false, None, 1));
        assert!(
            !restart_needs_arming(false, Some(1), 1),
            "a stale arming press on an idle session is not a reason to stop"
        );
    }

    #[test]
    fn a_busy_session_arms_then_confirms() {
        assert!(restart_needs_arming(true, None, 1), "first press arms");
        assert!(
            !restart_needs_arming(true, Some(1), 1),
            "the second press on the same session goes through"
        );
    }

    /// The whole reason the arming is keyed by session. Arm a restart on one
    /// busy conversation, switch to another that is also busy, and that
    /// session's first press must still be its own warning -- not the
    /// confirmation of a press aimed somewhere else.
    #[test]
    fn arming_one_session_never_confirms_another() {
        assert!(restart_needs_arming(true, Some(1), 2));
    }

    /// Deleting is guarded unconditionally, and the guard names the row.
    ///
    /// There is no state that makes a delete safe to do in one press, the way
    /// an idle session makes a restart safe: everything else this app offers can
    /// be done again, and a deleted conversation cannot. And arming one row must
    /// never confirm the row below it — the second press would then delete
    /// something the first one never named.
    #[test]
    fn deleting_always_asks_first_and_asks_about_one_row() {
        let (one, two) = (Path::new("/store/a"), Path::new("/store/b"));
        assert!(delete_needs_arming(None, one), "first press arms");
        assert!(
            !delete_needs_arming(Some(one), one),
            "the second press on the same conversation goes through"
        );
        assert!(
            delete_needs_arming(Some(one), two),
            "a press aimed at one conversation says nothing about another"
        );
    }

    /// A healthy, idle, already-read session draws **nothing**. This is the
    /// case that makes the other four legible: a rail where every row has a dot
    /// is a rail where no dot means anything.
    #[test]
    fn a_calm_session_carries_no_signal() {
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, false, false),
            None
        );
    }

    /// Connecting is not failing. Reading `tx.is_none()` would conflate them
    /// and paint a danger dot for the second or two every session spends
    /// coming up.
    #[test]
    fn coming_up_is_not_a_signal() {
        assert_eq!(
            SessionSignal::pick(Link::Connecting, false, false, false),
            None
        );
    }

    #[test]
    fn each_state_shows_when_it_is_the_only_one() {
        use SessionSignal::*;
        assert_eq!(
            SessionSignal::pick(Link::Lost, false, false, false),
            Some(Lost)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, true, false, false),
            Some(AwaitingUser)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, true, false),
            Some(Busy)
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, false, true),
            Some(UnseenTurn)
        );
    }

    /// The whole point of the reduction. Each of these is a real pairing:
    /// an adapter that died with a question still parked, a turn that finished
    /// unseen on a session that then lost its adapter, a busy session the user
    /// has not looked at since its last turn.
    #[test]
    fn the_more_urgent_state_wins() {
        use SessionSignal::*;
        assert_eq!(
            SessionSignal::pick(Link::Lost, true, false, true),
            Some(Lost),
            "a dead adapter outranks a question nobody can answer any more"
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, true, true, true),
            Some(AwaitingUser),
            "a parked question outranks busy: only one of them moves on its own"
        );
        assert_eq!(
            SessionSignal::pick(Link::Connected, false, true, true),
            Some(Busy),
            "what is happening now outranks what happened last turn"
        );
    }

    /// A project's mark is the most urgent of its sessions', on the same
    /// ordering a single session uses — otherwise the same shape would mean two
    /// different things one row apart.
    #[test]
    fn a_project_rolls_up_to_its_most_urgent_session() {
        use SessionSignal::*;
        assert_eq!(SessionSignal::most_urgent([]), None);
        assert_eq!(
            SessionSignal::most_urgent([UnseenTurn, Lost, Busy]),
            Some(Lost)
        );
        assert_eq!(
            SessionSignal::most_urgent([UnseenTurn, Busy, AwaitingUser]),
            Some(AwaitingUser)
        );
        assert_eq!(
            SessionSignal::most_urgent([UnseenTurn, Busy]),
            Some(Busy),
            "a project with a turn running says so over a stale badge"
        );
    }

    /// A session with several facts true at once and a project holding those
    /// same facts one per session must land on the same mark. They are the same
    /// question asked at two altitudes, and `rank` is the single answer.
    #[test]
    fn one_session_and_one_project_agree() {
        use SessionSignal::*;
        for (link, awaiting, busy, unseen) in [
            (Link::Lost, true, false, true),
            (Link::Connected, true, true, true),
            (Link::Connected, false, true, true),
            (Link::Connected, false, false, true),
        ] {
            let parts = [
                (link == Link::Lost).then_some(Lost),
                awaiting.then_some(AwaitingUser),
                busy.then_some(Busy),
                unseen.then_some(UnseenTurn),
            ];
            assert_eq!(
                SessionSignal::pick(link, awaiting, busy, unseen),
                SessionSignal::most_urgent(parts.into_iter().flatten()),
            );
        }
    }
}
