//! The composer: the prompt buffer, its `@`/`/` completion popup, the
//! mode/model selectors and Send/Stop.
//!
//! The *rules* are core's. `onehand_core::completion` decides where a trigger
//! starts, what matches it and how accepting rewrites the text; `Chat::submit`
//! decides whether a prompt may be sent at all. What lives here is the widget
//! state and the drawing — which is the whole reason those two are in core.
//!
//! ## Why the popup is ours, and how the arrow keys reach it
//!
//! gpui-component's own completion menu lives *inside* the input, but the hook
//! to reach it (`CompletionProvider`) is editor-only — an ordinary input or
//! textarea has no language server and no field to reach one through. So the
//! list here is this file's.
//!
//! That leaves `up`/`down`, which the input binds for the caret. A binding wins
//! by the *depth* in the focus stack at which its predicate holds, and only
//! then by being registered later; a predicate written `A > B` is scored at
//! `B`'s depth, so `ChatComposer > Input` ties with the input's own `Input` and
//! the tie goes to the app, which binds after the library. The card claims
//! `ChatComposer` **only while a list is open**, so with nothing to walk the
//! keys go back to moving the caret.
//!
//! `Esc` needs none of that: the input propagates an escape it has no use for,
//! and the pane catches the action on its way out.

use super::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Rems, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt,
};
use onehand_core::attachment::{
    AttachmentDelivery, AttachmentKind, AttachmentSource, StagedAttachment,
};
use onehand_core::completion::{self, ActiveTrigger, TriggerKind};

mod presentation;
use presentation::{
    Pick, Row, composer_status, fast_action, fast_rows, mode_action, mode_rows, options_action,
    options_rows, segmented_group,
};

/// Rows drawn in the completion popup. The list scrolls past this; the cap is
/// what keeps a 10 000-file repo from building 10 000 elements (bounded rendering).
const MAX_COMPLETION_ROWS: usize = 50;
/// The least room a list is ever given, however short the panel is.
///
/// Rems, like every other size here: a panel's zoom overrides the rem base for
/// its whole subtree, so a popup measured in pixels is the one thing on screen
/// that does not grow with the text it is completing.
const POPUP_MIN_H: Rems = rems(9.);
/// What a list leaves between itself and the top of the panel.
///
/// A card that grows until it touches the header's rule reads as one that has
/// run out of window rather than as one sized to its contents, and there is
/// nothing above it to say whether anything was cut.
const POPUP_HEADROOM: Rems = rems(1.);

/// How tall a list may grow, given the panel it is opening inside.
///
/// **A cap and not a size.** A list shorter than this draws whole and does not
/// scroll, which is the point: a popup that scrolled with four choices in it
/// hid the fourth behind a gesture nobody needed to make. What the cap is for
/// is the other end — a list must not grow past the panel, where its top rows
/// would be drawn over the header or off the window entirely, with nothing on
/// screen saying so.
///
/// It is the panel's own height and not a constant, because a constant is a
/// guess about a panel that is dragged: fifteen rems was most of a short pane
/// and a third of a tall one, so the same list scrolled on a maximized window
/// with room to spare beneath it.
///
/// The floor is what is left when the panel is too short for any of that: a
/// squeezed pane scrolls its list, which is honest, but it is never reduced to
/// one row and a scrollbar.
pub fn popup_room(panel: gpui::Pixels, reserved: gpui::Pixels, rem: gpui::Pixels) -> gpui::Pixels {
    (panel - reserved - POPUP_HEADROOM.to_pixels(rem)).max(POPUP_MIN_H.to_pixels(rem))
}
/// How much room either option action may take before its current value truncates.
const OPTION_MAX_W: Rems = rems(9.);
/// How much of an attachment's name is shown before it truncates.
const ATTACHMENT_MAX_W: Rems = rems(10.);
/// Attachment chips drawn before the tray starts counting instead.
const MAX_TRAY_CHIPS: usize = 12;
/// Rows built at once in the expanded attachment manager. Removing a visible
/// row reveals the next one, so every item remains manageable without laying
/// out a dropped directory's entire contents on each frame.
const MAX_ATTACHMENT_MANAGER_ROWS: usize = 100;
/// The size the composer's own controls are lettered at.
///
/// The smallest named reading size. These controls should remain quieter than
/// the prompt without falling below the rest of the app's type ladder. It is
/// still a rem, so panel zoom carries it like everything else.
pub(super) const CHIP_TEXT: Rems = rems(0.75);
/// How tall every control in the composer's row stands, and every row of the
/// list a control opens.
///
/// Fixed, because otherwise the *content* decides it and the content is not the
/// same shape: a chip with a word in it is as tall as that word's line box
/// (`text_xs` times gpui's default leading, about 1.21rem), while a chip
/// holding only an icon is as tall as the icon (0.75rem). Left to themselves
/// they came out about seven pixels apart on the same row.
///
/// The value is the floor a pointer target can stand at and still be aimed for
/// without care, and it keeps the metadata row subordinate to the prompt. It
/// does not go lower: what makes this row compact is the card's inset and the
/// gaps, which cost nothing to give back, where the target does not.
///
/// **The popup's rows take it too**, so the choices behind a chip stand as tall
/// as the chip. They are library buttons, and a button nobody gives a size to
/// takes the library's default of 2rem — which is a step above everything in
/// the row that opened it, chosen by nobody and noticed only once a selector's
/// list stopped being as wide as the reading column. The two notice rows in
/// that list are plain text and take it as well, or a list saying it has
/// nothing stands taller than the same list saying anything.
pub(super) const CHIP_H: Rems = rems(1.5);
/// How big the glyph in an icon-only composer action is drawn.
///
/// A step up from the 0.75rem the row's *text* is set at, and deliberately not
/// the same number. A word is read, so it can be small and still legible; a
/// glyph is aimed at and recognised by its shape, and at the text's own size
/// the `+` was a faint mark the eye had to hunt for beside a chip carrying a
/// whole model name.
///
/// It stays **under the chip's height** with room either side, so this is the
/// glyph growing inside a target that does not move: the row's height is set by
/// `CHIP_H` and every control in it stands at that whether it holds a word or a
/// drawing. The last step this can take before the two numbers meet and the
/// icon starts deciding the row.
const ACTION_ICON: Rems = rems(1.25);

/// What is showing above the composer. Mutually exclusive **by construction**:
/// one `Option` makes that structural, where a flag per overlay needs a
/// "close the others" call on every path that opens one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// The `@`/`/` candidate list.
    Completion,
    /// The session mode's choices.
    Mode,
    /// Model, effort, and every other agent-advertised config choice in one
    /// directly selectable list.
    Options,
    /// The one config group given a chip of its own on the strip below the
    /// card. A picker like the two above and not a switch: two rows name both
    /// values and tick the one in force, where a switch shows a position and
    /// leaves the reader to work out which way round it is -- and the agent's
    /// own sentence about each value, which is where a setting that refuses to
    /// stay put says why, has somewhere to go.
    Fast,
    /// All staged attachments, including the entries hidden by the compact
    /// tray's rendering bound.
    Attachments,
}

/// The rows of a settings list, and `None` for an overlay that is not one.
///
/// **One answer, because three places ask it.** Opening a list seeds the
/// highlight from its rows, walking one is bounded by how many it has, and
/// drawing it needs the rows themselves — written out per site, the three had
/// already begun to differ in which overlays they recognised, and a list whose
/// count comes from one rule and whose rows come from another highlights a row
/// that is not there. The match is exhaustive on purpose: a fourth overlay is a
/// decision about all three at once, so it should not compile until it is made.
fn picker_rows(overlay: &Overlay, session: &Entity<ChatSession>, cx: &App) -> Option<Vec<Row>> {
    match overlay {
        Overlay::Mode => Some(mode_rows(session, cx)),
        Overlay::Options => Some(options_rows(session, cx)),
        Overlay::Fast => Some(fast_rows(session, cx)),
        Overlay::Completion | Overlay::Attachments => None,
    }
}

/// Everything the user has composed and not sent: the prompt text and whatever
/// is staged to go with it.
///
/// Lifted out of the composer so it can be set aside per session. The composer
/// itself is one widget shared by the whole pane, and what is typed into it
/// belongs to the conversation that was on screen at the time -- an attachment
/// staged from one project's tree is an absolute path the *next* project's agent
/// has no business being handed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Draft {
    text: String,
    attachments: Vec<StagedAttachment>,
}

impl Draft {
    /// Nothing typed and nothing staged, so there is nothing to set aside.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.attachments.is_empty()
    }
}

/// Which row the highlight is actually on, given how many rows there are.
///
/// The stored index is made against one candidate list and read against
/// another: typing narrows the list, and the agent may advertise files while a
/// popup is open. Left unclamped, an index past the end draws no highlight and
/// -- because Enter accepts *the highlighted row* and falls through to Send
/// when there is none -- turns the next Enter into a prompt sent with the
/// half-typed trigger still in it.
fn highlight(selected: usize, rows: usize) -> Option<usize> {
    (rows > 0).then(|| selected.min(rows - 1))
}

/// A candidate path split into the part that is read first and the part that
/// tells two of the same name apart.
///
/// The row has one line and paths are longer than it. Printed whole and
/// truncated, what goes missing is the tail -- which is the filename, the one
/// piece of the path the query was typed against. Leading with the name and
/// letting the folder be the part that gets cut keeps the row answering the
/// question it was opened to answer.
fn split_path(candidate: &str) -> (&str, Option<&str>) {
    match candidate.rsplit_once('/') {
        Some((parent, name)) if !name.is_empty() => (name, Some(parent)),
        _ => (candidate, None),
    }
}

/// Where a trigger typed from the toolbar belongs in the buffer.
#[derive(Debug, PartialEq, Eq)]
enum TriggerSpot {
    /// Put the character in at this byte offset.
    Insert(usize),
    /// One is already there; only the caret moves, to this offset.
    Reuse(usize),
}

/// Decide where the toolbar's `@` or `/` goes.
///
/// A **mention** is positional: it names a file at the point in the sentence
/// where it is written, so it goes at the caret.
///
/// A **command** is not. A prompt is one message and the adapter reads a
/// command off the front of it, which is why a `/` anywhere else is not a
/// trigger at all -- `src/main.rs` and `and/or` must stay prose. So the
/// toolbar's slash goes to the **front of the buffer** wherever the caret was:
/// pressed with a sentence already typed, it used to drop a slash in the middle
/// of it, open nothing, and leave the user with a stray character and no way to
/// tell what had gone wrong. At the front it opens the list, and whatever was
/// already written stays put as the command's argument.
///
/// A buffer already beginning with `/` gets no second one -- the caret just
/// moves in behind it, which is where the query is typed.
fn trigger_spot(ch: char, text: &str, caret: usize) -> TriggerSpot {
    if ch != '/' {
        return TriggerSpot::Insert(caret.min(text.len()));
    }
    match text.starts_with('/') {
        true => TriggerSpot::Reuse(1),
        false => TriggerSpot::Insert(0),
    }
}

/// What the composer asks its owner for, because it has no business doing it
/// itself: the pane knows which conversation is on screen and whether a turn is
/// running, and the composer only knows the button was pressed.
///
/// One variant, because Send and Stop are one control at two moments and the
/// decision between them belongs to whoever can still see the turn — pressed a
/// frame after it ended, the button that said Stop must not cancel anything.
pub enum ComposerEvent {
    SendPressed,
    /// Stop and Send/Queue are one button at two moments of a running turn,
    /// but they stay two *events*: which of them a press meant is decided
    /// where the composer's contents are known, and a single variant would
    /// leave the pane to work it out again from a draft that may have changed
    /// in the meantime.
    StopPressed,
    /// Show a staged file, so the one of three files called `main.rs` that is
    /// actually attached can be checked before the prompt goes. Asked for
    /// rather than done here: which dock the Workbench lives in and whether it
    /// has to be opened first are the shell's business, not the composer's.
    OpenFile(std::path::PathBuf),
}

impl gpui::EventEmitter<ComposerEvent> for Composer {}

pub struct Composer {
    pub state: Entity<TextareaState>,
    /// The live `@`/`/` trigger, recomputed on every edit.
    trigger: Option<ActiveTrigger>,
    /// The open picker, if any.
    overlay: Option<Overlay>,
    /// Row highlighted in the popup. Reset whenever the trigger changes so a
    /// stale index cannot survive into a different candidate list.
    selected: usize,
    /// Files staged with 📎, sent with the next prompt.
    pub attachments: Vec<StagedAttachment>,
    /// The popup's scroll, so the highlight can be kept on screen.
    rows_scroll: gpui::ScrollHandle,
    /// A recoverable composer-side failure that has no chat-model blocker of
    /// its own, such as failing to persist an image from the clipboard.
    feedback: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl Composer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                // Enter is the composer's own key, not the buffer's.
                //
                // A textarea's default is that Enter inserts a newline and
                // *then* announces itself, which broke both halves of the
                // gesture: the newline arrived first, its change recomputed the
                // trigger, and the popup was already closed by the time the
                // announcement reached the pane -- so Enter could never take a
                // candidate or an option, only send. Shift+Enter still writes
                // the newline.
                .submit_on_enter(true)
                // What the two trigger buttons under the field already say, in
                // the character each of them draws and the tooltip each of them
                // carries. Spelled out here as well it was the longest string
                // in the pane, and on a narrow panel it truncated into half a
                // sentence of instructions -- so the field spent its one line
                // saying something incompletely that the row below says whole.
                .placeholder("Ask the agent…")
        });

        let subscription = cx.subscribe(
            &state,
            |composer: &mut Self, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let caret = state.read(cx).cursor();
                    composer.retrigger(&text, caret, cx);
                }
            },
        );

        Self {
            state,
            trigger: None,
            overlay: None,
            selected: 0,
            attachments: Vec::new(),
            rows_scroll: gpui::ScrollHandle::new(),
            feedback: None,
            _subscriptions: vec![subscription],
        }
    }

    pub fn text(&self, cx: &App) -> String {
        self.state.read(cx).value().to_string()
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.trigger = None;
        self.overlay = None;
        self.selected = 0;
        self.attachments.clear();
        self.feedback = None;
    }

    /// Lift out what is unsent and leave the composer empty.
    ///
    /// The popup and its selection go too, via [`Self::clear`]: a candidate list
    /// is computed against one session's files and commands, so carrying it to
    /// the next one would offer paths that are not there.
    pub fn take_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Draft {
        let draft = Draft {
            text: self.text(cx),
            attachments: self.attachments.clone(),
        };
        self.clear(window, cx);
        draft
    }

    /// Put a set-aside draft back.
    pub fn restore_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.set_value(&draft.text, window, cx));
        self.attachments = draft.attachments;
        cx.notify();
    }

    /// Put a cancelled queued prompt back where it was written.
    ///
    /// It goes *in front of* whatever is in the composer now rather than over
    /// it: the turn it was queued behind can take minutes, and something else
    /// typed in the meantime is no less the user's than the prompt coming back.
    pub fn restore_queued(
        &mut self,
        queued: onehand_core::chat::QueuedPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.text(cx);
        let text = if current.trim().is_empty() {
            queued.text
        } else {
            format!("{}\n{current}", queued.text)
        };
        self.state
            .update(cx, |state, cx| state.set_value(&text, window, cx));
        let mut attachments = queued.attachments;
        attachments.append(&mut self.attachments);
        self.attachments = attachments;
        cx.notify();
    }

    /// Take whichever row the open list has highlighted, if any.
    ///
    /// What Enter means, in one place for both lists. Split between them, the
    /// arrow keys walked a selector's choices while Enter sent the prompt
    /// anyway -- a list that can be moved through but not committed reads as
    /// broken rather than as one Enter has no opinion about.
    pub fn commit(
        &mut self,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.overlay.clone() {
            None => false,
            Some(Overlay::Completion) => self.accept(session, window, cx),
            Some(Overlay::Fast) => {
                let rows = fast_rows(session, cx);
                let Some(row) =
                    highlight(self.selected, rows.len()).and_then(|row| rows.into_iter().nth(row))
                else {
                    self.close_overlay(cx);
                    return true;
                };
                self.apply_pick(&row.pick, session, window, cx)
            }
            Some(Overlay::Mode) => {
                let rows = mode_rows(session, cx);
                let Some(row) =
                    highlight(self.selected, rows.len()).and_then(|row| rows.into_iter().nth(row))
                else {
                    self.close_overlay(cx);
                    return true;
                };
                self.apply_pick(&row.pick, session, window, cx)
            }
            Some(Overlay::Options) => {
                let rows = options_rows(session, cx);
                let Some(row) =
                    highlight(self.selected, rows.len()).and_then(|row| rows.into_iter().nth(row))
                else {
                    self.close_overlay(cx);
                    return true;
                };
                self.apply_pick(&row.pick, session, window, cx)
            }
            Some(Overlay::Attachments) => {
                self.close_overlay(cx);
                true
            }
        }
    }

    /// Do what a row was for, whether it was clicked or committed with Enter.
    fn apply_pick(
        &mut self,
        pick: &Pick,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Whatever the pick was, the caret comes back to the prompt: a click
        // lands on a plain div, which lets the pane take focus, and the next
        // thing the user does is keep writing the message.
        self.state.update(cx, |state, cx| state.focus(window, cx));
        match pick {
            Pick::Complete => self.accept(session, window, cx),
            Pick::Mode(id) => {
                let id = id.clone();
                session.update(cx, |session, cx| {
                    session.chat.set_mode(&id);
                    cx.notify();
                });
                self.close_overlay(cx);
                true
            }
            Pick::Config { config_id, value } => {
                let (config_id, value) = (config_id.clone(), value.clone());
                session.update(cx, |session, cx| {
                    session.chat.set_config_option(&config_id, &value);
                    cx.notify();
                });
                self.close_overlay(cx);
                true
            }
        }
    }

    /// Open one of the two option lists, or close it if it is already open.
    ///
    /// **Focus goes to the prompt field, not to the chip.** The chip is a plain
    /// div, so the click that opened the list travels up to the pane, which
    /// takes focus -- and the keys that walk the list only reach it while focus
    /// is in an input inside the composer. Without this the arrows did nothing
    /// at all on a list opened with the mouse, which is every list of the
    /// agent's own options.
    ///
    /// The list opens on the first value already in force. The Options popup is
    /// intentionally flat: model, effort and remaining agent options are all
    /// visible and directly selectable without entering another screen.
    fn toggle_picker(
        &mut self,
        target: Overlay,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rows) = picker_rows(&target, session, cx) else {
            return;
        };
        self.selected = rows.iter().position(|row| row.checked).unwrap_or(0);
        self.overlay = (self.overlay.as_ref() != Some(&target)).then_some(target);
        self.reveal_selected();
        self.state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn toggle_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = (self.overlay != Some(Overlay::Attachments)).then_some(Overlay::Attachments);
        self.selected = 0;
        self.state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// Keep the highlighted row on screen.
    ///
    /// A list taller than its box scrolls, and the highlight is the only thing
    /// saying what Enter takes -- walked past the fold it left the user pressing
    /// a key with nothing on screen changing.
    fn reveal_selected(&self) {
        self.rows_scroll.scroll_to_item(self.selected);
    }

    /// Recompute the active trigger after an edit.
    fn retrigger(&mut self, text: &str, caret: usize, cx: &mut Context<Self>) {
        let next = completion::detect(text, caret);
        // The selection belongs to a candidate list, so it only survives while
        // the trigger it was made against does -- *including* the query, since
        // one more typed letter refilters the list and leaves the same index
        // pointing at a different file.
        if next != self.trigger {
            self.selected = 0;
            // A list opening on top of another one's scroll offset shows its
            // middle, with the highlighted first row above the fold.
            self.reveal_selected();
        }
        self.trigger = next;
        // Typing dismisses a selector: the user has moved on to the prompt.
        self.overlay = self.trigger.is_some().then_some(Overlay::Completion);
        cx.notify();
    }

    /// Candidates for the live trigger, drawn from the session the agent
    /// advertised them on.
    fn candidates(&self, session: &Entity<ChatSession>, cx: &App) -> Vec<SharedString> {
        self.matches(session, cx).0
    }

    /// The candidates that will be drawn, and how many matched in total.
    ///
    /// Both, because the list is capped and a capped list has to say so. Cut to
    /// fifty rows with nothing admitting it, a query that matched four hundred
    /// files reads as one that matched fifty -- and the file the user is
    /// looking for is missing for no visible reason.
    fn matches(&self, session: &Entity<ChatSession>, cx: &App) -> (Vec<SharedString>, usize) {
        let Some(trigger) = &self.trigger else {
            return (Vec::new(), 0);
        };
        let chat = &session.read(cx).chat;
        let pool: Vec<String> = match trigger.kind {
            TriggerKind::File => chat.files.clone(),
            TriggerKind::Command => chat.commands.iter().map(|c| c.name.clone()).collect(),
        };
        let matched = completion::filter(&pool, &trigger.query);
        let total = matched.len();
        (
            matched
                .into_iter()
                .take(MAX_COMPLETION_ROWS)
                .map(|c| SharedString::from(c.clone()))
                .collect(),
            total,
        )
    }

    /// How many rows the open list has, which is what walking it is bounded by.
    fn row_count(&self, session: &Entity<ChatSession>, cx: &App) -> usize {
        match &self.overlay {
            None => 0,
            Some(Overlay::Completion) => self.candidates(session, cx).len(),
            // The tray draws its own list and the arrows do not walk it.
            Some(Overlay::Attachments) => 0,
            Some(picker) => picker_rows(picker, session, cx).map_or(0, |rows| rows.len()),
        }
    }

    /// Move the highlight by one row, wrapping at both ends.
    ///
    /// Wrapping because the list is short and the keys are held: stopping dead
    /// at the last row makes the user let go and reach for the other arrow to
    /// get back to a match they passed.
    fn step(&mut self, delta: isize, session: &Entity<ChatSession>, cx: &mut Context<Self>) {
        let rows = self.row_count(session, cx);
        let Some(from) = highlight(self.selected, rows) else {
            return;
        };
        let rows = rows as isize;
        self.selected = ((from as isize + delta).rem_euclid(rows)) as usize;
        self.reveal_selected();
        cx.notify();
    }

    /// Accept the highlighted candidate, rewriting the buffer around the
    /// trigger. Returns whether anything was accepted.
    pub fn accept(
        &mut self,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let candidates = self.candidates(session, cx);
        let Some(choice) =
            highlight(self.selected, candidates.len()).and_then(|row| candidates.get(row).cloned())
        else {
            return false;
        };
        let Some(trigger) = self.trigger.clone() else {
            return false;
        };

        let text = self.text(cx);
        let caret = self.state.read(cx).cursor();
        let (next, next_caret) = completion::apply(&text, caret, &trigger, &choice);

        self.state.update(cx, |state, cx| {
            state.set_value(next, window, cx);
            // An empty range is a caret: `completion::apply` returns where the
            // caret belongs after the rewrite, which is past the inserted
            // value, not at the end of the buffer.
            state.set_selected_range(next_caret..next_caret, cx);
        });
        self.trigger = None;
        self.overlay = None;
        self.selected = 0;
        cx.notify();
        true
    }

    /// Type a trigger character into the buffer from code.
    ///
    /// This exists because of an input-method bug, not as a convenience. With a
    /// Vietnamese IME active on Linux, a typed `/` can be swallowed before it
    /// ever reaches the composer — so the slash-command popup cannot be opened
    /// by typing at all, and `@` is unreliable for the same reason. Inserting
    /// the character here bypasses the IME entirely.
    ///
    /// `retrigger` is called explicitly rather than trusting the `Change` event
    /// to arrive: the whole point is to open the popup, and a button that
    /// inserts an `@` without opening anything is the bug wearing a different
    /// hat.
    fn insert_trigger(&mut self, ch: char, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let caret = self.state.read(cx).cursor().min(text.len());
        let (next, after) = match trigger_spot(ch, &text, caret) {
            TriggerSpot::Reuse(after) => (text.clone(), after),
            TriggerSpot::Insert(at) => {
                let mut next = String::with_capacity(text.len() + ch.len_utf8());
                next.push_str(&text[..at]);
                next.push(ch);
                next.push_str(&text[at..]);
                (next, at + ch.len_utf8())
            }
        };

        self.state.update(cx, |state, cx| {
            state.set_value(next.clone(), window, cx);
            state.set_selected_range(after..after, cx);
            // Focus goes back to the buffer: the click that inserted the
            // trigger took it, and the next thing the user does is type the
            // query after it.
            state.focus(window, cx);
        });
        self.retrigger(&next, after, cx);
    }

    /// The card: the attachment tray, the field, and the row of controls under
    /// it. One card holding the text and everything done to it — a rule across
    /// the pane instead would say the composer is the bottom of the window; it
    /// is the message being written, and a message has edges.
    ///
    /// Drawn here rather than by the pane that mounts it. What the composer is
    /// made of is the composer's own business, and split across two files it
    /// was: the popup and the tray here, the field and the controls they belong
    /// to over there. The pane still decides *where* the card goes and how much
    /// room it takes out of the conversation.
    ///
    /// `typing_here` is passed rather than measured, because focus is a
    /// question about a window and the caller is holding one.
    pub fn card(
        &mut self,
        session: &Entity<ChatSession>,
        blocked: Option<onehand_core::chat::SubmitBlock>,
        typing_here: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (step_down, step_up, take_row) = (session.clone(), session.clone(), session.clone());
        let tray = self.tray(cx).map(IntoElement::into_any_element);
        let options = options_action(session, cx);
        let options_open = self.overlay == Some(Overlay::Options);
        let fast_open = self.overlay == Some(Overlay::Fast);
        let fast = fast_action(session, cx)
            .map(|label| status_action(Overlay::Fast, label, fast_open, session, cx));
        let options_control = options.map(|label| {
            option_action("model-selector", label, options_open, session, cx).into_any_element()
        });

        div()
            .v_flex()
            // The card's own inset, and the gap between the prompt and the row
            // of controls under it. Both a step tighter than the app's ordinary
            // panel inset, because this card is not a panel: it holds exactly
            // two things, and every rem of air around them is taken off the
            // conversation above rather than out of unused space. What the
            // inset still has to do is keep the caret and the controls clear of
            // the border the focus ring is drawn on, which one step does.
            .gap_1()
            .w_full()
            .p_1p5()
            // A floating input needs its own opaque elevation: using the
            // reading surface here makes transcript content behind it visually
            // bleed into the card.
            .bg(cx.theme().popover.alpha(1.))
            .shadow_lg()
            // `radius_lg`: the theme's named step for a card, which is what the
            // Workbench and the terminal round their own dock cards at. This is
            // a card in the same sense — a bordered surface floating on the
            // reading one — so it takes the value rather than a second answer
            // arithmetic on `radius` happened to land near. The doubled step it
            // had is the transcript's, for a block the width of the reading
            // column, and one window drawing its floating surfaces at two
            // different corners is a difference nobody chose.
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(if typing_here {
                cx.theme().ring
            } else {
                cx.theme().border
            })
            // Held always, so `Ctrl+V` can be taken from the input wherever the
            // caret is in the prompt. The list context below is the one that
            // comes and goes.
            .key_context("ChatComposerCard")
            .on_action(cx.listener(
                |composer: &mut Self, _: &crate::shell::PasteHere, window, cx| {
                    composer.paste(window, cx);
                },
            ))
            // A file dragged onto the message being written is being offered to
            // the agent, and the card is what the user aims at. The whole card
            // takes it rather than the tray, which is not there yet the first
            // time and is exactly where a first attachment cannot be dropped.
            .on_drop(cx.listener(
                |composer: &mut Self, dropped: &gpui::ExternalPaths, _, cx| {
                    composer.stage(dropped.paths().to_vec(), AttachmentSource::Picker, cx);
                },
            ))
            .drag_over::<gpui::ExternalPaths>(move |card, _, _, cx| {
                // The same edge the caret lights, because it answers the same
                // question -- whether letting go now puts the file here.
                card.border_color(cx.theme().ring)
            })
            .children(tray)
            // The card is the border, so the field inside it draws none: two
            // rings around one input read as two inputs.
            .child(
                div()
                    // Claimed only while a list is open, because this is what
                    // takes the arrow keys away from the caret: with nothing to
                    // walk, the context is gone and the input's own bindings
                    // win again.
                    .when(self.overlay_open(), |field| {
                        field.key_context("ChatComposer")
                    })
                    .on_action(cx.listener(
                        move |composer: &mut Self, _: &crate::shell::CompletionNext, _, cx| {
                            composer.step(1, &step_down, cx);
                        },
                    ))
                    .on_action(cx.listener(
                        move |composer: &mut Self, _: &crate::shell::CompletionPrev, _, cx| {
                            composer.step(-1, &step_up, cx);
                        },
                    ))
                    // Tab takes the highlighted row, through the same call
                    // Enter makes. Two keys meaning one thing is the point:
                    // what they must not become is two answers to "what is
                    // highlighted for", which is what a second accept path
                    // written out here would drift into. The key is claimed
                    // only while a list is open, so there is always a row for
                    // it to be about.
                    .on_action(cx.listener(
                        move |composer: &mut Self,
                              _: &crate::shell::CompletionAccept,
                              window,
                              cx| {
                            composer.commit(&take_row, window, cx);
                        },
                    ))
                    .child(Textarea::new(&self.state).appearance(false)),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    // What goes *into* the prompt, at the start of the row: the
                    // two ways a file joins it, the slash commands, and what
                    // will answer it. The right-hand end is the turn itself.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(add_menu(cx))
                            // Ahead of the model rather than after it, because
                            // it is read as a qualifier of the name beside it --
                            // and because the model chip is the one thing on
                            // this row that truncates, so anything standing
                            // after it would be the thing pushed off the end.
                            .children(fast)
                            .children(options_control),
                    )
                    .child(
                        div().h_flex().items_center().gap_2().child(
                            send_controls(
                                blocked.clone(),
                                !self.text(cx).trim().is_empty() || !self.attachments.is_empty(),
                                cx.listener(|_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::SendPressed);
                                }),
                                cx.listener(|_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::StopPressed);
                                }),
                            )
                            .flex_none(),
                        ),
                    ),
            )
            .children(composer_status(blocked, cx).map(IntoElement::into_any_element))
            .children(self.feedback.clone().map(|message| {
                div()
                    .h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(crate::theme::status_ink(cx).danger)
                    .child(Icon::new(IconName::Info).size_3())
                    .child(message)
                    .into_any_element()
            }))
    }

    /// The bare strip under the card: what the project is, and what the turn
    /// will be taken under.
    ///
    /// **Outside the card, and with no chrome of its own.** The card is the
    /// message being written and everything inside it acts on that message;
    /// these two facts do not. The branch is about the project and holds across
    /// every session in it, and the two settings here outlive the prompt in the
    /// field — so a strip resting under the card says "this is the standing
    /// state" where a fourth control inside the row would have said they were
    /// part of what is being typed.
    ///
    /// **Left is the project, right is the turn.** The branch is about the
    /// repository the whole window is on; the permission mode is about the
    /// prompt about to be sent. Which side a thing is on is the whole of what
    /// says which kind it is.
    ///
    /// Both sides are pressable. The branch is a control and not a label,
    /// because everything a reader might want to do about the branch they are
    /// reading — switch it, rename it, take it to a worktree — is a thing they
    /// have to leave for the rail to reach otherwise, and a word that answers
    /// "which branch" while refusing "and now what" is the one place in this
    /// row that stops short.
    ///
    /// Nothing at all where there is neither: on a project that is not a
    /// repository, with an agent advertising no modes, an empty rule of blank
    /// space under the composer would be chrome reporting that it has nothing
    /// to report.
    pub fn status_row(
        &mut self,
        session: &Entity<ChatSession>,
        git: Option<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let mode_open = self.overlay == Some(Overlay::Mode);
        let mode = mode_action(session, cx).map(|label| {
            status_action(Overlay::Mode, label, mode_open, session, cx).into_any_element()
        });
        if git.is_none() && mode.is_none() {
            return None;
        }

        Some(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                // The card's own inset, so the branch lines up with the `+`
                // above it and the controls with Send -- the strip is under the
                // card rather than in it, and a column that did not line up
                // would read as a second panel that had slipped.
                .px_1p5()
                .pt_1p5()
                // Built by the pane and dropped in here: git is the project's,
                // and the panel that talks to the shell about the project is
                // the one that can offer to do anything about it. The composer
                // decides only where on the row it goes.
                .child(div().h_flex().items_center().min_w_0().children(git))
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .flex_none()
                        .children(mode),
                ),
        )
    }

    /// Stage paths that arrived from somewhere other than the picker.
    fn stage(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        source: AttachmentSource,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        self.feedback = None;
        self.attachments.extend(
            paths
                .into_iter()
                .map(|path| StagedAttachment::inspect(path, source)),
        );
        cx.notify();
    }

    /// What `Ctrl+V` does in the composer.
    ///
    /// The clipboard holds *entries*, and only one kind of them is text. An
    /// image copied out of a screenshot tool and a file copied out of a file
    /// manager both arrive here, and both are things to attach rather than
    /// things to type — pasted into the buffer they produced nothing at all,
    /// which is a paste that looks broken.
    ///
    /// Anything else is the input's own business and is handed straight back to
    /// it, so ordinary text paste keeps working exactly as it did, undo history
    /// and all.
    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mine = cx.read_from_clipboard().is_some_and(|item| {
            let mut mine = false;
            for entry in item.into_entries() {
                match entry {
                    gpui::ClipboardEntry::Image(image) => {
                        mine = true;
                        self.stage_pasted_image(image, cx);
                    }
                    gpui::ClipboardEntry::ExternalPaths(paths) => {
                        mine = true;
                        self.stage(paths.paths().to_vec(), AttachmentSource::Clipboard, cx);
                    }
                    gpui::ClipboardEntry::String(_) => {}
                }
            }
            mine
        });
        if !mine {
            window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
        }
    }

    /// Write a pasted image out and stage the file it became.
    ///
    /// The write is a real one and goes to the background executor; the id is
    /// the clipboard's own content hash, so pasting the same image twice
    /// rewrites one file instead of littering the temp directory.
    fn stage_pasted_image(&mut self, image: gpui::Image, cx: &mut Context<Self>) {
        cx.spawn(async move |composer, cx| {
            let written = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::attachment::write_clipboard_image(
                        image.id,
                        image.format.extension(),
                        &image.bytes,
                    )
                })
                .await;
            let Ok(path) = written else {
                let _ = composer.update(cx, |composer: &mut Self, cx| {
                    composer.feedback = Some("Could not attach the pasted image".into());
                    cx.notify();
                });
                return;
            };
            let _ = composer.update(cx, |composer: &mut Self, cx| {
                composer.stage(vec![path], AttachmentSource::Clipboard, cx);
            });
        })
        .detach();
    }

    /// Stage files through the native picker.
    ///
    /// Off the UI loop, like every other dialog in the app -- `pick_files` blocks
    /// until the user is done, which on this thread would freeze the window.
    fn attach(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |composer, cx| {
            let picked = cx
                .background_executor()
                .spawn(async { rfd::FileDialog::new().pick_files() })
                .await;
            let Some(paths) = picked else {
                return;
            };
            let _ = composer.update(cx, |composer: &mut Self, cx| {
                composer.feedback = None;
                composer.attachments.extend(
                    paths
                        .into_iter()
                        .map(|path| StagedAttachment::inspect(path, AttachmentSource::Picker)),
                );
                cx.notify();
            });
        })
        .detach();
    }

    fn unstage(&mut self, id: onehand_core::attachment::AttachmentId, cx: &mut Context<Self>) {
        self.attachments.retain(|a| a.id != id);
        if self.attachments.is_empty() && self.overlay == Some(Overlay::Attachments) {
            self.overlay = None;
        }
        cx.notify();
    }

    /// The staged files, as a horizontally scrolling tray.
    ///
    /// Bounded like everything else that grows with what the user did: a folder
    /// dropped on the card is however many files it held, and a tray of two
    /// hundred chips is two hundred elements laid out on every keystroke. What
    /// is over the bound is counted rather than dropped silently.
    ///
    /// **A chip names a staged file; it does not show it.** The picture is
    /// previewed once the prompt is sent, in the transcript, where the
    /// attachment is a block of the conversation rather than a strip along the
    /// top of the card holding what is being typed — a tray of thumbnails takes
    /// that room from the prompt itself, and it is the prompt the card is for.
    fn tray(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.attachments.is_empty() {
            return None;
        }
        let (border, muted, danger_border, danger_text, radius) = (
            cx.theme().border,
            cx.theme().muted_foreground,
            cx.theme().danger,
            crate::theme::status_ink(cx).danger,
            cx.theme().radius,
        );
        let over = self.attachments.len().saturating_sub(MAX_TRAY_CHIPS);

        Some(
            div()
                .id("attachments")
                .h_flex()
                .gap_2()
                .w_full()
                .overflow_x_scroll()
                .children(
                    self.attachments
                        .iter()
                        .take(MAX_TRAY_CHIPS)
                        .enumerate()
                        .map(|(i, a)| {
                            let id = a.id;
                            // An unreadable file blocks Send entirely, so it has
                            // to look wrong here rather than fail silently at
                            // the moment the user hits Enter.
                            let unavailable = a.delivery == AttachmentDelivery::Unavailable;
                            let parts = [
                                Icon::new(match a.kind {
                                    AttachmentKind::Image => IconName::Frame,
                                    AttachmentKind::File => IconName::File,
                                })
                                .size_3()
                                .into_any_element(),
                                div()
                                    .max_w(ATTACHMENT_MAX_W)
                                    .truncate()
                                    .when(unavailable, |el| el.text_color(danger_text))
                                    .child(a.name.clone())
                                    .into_any_element(),
                            ];
                            // The size, because two screenshots taken a minute
                            // apart have interchangeable names, and because it
                            // is the only warning that a large image will go as
                            // a link instead of inline.
                            let size = a.bytes.map(|bytes| {
                                div()
                                    .flex_none()
                                    .text_color(muted)
                                    .child(onehand_core::attachment::size_label(bytes))
                                    .into_any_element()
                            });
                            // A real button, not a bare glyph: this one is
                            // small, sits beside the name it destroys, and
                            // needs the hover and the focus ring that say which
                            // of the two the pointer is on.
                            //
                            // It is one clickable inside another wherever the
                            // chip itself opens, which is what the stop is for:
                            // without it the press that unstages a file also
                            // asks the Workbench to open the file just removed.
                            let unstage = crate::controls::action(("unstage", i))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Close))
                                .tooltip("Remove this attachment")
                                .on_click(cx.listener(move |composer: &mut Self, _, _, cx| {
                                    cx.stop_propagation();
                                    composer.unstage(id, cx);
                                }))
                                .into_any_element();

                            match openable(a) {
                                Some(path) => attachment_shape(
                                    crate::controls::action(("attachment", i)).ghost(),
                                    unavailable,
                                    border,
                                    danger_border,
                                    radius,
                                )
                                .children(parts)
                                .children(size)
                                .child(unstage)
                                .tooltip("Open this file in the Workbench")
                                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::OpenFile(path.clone()));
                                }))
                                .into_any_element(),
                                None => attachment_shape(
                                    div(),
                                    unavailable,
                                    border,
                                    danger_border,
                                    radius,
                                )
                                .children(parts)
                                .children(size)
                                .child(unstage)
                                .into_any_element(),
                            }
                        }),
                )
                .when(over > 0, |tray| {
                    tray.child(
                        crate::controls::action("all-attachments")
                            .ghost()
                            .xsmall()
                            .flex_none()
                            .label(format!("View all {}", self.attachments.len()))
                            .tooltip("Review or remove staged attachments")
                            .on_click(cx.listener(|composer: &mut Self, _, window, cx| {
                                composer.toggle_attachments(window, cx);
                            })),
                    )
                }),
        )
    }

    /// Whether a popup is on screen, so Esc and a click elsewhere have
    /// something to dismiss.
    pub fn overlay_open(&self) -> bool {
        self.overlay.is_some()
    }

    /// Whatever is open, as the card that sits above the composer.
    ///
    /// Every overlay comes through here. It is drawn by the pane and not by
    /// [`Self::card`] because it must sit *outside* the box the transcript's
    /// bottom clearance is measured from: measured, that clearance would grow
    /// by the popup's height the moment one opened and shrink again when it
    /// closed, so every `@` typed would shove the conversation up.
    pub fn detached_popup(
        &self,
        session: &Entity<ChatSession>,
        room: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        self.popup(session, room, cx)
    }

    pub fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = None;
        cx.notify();
    }

    /// Put the highlight on a row.
    ///
    /// Guarded, because the pointer moving across a row calls this on every
    /// mouse event it generates and an unguarded notify would redraw the panel
    /// for each one.
    fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.selected == row {
            return;
        }
        self.selected = row;
        cx.notify();
    }

    /// The open completion, settings, or attachment surface.
    fn popup(
        &self,
        session: &Entity<ChatSession>,
        room: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let overlay = self.overlay.clone()?;
        if overlay == Overlay::Attachments {
            return Some(self.attachments_popup(room, cx));
        }
        // Set while the rows are built, because that is the only place both
        // halves of the count are in hand.
        let mut capped = 0usize;
        let rows: Vec<Row> = match &overlay {
            Overlay::Completion => {
                let kind = self.trigger.as_ref().map(|trigger| trigger.kind);
                let (values, total) = self.matches(session, cx);
                capped = total.saturating_sub(values.len());
                let chat = &session.read(cx).chat;
                values
                    .into_iter()
                    .map(|value| {
                        // What the row *says* is not what it inserts. A command
                        // without its description is a name to guess at, and a
                        // path truncated from the right hides the filename the
                        // query was typed against -- which is the one part of
                        // it the user is looking for.
                        let (label, detail) = match kind {
                            Some(TriggerKind::Command) => (
                                value.clone(),
                                chat.commands
                                    .iter()
                                    .find(|command| command.name == value.as_ref())
                                    .map(|command| command.description.trim())
                                    .filter(|description| !description.is_empty())
                                    .map(SharedString::from),
                            ),
                            _ => {
                                let (name, parent) = split_path(&value);
                                (
                                    SharedString::from(name.to_string()),
                                    parent.map(|parent| SharedString::from(parent.to_string())),
                                )
                            }
                        };
                        Row {
                            label,
                            detail,
                            checked: false,
                            pick: Pick::Complete,
                            group: None,
                        }
                    })
                    .collect()
            }
            // Attachments returned above, so what is left is one of the three
            // settings lists and `picker_rows` has them all.
            picker => picker_rows(picker, session, cx).unwrap_or_default(),
        };
        // A trigger that matches nothing still has to say so. Vanishing reads
        // as completion being broken, which is the opposite of the truth: the
        // popup is the only thing on screen that ever confirms the `@` or `/`
        // was understood at all. A selector with no choices has nothing to
        let segments = match overlay {
            Overlay::Options => segmented_group(session, cx),
            _ => None,
        };
        // confirm, so that one stays away -- unless the rail below the list is
        // the whole of what it has, which is an agent offering effort and
        // nothing else.
        if rows.is_empty() && segments.is_none() && overlay != Overlay::Completion {
            return None;
        }
        let selected = highlight(self.selected, rows.len());
        let muted = cx.theme().muted_foreground;

        Some(
            // The surface and the scrolling list are two boxes, and the inset
            // between them belongs to the *surface*.
            //
            // Padding on the scrolling box is inside the box that scrolls, and
            // `scroll_to_item` aligns a row to the container's outer edge --
            // so walking the list with the arrows scrolled the inset away and
            // pinned the highlighted row against the border, which is exactly
            // the state the inset exists to prevent. Held out here, nothing the
            // list does to its own offset can consume it.
            div()
                .v_flex()
                // **Every list takes the reading column**, which is the width of
                // the card it opens over. A file candidate is a path and always
                // needed it; a choice needs it too, now that a row carries the
                // agent's own sentence about what the choice is for. Sized to
                // its own rows instead, that sentence had nowhere to go and the
                // names it belongs to were narrower than the words in them.
                //
                // No cap, because the box this is dropped into is already the
                // column: a flex child shrinks to its parent before it
                // overflows, so the column is the maximum without this naming
                // it.
                .w_full()
                .rounded(cx.theme().radius)
                .border_1()
                // **A hairline is not enough here, and this is the one place
                // that is true.** A floating control is told apart from the
                // transcript by the step its surface takes above it, with the
                // hairline only drawing the corner. But these lists open from a
                // button *inside the composer*, and the composer is floating
                // too -- so the popup lands on a surface of exactly its own
                // colour, the step is zero, and the panel reads as having no
                // background at all rather than as a panel. The edge is the
                // only thing left that can say where one ends, so it is drawn a
                // real step up instead of at hairline strength.
                .border_color(cx.theme().accent)
                // A list that opens over the field it completes is floating, so
                // it takes the floating surface and the shadow that says so.
                .bg(cx.theme().popover.alpha(1.))
                .shadow_lg()
                .p_1()
                .child(
                    div()
                        .id("completion")
                        .v_flex()
                        .w_full()
                        .max_h(room)
                        .overflow_y_scroll()
                        // Held by the composer rather than by the element, so walking
                        // the list with the keys can scroll it: the handle is what
                        // `reveal_selected` reaches the rows through, and an element's
                        // own handle is gone by the time a key arrives.
                        .track_scroll(&self.rows_scroll)
                        .children(rows.into_iter().enumerate().map(|(i, row)| {
                            let session = session.clone();
                            let pick = row.pick.clone();
                            let heading = row.group.clone();
                            // A click is a choice already made, so it takes the
                            // row rather than only pointing at it. The highlight
                            // moves first, so what was clicked is what gets
                            // taken and not whatever the keyboard had left
                            // selected.
                            let take = cx.listener(move |composer: &mut Self, _, window, cx| {
                                composer.select(i, cx);
                                composer.apply_pick(&pick, &session, window, cx);
                            });
                            let highlighted = Some(i) == selected;
                            let body = match overlay == Overlay::Completion {
                                true => candidate_row(i, row, highlighted, muted, cx),
                                false => choice_row(i, row, highlighted, cx),
                            }
                            // **The pointer moves the highlight, exactly as the
                            // arrows do.** One fill means one thing -- the row
                            // about to be taken -- so it cannot be left behind
                            // on the row the keyboard last stood on while the
                            // pointer is somewhere else. It also settles what
                            // `Enter` takes when the mouse has moved since:
                            // what is lit.
                            //
                            // `on_mouse_move` rather than a hover style,
                            // because the library gives a `Button` no hook to
                            // set its own hover fill, and a second fill derived
                            // from a different token an inch below the first is
                            // the drift this list is being flattened to avoid.
                            .on_mouse_move(cx.listener(
                                move |composer: &mut Self, _, _, cx| {
                                    composer.select(i, cx);
                                },
                            ));
                            // The heading rides *inside* the row's own box
                            // rather than beside it in the list, so the index
                            // the arrow keys walk still counts choices and
                            // nothing else -- and so scrolling to a row brings
                            // the heading that introduces it along.
                            div()
                                .v_flex()
                                .w_full()
                                .children(
                                    heading.map(|heading| notice(cx).text_xs().child(heading)),
                                )
                                .child(body.on_click(take))
                        }))
                        .when(selected.is_none(), |list| {
                            list.child(notice(cx).text_sm().child("No matches"))
                        })
                        .when(capped > 0, |list| {
                            list.child(
                                notice(cx)
                                    .text_xs()
                                    .child(format!("{capped} more — keep typing to narrow them")),
                            )
                        })
                        .when(overlay == Overlay::Completion, |list| {
                            list.child(
                                notice(cx)
                                    .text_xs()
                                    .child("↑↓ Navigate · Enter Select · Esc Close"),
                            )
                        }),
                )
                .children(segments.map(|segments| self.segment_rail(segments, session, cx))),
        )
    }

    /// A config group drawn as one rail of segments at the foot of the list.
    ///
    /// **A rail and not rows**, because effort is the one setting here whose
    /// values are a ladder — less of a thing, then more of it — and three or
    /// four words on one line say that, where a column of rows says only that
    /// there are four of them.
    ///
    /// **Below the list and outside the scroll**, because it is not one of the
    /// choices being scrolled through: it is a second setting, and a control
    /// that scrolls away while the list above it is walked is one the reader
    /// has to go looking for. The rule above it is what says the two are
    /// different questions.
    ///
    /// **It does not close the popup**, unlike every row here, and that is the
    /// difference between picking from a list and nudging a control. A row that
    /// stayed open after being taken would leave the reader wondering whether
    /// the click landed; a segment lights where it was pressed and says so
    /// itself, and the next thing somebody does with a ladder is often try the
    /// rung beside it.
    ///
    /// **The keyboard does not reach it.** The arrow keys walk the list and
    /// `Enter` takes a row, both counting in an index this rail is not part of;
    /// wiring it in means a second key model for a control holding several
    /// values on one line.
    fn segment_rail(
        &self,
        segments: presentation::Segments,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let presentation::Segments {
            name,
            config_id,
            choices,
            current,
        } = segments;
        let session = session.clone();
        let values: Vec<String> = choices.iter().map(|(_, value)| value.clone()).collect();
        let labels: Vec<SharedString> = choices.into_iter().map(|(label, _)| label).collect();
        let (fill, ink, radius) = (
            cx.theme().accent,
            cx.theme().accent_foreground,
            cx.theme().radius,
        );

        // **Not a `ButtonGroup`**, which is the component for joining *bordered*
        // buttons into one segmented block -- it hands each child the corners
        // and edges of its place in the row, so the two ends round outward and
        // everything between them stays square. That is exactly right for a
        // joined block and exactly wrong here: flat, the only thing drawn is the
        // fill under the rung in force, and a fill square on two sides reads as
        // a rectangle laid over the words rather than as a rounded chip around
        // one. A plain row of buttons has no such opinion, and it costs less
        // than the component did -- each rung carries its own press instead of
        // the group reporting an index for the row to look up.
        let rail = div().h_flex().items_center().gap_1().flex_none().children(
            labels.into_iter().enumerate().map(|(i, label)| {
                let current = Some(i) == current;
                let value = values.get(i).cloned().unwrap_or_default();
                let config_id = config_id.clone();
                let session = session.clone();
                // Through the app's own wrapper, so a segment answers the
                // pointer like every other control here -- the library's
                // buttons draw the arrow, and a rail of six of them is six
                // places for that to show.
                crate::controls::action(("segment", i))
                    .ghost()
                    // The list's own size, so the rail letters at the step the
                    // rows above it do. A step under that and it read as a
                    // footnote on the list rather than as a setting beside it.
                    .small()
                    .h(CHIP_H)
                    .px_2()
                    .rounded(radius)
                    .label(label)
                    .selected(current)
                    // The popup's one spelling for "in force", which is the
                    // same one the rows above use. The library's own selected
                    // fill for a ghost button is derived from a different
                    // token, so the two would disagree an inch apart about what
                    // being selected looks like.
                    .when(current, |segment| segment.bg(fill).text_color(ink))
                    .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                        let (config_id, value) = (config_id.clone(), value.clone());
                        session.update(cx, |session, cx| {
                            session.chat.set_config_option(&config_id, &value);
                            cx.notify();
                        });
                        // The rail is drawn by the composer, so the session's
                        // own notify does not redraw it: without this the value
                        // goes out and the rail keeps lighting the rung that
                        // was in force before.
                        cx.notify();
                    }))
            }),
        );

        div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .px_2()
            // Deeper than a row of the list, on purpose. This block is a second
            // setting sitting under the answer to the first, and at a row's own
            // inset it read as one more entry in the list that happened to have
            // buttons in it -- the rule above says they are different questions
            // and the air is what makes the rule look deliberate.
            .py_3()
            .mt_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_none()
                    // The rows' own size too: the rail's label and the list's
                    // headings are the same kind of word, and one of them a
                    // step smaller reads as a caption on the other.
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(name),
            )
            .child(rail)
    }

    fn attachments_popup(&self, room: gpui::Pixels, cx: &mut Context<Self>) -> gpui::Div {
        let muted = cx.theme().muted_foreground;
        let danger = crate::theme::status_ink(cx).danger;
        let hidden = self
            .attachments
            .len()
            .saturating_sub(MAX_ATTACHMENT_MANAGER_ROWS);

        let list = div()
            .id("attachment-manager")
            .v_flex()
            .w_full()
            .max_h(room)
            .overflow_y_scroll()
            .child(
                div()
                    .h_flex()
                    .px_2()
                    .h(CHIP_H)
                    .text_sm()
                    .child(format!("Staged attachments · {}", self.attachments.len())),
            )
            .children(
                self.attachments
                    .iter()
                    .take(MAX_ATTACHMENT_MANAGER_ROWS)
                    .enumerate()
                    .map(|(i, attachment)| {
                        let id = attachment.id;
                        let unavailable = attachment.delivery == AttachmentDelivery::Unavailable;
                        let path = openable(attachment);
                        let detail = attachment
                            .bytes
                            .map(onehand_core::attachment::size_label)
                            .unwrap_or_else(|| "Size unavailable".to_string());
                        let name = attachment.name.clone();
                        let remove = crate::controls::action(("remove-managed-attachment", i))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Close))
                            .tooltip("Remove this attachment")
                            .on_click(cx.listener(move |composer: &mut Self, _, _, cx| {
                                cx.stop_propagation();
                                composer.unstage(id, cx);
                            }));
                        let row = div()
                            .h_flex()
                            .gap_2()
                            .w_full()
                            .min_w_0()
                            .px_2()
                            .h(CHIP_H)
                            .children(
                                unavailable
                                    .then(|| Icon::new(IconName::Info).size_3().text_color(danger)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .when(unavailable, |label| label.text_color(danger))
                                    .child(name),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(detail))
                            .child(remove);
                        match path {
                            Some(path) => crate::controls::action(("open-managed-attachment", i))
                                .ghost()
                                .w_full()
                                .child(row)
                                .tooltip("Open this file in the Workbench")
                                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::OpenFile(path.clone()));
                                }))
                                .into_any_element(),
                            None => row.into_any_element(),
                        }
                    }),
            )
            .when(hidden > 0, |list| {
                list.child(notice(cx).text_xs().child(format!(
                    "{hidden} more — remove visible items to reveal them"
                )))
            });

        div()
            .v_flex()
            .w_full()
            .rounded(cx.theme().radius)
            .border_1()
            // A step up rather than the hairline every other edge takes, for
            // the reason the option lists carry the same colour: this opens
            // from inside the composer, which is floating too, so its surface
            // and the one behind it are the same and only the edge can say
            // where one ends.
            .border_color(cx.theme().accent)
            .bg(cx.theme().popover.alpha(1.))
            .shadow_lg()
            .p_1()
            .child(list)
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The composer is drawn by the pane, which owns the layout it sits in.
        div()
    }
}

/// The shell both kinds of popup row are built from.
///
/// **Two facts, two ways of drawing them.** Which value is in force is a
/// property of the setting and outlives the popup; where the keyboard is
/// standing is a property of this moment. Drawn the same way they cannot be
/// told apart, and the list opens *on* the current value, so the one frame
/// where they coincide is the frame most people see.
///
/// **One fill, and it means one thing: the row about to be taken.** It follows
/// the pointer and the arrow keys alike -- whichever moved last -- so what is
/// lit is always what `Enter` or a click would pick. The value already in force
/// is said by the tick at the row's end and by nothing else.
///
/// The two were drawn apart once, a strong fill for the value in force and a
/// faint one for the cursor. Which is readable standing still and unreadable in
/// motion: a list opens *on* its current value, so the two coincide on the
/// first frame, and walking away from that row left a second fill behind that
/// looked exactly like a second candidate. A mark cannot be confused with a
/// fill however the two move.
///
/// The fill is `accent` with the ink that goes on it, the one spelling a
/// selected thing takes everywhere in this window -- a tab, a mode chip, a rail
/// row.
fn popup_row(id: usize, highlighted: bool, cx: &App) -> Button {
    crate::controls::action(("candidate", id))
        .ghost()
        // Not for the geometry, which is set outright below and lands after the
        // library's. This is what the row's own `text_sm` could not do: the
        // library letters a button from its `Size`, on the box holding the
        // words and so closer to them than anything the call site sets, and
        // with no size named that is a full 1rem -- so these rows were reading
        // a step larger than the line right here asks for.
        .small()
        .h_flex()
        .gap_2()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .px_2()
        .text_sm()
        .rounded(cx.theme().radius)
        .when(highlighted, |el| {
            el.bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
        })
}

/// A completion candidate: the name, and the part that tells two of the same
/// name apart beside it.
///
/// One line and side by side, unlike a choice below. A file candidate's detail
/// is its folder, which is *where the name is* rather than something about it —
/// stacked under the name it would double the height of a list whose whole job
/// is to put fifty paths in front of somebody typing.
fn candidate_row(id: usize, row: Row, highlighted: bool, muted: gpui::Hsla, cx: &App) -> Button {
    popup_row(id, highlighted, cx)
        .h(CHIP_H)
        .label(row.label)
        // **What makes the row read from the left.** The library centres a
        // button's content and does it on a box the call site cannot reach, so
        // no amount of justifying out here moves it. What does move it is
        // giving the row something that takes the leftover width: the label is
        // built `flex_none`, so everything spare lands on this and the words are
        // pushed against the start. A row with a detail already has one doing
        // that job, which is why this only appears where there is none.
        .children(row.detail.is_none().then(|| div().flex_1()))
        .children(row.detail.map(|detail| {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(muted)
                .child(detail)
        }))
}

/// One value of an agent-advertised setting: its name, the agent's sentence
/// about it underneath, and a tick where it is the one in force.
///
/// **Stacked, and that is the whole difference from a candidate.** A model's
/// description is a sentence and the names it tells apart are two words each,
/// so beside the name it either pushes the name off the row or truncates to the
/// three words every model's description opens with. Under it, at the quieter
/// size, the names stay a column that can be scanned and the sentences are
/// there for the one being considered.
///
/// The tick comes back here because the objection to it does not hold in this
/// shape: it used to pull a *centred* label off centre, and the content of this
/// row is pinned to the start by a `flex_1` of its own. What the fill alone
/// cannot do is survive a reader who does not separate its colour from the row
/// above — so the answer is said twice, in the fill and in a mark.
fn choice_row(id: usize, row: Row, highlighted: bool, cx: &App) -> Button {
    let detail_ink = match highlighted {
        // On the fill, the muted ink of an unlit row is close to unreadable;
        // this is the same relationship one step down from the ink that belongs
        // on this fill.
        true => cx.theme().accent_foreground.alpha(0.75),
        false => cx.theme().muted_foreground,
    };
    let checked = row.checked;
    popup_row(id, highlighted, cx)
        // **The height has to be taken back from the library, explicitly.** A
        // `Button` writes a fixed height per size -- 1.5rem at this one -- and
        // then wraps everything the call site gave it in a box set to the full
        // height of that, centred. A second line does not make the button
        // taller: it overflows the box it was centred in and is painted across
        // the rows either side of it, which is two lines of one choice sitting
        // on top of the next choice's name. Nothing about it looks like a
        // height; it looks like the list has been drawn twice.
        //
        // The floor keeps a choice the agent sent no sentence for standing at
        // exactly the height every other one-line row in this popup does.
        .h_auto()
        .min_h(CHIP_H)
        .py_1()
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .child(div().w_full().truncate().child(row.label))
                .children(row.detail.map(|detail| {
                    div()
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(detail_ink)
                        .child(detail)
                })),
        )
        .children(checked.then(|| Icon::new(IconName::Check).size_4().flex_none()))
}

/// A row of the popup that is a sentence about the list rather than a choice
/// in it — that it matched nothing, that it is holding some back, or which
/// setting the choices under it belong to.
///
/// It stands at the rows' own height for the same reason they stand at one
/// another's: these two appear at the top and bottom of a list of choices, and
/// one of them taller than its neighbours reads as a row that can be taken.
fn notice(cx: &App) -> gpui::Div {
    div()
        .h_flex()
        .items_center()
        .px_2()
        .h(CHIP_H)
        .text_color(cx.theme().muted_foreground)
}

/// Where a staged attachment leads, if it leads anywhere.
///
/// **Only a text file, and only one that could be read.** Three files named
/// `main.rs` are three chips that say `main.rs`, and the only way to tell which
/// one is staged is to look at it — so the chip carries the way to. But the
/// Workbench's editor reads a file as text, so an image handed to it comes back
/// as a decoding error naming a file the user can see is right there, and a
/// file already marked unreadable would fail for the reason the chip is already
/// showing in the danger tint. Neither is worth a second telling, so neither
/// chip offers the press: the pointer appears over the ones that open and
/// nowhere else, which is the only warning a control of this size can carry.
fn openable(attachment: &StagedAttachment) -> Option<std::path::PathBuf> {
    let openable = attachment.kind == AttachmentKind::File
        && attachment.delivery != AttachmentDelivery::Unavailable;
    openable.then(|| attachment.path.clone())
}

/// The chip an attachment is drawn as, applied to whichever container carries
/// it.
///
/// Two containers, because only some of these do something when pressed. The
/// shape is shared rather than written twice so that being pressable stays the
/// *only* difference between them: the two things this tray has to say — the
/// file's name, and whether it can be read — are said the same way whether or
/// not there is anywhere to go, and a chip that changed size or inset on
/// becoming clickable would be saying a third thing nobody meant.
fn attachment_shape<E: Styled>(
    el: E,
    unavailable: bool,
    border: gpui::Hsla,
    danger: gpui::Hsla,
    radius: gpui::Pixels,
) -> E {
    el.h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .pl_2()
        .pr_1()
        .py_1()
        .rounded(radius)
        .border_1()
        .border_color(if unavailable { danger } else { border })
        .text_xs()
}

/// The shell every control in the composer's row is built from.
///
/// One shape, because they are one *rank* of control: small, quiet things
/// acting on the message being written, sitting in a row under it. Built two
/// ways -- a library `Button` for the icons and a hand-made chip for the
/// selectors -- they came out at two sizes, two inks and two hover fills, and
/// the icons, which carry the smaller job, read as the louder half. Sharing the
/// shell makes them one family structurally rather than by two sets of style
/// rules kept in step by hand.
///
/// Two fills and no rules. Hover is the fainter step and an open popup the
/// stronger one, which is the whole of the difference between "the pointer is
/// here" and "this is the control you are editing" -- and it costs the row no
/// width, where a border would have had to be carried by every control at rest
/// to keep the row from shifting.
fn chip(id: impl Into<gpui::ElementId>, open: bool, cx: &App) -> Button {
    let (open_fill, fg, radius) = (
        cx.theme().accent,
        cx.theme().muted_foreground,
        cx.theme().radius,
    );
    crate::controls::action(id)
        .ghost()
        // Not for the geometry -- the height and padding below are set outright
        // and land after the library's own, so they win either way. This is for
        // the **caret**, which takes its size from the button's size rather than
        // from the text beside it: left at the default it is a chevron a third
        // taller than the word it belongs to, on a control whose whole job is to
        // be quiet.
        .xsmall()
        .selected(open)
        .h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .h(CHIP_H)
        .px_1p5()
        .rounded(radius)
        // Ink here, but **not the text size**: the library sets that on the box
        // holding the words, from the button's `Size` and not from anything the
        // call site asks for -- so a size set out here is overridden by one set
        // closer to the text, and setting it looks like it worked while nothing
        // moves. Whatever wants a size of its own says so on the child that
        // carries the words.
        .text_color(fg)
        .when(open, |chip| chip.bg(open_fill))
}

/// The `+` menu: everything that can be put into the prompt from a control.
///
/// Attaching a file, mentioning one and starting a slash command are three
/// answers to one question — *put something in front of the agent* — and as
/// three icons they took the row's whole left-hand end to say it three times.
/// Behind one control they are what the `+` means, which is also the only thing
/// a plus sign can mean here.
///
/// **Each row still draws the mark it stands for, and that is load-bearing.**
/// With a Vietnamese input method on Linux a typed `@` or `/` can be swallowed
/// before it reaches the composer, so these rows are the only route to either
/// trigger — and for somebody who cannot type the character, the row is the
/// only thing on screen naming it. A stand-in glyph would leave them with a
/// menu entry about a character they have no way to see.
///
/// Anchored at the trigger's bottom, because the composer is at the foot of the
/// window: a menu hung below it opens off the bottom of the frame.
fn add_menu(cx: &mut Context<Composer>) -> impl IntoElement + use<> {
    let composer = cx.entity();
    chip("add", false, cx)
        .child(Icon::new(crate::icons::Icon::PlusLight).size(ACTION_ICON))
        .tooltip("Attach a file, mention one, or run a slash command")
        .dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |menu, _, _| {
            let (attach, mention, command) = (composer.clone(), composer.clone(), composer.clone());
            menu.item(
                crate::controls::menu_item("Attach files…")
                    .icon(Icon::new(crate::icons::Icon::Paperclip))
                    .on_click(move |_, _, cx: &mut gpui::App| {
                        attach.update(cx, |composer: &mut Composer, cx| composer.attach(cx));
                    }),
            )
            .item(
                crate::controls::menu_item("Mention a file")
                    .icon(Icon::new(crate::icons::Icon::AtSign))
                    .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                        mention.update(cx, |composer: &mut Composer, cx| {
                            composer.insert_trigger('@', window, cx);
                        });
                    }),
            )
            .item(
                crate::controls::menu_item("Run a slash command")
                    .icon(Icon::new(crate::icons::Icon::SquareSlash))
                    .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                        command.update(cx, |composer: &mut Composer, cx| {
                            composer.insert_trigger('/', window, cx);
                        });
                    }),
            )
        })
}

/// A picker for a setting a turn is run under: a mark, the value in force, and
/// its list.
///
/// Two of these, and they sit apart because the two questions do. Fast mode is
/// in the card's own row, ahead of the model, because it is read as a qualifier
/// of the name beside it -- the two were briefly one chip for that reason. The
/// permission mode is on the strip below, with the branch, because what the
/// agent may do without asking is standing state about the project and the
/// session rather than about this message.
///
/// **Three things separate both from the Model chip**, which is the card's other
/// picker.
///
/// They letter their value at **full strength, like every other chip**. The
/// chip's own muted ink is what the *mark* takes, and the caret and the chrome
/// with it -- what a chip is for is the value in it, and a chip standing beside
/// another with its word a shade fainter reads as one that is somehow less
/// settled rather than as one about a different question.
///
/// They draw **no caret**. Between the two chips and the branch's own mark that
/// would be three small glyphs in a row an inch long, and the caret is the
/// least of the three: a chip is the only thing near it with a hover fill and a
/// pointer, so what can be pressed is already said twice.
///
/// They lead with the **mark of the setting**, which is what makes a value
/// legible without reading it -- and it is the setting and not the value,
/// because a mark per value would mean the app deciding what an agent-chosen
/// word means. What the agent picked is the word right beside it.
fn status_action(
    target: Overlay,
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    // The id, the mark and the sentence are all per chip, so they are decided
    // where the chip is decided. Handed in at the call site they were three
    // things that had to agree with a fourth, in two places.
    let (id, icon, hint) = match target {
        Overlay::Fast => (
            "fast-selector",
            crate::icons::Icon::Zap,
            "Choose how fast the agent answers",
        ),
        _ => (
            "mode-selector",
            crate::icons::Icon::Shield,
            "Choose what the agent may do without asking",
        ),
    };
    let session = session.clone();
    chip(id, open, cx)
        .max_w(OPTION_MAX_W)
        .flex_shrink_1()
        .min_w_0()
        .child(Icon::new(icon).size_3())
        // A child and not the button's `label`, for the reason the card's chip
        // does the same: a label is wrapped in a box the library builds
        // `flex_none`, so against the cap above it takes its whole natural
        // width and the end of the word is clipped with nothing saying it was.
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(CHIP_TEXT)
                .text_color(cx.theme().foreground)
                .child(label),
        )
        .tooltip(hint)
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(target.clone(), &session, window, cx);
        }))
}

/// The card's own picker: the model in force, and behind it every config group
/// the strip below has not taken a chip of.
///
/// `detail` is the value that qualifies the one in the label — the effort a
/// model is being run at — and is drawn in the chip's own muted ink while the
/// label takes full strength. Two words at one weight read as one name.
fn option_action(
    id: &'static str,
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    let session = session.clone();
    let fg = cx.theme().foreground;
    chip(id, open, cx)
        .max_w(OPTION_MAX_W)
        .flex_shrink_1()
        .min_w_0()
        // The value is a **child and not the button's `label`**. A label is
        // wrapped in a box the library builds `flex_none`, so it can neither
        // shrink nor truncate: against the cap above it took its whole natural
        // width, pushed the caret out past the clip, and left the end of the
        // word and the mark that says the button opens both cut off, with
        // nothing on screen saying either had been. As a child it gives way and
        // ends in an ellipsis, and the caret keeps its place.
        //
        // What it costs is the accessible name, which the library derives from
        // that same `label` and offers no other way to set -- `aria_label` is a
        // stateful-element method and this button is not one. So the name is
        // left to be read off the button's own text, which is the value; the
        // tooltip still says which setting the value belongs to.
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(CHIP_TEXT)
                .text_color(fg)
                .child(label),
        )
        .dropdown_caret(true)
        .tooltip("Choose model, effort and other options")
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(Overlay::Options, &session, window, cx);
        }))
}

/// Send, or Stop while a turn is in flight — the same button, because they are
/// the same affordance at two moments of one turn.
///
/// Every state is read off the conversation's own answer to "may this be sent",
/// the running turn included. A button that refuses without saying why does
/// nothing when pressed and gives no reason, which is the shape of a broken app
/// rather than of a rule — so a refusal disables the control and puts the reason
/// on it. One argument and not two, because "a turn is running" and "Send would
/// refuse" are the same fact, and two of them can be made to disagree.
fn send_controls(
    blocked: Option<onehand_core::chat::SubmitBlock>,
    has_draft: bool,
    on_send: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_stop: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    use onehand_core::chat::SubmitBlock;

    // Whichever of these is showing, it is the row's size and not the
    // library's default.
    //
    // **The size and the height both, and the size is the half that was
    // missing.** With no size named these took the library's `Medium`, and the
    // explicit height below only answered half of it: an icon-only button at
    // that size is a *square* of 2rem, so pinning the height left a 2rem-wide
    // box 1.5rem tall -- a lozenge beside a row of controls that are all as
    // wide as what is in them. `Small` is the row's own height in both
    // directions, so Send comes out a square the size of every chip beside it,
    // and it takes the padding, the text and the glyph size down with it.
    //
    // Nothing else changes: the primary fill is what says this is the row's
    // one important action, and it never needed the extra quarter-inch to.
    fn control(id: &'static str) -> Button {
        crate::controls::action(id).small().h(CHIP_H)
    }

    // **Stop is always there while a turn runs, and Queue joins it once there
    // is something to queue.** Stop is the one control in this app that throws
    // running work away, and it is reachable at every moment the work is
    // running -- a Stop that disappeared the instant somebody started typing
    // would be gone at exactly the moment they had most to say about the turn.
    //
    // They are never one button wearing two faces, and never on screen
    // together: Send is drawn only with no turn running and Stop only while one
    // is. They do share a place, though -- the trailing end of the row is the
    // turn's own control whichever of them is in it -- so what tells them apart
    // is the tint, the glyph, and Queue arriving *labelled* beside Stop the
    // moment there is a draft. The cost is real and is taken knowingly: press
    // that same spot twice quickly and the second press stops the turn the
    // first one started. A slot reserved so neither ever moves spends a fixed
    // inch of a row that runs out of width before anything else in the card
    // does, and spends it on the state that is idle most of the time.
    if blocked == Some(SubmitBlock::Busy) {
        return div()
            .h_flex()
            .gap_2()
            .children(has_draft.then(|| {
                control("queue")
                    .primary()
                    .icon(Icon::new(crate::icons::Icon::ArrowUpLight))
                    .label("Queue")
                    .tooltip("Send this prompt when the current turn finishes")
                    .on_click(on_send)
            }))
            .child(
                // The word is gone and the tint carries it: this is the one
                // press here that ends something rather than starting it. It
                // keeps the word's *place* though -- Queue is labelled, so the
                // two are still told apart by more than colour.
                control("stop")
                    .danger()
                    .icon(Icon::new(IconName::Pause))
                    .tooltip("Stop the current turn")
                    .on_click(on_stop),
            );
    }
    // The arrow alone. The word sat beside four other controls in a row that
    // runs out of width before anything else in the card does, and it is the
    // one control here whose meaning never changes -- so it is the one that can
    // afford to be a glyph. Queue and Stop keep their words: they appear only
    // mid-turn, they appear together, and which of the two a press lands on is
    // exactly what a reader has to be sure of.
    let send = control("send")
        .primary()
        .icon(Icon::new(crate::icons::Icon::ArrowUpLight));
    match blocked {
        // No pointer over a button that refuses: the cursor is a promise a
        // press will do something, and this one is here to say it will not.
        Some(reason) => div().child(
            crate::controls::resting(send)
                .disabled(true)
                .tooltip(SharedString::from(reason.hint())),
        ),
        // What Enter does, on the control it does it to. Standing in the row as
        // its own line instead, it was a fixed width competing with the
        // selector chips for a narrow panel's last inch -- and the chips say
        // something that changes, where this says the same thing forever.
        None => div().child(
            send.tooltip("Enter sends · Shift+Enter for a newline")
                .on_click(on_send),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{Draft, TriggerSpot, highlight, split_path, trigger_spot};
    use onehand_core::attachment::{AttachmentSource, StagedAttachment};
    use std::path::PathBuf;

    #[test]
    fn the_command_trigger_goes_to_the_front_and_the_mention_stays_put() {
        assert_eq!(trigger_spot('/', "", 0), TriggerSpot::Insert(0));
        assert_eq!(
            trigger_spot('/', "review this for me", 18),
            TriggerSpot::Insert(0),
            "what was written becomes the command's argument"
        );
        assert_eq!(
            trigger_spot('/', "/compact", 8),
            TriggerSpot::Reuse(1),
            "one slash is enough"
        );
        assert_eq!(trigger_spot('@', "look at ", 8), TriggerSpot::Insert(8));
        assert_eq!(trigger_spot('@', "look at ", 99), TriggerSpot::Insert(8));
    }

    #[test]
    fn a_selection_past_the_end_falls_back_to_the_last_row() {
        assert_eq!(highlight(0, 3), Some(0));
        assert_eq!(highlight(2, 3), Some(2));
        assert_eq!(highlight(7, 3), Some(2));
        assert_eq!(
            highlight(0, 0),
            None,
            "nothing to highlight, nothing to accept"
        );
    }

    #[test]
    fn a_candidate_path_leads_with_its_filename() {
        assert_eq!(
            split_path("crates/app/src/chat/composer.rs"),
            ("composer.rs", Some("crates/app/src/chat"))
        );
        assert_eq!(split_path("README.md"), ("README.md", None));
        assert_eq!(split_path("crates/app/"), ("crates/app/", None));
    }

    #[test]
    fn a_list_is_capped_by_the_panel_and_never_below_its_floor() {
        use super::{POPUP_MIN_H, popup_room};
        use gpui::px;

        let rem = px(16.);
        assert_eq!(
            popup_room(px(800.), px(120.), rem),
            px(800. - 120. - 16.),
            "a tall panel hands the list everything the composer is not standing in"
        );
        assert_eq!(
            popup_room(px(200.), px(180.), rem),
            POPUP_MIN_H.to_pixels(rem),
            "a squeezed panel bottoms out rather than collapsing to one row"
        );
        assert!(
            popup_room(px(0.), px(400.), rem) > px(0.),
            "a panel measured before its first layout must not produce a negative cap"
        );
    }

    #[test]
    fn an_attachment_alone_is_not_an_empty_draft() {
        let draft = Draft {
            text: String::new(),
            attachments: vec![StagedAttachment::inspect(
                PathBuf::from("/tmp/notes.md"),
                AttachmentSource::Picker,
            )],
        };
        assert!(!draft.is_empty());
        assert!(Draft::default().is_empty());
    }
}
