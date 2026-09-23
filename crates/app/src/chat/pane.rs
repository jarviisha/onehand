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
use super::transcript::{self, radius_tag};
use super::viewport::{self, FindState, RunKind};
use gpui::prelude::FluentBuilder as _;
use gpui::{Animation, AnimationExt as _};
use gpui::{
    App, AppContext, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ListState, ParentElement, Rems, Render, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, list, px, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt, WindowExt as _};
use onehand_core::chat::{
    Away, Chat, ChatItem, ConvMeta, Link, Presence, Telling, TranscriptItemId,
};
use onehand_core::config::AgentSpec;
use onehand_core::remote::types::Button;
use onehand_core::remote::{Press, press};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};

/// The rest the transcript comes to above the composer.
///
/// **Inside the scroll, not around the card.** The overlay itself must stay
/// transparent outside its surfaces so the transcript can remain visible
/// around the floating composer. Held here, this is still real scrollable
/// space: the last row can rest clear of the card without an opaque footer.
///
/// **Wider than any gap inside the conversation, because it is not one.** Every
/// other space in the transcript separates two things of the same kind — two
/// blocks, two turns — and is drawn from one ladder for exactly that reason.
/// This one is where the column *ends*: below it is a surface of a different
/// sort, floating, with its own edge and its own fill. A boundary between two
/// kinds of thing that measures the same as a boundary inside one of them reads
/// as the composer being the next paragraph.
///
/// Half again the space between two turns, which is the widest step the
/// conversation itself uses. Written against that step rather than as its own
/// number: it was set to match it once, the turn gap moved, and this quietly
/// stopped being what its own comment said it was.
const COMPOSER_REST: Rems = rems(transcript::TURN_GAP.0 * 1.5);
/// Safe first-frame clearance before the overlay has reported its real height.
/// The resting composer is about this tall; using zero until prepaint is what
/// lets the initial transcript tail land behind it.
const COMPOSER_MIN_H: Rems = rems(6.5);
/// The air between the column and the edge of the panel.
///
/// The narrow figure is for a panel too short to spare the wide one: below that
/// width the margin is taking room from the line itself rather than framing it.
const SIDE_MARGIN: Rems = rems(1.25);
const SIDE_MARGIN_NARROW: Rems = rems(0.75);
/// Where a panel stops being wide enough to hold the column off its edges.
const NARROW_PANEL: Rems = rems(30.);
/// The disc offering the way back to the end of the conversation.
///
/// A step above the controls in the card below it, because it is the one thing
/// on screen floating over the conversation with nothing around it — and a step
/// under the card's own height, because it is not part of that card. Square, so
/// the full radius makes it a circle: it holds one arrow and nothing else.
const JUMP_PILL_H: Rems = rems(1.625);
/// The conversation header, which is the one row in the panel that never
/// scrolls and so the edge every other measurement here is taken from.
const HEADER_H: Rems = rems(2.75);
/// The narrower cap the composer and the surfaces that belong to it take.
///
/// **A message being written is not a message being read.** The transcript's
/// column is set by how far a line of prose can run before the eye loses its
/// place returning to the next one; the composer holds a few lines at most, and
/// at the reading column its controls -- which sit at the two ends of one row --
/// ended up a hand's width apart with nothing between them. Narrower, the row
/// reads as one control strip and the card reads as a thing resting on the
/// conversation rather than as its last paragraph.
///
/// The popup above it takes this too, since it opens over the card and is the
/// same object. **So does anything pinned above it** — a parked permission, a
/// parked question, an adapter still connecting, a queued prompt: while a card
/// is pinned it rests directly on the composer, shares its surface and its
/// radius, and is read as one object with it, so a card an inch wider than the
/// box under it reads as two panels that failed to line up.
///
/// **Width follows where a card is, not what it is.** Answered, that same
/// permission is drawn in the transcript and takes the reading column like
/// every block around it. The rule it replaced said the two widths must match
/// so a card did not appear to change on being answered — but a transcript row
/// is inset inside the reading column while a pinned card never was, so the two
/// were already different objects and holding one width only made the pinned
/// one wrong.
const COMPOSER_COLUMN: Rems = rems(44.);
/// The two measurements the composer's overlay is sized against, both read off
/// the same frame and handed down together.
///
/// One value rather than two arguments, the way the transcript's own top room
/// already is: they are taken in the same place from the same layout, and a
/// caller that passed a fresh popup room beside a stale panel height would be
/// sizing one half of the overlay against a frame the other half never saw.
#[derive(Clone, Copy)]
struct OverlayRoom {
    /// How far a popup may open before it reaches the top of the panel.
    popup: gpui::Pixels,
    /// The panel's own height, for a card bounding itself against the space it
    /// has. `None` before the list has measured itself once.
    well: Option<gpui::Pixels>,
}

/// The space between two ordinary blocks of one turn, and the step every other
/// gap in the conversation is written against.
///
/// Read from the transcript's own scale: it is the outermost step of the same
/// ladder the blocks inside a turn are spaced on, and kept here as a separate
/// number it was free to stop being a ladder at all.
const BLOCK_GAP: Rems = transcript::BLOCK_GAP;
/// The space between two collapsed history rows, which are an index and are
/// read as one.
const COMPACT_GAP: Rems = transcript::TIGHT_GAP;
/// How far above the clip the transcript dissolves into the surface under it.
///
/// **A gradient and not a second edge.** The clip at the composer's middle is
/// what stops the conversation being drawn, and on its own it is a line: text
/// at full strength for one row and gone the next, which reads as a rendering
/// fault everywhere the composer's own card is not directly behind it — the
/// strips at either side of the card, and the gap above it. Faded into it over
/// this distance there is no line to see at all, and the conversation reads as
/// running out under the box rather than as being cut off by it.
///
/// Set at a few lines of prose, which is what makes it a fade rather than a
/// shadow: over a shorter run the eye still finds the edge, it is just a
/// blurred one.
const SMOKE: Rems = rems(7.);
/// The transcript's own head start, inside the scroll rather than around it.
///
/// Named because it is read twice: it is the padding the list draws with, and
/// it is where the top of a question held at the top of the panel comes to rest
/// — so the rule that decides when to stop holding one has to be measured from
/// the same number the row is actually drawn at.
const LIST_HEAD: Rems = rems(1.5);

/// How much of a finished answer rides along with the turn-ended announcement.
///
/// Sized for the surface it lands on rather than for the answer: a notification
/// is read at a glance on a phone, so this is a few sentences — enough for the
/// paragraph an answer closes with, and short of the point where the reader is
/// scrolling a transcript in a chat client. Whatever the channel itself will not
/// carry is clipped again on the way out, so this is the smaller of two bounds
/// and the one chosen for how it reads.
const ANSWER_TAIL_MAX: usize = 700;

/// How much of a taken-back prompt is quoted back at whoever wrote it.
///
/// Enough to recognise it by, not enough to make a stop confirmation into a wall
/// of text — it is there so the words are not simply gone, and the words
/// themselves are in the sender's own chat history a few messages up.
const QUOTED_PROMPT_MAX: usize = 120;

/// What a press on a card that is no longer open is told.
///
/// One sentence for every way it can happen — answered in the window, answered
/// by an earlier press on the same message, or a form this build can no longer
/// read — because they are the same fact to whoever pressed it: there is nothing
/// there to answer. Telling them which of the three would be telling them about
/// the app's own bookkeeping.
const SETTLED: &str = "That's already been answered.";

/// How many past conversations the project page lists.
///
/// The page is an entrance, not an archive browser: the newest handful is what
/// "where was I" needs, and a project worked in for months would otherwise draw
/// a column of hundreds for the sake of the two or three anybody came for. What
/// the cap cut off is said on screen rather than silently dropped. The list
/// scrolls as well, because this many rows already outgrows a short window.
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

/// The conversations already had in the project on screen, for the header's
/// *Open a past conversation* menu.
///
/// **Held rather than read when the menu opens.** Building a menu happens inside
/// a render, and a render cannot wait on a directory of files; a menu that came
/// up empty and filled itself in afterwards would be one the user had already
/// closed and drawn their conclusion from.
struct Archives {
    /// The project these belong to. An answer that lands after the pane has
    /// moved on names a project nobody is looking at, and adopting it would
    /// offer one project's conversations under another's name.
    root: PathBuf,
    /// `None` while the read is out — which is a different thing from a project
    /// that has never been prompted, and the menu says the two differently.
    found: Option<Vec<ConvMeta>>,
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
    /// A handle to this pane, for the callbacks the list builds outside the
    /// `render` that owns `Context<Self>`.
    handle: gpui::WeakEntity<Self>,
    /// The project the pane offers to start something in, when there is one.
    ///
    /// Only ever read while no session is showing.
    empty: Option<EmptyProject>,
    /// The past conversations of the project that *is* showing, for the
    /// header's menu. Separate from `empty` above, which is the same listing for
    /// the opposite state — that one is the body of the page shown when a
    /// project has nothing running, this one is offered while something is.
    archives: Option<Archives>,
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
    /// The active project's branch and change count, as one line.
    ///
    /// Pushed by the shell like the two flags above, and for the same reason:
    /// the sweep is the window's and this panel has no handle on it. The
    /// sentence is `GitStatus::label`, core's own, because the rail prints the
    /// same fact a few inches away and two spellings of one line is the kind of
    /// difference a reader assumes means something.
    ///
    /// `None` on a project that is not a repository, which is not the same as a
    /// repository with nothing changed -- that one has a branch to name.
    git: Option<SharedString>,
    /// How tall the composer overlay measured, last time it was drawn.
    ///
    /// The composer floats over the transcript, so the transcript has to end
    /// above it or its last line is permanently behind the box the user types
    /// in — and how much room that takes is not knowable in advance: the field
    /// grows with what is typed and the attachment tray appears and goes. So
    /// the overlay is measured where it is drawn and the list is padded by
    /// what it measured.
    ///
    /// **The pinned cards are deliberately not in it**, nor is the completion
    /// popup. Both are surfaces that come and go over the conversation, and
    /// measuring either means the transcript shifts under the reader's eye
    /// every time one appears. The composer is measured because it is there
    /// the whole time.
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
    /// When the turn on screen started, and the ticker keeping its clock true.
    ///
    /// **Stamped here because the model does not carry it.** A turn's start is
    /// `pub(crate)` in core, and the status line is not a good enough reason to
    /// widen it -- so the pane notices `busy` going up and reads its own clock.
    /// What that costs is honest and small: a session switched away from and
    /// back, or an app restarted mid-turn, starts the count again. It is a
    /// liveness reading, not a measurement, and the measured figure a turn
    /// leaves behind is the answer's own footer.
    ///
    /// The task is the other half. A clock that only redrew when something else
    /// did would sit at `0s` through a minute of silence, which is the one
    /// stretch it exists for; this wakes once a second while a turn is live and
    /// does nothing at all when one is not.
    turn_began: Option<std::time::Instant>,
    ticker: Option<gpui::Task<()>>,
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

            // Queue and Stop are distinct gestures while a turn is live. The
            // pane still validates the live state at activation time so a
            // delayed click cannot cancel the following turn.
            cx.subscribe_in(
                &composer,
                window,
                |pane: &mut Self, _, event: &ComposerEvent, window, cx| match event {
                    ComposerEvent::SendPressed => pane.submit(window, cx),
                    ComposerEvent::StopPressed => {
                        if pane.busy(cx) {
                            pane.stop(cx);
                        }
                    }
                    // Straight on to the shell, which is the half that knows
                    // the Workbench is a dock and whether it is open. The path
                    // is already absolute -- everything staged here arrives
                    // from the picker, the clipboard or a drop.
                    ComposerEvent::OpenFile(path) => cx.emit(ChatPaneEvent::OpenFile(path.clone())),
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
                window: window.window_handle(),
                handle: cx.entity().downgrade(),
                empty: None,
                archives: None,
                pending_resume: None,
                rail_hidden: false,
                terminal_live: false,
                git: None,
                composer_h: Default::default(),
                composer_drawn: false,
                turn_began: None,
                ticker: None,
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
        if switching_away(self.active, uid) {
            self.leave_shown_session(window, cx);
            self.restore_draft(uid, window, cx);
        }
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
        self.scan_project_history(cx);
        // Going to no session at all is still leaving the one that was showing,
        // and the draft has to be put down here too: a prompt left in the box
        // while passing through an empty project would otherwise be sitting
        // there, addressed to nobody, when the next session opens.
        self.leave_shown_session(window, cx);
        self.active = None;
        cx.notify();
    }

    /// Point the header's menu at `root`, reading its conversations if it is not
    /// already the one being held.
    fn follow_archives(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.archives.as_ref().is_some_and(|held| held.root == root) {
            return;
        }
        self.archives = Some(Archives {
            root: root.to_path_buf(),
            found: None,
        });
        self.scan_archives(cx);
    }

    /// Read the held project's conversations again.
    ///
    /// Called at the two moments the listing on disk actually changes under a
    /// running window: a turn ending, which is when a conversation is written —
    /// so a session's first turn is when it appears here at all — and a session
    /// closing, which is the moment somebody is most likely to want it back.
    /// Neither re-reads the *directory* on the UI thread; both go the same way
    /// the first read did.
    fn refresh_archives(&mut self, cx: &mut Context<Self>) {
        if self.archives.is_some() {
            self.scan_archives(cx);
        }
    }

    /// The read itself. Leaves whatever is held in place until the answer lands,
    /// so a refresh does not blank a menu that already had something in it.
    fn scan_archives(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.archives.as_ref().map(|held| held.root.clone()) else {
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
                // Only for the project it was asked about: reading a directory
                // of archives takes long enough that the user can have moved to
                // another project twice over, and one project's conversations
                // are not an answer about another's.
                let Some(held) = pane.archives.as_mut().filter(|held| held.root == path) else {
                    return;
                };
                held.found = Some(past);
                cx.notify();
            });
        })
        .detach();
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

    /// Ask before deleting a conversation, and delete only on the answer.
    ///
    /// A modal rather than a control that arms on the first press and acts on
    /// the second. Arming reads as a control that did nothing: the press lands,
    /// the word changes, and a user who has looked away comes back to a row
    /// that is one accidental press from gone with no warning left on screen.
    /// This one names the conversation it is about, cannot be missed, and has
    /// to be answered before anything else in the window can be -- which is the
    /// weight the only irreversible thing this app does should carry.
    ///
    /// **The name is passed in rather than looked up.** The archive list is the
    /// page's, and by the time the answer comes back the page may have been
    /// replaced by another project's; the sentence the user is reading has to
    /// be about the row they pressed.
    fn confirm_delete(
        &mut self,
        dir: PathBuf,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            // Cloned per build: a dialog's builder runs again on every frame it
            // is on screen, so nothing captured here can be consumed by one.
            let (pane, dir, name) = (pane.clone(), dir.clone(), name.clone());
            alert
                // The library's own title and description survive here, unlike
                // on a dialog opened from a trigger: this builder is what the
                // window keeps, so both are rebuilt with the rest of it.
                .title("Delete this conversation?")
                .description(format!(
                    "“{name}” will be removed from disk, with every message and \
                     image in it. This cannot be undone."
                ))
                // Ours rather than the default pair, for the reason every button
                // in this app is ours: the library draws its own with the arrow
                // cursor, and the one dialog that asks before destroying
                // something is the last place to say "this does nothing" with
                // the pointer. Keep is first and plain, Delete last and in the
                // danger tint.
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(
                                crate::controls::action("keep-conversation")
                                    .ghost()
                                    .label("Keep"),
                            ),
                        )
                        .child(
                            crate::controls::action("confirm-delete-conversation")
                                .danger()
                                .label("Delete")
                                .on_click(move |_, window: &mut Window, cx: &mut App| {
                                    window.close_dialog(cx);
                                    let dir = dir.clone();
                                    pane.update(cx, |pane: &mut Self, cx| {
                                        pane.delete_conversation(dir, cx);
                                    });
                                }),
                        ),
                )
        });
    }

    /// Delete an archived conversation, the question already answered.
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

    pub fn set_git(&mut self, line: Option<SharedString>, cx: &mut Context<Self>) {
        if self.git == line {
            return;
        }
        self.git = line;
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

    /// A turn settled. Badge it, and say so out loud wherever
    /// [`Attention::telling`] sends it.
    ///
    /// [`Attention::telling`]: onehand_core::chat::Attention::telling
    fn turn_ended_detached(&mut self, uid: u64, agent: &str, root: &str, cx: &mut Context<Self>) {
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
            super::session::notify_turn_ended(agent.to_string(), root.to_string());
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
    fn answer_tail(&self, uid: u64, cx: &App) -> Option<String> {
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
    fn link_lost_detached(&mut self, uid: u64, agent: &str, root: &str, cx: &mut Context<Self>) {
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
    fn awaiting_user_detached(
        &mut self,
        uid: u64,
        ask: onehand_core::chat::UserAsk,
        agent: &str,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        let say = self.telling(uid, Away::Asked(ask), cx);
        if say.desktop {
            super::session::notify_awaiting_user(ask, agent.to_string(), root.to_string());
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
                    // Bounded by the panel, for the same reason the project
                    // page's column is: this list is every conversation the
                    // agent has had in the project, and a column taller than
                    // the panel is centred into rows nothing can reach.
                    .max_h_full()
                    .min_h_0()
                    .child(div().font_semibold().child("Resume a conversation"))
                    // The rows are what grows, so the rows are what scrolls. The
                    // heading above and the way out below stay where they are --
                    // a picker whose *Start a new conversation* scrolls off the
                    // bottom is a screen with no way out of it.
                    .child(
                        div()
                            .id("resume-choices")
                            .v_flex()
                            .gap_3()
                            .w_full()
                            .min_h_0()
                            .overflow_y_scroll()
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
                            })),
                    )
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
                            // Bounded by the panel it sits in, so the page is
                            // centred while it fits and fills the space when it
                            // does not. Without this the column takes its
                            // content's height whatever that is, and a project
                            // with a full list of archives on a short window
                            // pushed its own rows out through the top and bottom
                            // of the panel -- unreachable, because the centring
                            // spends the overflow at both ends and there is
                            // nothing to scroll.
                            .max_h_full()
                            .min_h_0()
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
                            // The archives are the one part of this page that
                            // grows, so they are the part that scrolls. *New
                            // session* above and the count of what was left out
                            // below stay put: the first is why most people are
                            // on this page, and the second is the page saying a
                            // bound bit, which it cannot do from under the fold.
                            .children((!shown.is_empty()).then(|| {
                                div()
                                    .id("project-history")
                                    .v_flex()
                                    .gap_3()
                                    .w_full()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child("Past conversations"),
                                    )
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
                                        let (agent, archive) = (
                                            SharedString::from(meta.agent.clone()),
                                            meta.dir.clone(),
                                        );
                                        let dir = meta.dir.clone();
                                        let name = SharedString::from(meta.title.clone());
                                        conversation_card(
                                            ("home", i),
                                            meta.title.clone().into(),
                                            subtitle.into(),
                                            cx,
                                        )
                                        .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                                            cx.emit(ChatPaneEvent::StartSession {
                                                agent: Some(agent.clone()),
                                                resume: Some(archive.clone()),
                                            });
                                        }))
                                        // A word rather than a glyph, and this is the one
                                        // control in the app that earns the distinction:
                                        // everything else it offers can be done again --
                                        // a closed session respawns, a removed project is
                                        // added back -- and a deleted conversation cannot.
                                        //
                                        // Inside the card, so it is plainly about the
                                        // conversation beside it rather than about the row
                                        // it happened to be nearest. That puts one clickable
                                        // inside another, which is what the stop below is
                                        // for: without it the press that asks to delete a
                                        // conversation also opens it.
                                        .child(
                                            crate::controls::action(("home-delete", i))
                                                .ghost()
                                                .small()
                                                .text_color(danger)
                                                .label("Delete")
                                                .tooltip("Delete this conversation")
                                                .on_click(cx.listener(
                                                    move |pane: &mut Self, _, window, cx| {
                                                        cx.stop_propagation();
                                                        pane.confirm_delete(
                                                            dir.clone(),
                                                            name.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        )
                                    }))
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
        // A pinned card rests on the composer rather than inside the list, but
        // what it must not outgrow is the same panel.
        well: Option<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        // A question's free-text box needs a window to be built and the session
        // never has one, so the card's own way to the screen is where it is
        // made. Before the cards are read, since the box is one of them.
        session.update(cx, |s, cx| {
            s.sync_ask_inputs(window, cx);
            s.sync_perm_focus(window, cx);
        });
        let Some(chat) = self.active_chat(cx) else {
            return Vec::new();
        };
        let mut out: Vec<(usize, gpui::AnyElement)> = chat
            .pending_permissions()
            .into_iter()
            .map(|(idx, p)| {
                (
                    idx,
                    transcript::permission(session, p, TranscriptItemId::Live(idx), well, cx)
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
            transcript::floating_card(cx)
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Spinner::new().xsmall())
                .child(status),
        )
    }

    /// The widest the elapsed column ever has to be.
    ///
    /// **Reserved, and the digits sit against its right edge.** The whole point
    /// of the column is that nothing after it moves when `9s` becomes `10s` or
    /// `59s` becomes `1m 0s`, and a box that shrink-wraps its digits moves on
    /// every one of those. Fixing the box and putting the digits at its right
    /// edge is the whole of the fix: what follows the clock begins at the same
    /// place whatever the clock says, and the digits grow leftward into room
    /// that was already spoken for.
    ///
    /// **Drawn in the row's own face, not in mono**, which the reserved box is
    /// what makes affordable. Tabular digits answer a narrower question -- that
    /// the text inside a shrink-wrapping box not slide -- and they answer it by
    /// putting a second typeface on a row of text. Two faces on one line do not
    /// share a baseline, so the clock sat a shade off everything beside it,
    /// which reads as the row not being on one line at all. There is no jump
    /// left for them to prevent.
    ///
    /// **Held at what the longest form actually needs and no wider.** Reserved
    /// generously it is dead space that never goes away, and right-aligned
    /// digits put all of it on the *left* -- so every short clock read as the
    /// mark beside it having drifted away from the words.
    const CLOCK_W: Rems = rems(2.75);

    /// The mark that says a turn is alive, and how far it breathes.
    ///
    /// **A square that swells and shrinks rather than a spinner.** A spinner is
    /// a wait with no progress in it, which is what this is not: the thing it
    /// stands beside is a clock counting up and a sentence that changes.
    ///
    /// **It grows about its own centre, and the slot around it never changes
    /// size.** Growing a box on a row of text pushes that row's baseline
    /// around, and a mark that moved the words beside it every second would be
    /// worse than no mark. So the slot is held at the largest the square ever
    /// gets and the square is centred inside it: what breathes is the ink, and
    /// the space it occupies is constant.
    const PULSE_SIZE: Rems = rems(0.875);
    const PULSE_MIN: Rems = rems(0.4375);

    fn working_strip(&self, cx: &App) -> gpui::AnyElement {
        let running = self
            .active_conversation()
            .and_then(|conv| conv.session())
            .map(|session| session.read(cx))
            .map(|session| {
                session
                    .chat
                    .items
                    .iter()
                    .filter(|item| {
                        matches!(
                            item,
                            onehand_core::chat::ChatItem::Tool(tool)
                                if matches!(
                                    tool.call.status,
                                    onehand_core::acp::ToolStatus::InProgress
                                        | onehand_core::acp::ToolStatus::Pending
                                )
                        )
                    })
                    .count()
            })
            .unwrap_or(0);
        let status = self.active_chat(cx).and_then(|chat| chat.activity_status());

        let elapsed = self.turn_began.map_or(0, |began| began.elapsed().as_secs());
        let clock = match elapsed {
            0..=59 => format!("{elapsed}s"),
            _ => format!("{}m {}s", elapsed / 60, elapsed % 60),
        };

        // **Only what is actually there.** A separator standing between a thing
        // and nothing is punctuation for a clause that was never written, and
        // the clock never takes one at all: it is the row's own left edge
        // rather than one side of a pair.
        let mut parts: Vec<gpui::AnyElement> = Vec::new();
        if running > 0 {
            parts.push(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .child(match running {
                        1 => "1 running task".to_string(),
                        n => format!("{n} running tasks"),
                    })
                    .into_any_element(),
            );
        }
        if let Some(status) = status {
            // The one part that gives way: it is the agent's own words about
            // what it is doing, and the only thing here whose length nothing
            // bounds.
            parts.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(status)
                    .into_any_element(),
            );
        }

        let (big, small) = (Self::PULSE_SIZE.0, Self::PULSE_MIN.0);
        let mut row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .h(rems(1.5))
            .text_xs()
            // **One ink for the words, the accent for the mark alone.** A
            // status line tinted to be noticed is a status line competing with
            // the answer arriving above it.
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex_none()
                    .size(Self::PULSE_SIZE)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .rounded(radius_tag(cx))
                            .bg(crate::theme::status_ink(cx).success)
                            .with_animation(
                                "turn-pulse",
                                // Capped well under the frame rate: this is a
                                // mark keeping time, not something being
                                // watched, and an uncapped repeat redraws the
                                // whole window on every frame for as long as a
                                // turn runs.
                                Animation::new(std::time::Duration::from_millis(1_100))
                                    .repeat()
                                    .with_max_fps(30.),
                                move |square, t| {
                                    // Centred by the slot rather than by an
                                    // offset of its own, so the growth is even
                                    // on all four sides and the arithmetic has
                                    // nowhere to be wrong.
                                    let phase = t * std::f32::consts::TAU;
                                    let swell = (1. + phase.sin()) / 2.;
                                    square.size(rems(small + (big - small) * swell))
                                },
                            ),
                    ),
            )
            .child(div().flex_none().w(Self::CLOCK_W).text_right().child(clock));
        for (n, part) in parts.into_iter().enumerate() {
            if n > 0 {
                row = row.child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground.opacity(0.6))
                        .child("·"),
                );
            }
            row = row.child(part);
        }
        row.into_any_element()
    }

    /// Notice a turn starting and ending, and keep its clock true.
    ///
    /// **Stamped here because the model does not carry it.** A turn's start is
    /// `pub(crate)` in core, and the status line is not a good enough reason to
    /// widen it -- so the pane notices `busy` going up and reads its own clock.
    /// What that costs is an approximation: a session switched away from and
    /// back, or an app restarted mid-turn, starts counting again from zero.
    fn track_turn(&mut self, window: &Window, cx: &mut Context<Self>) {
        let live = self
            .active_chat(cx)
            .is_some_and(|chat| chat.busy && chat.link != Link::Connecting);
        if !live {
            // The clock is the turn's, so it goes with it.
            self.turn_began = None;
            self.ticker = None;
            return;
        }
        self.turn_began.get_or_insert_with(std::time::Instant::now);
        self.start_ticker(window, cx);
    }

    /// Wake once a second while a turn is live, and not otherwise.
    ///
    /// **Not a frame timer**, which is what a clock drawn from the render pass
    /// would become: this asks for a redraw at the rate the thing it draws
    /// actually changes. It stands down while the window is not the one in
    /// front of the user -- the seconds keep passing either way, and the count
    /// is read off a start instant rather than accumulated, so coming back to
    /// the window shows the right number rather than the number of ticks that
    /// were drawn.
    fn start_ticker(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.ticker.is_some() || !window.is_window_active() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |pane, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let live = pane.update(cx, |pane: &mut Self, cx| {
                    let live = pane.turn_began.is_some();
                    if live {
                        cx.notify();
                    }
                    live
                });
                if !matches!(live, Ok(true)) {
                    break;
                }
            }
        }));
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
            transcript::floating_card(cx)
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
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
            // **A fixed height, not one the tallest control happens to make.**
            // It is the one row that never scrolls, so it is the edge every
            // other measurement in the panel is taken from -- and sized by its
            // contents it moved whenever a badge appeared or a title wrapped,
            // taking the top of the conversation with it.
            .flex_none()
            .h(HEADER_H)
            .px_4()
            // **No rule under it.** A hairline is an edge between two surfaces,
            // and there are not two here: the header and the transcript are one
            // reading surface, and what separates them is that one is a row of
            // controls and the other is prose -- which the muted ink and the
            // spacing already say. The panels either side of this one draw
            // their own edges and nothing else does, so a line across the top of
            // the conversation was the last one left marking an inside.
            //
            // It was tried, on the reasoning that the list clips at exactly this
            // line and an unmarked cut reads as a rendering fault. What that
            // costs is a rule drawn permanently for a state the reader is only
            // in while scrolling -- and the fade at the other end of the list is
            // the answer that shape of problem actually takes.
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
            // Only while a session is showing, and for a reason worth stating:
            // this is the same list the project page draws, and that page is
            // exactly what the centre of the window shows when there is no
            // session — offering it there too would be saying one thing twice
            // within an inch of itself.
            .when(live, |header| header.child(self.history_control(cx)))
            // Beside the Workbench button: both are docks this panel is
            // sitting between, and a closed one leaves nothing on screen at all
            // -- no edge, no strip, no name -- so the route to it belongs with
            // the panel that took the space. Which mode it opens on and whether
            // a second press closes it are the shell's rules.
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

    /// The way back to a conversation this project has already had — as a
    /// session of its own, beside the one on screen.
    ///
    /// **The gap it fills.** Every other route to an archive costs the session
    /// in front of the user. The project page lists them and mints a session on
    /// the one picked, but it is what the centre of the window shows *instead
    /// of* a conversation, so reaching it meant closing every session in the
    /// project first; and the title menu's own picker leaves nothing running —
    /// it takes the session on screen off its conversation to ask the question.
    /// So this is the one that opens an old conversation and keeps the current
    /// one where it is, as a second row in the rail.
    ///
    /// **A menu on a header button rather than a dialog**, because it is a
    /// short list of one project's own conversations and the header is already
    /// where the things about this pane are. A modal over the conversation to
    /// pick a conversation is a heavier gesture than the choice deserves.
    ///
    /// **Every agent's**, as the project page's list is and for the same reason:
    /// the question is which conversation, and which agent had it is a property
    /// of the answer rather than a filter on the question. The title menu's
    /// picker is the narrow one, because there the agent is already decided.
    ///
    /// **A conversation already open is listed and refused, not hidden.** Two
    /// sessions on one archive both believe the transcript so far is on disk, so
    /// the second one's first turn writes the file back holding only what came
    /// after it. Dropping the row instead would leave the one conversation the
    /// user is most likely to look for missing from the list with nothing said.
    fn history_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // Owned rather than borrowed out of the sessions: the menu is built
        // later, from a closure that outlives this borrow of the pane.
        let open: Vec<String> = self
            .conversations
            .values()
            .filter_map(Conversation::session)
            .filter_map(|session| session.read(cx).chat.session_id.clone())
            .collect();
        let now = onehand_core::chat::now_secs();
        let held = self.archives.as_ref().and_then(|held| held.found.as_ref());
        let rows: Vec<HistoryRow> = held
            .into_iter()
            .flatten()
            .take(HISTORY_ROWS)
            .map(|meta| HistoryRow {
                title: SharedString::from(meta.title.clone()),
                // The age, and the agent only where it is not the one running
                // this session — on a machine with one agent configured, naming
                // it on every row is a column of the same word.
                aside: SharedString::from(format!(
                    "{} · {}",
                    rel_time(now, meta.updated),
                    meta.agent
                )),
                open: open.iter().any(|id| id == &meta.session_id),
                agent: SharedString::from(meta.agent.clone()),
                dir: meta.dir.clone(),
            })
            .collect();
        let hidden = held.map_or(0, |all| all.len().saturating_sub(HISTORY_ROWS));
        // `None` is the read still out and `Some([])` is a project that has
        // never been prompted. One is a wait and the other is an answer, and a
        // menu that gives the second while the first is true tells somebody with
        // a hundred conversations that they have none.
        let standing = match held {
            None => Some(SharedString::from("Reading conversations…")),
            Some(all) if all.is_empty() => {
                Some(SharedString::from("No conversations in this project yet"))
            }
            Some(_) => None,
        };
        let this = cx.entity();

        header_control("history", IconName::GalleryVerticalEnd, cx)
            .tooltip("Open a past conversation in a new session")
            // Anchored to its own right-hand corner: this button sits at the end
            // of the header, and a menu hanging rightwards from it opens off the
            // edge of the window.
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| {
                let muted = cx.theme().muted_foreground;
                // A project is worked in for months and every conversation had
                // in it is a row here, so the list is longer than a menu's
                // height by design. Without this the rows past the bottom are
                // built and drawn with no way to reach them.
                let menu = menu.scrollable(true);
                let menu = match standing.clone() {
                    Some(line) => menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div().text_color(muted).child(line.clone())
                        })
                        .disabled(true),
                    ),
                    None => menu,
                };
                let menu = rows.iter().fold(menu, |menu, row| {
                    let (title, aside, open) = (row.title.clone(), row.aside.clone(), row.open);
                    let draw =
                        move |_: &mut Window, _: &mut App| {
                            div()
                                .h_flex()
                                .items_center()
                                .gap_3()
                                .w_full()
                                .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                                .child(div().flex_none().text_xs().text_color(muted).child(
                                    if open {
                                        SharedString::from("already open")
                                    } else {
                                        aside.clone()
                                    },
                                ))
                        };
                    match open {
                        // Already open keeps the library's own cursor with its
                        // refusal, so the pointer stays a promise a row can
                        // keep.
                        true => menu.item(PopupMenuItem::element(draw).disabled(true)),
                        false => {
                            let item = crate::controls::menu_row(draw);
                            let (start, agent, dir) =
                                (this.clone(), row.agent.clone(), row.dir.clone());
                            menu.item(item.on_click(move |_, _, cx: &mut App| {
                                start.update(cx, |_: &mut Self, cx| {
                                    cx.emit(ChatPaneEvent::StartSession {
                                        agent: Some(agent.clone()),
                                        resume: Some(dir.clone()),
                                    });
                                });
                            }))
                        }
                    }
                });
                // Said out loud rather than left as a list that simply stops: a
                // cut nobody is told about reads as archives that were lost.
                match hidden {
                    0 => menu,
                    n => menu.separator().item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{n} older, not shown"))
                        })
                        .disabled(true),
                    ),
                }
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
            .child(title.clone());
        if !live && project.is_none() {
            return div().flex_none().min_w_0().child(name).into_any_element();
        }
        let project = project.map(|project| (project.pinned, project.is_repo));

        let radius = cx.theme().radius;
        let this = cx.entity();

        let row = crate::controls::action("conversation-title")
            .ghost()
            .h_flex()
            .items_center()
            .gap_1()
            .flex_initial()
            .min_w_0()
            .overflow_hidden()
            .px_1p5()
            .py_0p5()
            .rounded(radius)
            .label(title)
            .dropdown_caret(true)
            .text_color(cx.theme().foreground)
            .font_semibold();

        if let Some((pinned, is_repo)) = project {
            let target = this.clone();
            return row
                .dropdown_menu_with_anchor(
                    gpui::Anchor::TopLeft,
                    project_menu(pinned, is_repo, target),
                )
                .into_any_element();
        }

        row.dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |menu, _, cx| {
            let danger = crate::theme::status_ink(cx).danger;
            let (rename, export, history) = (this.clone(), this.clone(), this.clone());
            let (restart, remove) = (this.clone(), this.clone());
            let archive = archive.clone();
            menu.item(
                crate::controls::menu_item("Rename…")
                    .icon(Icon::new(crate::icons::Icon::SquarePen))
                    .on_click(move |_, _, cx: &mut App| {
                        rename.update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Rename));
                    }),
            )
            .item(
                crate::controls::menu_item("Export as Markdown…")
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
                // Named for what it does *to this session*, because the
                // header now carries a control that reaches the same
                // archives and leaves the session alone: this one swaps
                // what the conversation on screen is, and the difference
                // between the two is the whole question.
                //
                // Disabled mid-turn rather than guarded by a second click:
                // going back to the picker throws the running turn away
                // exactly as a restart does, and a menu that has to be
                // opened twice to be believed is a worse warning than an
                // item that will not go.
                match busy {
                    true => PopupMenuItem::new("Resume in this session…"),
                    false => crate::controls::menu_item("Resume in this session…"),
                }
                .icon(Icon::new(IconName::Undo))
                .disabled(busy)
                .on_click(move |_, _, cx: &mut App| {
                    history.update(cx, |pane: &mut Self, cx| pane.show_history(cx));
                }),
            )
            .item(
                crate::controls::menu_item("Restart the agent")
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
                {
                    let row = move |_: &mut Window, _: &mut App| {
                        div().text_color(danger).child("Delete conversation")
                    };
                    // Nothing on disk to remove until the first turn has
                    // ended, so until then this refuses -- and a refusal
                    // keeps the library's own cursor, since a pointer over
                    // it would promise a press that does nothing.
                    match archive.is_none() {
                        true => PopupMenuItem::element(row),
                        false => crate::controls::menu_row(row),
                    }
                }
                .icon(Icon::new(IconName::Delete).text_color(danger))
                // An entry that can only report that it has nothing to do is
                // one the eye has to learn to skip, so it is refused rather
                // than hidden -- the menu keeps its shape between one turn
                // and the next.
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
        // What a block bounds itself against, measured on the panel rather than
        // the window: with a dock open, half the window is taller than the whole
        // conversation.
        //
        // **Handed in, never read here.** The list holds its own state mutably
        // for as long as it is calling this, so asking it how tall its viewport
        // is from inside is a second borrow and a panic -- which is a crash on
        // the first frame of any conversation carrying a permission, not a rare
        // race. The caller reads it before the list starts, which is the last
        // moment it can be asked.
        room: transcript::Room,
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
        let body =
            |targets: &[TranscriptItemId], room: &transcript::Room| -> Vec<gpui::AnyElement> {
                targets
                    .iter()
                    .filter_map(|&target| {
                        viewport::item(chat, target).map(|item| {
                            let find_emphasis =
                                self.find.as_ref().and_then(|find| find.emphasis(target));
                            transcript::item(
                                session,
                                item,
                                target,
                                find_emphasis,
                                room.clone(),
                                window,
                                cx,
                            )
                            .into_any_element()
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
        let margin = match room.narrow {
            true => SIDE_MARGIN_NARROW,
            false => SIDE_MARGIN,
        };

        let Some(strip) = plan.strip.clone() else {
            // The run the layout appends while a turn is live carries no
            // transcript item, because what it reports is the turn rather than
            // anything in it.
            //
            // **It takes the widest boundary in the conversation**, the one a
            // prompt gets, and ignores the cadence the run above it asked for.
            // Every other gap here is between two things the agent said; this
            // one is between what it said and the app talking about it, and set
            // at a block's distance the line read as one more entry in the
            // turn -- worst directly under a cluster, where the two closed
            // ranks and the status line looked like another folded step.
            if plan.members.is_empty() {
                // The other memberless row: what a finished turn wrote. It
                // takes the same wide boundary, and for the same reason -- it
                // is the app talking about the turn rather than part of it.
                let body = match &plan.changes {
                    Some(plan_changes) => {
                        let anchor = plan_changes.anchor;
                        let this = self.handle.clone();
                        let folded = session.clone();
                        transcript::turn_changes(
                            &plan_changes.changes,
                            plan.open,
                            move |_, _, cx: &mut App| {
                                folded.update(cx, |session, cx| {
                                    session.toggle_activity(anchor);
                                    cx.notify();
                                });
                                let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                            },
                            ("changes", anchor.index()).into(),
                            cx,
                        )
                    }
                    None => self.working_strip(cx),
                };
                return column(rems(BLOCK_GAP.0 * 2.), margin, vec![body], cx).into_any_element();
            }
            return column(lead, margin, body(&plan.members, &room), cx).into_any_element();
        };

        let anchor = plan.members[0];
        let this = self.handle.clone();
        let folded = session.clone();

        // **Nothing under the line is built while it is closed.** A cluster is
        // every step between two paragraphs, which in a long turn is dozens —
        // and a collapsed line that built them all to draw none of them is that
        // cost paid per frame for something nobody asked to see.
        let inside = match plan.open {
            false => Vec::new(),
            true => strip
                .sections
                .iter()
                .enumerate()
                .map(|(n, section)| self.section_element(section, n, session, &room, window, cx))
                .collect(),
        };

        column(
            lead,
            margin,
            vec![transcript::cluster(
                &strip,
                plan.open,
                move |_, _, cx: &mut App| {
                    folded.update(cx, |session, cx| {
                        session.toggle_activity(anchor);
                        cx.notify();
                    });
                    // The pane owns the run layout the list reads back, so it
                    // is the half that has to be told to draw again -- the
                    // session's own notify redraws the session, not the plan.
                    let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                },
                ("activity", anchor.index()).into(),
                inside,
                cx,
            )],
            cx,
        )
        .into_any_element()
    }

    /// One stretch of one kind of work inside an opened cluster.
    ///
    /// A section of one member is that member's own row; a section of several
    /// is one row standing for them that opens into the rest.
    fn section_element(
        &self,
        section: &viewport::Section,
        // Which of the cluster's sections this is, for the rule that a
        // hairline goes between two of them and never above the first.
        index: usize,
        session: &Entity<ChatSession>,
        room: &transcript::Room,
        window: &Window,
        cx: &App,
    ) -> gpui::AnyElement {
        let Some(chat) = self.active_chat(cx) else {
            return div().into_any_element();
        };
        let body =
            |targets: &[TranscriptItemId], room: &transcript::Room| -> Vec<gpui::AnyElement> {
                targets
                    .iter()
                    .filter_map(|&target| {
                        viewport::item(chat, target).map(|item| {
                            let find_emphasis =
                                self.find.as_ref().and_then(|find| find.emphasis(target));
                            transcript::item(
                                session,
                                item,
                                target,
                                find_emphasis,
                                room.clone(),
                                window,
                                cx,
                            )
                            .into_any_element()
                        })
                    })
                    .collect()
            };

        let anchor = section.members[0];
        let open = session.read(cx).section_is_open(anchor);
        let this = self.handle.clone();
        let folded = session.clone();

        // **A section of one is that step's own row, and so is a section of
        // several — the merge is in the row's own words.** A row standing for
        // one step would be the same row twice, one inside the other; and a row
        // standing for three reads says `Read 3 files` and opens into the three
        // paths, which is one level rather than two.
        let single = section.members.len() < 2;
        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .children((index > 0).then(|| transcript::rule(cx)))
            .map(|block| match single {
                true => block.children(body(&section.members, room)),
                false => block.child(transcript::activity_group(
                    section,
                    open,
                    move |_, _, cx: &mut App| {
                        folded.update(cx, |session, cx| {
                            session.toggle_section(anchor);
                            cx.notify();
                        });
                        let _ = this.update(cx, |_: &mut Self, cx| cx.notify());
                    },
                    ("section", anchor.index()).into(),
                    match open {
                        true => body(&section.members, room),
                        false => Vec::new(),
                    },
                    cx,
                )),
            })
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
    /// **The one place the order lives.** Three readers now — a session row
    /// reducing its own four facts, a project row reducing its sessions'
    /// signals, and the rail's flat session list sorting itself so the ones
    /// wanting an answer rise to the top — and an order written out twice is an
    /// order that will disagree with itself the first time someone edits one
    /// copy.
    pub(crate) fn rank(self) -> u8 {
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
    /// Rename the branch checked out in it. Offered on repositories only, for
    /// the reason above and by the same fact: a project with no status from the
    /// last sweep has no branch to rename.
    RenameBranch,
    CopyPath,
    RefreshGit,
    /// Drop it from the workspace. The shell still guards this behind a second
    /// press while anything is running in it.
    Remove,
}

/// The branch line, as the control it is.
///
/// **A menu and not a label**, because everything the reader might do about
/// what it says is a thing the shell already does: split this branch into a
/// second checkout, rename it, or go and look again. Printed flat, the strip's
/// one piece of project state was the one piece with no way to act on it, and
/// both of those actions were reachable only from a rail row or a page that is
/// not on screen while a conversation is.
///
/// Built by the pane rather than by the composer, which draws the rest of the
/// strip: git is the project's and this panel is what talks to the shell about
/// the project. The composer has no vocabulary for any of it.
///
/// Drawn to match the two setting chips beside it — same height, same inset,
/// same muted ink, a mark then a word and no caret — so the strip stays one row
/// of one kind of thing. It is the same reason the line was never a sentence in
/// prose: what differs is which side of the row it is on.
fn branch_control(
    line: SharedString,
    pane: Entity<ChatPane>,
    cx: &mut Context<ChatPane>,
) -> impl IntoElement + use<> {
    let act = |action: ProjectAction, pane: Entity<ChatPane>| {
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            pane.update(cx, |_: &mut ChatPane, cx| {
                cx.emit(ChatPaneEvent::Project(action));
            });
        }
    };
    let (worktree, rename, refresh) = (pane.clone(), pane.clone(), pane);

    crate::controls::action("branch")
        .ghost()
        .xsmall()
        .h_flex()
        .items_center()
        .gap_1()
        .flex_shrink_1()
        .min_w_0()
        .h(super::composer::CHIP_H)
        .px_1p5()
        .rounded(cx.theme().radius)
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(crate::icons::Icon::GitBranch).size_3())
        // The branch leads the line, so what the cap takes first is the change
        // count behind it.
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(super::composer::CHIP_TEXT)
                // Full strength, as every chip's value is: the muted ink on the
                // button is what the mark beside this takes. A branch written a
                // shade fainter than the setting at the other end of the strip
                // reads as less certain rather than as a different kind of
                // thing.
                .text_color(cx.theme().foreground)
                .child(line),
        )
        .tooltip("The branch checked out here")
        .dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |menu, _, _| {
            menu.item(
                crate::controls::menu_item("Rename branch…")
                    .icon(Icon::new(crate::icons::Icon::SquarePen))
                    .on_click(act(ProjectAction::RenameBranch, rename.clone())),
            )
            .item(
                crate::controls::menu_item("New worktree…")
                    .icon(Icon::new(crate::icons::Icon::GitBranch))
                    .on_click(act(ProjectAction::Worktree, worktree.clone())),
            )
            .separator()
            .item(
                crate::controls::menu_item("Refresh Git status")
                    .icon(Icon::new(IconName::Redo))
                    .on_click(act(ProjectAction::RefreshGit, refresh.clone())),
            )
        })
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
        self.track_turn(window, cx);
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
        // **Where the transcript stops being drawn: the composer's own middle.**
        // The overlay is transparent around its surfaces, so a row scrolling
        // under it stayed visible in the strip above the card, at both sides of
        // it and under it -- a line of the conversation cut in two by a box
        // resting on top of it, which reads as the card having been dropped on
        // the text rather than as the text ending. Clipped here it ends behind
        // the card's opaque top half, so nothing is ever seen sliced: the cut
        // itself is under a surface. Half the overlay rather than half the card
        // measured separately, which lands inside that half for every composer
        // taller than its own status row plus its inset -- and the resting
        // composer is four times that.
        let cut = overlay_h / 2.;
        let tail_room = self
            .active_conversation()
            .map_or((floor - cut).max(px(0.)), |conv| {
                conv.viewport.tail_room(floor, cut)
            });
        let this = cx.entity();
        let for_render = session.clone();
        let scrolled_up = away_from_tail(&list_state) && !holding;
        // The clip is already taken off inside `tail_room`, which is the only
        // place that knows whether the number it returned was measured from the
        // clipped viewport or is a constant that never heard of it.
        let tail_pad = tail_room;
        // How much room a composer popup has to open into: the well, less what
        // the composer and its rest already stand in.
        //
        // **The list's own viewport is the measurement**, because the list is
        // inside that well -- so the number is there for the asking and a second
        // canvas measuring the same box would be a second answer to keep in
        // step. It is last frame's, which is the frame the popup was opened
        // from. The clip is added back because the popup opens into the *well*
        // and the well is what the list stops short of.
        let popup_room = super::composer::popup_room(
            list_state.viewport_bounds().size.height + cut,
            floor,
            window.rem_size(),
        );
        // What a block inside the conversation bounds itself against, read here
        // and handed down rather than asked for where it is used.
        //
        // **This is the last moment it can be asked.** The list borrows its own
        // state mutably for as long as it is building rows, so a row reaching
        // back to ask how tall the viewport is panics -- and it is the rows of
        // *this* list that want the answer. Read before the list starts, one
        // value serves every row and the pinned cards above them alike.
        let well = (list_state.viewport_bounds().size.height > px(0.))
            .then(|| list_state.viewport_bounds().size.height);
        // Read off the same frame and for the same reason: how wide the panel
        // is decides what a block may spend on margins and on columns that are
        // not the one thing it has to say. A width of zero is the frame before
        // the list has measured itself, which is not a narrow panel.
        let list_w = list_state.viewport_bounds().size.width;
        let narrow = list_w > px(0.) && list_w < NARROW_PANEL.to_pixels(window.rem_size());
        let room = transcript::Room::new(well, narrow);
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
                        // The list runs to the well's top edge, so it clips
                        // exactly at the header's rule rather than at an inset
                        // below it -- a band of blank surface above a line of
                        // text sliced in half reads as a rendering fault, not
                        // as a margin. At the bottom it stops at the cut, and
                        // that edge is under the composer where no slice shows.
                        // Its breathing room is *inside* the scroll: padding on
                        // the list is part of what scrolls, which is how the
                        // transcript comes to rest above the composer rather
                        // than merely disappearing behind it.
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom(cut)
                            .overflow_hidden()
                            .child(
                                list(list_state, move |ix, window: &mut Window, cx: &mut App| {
                                    this.read(cx).run_element(
                                        ix,
                                        &for_render,
                                        room.clone(),
                                        window,
                                        cx,
                                    )
                                })
                                .size_full()
                                .pt(LIST_HEAD)
                                .pb(tail_pad),
                            ),
                    )
                    // The transcript dissolving into the surface it is drawn
                    // on, right down to the clip. Between the list and every
                    // control, so what fades is the conversation alone: the
                    // jump pill, the pinned cards and the composer are all
                    // drawn after this and each carries its own opaque
                    // surface. It ends *at the clip* rather than at the top of
                    // the composer, because the composer's card is narrower
                    // than the panel -- a fade stopping at the card's own edge
                    // would leave the strips either side of it showing full
                    // strength text for the height of the card.
                    .child(div().absolute().bottom(cut).left_0().right_0().h(SMOKE).bg(
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(cx.theme().background.alpha(0.), 0.),
                            // Solid a little before the end, so the last of
                            // the text is gone by the time the clip takes
                            // it rather than exactly as it does.
                            gpui::linear_color_stop(cx.theme().background, 0.9),
                        ),
                    ))
                    // Over the transcript rather than in a row of its own: a
                    // control that appears and disappears cannot own layout, or
                    // the whole conversation shifts by its height every time the
                    // reader scrolls up and back down.
                    .when(scrolled_up, |well| {
                        well.child(
                            div()
                                .absolute()
                                // **Measured from the composer's own top edge,
                                // not from where the transcript comes to rest.**
                                // It was placed against that resting line, which
                                // is deliberately the widest space in the
                                // conversation -- so the control floated most of
                                // an inch clear of the thing it belongs beside.
                                // Held off by a block's gap and no more: two
                                // floating surfaces touching read as one surface
                                // with a notch taken out of it.
                                .bottom(overlay_h + BLOCK_GAP.to_pixels(window.rem_size()))
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
                                                // **The arrow alone.** The
                                                // words named what was down
                                                // there, which the transcript
                                                // itself says the moment the
                                                // control is used -- and a
                                                // label on a thing floating
                                                // over the conversation is a
                                                // sentence competing with the
                                                // one being read. What it
                                                // means is in the tooltip,
                                                // where a control that needs
                                                // explaining keeps it.
                                                .size(JUMP_PILL_H)
                                                .icon(Icon::new(IconName::ChevronDown))
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
                    .child(self.overlay(
                        &session,
                        measure,
                        OverlayRoom {
                            popup: popup_room,
                            well,
                        },
                        blocked,
                        window,
                        cx,
                    )),
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
        room: OverlayRoom,
        blocked: Option<onehand_core::chat::SubmitBlock>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let pinned = self.pinned(session, room.well, window, cx);
        let git = self
            .git
            .clone()
            .map(|line| branch_control(line, cx.entity(), cx).into_any_element());
        let pane = cx.entity();
        // The field draws no ring of its own once the card is its border, so
        // the card has to answer "does typing go here" -- with an app keymap
        // that reaches over the terminal and a rail that can take focus, an
        // input with no focused state is one the user has to test by typing.
        //
        // Asked here rather than handed in: it is a question about a window,
        // and this is the innermost place holding one.
        let typing_here = self
            .composer
            .read(cx)
            .state
            .focus_handle(cx)
            .contains_focused(window, cx);

        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .v_flex()
            // A step over the gap the cards keep between themselves: a pinned
            // card and the composer are two objects and the cards in a stack
            // are one, so the seam that has to read as a seam is this one.
            .gap_2p5()
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
            // **The popup and the pinned cards share one slot, and the popup
            // is drawn over them.** Stacked in the column instead, the two were
            // additive: a list at its full height plus a permission card
            // carrying a long command plus the composer could outgrow the
            // panel, and since the column is anchored at the bottom and grows
            // up, what ran off the top was the popup — header, first rows and
            // all, unreachable because the list scrolls inside itself. Sharing
            // a slot makes the cost `max` rather than `sum`, so the list keeps
            // its full height and nothing is pushed anywhere.
            //
            // The cards are what is in flow, so the slot is as tall as they
            // are and the transcript's clearance is unchanged. The popup is
            // absolute and anchored to the slot's bottom edge: with no card it
            // sits exactly where it always did, directly above the composer,
            // and with one it covers it and carries on upward.
            //
            // **The popup is the later child on purpose.** Paint order is what
            // puts it over the card rather than under, and it is also what
            // gives it the click: a list of choices opened over a card is the
            // thing being aimed at.
            //
            // What this costs is that a card already on screen is hidden while
            // a popup is open over it. That is the user's own doing — they
            // opened the picker and can see they did. A card that *arrives*
            // while one is open is the case that would be silent, and that is
            // answered at the event instead: parking an ask closes the popup.
            .children({
                let popup = self
                    .composer
                    .update(cx, |composer, cx| {
                        composer.detached_popup(session, room.popup, window.rem_size(), cx)
                    })
                    // **Every overlay is the same card, in the same place.** The
                    // option lists used to hang off the chip that opened them,
                    // on the reasoning that keeping a compact surface against
                    // its trigger says which control it belongs to. What it
                    // cost is the thing a list of choices is for: sized to its
                    // own rows and pinned to one end of the card, a model list
                    // had no room for the sentence the agent sends about each
                    // choice, and the rows it did fit were narrower than the
                    // words in them. The card above the composer is the width
                    // of the reading column, which is what every choice here
                    // needs -- and the chip stays lit underneath for as long as
                    // its list is open, which is what actually says where the
                    // list came from.
                    .map(|popup| {
                        div()
                            .w_full()
                            .px_4()
                            // **Lifted off whatever is under it, and only when
                            // something is.** Flush, the popup and a parked
                            // card have the same width, nearly the same
                            // surface and a shared edge, so the two read as one
                            // tall panel. The usual cue for "this is above
                            // that" is a drop shadow, and it is not available
                            // here: over the dark palette's near-black it is
                            // invisible, which is why that palette needs a real
                            // step for a floating control in the first place.
                            // The gap leaves the card's bottom edge and border
                            // showing, and two horizontal edges a few pixels
                            // apart is a stack where one is a panel.
                            //
                            // Left as a rem. Resolved against
                            // `window.rem_size()` it would be the one length in
                            // this stack measured from the *window's* base — and
                            // a panel's zoom overrides the rem base for its own
                            // subtree, so the peek would be the only part of it
                            // that did not grow with the text beside it.
                            .when(!pinned.is_empty(), |popup| {
                                popup.mb(super::composer::POPUP_STACK_PEEK)
                            })
                            .child(div().w_full().max_w(COMPOSER_COLUMN).mx_auto().child(popup))
                    });
                // **Outside the measured box, for the reason the popup is.** A
                // parked card is a surface over the conversation, not a floor
                // under it: measured, every card that arrives grows the
                // transcript's bottom padding, and a list anchored at its tail
                // answers that by shifting everything the user was reading
                // upward -- at the exact moment their attention is being asked
                // for, by a card that appeared because the agent chose to park
                // and not because anybody pressed anything. What it costs is
                // the last row or two of the conversation sitting behind the
                // card while it is up, which the reader can scroll to and which
                // comes back the moment the card is answered. The composer
                // stays measured: it is there the whole time, and a transcript
                // that ended behind the box being typed in would hide its own
                // last line permanently.
                let cards = (!pinned.is_empty()).then(|| {
                    div()
                        .w_full()
                        .px_4()
                        // **A card covering the conversation must not move it.**
                        // The same leak the popup above has: gpui's handler for
                        // a scrolling box adjusts its own offset and never
                        // claims the event, so a wheel over a parked card went
                        // on to the transcript underneath and scrolled the very
                        // rows the card is sitting on top of. Claimed on the
                        // wrapper, so the card's own wells still take what they
                        // can use first — bubble order runs the deeper listener
                        // before this one.
                        //
                        // The composer is deliberately not given this. It is
                        // the one surface down here the transcript *clears*
                        // rather than hides behind, so there is nothing under
                        // it being moved out of sight.
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .v_flex()
                                .gap_2()
                                .w_full()
                                .max_w(COMPOSER_COLUMN)
                                .mx_auto()
                                // The composer's column, because while a card is
                                // pinned it is part of that stack: these boxes sit
                                // directly on the card, share its surface and its
                                // radius, and are read as one object with it. A card
                                // an inch wider than the box it rests on reads as
                                // two panels that failed to line up.
                                //
                                // **Width follows where a card is, not what it is.**
                                // Answered, it is drawn in the transcript and takes
                                // the transcript's column like every block around
                                // it. That was already half true -- a transcript row
                                // is inset inside the reading column while a pinned
                                // card was not -- so the rule that said the two must
                                // match was describing something the layout had
                                // never quite done.
                                //
                                // The text size is still the transcript's, which is
                                // the part that does have to hold: a question
                                // re-read in the history has to be the same words at
                                // the same weight as the question that stopped
                                // everything.
                                .text_size(transcript::TEXT)
                                .children(pinned),
                        )
                });
                // Nothing at all rather than an empty box, so the column's own
                // gap is not spent on a slot with no height and the composer
                // does not drift down whenever neither is showing.
                // **The popup is the one in flow and the card is the one taken
                // out of it**, which is the opposite of the obvious way round
                // and the only way round that works.
                //
                // Absolute, the popup contributed no height, so this block's
                // own bounds ended below it — and the handler that closes a
                // popup when the mouse goes down *outside* this block compares
                // against exactly those bounds, in the capture phase. Every
                // click on a row was therefore a click outside, and the list
                // closed before the press could reach it: the keyboard picked
                // rows and the mouse could not.
                //
                // In flow the popup sets the height and the card hangs off the
                // bottom of it, behind it, which is where it was drawn anyway.
                // The card is the later thing to lose its height, and it can
                // afford to: nothing measures a parked card by design.
                match (popup, cards) {
                    (Some(popup), Some(cards)) => Some(
                        div()
                            .relative()
                            .w_full()
                            .child(div().absolute().bottom_0().left_0().right_0().child(cards))
                            .child(popup),
                    ),
                    (Some(popup), None) => Some(div().w_full().child(popup)),
                    (None, Some(cards)) => Some(div().w_full().child(cards)),
                    (None, None) => None,
                }
            })
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
                            .w_full()
                            .px_4()
                            // Transparent spacing around the cards is what makes
                            // this read as an overlay rather than a footer. The
                            // transcript keeps painting through it; only the
                            // surfaces below cover what sits directly behind
                            // them.
                            .pb_4()
                            .child(
                                div()
                                    .v_flex()
                                    .w_full()
                                    .max_w(COMPOSER_COLUMN)
                                    .mx_auto()
                                    .child(self.composer.update(cx, |composer, cx| {
                                        composer.card(session, blocked, typing_here, cx)
                                    }))
                                    // Under the card and inside the measured
                                    // box, so the transcript ends above the
                                    // strip rather than behind it: the height
                                    // the conversation clears is whatever this
                                    // whole overlay comes to, and the strip
                                    // appears and disappears with the project.
                                    .children(self.composer.update(cx, |composer, cx| {
                                        composer.status_row(session, git, cx)
                                    })),
                            ),
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
        // **A turn opens above the prompt and not below it.** The space over a
        // question is what a reader scrolling back finds the last one by, so it
        // is the widest boundary inside the conversation -- twice what two
        // blocks of one answer take. Under it the answer is the *reply*, and a
        // gap as wide as the one above would cut the question off from the
        // thing answering it. They were symmetrical, which said the prompt
        // belonged to neither side.
        (_, RunKind::Prompt) => rems(BLOCK_GAP.0 * 2.),
        (RunKind::Prompt, _) => BLOCK_GAP,
        // Index entries close ranks with each other and with nothing else.
        (RunKind::Compact, RunKind::Compact) => COMPACT_GAP,
        _ => BLOCK_GAP,
    }
}

/// The frame one run of the transcript is drawn in: a centred reading column
/// that shrinks with its panel. Width lives here rather than around each item
/// because a run is what the virtual list draws; activity summaries drawn by
/// the pane and their steps must share the same two edges.
fn column(lead: Rems, margin: Rems, content: Vec<gpui::AnyElement>, cx: &App) -> gpui::Div {
    let _ = cx;
    div()
        .h_flex()
        .w_full()
        // The reading size is set here, on the frame every run is drawn in, so
        // one place decides it for prose, cards, wells and rows alike. Set per
        // block instead, the blocks that never asked would keep the app's own
        // base and the transcript would be two sizes.
        .text_size(transcript::TEXT)
        // **One margin, on the run rather than on the box that clips the
        // transcript.** Padding there would inset the clip too, cutting text
        // short of the header's rule and leaving a band of blank surface above
        // whatever line the scroll stopped on. It was two insets — one here and
        // one on the column inside — which is a single number written as a sum
        // whose halves had already started moving independently.
        .px(margin)
        .pt(lead)
        .child(
            div()
                .w_full()
                .min_w_0()
                .max_w(transcript::CONTENT_COLUMN)
                .mx_auto()
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
///
/// **A row, not a column, so a caller can add its own control at the end.** The
/// name and the line under it are one column inside it, taking the width that
/// is left; anything a caller hangs on afterwards sits at the right-hand edge,
/// inside the card's own border rather than out beside it. The project page's
/// delete is the reason -- a control that acts on one conversation belongs
/// within the card naming it, and the same shape holds for the picker, which
/// simply adds nothing.
fn conversation_card(
    id: impl Into<ElementId>,
    title: SharedString,
    subtitle: SharedString,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
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
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
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
            crate::controls::menu_item(if pinned { "Unpin" } else { "Pin to top" })
                .icon(Icon::new(IconName::Star))
                .on_click(act(ProjectAction::TogglePin, pane.clone())),
        )
        // Only where there is a repository to split. On a plain folder this
        // could do nothing but report that git said no, and an entry whose whole
        // job is to fail is one the eye has to learn to skip.
        .when(is_repo, |menu| {
            menu.item(
                crate::controls::menu_item("New worktree…")
                    .icon(Icon::new(crate::icons::Icon::GitBranch))
                    .on_click(act(ProjectAction::Worktree, pane.clone())),
            )
        })
        .item(
            crate::controls::menu_item("Copy project path")
                .icon(Icon::new(IconName::Copy))
                .on_click(act(ProjectAction::CopyPath, pane.clone())),
        )
        .item(
            crate::controls::menu_item("Refresh Git status")
                .icon(Icon::new(IconName::Redo))
                .on_click(act(ProjectAction::RefreshGit, pane.clone())),
        )
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| {
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

/// How many past conversations the header's menu offers before it stops and
/// says so.
///
/// **High enough that it is not the thing deciding what the list shows.** The
/// menu scrolls, so what a reader can reach is not bounded by what fits; this
/// bounds the *work*, because a menu builds every row it holds the moment it
/// opens. It sits far past what anybody scrolls a menu for — the menu's own
/// height shows on the order of a dozen rows at a time — so it is a backstop
/// against a store nothing ever prunes rather than an editorial cut, and the one
/// time it bites it says so.
const HISTORY_ROWS: usize = 200;

/// One row of that menu, prepared before the menu builder runs.
///
/// The builder is an `Fn` that outlives the borrow of the pane these came from,
/// so everything it needs is copied out here rather than read through a handle
/// at the moment it draws.
struct HistoryRow {
    title: SharedString,
    /// How long ago, and which agent had it.
    aside: SharedString,
    /// Whether a session in this window is already on this conversation.
    open: bool,
    agent: SharedString,
    dir: PathBuf,
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
        .flex_initial()
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

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_GAP, COMPACT_GAP, RunKind, SessionSignal, TranscriptItemId, away_from_tail, lead_gap,
        rems, restart_needs_arming, switching_away, viewport, waits_alone,
    };
    use onehand_core::chat::Link;

    /// The two sides of a prompt are one space, so they are one number —
    /// whatever sits above the prompt and whatever follows it.
    ///
    /// This is the property the old layout could not hold. Spacing hung off the
    /// *upper* run, so the gap above a prompt was that run's own bottom padding
    /// plus the prompt's, and it therefore changed with what happened to
    /// precede it while the gap below never did.
    #[test]
    fn a_prompt_opens_a_turn_and_does_not_close_one() {
        // Above: the widest boundary in the conversation, whatever precedes it
        // -- that space is what a reader scrolling back finds the last question
        // by.
        for above in [RunKind::Block, RunKind::Compact, RunKind::Prompt] {
            assert_eq!(
                lead_gap(Some(above), RunKind::Prompt),
                rems(BLOCK_GAP.0 * 2.),
                "{above:?} above a prompt"
            );
        }
        // Below: an ordinary block gap, because what follows is the *reply*.
        // Symmetrical, the two said the prompt belonged to neither side.
        for below in [RunKind::Block, RunKind::Compact] {
            assert_eq!(
                lead_gap(Some(RunKind::Prompt), below),
                BLOCK_GAP,
                "{below:?} below a prompt"
            );
        }
        // **A prompt under a prompt is still a turn opening**, and opening wins
        // over closing: somebody who asked twice without waiting asked two
        // questions, and the space has to say so from the side that knows.
        assert_eq!(
            lead_gap(Some(RunKind::Prompt), RunKind::Prompt),
            rems(BLOCK_GAP.0 * 2.),
        );
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
                summary: onehand_core::chat::ClusterSummary::default(),
                sections: Vec::new(),
            }),
            changes: None,
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
