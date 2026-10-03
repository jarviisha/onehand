//! The dock's centre panel.
//!
//! Holds one [`Conversation`] per session the user has opened and renders the
//! active one. The pane is a *coordinator*: it decides which conversation is
//! showing and draws it, and what belongs to a single session lives on that
//! session rather than here. Switching is a lookup, not a save/restore.
//!
//! What is left at this level is chrome — the composer widget, the zoom, the
//! window handle — plus the one question the pane alone can answer, which is
//! which conversation the user is looking at.

use super::composer::{Composer, ComposerEvent};
use super::conversation::Conversation;
use super::session::ChatSession;
use super::transcript;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Rems, Render, SharedString, Styled, Window, div, rems,
};
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::input::InputEvent;
use onehand_core::chat::Link;
use std::collections::HashMap;
use std::path::PathBuf;

mod body;
mod bridge;
mod header;
mod project_page;
mod runs;
mod sessions;
mod workspace_page;
use header::Archives;
use project_page::EmptyProject;
pub use workspace_page::PageProject;
use workspace_page::WorkspacePage;

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

/// What a project's menu says about it, wherever that menu is drawn.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectFacts {
    /// Held at the top of the rail.
    pub pinned: bool,
    /// A git repository, so there is a branch to rename and a worktree to split.
    pub is_repo: bool,
    /// Whether its labelled issues may be worked unattended — `None` where the
    /// switch is not offered at all, which is a run's own worktree: not a
    /// project anybody chose, and one no run ever searches.
    pub unattended: Option<bool>,
}

impl ProjectFacts {
    /// The facts about `root`, given whether the last `git status` sweep found
    /// a repository there. One place, because the rail and the project page
    /// both build these and a copy in each is two answers to "is the switch
    /// offered here".
    pub fn of(root: &onehand_core::workspace::ProjectRoot, is_repo: bool) -> Self {
        Self {
            pinned: root.pinned,
            is_repo,
            unattended: (!root.transient).then_some(root.unattended),
        }
    }
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
    /// The workspace page, while it is what the pane shows. Set only with no
    /// session showing, and cleared by anything that shows a session or a
    /// project.
    workspace: Option<WorkspacePage>,
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
                zoom: crate::zoom::Zoom::default(),
                restart_armed: None,
                window: window.window_handle(),
                handle: cx.entity().downgrade(),
                empty: None,
                workspace: None,
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
    pub fn set_project_facts(&mut self, facts: ProjectFacts, cx: &mut Context<Self>) {
        let Some(project) = self.empty.as_mut() else {
            return;
        };
        if project.facts == facts {
            return;
        }
        project.facts = facts;
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

    /// When the agent of the session on screen started, if one is showing.
    pub fn active_started(&self, cx: &App) -> Option<std::time::Instant> {
        let session = self.session_of(self.active?)?;
        Some(session.read(cx).started)
    }

    /// `uid`'s live session, if it has reached one.
    fn session_of(&self, uid: u64) -> Option<&Entity<ChatSession>> {
        self.conversations.get(&uid)?.session()
    }

    /// Session `uid`'s live session, for a caller outside the pane that has to
    /// watch it.
    pub fn session_entity(&self, uid: u64) -> Option<Entity<ChatSession>> {
        self.session_of(uid).cloned()
    }

    /// The conversations with a live session in this pane, by the agent's
    /// session id, sorted. A session whose adapter is lost is not live: it is
    /// history on screen until somebody restarts it.
    pub fn live_conversations(&self, cx: &App) -> Vec<String> {
        let mut ids: Vec<String> = self
            .conversations
            .values()
            .filter_map(|conversation| {
                let chat = &conversation.session()?.read(cx).chat;
                (chat.link != onehand_core::chat::Link::Lost)
                    .then(|| chat.session_id.clone())
                    .flatten()
            })
            .collect();
        ids.sort();
        ids
    }

    /// The session holding the conversation the agent named `id`, if one in
    /// this pane does.
    pub fn uid_of_conversation(&self, id: &str, cx: &App) -> Option<u64> {
        self.conversations.iter().find_map(|(uid, conversation)| {
            let session = conversation.session()?;
            (session.read(cx).chat.session_id.as_deref() == Some(id)).then_some(*uid)
        })
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
    /// Let its labelled issues be worked unattended, or stop that.
    ToggleUnattended,
    /// Open its open issues, to pick one to work now.
    PickIssue,
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
    /// An agent was just spawned for a session — a first connect, a
    /// restart, a resume. Announced so the shell can tell whatever keeps
    /// count of what the running agent has loaded; every path that starts one
    /// passes through here, which no caller of those paths can promise.
    AgentStarted,
    /// The rail is hidden and the user asked for it back.
    ///
    /// Announced rather than acted on for the usual reason: the rail is the
    /// window's chrome, and a dock panel has no business reaching outside the
    /// dock to draw it.
    ShowRail,
    /// The user pressed the Workbench button in the conversation's header.
    ///
    /// Announced rather than acted on for the same reason as the rail: the
    /// Workbench is a dock, the dock is the window's arrangement, and the panel
    /// sitting in the middle of it does not get to rearrange the window. It
    /// also does not know which mode the Workbench would come back on, and two
    /// places deciding that would drift apart.
    ///
    /// **Open or closed, and not the third state the key has.** A key has one
    /// binding to serve every case, so an open-but-unfocused panel is focused
    /// rather than closed -- there is nothing else to reach it with. A button
    /// can see the dock, and the caret when it is pressed is almost always back
    /// in the composer, so the third state made the first press do nothing a
    /// presser could see.
    ToggleWorkbench,
    /// The user pressed the terminal button in the conversation's header.
    ///
    /// Announced rather than acted on for exactly the reasons above, and open
    /// or closed for the same one -- with one condition of its own: an open
    /// dock holding no shell is opened *into* rather than closed, because that
    /// is what closing the last tab leaves and the press there means "start
    /// one". The key is `` Ctrl+` ``, unshifted, because the shifted form
    /// cannot be typed.
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
    /// Open issue `number` of the project at `root` — a row of the workspace
    /// page. Selecting a project and opening the Workbench are both the
    /// shell's.
    OpenIssue {
        root: PathBuf,
        number: u64,
    },
    /// Show session `uid`, in whichever window holds it — a row of the
    /// workspace page, for an unattended run's session or one of this window's.
    ShowSession {
        uid: u64,
        window: gpui::AnyWindowHandle,
    },
    /// Select the project at this root — a row of the workspace page.
    ShowProject(PathBuf),
    /// Resume `archive` on `agent` in the project at `root` — a recent
    /// conversation on the workspace page.
    ResumeIn {
        root: PathBuf,
        agent: SharedString,
        archive: PathBuf,
    },
    /// The pipeline run on session `uid` waits for approval, and it is
    /// given: go on. Announced, because the run is the shell's and the strip
    /// that offers this only draws it.
    ContinuePipeline(u64),
    /// Send what that run waits on back, with a `note` on what to change.
    RevisePipeline {
        uid: u64,
        note: String,
    },
    /// Stop the pipeline run on session `uid`.
    StopPipeline(u64),
    /// Resume the unfinished pipeline run `id` — a row of the project page.
    ResumePipeline(String),
    /// Drop the unfinished pipeline run `id` and its file.
    DiscardPipeline(String),
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

pub use onehand_core::rel_time;

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

#[cfg(test)]
mod tests;
