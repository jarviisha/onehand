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
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt,
};
use onehand_core::attachment::{
    AttachmentDelivery, AttachmentKind, AttachmentSource, StagedAttachment,
};
use onehand_core::completion::{self, ActiveTrigger, TriggerKind};

mod presentation;
use presentation::{
    Pick, Row, composer_status, mode_action, mode_rows, options_action, options_rows,
};

/// Rows drawn in the completion popup. The list scrolls past this; the cap is
/// what keeps a 10 000-file repo from building 10 000 elements (bounded rendering).
const MAX_COMPLETION_ROWS: usize = 50;
/// How far the popup may grow before it scrolls instead.
///
/// Rems, like every other size here: a panel's zoom overrides the rem base for
/// its whole subtree, so a popup measured in pixels is the one thing on screen
/// that does not grow with the text it is completing.
///
/// Sized to keep roughly eight choices visible at the composer's control scale.
const POPUP_MAX_H: Rems = rems(15.);
/// How much room either option action may take before its current value truncates.
const OPTION_MAX_W: Rems = rems(9.);
/// How narrow a selector's list of choices may get.
///
/// A selector's popup is sized by its own rows rather than stretched across the
/// reading column, and three words like *Ask*, *Code* and *Plan* would size it
/// to about an inch — narrower than the chip that opened it, which reads as a
/// second, smaller control rather than as that chip's choices. This is a step
/// clear of the compact control for that reason, and it is a floor and not a width:
/// a longer choice still widens the list up to the column it sits in.
const SELECTOR_MIN_W: Rems = rems(12.);
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
const CHIP_TEXT: Rems = rems(0.75);
/// How tall every control in the composer's row stands, and every row of the
/// list a control opens.
///
/// Fixed, because otherwise the *content* decides it and the content is not the
/// same shape: a chip with a word in it is as tall as that word's line box
/// (`text_xs` times gpui's default leading, about 1.21rem), while a chip
/// holding only an icon is as tall as the icon (0.75rem). Left to themselves
/// they came out about seven pixels apart on the same row.
///
/// The value gives the icon-only actions a deliberate desktop target while
/// keeping the metadata row subordinate to the prompt.
///
/// **The popup's rows take it too**, so the choices behind a chip stand as tall
/// as the chip. They are library buttons, and a button nobody gives a size to
/// takes the library's default of 2rem — which is a step above everything in
/// the row that opened it, chosen by nobody and noticed only once a selector's
/// list stopped being as wide as the reading column. The two notice rows in
/// that list are plain text and take it as well, or a list saying it has
/// nothing stands taller than the same list saying anything.
const CHIP_H: Rems = rems(1.75);

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
    /// All staged attachments, including the entries hidden by the compact
    /// tray's rendering bound.
    Attachments,
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
    /// Stop is deliberately separate from Send/Queue. While a turn is live,
    /// clicking the primary action queues the draft; only this explicit danger
    /// action cancels work already in flight.
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
        let rows = match &target {
            Overlay::Mode => mode_rows(session, cx),
            Overlay::Options => options_rows(session, cx),
            _ => return,
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
            Some(Overlay::Mode) => mode_rows(session, cx).len(),
            Some(Overlay::Options) => options_rows(session, cx).len(),
            Some(Overlay::Attachments) => 0,
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
        let mode = mode_action(session, cx);
        let options = options_action(session, cx);
        let mode_open = self.overlay == Some(Overlay::Mode);
        let options_open = self.overlay == Some(Overlay::Options);
        let mode_popup = mode_open.then(|| self.popup(session, cx)).flatten();
        let options_popup = options_open.then(|| self.popup(session, cx)).flatten();
        let mode_control = mode.map(|label| {
            option_anchor(
                option_action(
                    "mode-selector",
                    label,
                    Overlay::Mode,
                    mode_open,
                    session,
                    cx,
                ),
                mode_popup,
                false,
            )
            .into_any_element()
        });
        let options_control = options.map(|label| {
            option_anchor(
                option_action(
                    "model-selector",
                    label,
                    Overlay::Options,
                    options_open,
                    session,
                    cx,
                ),
                options_popup,
                true,
            )
            .into_any_element()
        });

        div()
            .v_flex()
            .gap_2()
            .w_full()
            .p_3()
            // A floating input needs its own opaque elevation: using the
            // reading surface here makes transcript content behind it visually
            // bleed into the card.
            .bg(cx.theme().popover.alpha(1.))
            .shadow_lg()
            .rounded(cx.theme().radius * 2.)
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
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .child(action(
                                "attach",
                                Icon::new(crate::icons::Icon::Paperclip),
                                "Attach a file",
                                cx,
                                |composer, _, cx| composer.attach(cx),
                            ))
                            // The two triggers, insertable from code. On Linux
                            // with a Vietnamese IME a typed `/` can never reach
                            // the composer, which makes the slash-command popup
                            // unreachable by keyboard -- these are the way in.
                            //
                            // Which is also why each has to draw the character
                            // it types and not a stand-in for it: for the user
                            // who cannot type the character, the button is the
                            // only thing on screen naming it, and nothing else
                            // here says what a mention or a slash command is.
                            .child(action(
                                "mention",
                                Icon::new(crate::icons::Icon::AtSign),
                                "Mention a file",
                                cx,
                                |composer, window, cx| composer.insert_trigger('@', window, cx),
                            ))
                            .child(action(
                                "command",
                                Icon::new(crate::icons::Icon::SquareSlash),
                                "Run a slash command",
                                cx,
                                |composer, window, cx| composer.insert_trigger('/', window, cx),
                            )),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .children(mode_control)
                            .children(options_control),
                    )
                    .child(
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

    pub fn completion_open(&self) -> bool {
        self.overlay == Some(Overlay::Completion)
    }

    /// Popups that belong above the whole card. Option pickers are anchored by
    /// their own buttons inside [`Self::card`] instead.
    pub fn detached_popup(
        &self,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        match self.overlay {
            Some(Overlay::Mode | Overlay::Options) => None,
            _ => self.popup(session, cx),
        }
    }

    pub fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = None;
        cx.notify();
    }

    fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selected = row;
        cx.notify();
    }

    /// The open completion, settings, or attachment surface.
    fn popup(&self, session: &Entity<ChatSession>, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let overlay = self.overlay.clone()?;
        if overlay == Overlay::Attachments {
            return Some(self.attachments_popup(cx));
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
                        }
                    })
                    .collect()
            }
            Overlay::Mode => mode_rows(session, cx),
            Overlay::Options => options_rows(session, cx),
            Overlay::Attachments => unreachable!("handled above"),
        };
        // A trigger that matches nothing still has to say so. Vanishing reads
        // as completion being broken, which is the opposite of the truth: the
        // popup is the only thing on screen that ever confirms the `@` or `/`
        // was understood at all. A selector with no choices has nothing to
        // confirm, so that one stays away.
        if rows.is_empty() && overlay != Overlay::Completion {
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
                // **The completion list takes the column; a selector takes its
                // own rows.** A file candidate is a path and needs every inch
                // of the width the reading column allows, so that list is the
                // full one. A mode list is three short words, and stretched to
                // the same 52rem it read as a panel that had opened over the
                // conversation rather than as the choices behind the chip a
                // finger-width below it.
                //
                // Sized here and not capped, because the box it is dropped into
                // is already the reading column: a flex child shrinks to its
                // parent before it overflows, so the column remains the maximum
                // without this having to name it.
                .map(|popup| match &overlay {
                    Overlay::Completion => popup.w_full(),
                    Overlay::Mode | Overlay::Options => popup.min_w(SELECTOR_MIN_W),
                    Overlay::Attachments => popup.w_full(),
                })
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
                        .max_h(POPUP_MAX_H)
                        .overflow_y_scroll()
                        // Held by the composer rather than by the element, so walking
                        // the list with the keys can scroll it: the handle is what
                        // `reveal_selected` reaches the rows through, and an element's
                        // own handle is gone by the time a key arrives.
                        .track_scroll(&self.rows_scroll)
                        .children(rows.into_iter().enumerate().map(|(i, row)| {
                            let session = session.clone();
                            let pick = row.pick.clone();
                            crate::controls::action(("candidate", i))
                                .ghost()
                                // Not for the geometry, which is set outright
                                // below and lands after the library's. This is
                                // what the row's own `text_sm` could not do:
                                // the library letters a button from its `Size`,
                                // on the box holding the words and so closer to
                                // them than anything the call site sets, and
                                // with no size named that is a full 1rem -- so
                                // these rows were reading a step larger than
                                // the line right here asks for.
                                .small()
                                .h_flex()
                                .gap_2()
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .px_2()
                                .h(CHIP_H)
                                .text_sm()
                                .rounded(cx.theme().radius)
                                // **Two facts, two ways of drawing them.** Which
                                // value is in force is a property of the setting
                                // and outlives the popup; where the keyboard is
                                // standing is a property of this moment. Drawn
                                // the same way they cannot be told apart, and
                                // the list opens *on* the current value, so the
                                // one frame where they coincide is the frame
                                // most people see.
                                //
                                // In force is **weight and ink**, in place of
                                // the tick it replaces: a mark at the end of a
                                // row pulls the words off the centre they are
                                // otherwise set on, so the row that mattered
                                // most was the one row sitting crooked.
                                .when(row.checked, |el| {
                                    el.font_semibold().text_color(cx.theme().primary)
                                })
                                // The keyboard's place is a **whisper of a
                                // fill** -- present enough to follow while an
                                // arrow key is held, faint enough that it is
                                // not read as the answer. It was a full accent
                                // slab, which is a lot of paint for a cursor
                                // and buried the weight above it.
                                .when(Some(i) == selected, |el| {
                                    el.bg(cx.theme().accent.alpha(0.5))
                                })
                                .label(row.label)
                                // **What makes the row read from the left.**
                                // The library centres a button's content and
                                // does it on a box the call site cannot reach,
                                // so no amount of justifying out here moves it.
                                // What does move it is giving the row something
                                // that takes the leftover width: the label is
                                // built `flex_none`, so everything spare lands
                                // on this and the words are pushed against the
                                // start. A row with a detail already has one
                                // doing that job, which is why this only
                                // appears where there is none.
                                .children(row.detail.is_none().then(|| div().flex_1()))
                                .children(row.detail.map(|detail| {
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        // Muted on every row now. The fill under
                                        // the keyboard's row is faint enough that
                                        // the surface ink still reads against it,
                                        // so there is no longer a row this has to
                                        // fade from something else.
                                        .text_color(muted)
                                        .child(detail)
                                }))
                                // A click is a choice already made, so it takes the row
                                // rather than only pointing at it. The highlight moves
                                // first, so what was clicked is what gets taken and not
                                // whatever the keyboard had left selected.
                                .on_click(cx.listener(move |composer: &mut Self, _, window, cx| {
                                    composer.select(i, cx);
                                    composer.apply_pick(&pick, &session, window, cx);
                                }))
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
                ),
        )
    }

    fn attachments_popup(&self, cx: &mut Context<Self>) -> gpui::Div {
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
            .max_h(POPUP_MAX_H)
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

/// A row of the popup that is a sentence about the list rather than a choice
/// in it — that it matched nothing, or that it is holding some back.
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

/// One of the composer's own actions — attach, `@`, `/` — as an icon chip.
///
/// Takes a built [`Icon`] rather than an `IconName`, because two of the three
/// are drawn from the app's own registry: the bundled set has no at-sign and no
/// slash, and these are the two buttons whose entire job is to say which
/// character they type.
fn action<F>(
    id: &'static str,
    icon: Icon,
    hint: &'static str,
    cx: &mut Context<Composer>,
    on_click: F,
) -> impl IntoElement + use<F>
where
    F: Fn(&mut Composer, &mut Window, &mut Context<Composer>) + 'static,
{
    chip(id, false, cx)
        .child(icon.size_3())
        .tooltip(hint)
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            on_click(composer, window, cx);
        }))
}

/// One of the two option actions: Mode, or Model with the remaining config.
fn option_action(
    id: &'static str,
    label: SharedString,
    target: Overlay,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    let session = session.clone();
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
        .child(div().min_w_0().truncate().text_size(CHIP_TEXT).child(label))
        .dropdown_caret(true)
        .tooltip(match &target {
            Overlay::Mode => "Choose mode",
            Overlay::Options => "Choose model, effort and other options",
            _ => "Choose an option",
        })
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(target.clone(), &session, window, cx);
        }))
}

/// Keep an option popup spatially attached to the button that opened it.
/// Absolute positioning lets it overlap the transcript without contributing
/// to the composer's measured height.
fn option_anchor(
    control: impl IntoElement,
    popup: Option<gpui::Div>,
    align_end: bool,
) -> gpui::Div {
    div()
        .relative()
        .flex_shrink_1()
        .min_w_0()
        .child(control)
        .children(popup.map(|popup| {
            // **Painted late, on purpose.** A div paints its background, then
            // its children, and then its *border* -- the border last, over
            // everything inside it. This popup is anchored to a button inside
            // the composer card, so the card's own outline was being drawn
            // straight across the list, which reads as the list being
            // see-through when it is nothing of the kind. Deferring keeps the
            // layout exactly where it is, next to the button that opened it,
            // and moves only the painting to after every ancestor has finished.
            // It is what the component library does for each of its own menus
            // and dropdowns, and for this reason.
            gpui::deferred(
                div()
                    .absolute()
                    .bottom(rems(2.))
                    .when(align_end, |anchor| anchor.right_0())
                    .when(!align_end, |anchor| anchor.left_0())
                    .child(popup),
            )
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

    // Whichever of the three is showing, it stands at the row's height.
    //
    // These are library buttons with no size named, so they were taking the
    // library's own -- a quarter of a rem taller than every control beside
    // them. On a row this short that is not a subtle difference: the button sat
    // proud of the line it is on, and being the loud one here is a job the
    // primary fill already does on its own.
    //
    // Only the height is shared. Everything else about it stays as it was,
    // because it is the row's one primary action and is meant to look it.
    fn control(id: &'static str) -> Button {
        crate::controls::action(id).h(CHIP_H)
    }

    if blocked == Some(SubmitBlock::Busy) {
        return div()
            .h_flex()
            .gap_2()
            .children(has_draft.then(|| {
                control("queue")
                    .primary()
                    .icon(Icon::new(IconName::ArrowUp))
                    .label("Queue")
                    .tooltip("Send this prompt when the current turn finishes")
                    .on_click(on_send)
            }))
            .child(
                control("stop")
                    .danger()
                    .icon(Icon::new(IconName::Pause))
                    .label("Stop")
                    .tooltip("Stop the current turn")
                    .on_click(on_stop),
            );
    }
    let send = control("send")
        .primary()
        .icon(Icon::new(IconName::ArrowUp))
        .label("Send");
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
