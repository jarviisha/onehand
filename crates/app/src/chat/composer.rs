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
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Disableable as _, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::attachment::{AttachmentDelivery, AttachmentSource, StagedAttachment};
use onehand_core::completion::{self, ActiveTrigger, TriggerKind};

/// Rows drawn in the completion popup. The list scrolls past this; the cap is
/// what keeps a 10 000-file repo from building 10 000 elements (bounded rendering).
const MAX_COMPLETION_ROWS: usize = 50;
/// How far the popup may grow before it scrolls instead.
///
/// Rems, like every other size here: a panel's zoom overrides the rem base for
/// its whole subtree, so a popup measured in pixels is the one thing on screen
/// that does not grow with the text it is completing.
///
/// Sized by *how many candidates are visible* rather than by a round number, so
/// giving the rows more room does not quietly cost the list two of them: it is
/// about seven rows at their current height, and it moved when they did.
const POPUP_MAX_H: Rems = rems(17.5);
/// The popup's tick column, held whether or not a row is ticked so the labels
/// stay on one column.
const CHECK_GUTTER: Rems = rems(0.875);
/// How much of a selector's current value is shown before it truncates.
const CHIP_MAX_W: Rems = rems(8.125);
/// How much of an attachment's name is shown before it truncates.
const ATTACHMENT_MAX_W: Rems = rems(10.);
/// Attachment chips drawn before the tray starts counting instead.
const MAX_TRAY_CHIPS: usize = 12;
/// How tall every control in the composer's row stands.
///
/// Fixed, because otherwise the *content* decides it and the content is not the
/// same shape: a chip with a word in it is as tall as that word's line box
/// (`text_xs` times gpui's default leading, about 1.21rem), while a chip
/// holding only an icon is as tall as the icon (0.75rem). Left to themselves
/// they came out about seven pixels apart on the same row. This is that line
/// box plus the padding the chips already had, so the ones with words in them
/// stand exactly where they did.
const CHIP_H: Rems = rems(1.75);

/// What is showing above the composer. Mutually exclusive **by construction**:
/// one `Option` makes that structural, where a flag per overlay needs a
/// "close the others" call on every path that opens one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// The `@`/`/` candidate list.
    Completion,
    /// A selector's choices. `None` is the session mode; `Some(i)` is
    /// `config_options[i]`.
    Selector(Option<usize>),
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

#[cfg(test)]
mod tests {
    use super::{Draft, TriggerSpot, highlight, split_path, trigger_spot};
    use onehand_core::attachment::{AttachmentSource, StagedAttachment};
    use std::path::PathBuf;

    /// The slash has one place it can mean anything, and the button has to put
    /// it there. Left at the caret it produced a stray character in the middle
    /// of a sentence and no popup, which reads as a dead button.
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

        // A mention names a file where it is written.
        assert_eq!(trigger_spot('@', "look at ", 8), TriggerSpot::Insert(8));
        assert_eq!(trigger_spot('@', "look at ", 99), TriggerSpot::Insert(8));
    }

    /// A selection made against a longer list must land on a row that exists.
    /// The way this failed was silent and expensive: with the highlight past
    /// the end, Enter accepted nothing and sent the prompt instead, trigger
    /// and all.
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

    /// The filename leads and the folder follows, so the part that gets cut on
    /// a narrow panel is the part the user was not reading.
    #[test]
    fn a_candidate_path_leads_with_its_filename() {
        assert_eq!(
            split_path("crates/app/src/chat/composer.rs"),
            ("composer.rs", Some("crates/app/src/chat"))
        );
        assert_eq!(split_path("README.md"), ("README.md", None));
        // A trailing slash names a directory, and there is no name after it to
        // lead with -- so it stays whole rather than becoming an empty row.
        assert_eq!(split_path("crates/app/"), ("crates/app/", None));
    }

    /// A staged file with no prompt beside it is still something the user put
    /// there. Reading emptiness off the text alone would drop it on a session
    /// switch and hand it to whichever agent was next -- the quieter half of
    /// the same mistake, since nothing on screen says the attachment moved.
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

/// What the composer asks its owner for, because it has no business doing it
/// itself: the pane knows which conversation is on screen and whether a turn is
/// running, and the composer only knows the button was pressed.
///
/// One variant, because Send and Stop are one control at two moments and the
/// decision between them belongs to whoever can still see the turn — pressed a
/// frame after it ended, the button that said Stop must not cancel anything.
pub enum ComposerEvent {
    SendPressed,
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
                .placeholder("Ask the agent…  @ for files, / for commands")
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
        match self.overlay {
            None => false,
            Some(Overlay::Completion) => self.accept(session, window, cx),
            Some(Overlay::Selector(which)) => {
                let rows = selector_rows(session, which, cx);
                let Some(row) =
                    highlight(self.selected, rows.len()).and_then(|row| rows.into_iter().nth(row))
                else {
                    return false;
                };
                self.apply_pick(&row.pick, session, window, cx)
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

    /// Open a selector's choices, or close it if it is the one already open.
    /// Open a selector's choices, or close it if it is the one already open.
    ///
    /// **Focus goes to the prompt field, not to the chip.** The chip is a plain
    /// div, so the click that opened the list travels up to the pane, which
    /// takes focus -- and the keys that walk the list only reach it while focus
    /// is in an input inside the composer. Without this the arrows did nothing
    /// at all on a list opened with the mouse, which is every list of the
    /// agent's own options.
    ///
    /// It opens **on the current value** rather than at the top: the list is a
    /// setting's state, and arrowing away from where you are is the movement
    /// the user means.
    fn toggle_selector(
        &mut self,
        which: Option<usize>,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = Overlay::Selector(which);
        self.overlay = (self.overlay != Some(target)).then_some(target);
        self.selected = selector_rows(session, which, cx)
            .iter()
            .position(|row| row.checked)
            .unwrap_or(0);
        self.reveal_selected();
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
        match self.overlay {
            None => 0,
            Some(Overlay::Completion) => self.candidates(session, cx).len(),
            Some(Overlay::Selector(which)) => selector_rows(session, which, cx).len(),
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
        specs: &[(Option<usize>, String, Option<String>)],
        typing_here: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (step_down, step_up) = (session.clone(), session.clone());
        let tray = self.tray(cx).map(IntoElement::into_any_element);
        let chips: Vec<_> = specs
            .iter()
            .map(|(which, label, current)| {
                let open = self.selector_open(*which);
                selector(*which, label, current.as_deref(), open, session, cx).into_any_element()
            })
            .collect();

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
                    .child(Textarea::new(&self.state).appearance(false)),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(action(
                        "attach",
                        IconName::Inbox,
                        "Attach a file",
                        cx,
                        |composer, _, cx| composer.attach(cx),
                    ))
                    // The two triggers, insertable from code. On Linux with a
                    // Vietnamese IME a typed `/` can never reach the composer,
                    // which makes the slash-command popup unreachable by
                    // keyboard -- these are the way in.
                    .child(action(
                        "mention",
                        IconName::Asterisk,
                        "Mention a file",
                        cx,
                        |composer, window, cx| composer.insert_trigger('@', window, cx),
                    ))
                    .child(action(
                        "command",
                        IconName::Dash,
                        "Run a slash command",
                        cx,
                        |composer, window, cx| composer.insert_trigger('/', window, cx),
                    ))
                    // The chips take whatever is left and give it back first.
                    // A row of four selectors on a narrow panel used to push
                    // Send off its own edge, because the only flexible thing in
                    // the row was the gap before it.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .children(chips),
                    )
                    .child(
                        send_button(
                            blocked,
                            cx.listener(|_: &mut Self, _, _, cx| {
                                cx.emit(ComposerEvent::SendPressed);
                            }),
                        )
                        .flex_none(),
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
        cx.notify();
    }

    /// The staged files, as a horizontally scrolling tray.
    ///
    /// Bounded like everything else that grows with what the user did: a folder
    /// dropped on the card is however many files it held, and a tray of two
    /// hundred chips is two hundred elements laid out on every keystroke. What
    /// is over the bound is counted rather than dropped silently.
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
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .flex_none()
                                .pl_2()
                                .pr_1()
                                .py_1()
                                .rounded(radius)
                                .border_1()
                                .border_color(if unavailable { danger_border } else { border })
                                .text_xs()
                                .child(
                                    Icon::new(match a.kind {
                                        onehand_core::attachment::AttachmentKind::Image => {
                                            IconName::Frame
                                        }
                                        onehand_core::attachment::AttachmentKind::File => {
                                            IconName::File
                                        }
                                    })
                                    .size_3(),
                                )
                                .child(
                                    div()
                                        .max_w(ATTACHMENT_MAX_W)
                                        .truncate()
                                        .when(unavailable, |el| el.text_color(danger_text))
                                        .child(a.name.clone()),
                                )
                                // The size, because two screenshots taken a
                                // minute apart have interchangeable names, and
                                // because it is the only warning that a large
                                // image will go as a link instead of inline.
                                .children(a.bytes.map(|bytes| {
                                    div()
                                        .flex_none()
                                        .text_color(muted)
                                        .child(onehand_core::attachment::size_label(bytes))
                                }))
                                // A real button, not a bare glyph: this one is
                                // small, sits beside the name it destroys, and
                                // needs the hover and the focus ring that say
                                // which of the two the pointer is on.
                                .child(
                                    crate::controls::action(("unstage", i))
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(IconName::Close))
                                        .tooltip("Remove this attachment")
                                        .on_click(cx.listener(
                                            move |composer: &mut Self, _, _, cx| {
                                                composer.unstage(id, cx);
                                            },
                                        )),
                                )
                        }),
                )
                .when(over > 0, |tray| {
                    tray.child(
                        div()
                            .flex_none()
                            .py_1()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("+{over} more")),
                    )
                }),
        )
    }

    fn selector_open(&self, which: Option<usize>) -> bool {
        self.overlay == Some(Overlay::Selector(which))
    }

    /// Whether a popup is on screen, so Esc and a click elsewhere have
    /// something to dismiss.
    pub fn overlay_open(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = None;
        cx.notify();
    }

    fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selected = row;
        cx.notify();
    }

    /// The open picker's rows, or nothing when none is open.
    ///
    /// One popup for both jobs: an `@` list and a model list are the same
    /// affordance in the same place, and only one can be open at a time.
    pub fn popup(
        &self,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let overlay = self.overlay?;
        // Set while the rows are built, because that is the only place both
        // halves of the count are in hand.
        let mut capped = 0usize;
        let rows: Vec<Row> = match overlay {
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
            Overlay::Selector(which) => selector_rows(session, which, cx),
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
        let (muted, hover_fill) = (cx.theme().muted_foreground, cx.theme().list_hover);

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
                .w_full()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
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
                            div()
                                .id(("candidate", i))
                                .h_flex()
                                .gap_2()
                                .px_2()
                                .py_2()
                                .text_sm()
                                .cursor_pointer()
                                .rounded(cx.theme().radius)
                                // This list is walked with the arrow keys and committed
                                // with Enter, so the highlight is the only thing saying
                                // what Enter will insert -- and it is a fill, with no
                                // rule around it. The selected step is a real one and
                                // sits well clear of the hover step, which is what lets
                                // a bare fill carry the state on its own.
                                .when(Some(i) == selected, |el| {
                                    el.bg(cx.theme().accent)
                                        .text_color(cx.theme().accent_foreground)
                                })
                                // The fainter of the two fills, and only on rows
                                // that are not the selected one -- a hover step
                                // painted over the selected step would take the
                                // highlight *off* the row the pointer is on,
                                // which is the opposite of what it is for.
                                .when(Some(i) != selected, |el| el.hover(|row| row.bg(hover_fill)))
                                // A fixed-width gutter whether or not the tick is there,
                                // so the labels of a list stay on one column.
                                .child(
                                    div()
                                        .w(CHECK_GUTTER)
                                        .flex_none()
                                        .when(row.checked, |gutter| {
                                            gutter.child(Icon::new(IconName::Check).size_3())
                                        }),
                                )
                                .child(div().min_w_0().truncate().child(row.label))
                                .children(row.detail.map(|detail| {
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        // Secondary on the row it sits in, whichever
                                        // that is: on the highlighted row the muted ink
                                        // is the wrong one to fade *from*, since the
                                        // fill under it has already changed.
                                        .text_color(if Some(i) == selected {
                                            cx.theme().accent_foreground.alpha(0.75)
                                        } else {
                                            muted
                                        })
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
                            list.child(
                                div()
                                    .px_2()
                                    .py_2()
                                    .text_sm()
                                    .text_color(muted)
                                    .child("No matches"),
                            )
                        })
                        .when(capped > 0, |list| {
                            list.child(
                                div()
                                    .px_2()
                                    .py_2()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{capped} more — keep typing to narrow them")),
                            )
                        }),
                ),
        )
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The composer is drawn by the pane, which owns the layout it sits in.
        div()
    }
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
fn chip(id: impl Into<gpui::ElementId>, open: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    let (open_fill, hover_fill, fg, radius) = (
        cx.theme().accent,
        cx.theme().list_hover,
        cx.theme().muted_foreground,
        cx.theme().radius,
    );
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .h(CHIP_H)
        .px_2()
        .rounded(radius)
        .cursor_pointer()
        .text_xs()
        .text_color(fg)
        .hover(|chip| chip.bg(hover_fill))
        .when(open, |chip| chip.bg(open_fill))
}

/// One of the composer's own actions — attach, `@`, `/` — as an icon chip.
fn action<F>(
    id: &'static str,
    icon: IconName,
    hint: &'static str,
    cx: &mut Context<Composer>,
    on_click: F,
) -> impl IntoElement + use<F>
where
    F: Fn(&mut Composer, &mut Window, &mut Context<Composer>) + 'static,
{
    chip(id, false, cx)
        .child(Icon::new(icon).size_3())
        .tooltip(move |window, cx| Tooltip::new(hint).build(window, cx))
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            on_click(composer, window, cx);
        }))
}

/// One agent-advertised selector (mode, model, effort…) as a compact chip.
fn selector(
    which: Option<usize>,
    label: &str,
    current: Option<&str>,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    let session = session.clone();
    // `None` is the mode; `Some(i)` is `config_options[i]`. Element ids have to
    // be distinct, so the mode takes 0 and the options shift up by one.
    let id: usize = which.map(|i| i + 1).unwrap_or(0);
    let visible = SharedString::from(current.unwrap_or(label).to_string());
    let hint = SharedString::from(format!("{label}: {}", current.unwrap_or("—")));
    chip(("selector", id), open, cx)
        // The popup groups and the tooltip carry the setting's name. Repeating
        // `Mode:`, `Model:`, `Effort:` on every chip spends half the composer's
        // control row naming controls that are already in a stable order.
        .child(div().max_w(CHIP_MAX_W).truncate().child(visible))
        .child(Icon::new(IconName::ChevronDown).size_3())
        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_selector(which, &session, window, cx);
        }))
}

/// One row of the popup: what it is called, and the quieter half that tells two
/// rows of the same name apart.
struct Row {
    label: SharedString,
    detail: Option<SharedString>,
    checked: bool,
    pick: Pick,
}

/// What clicking a row does.
#[derive(Clone)]
enum Pick {
    /// Highlight only; Enter accepts.
    Complete,
    Mode(String),
    Config {
        config_id: String,
        value: String,
    },
}

/// The choices behind a selector chip.
fn selector_rows(session: &Entity<ChatSession>, which: Option<usize>, cx: &App) -> Vec<Row> {
    let chat = &session.read(cx).chat;
    match which {
        None => chat
            .modes
            .iter()
            .map(|mode| Row {
                label: SharedString::from(mode.name.clone()),
                detail: None,
                checked: Some(&mode.id) == chat.current_mode.as_ref(),
                pick: Pick::Mode(mode.id.clone()),
            })
            .collect(),
        Some(i) => chat
            .config_options
            .get(i)
            .map(|opt| {
                opt.choices
                    .iter()
                    .map(|choice| Row {
                        label: SharedString::from(choice.name.clone()),
                        detail: None,
                        checked: Some(&choice.value) == opt.current.as_ref(),
                        pick: Pick::Config {
                            config_id: opt.id.clone(),
                            value: choice.value.clone(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
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
fn send_button(
    blocked: Option<onehand_core::chat::SubmitBlock>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    use onehand_core::chat::SubmitBlock;

    if blocked == Some(SubmitBlock::Busy) {
        return crate::controls::action("stop")
            .danger()
            .icon(Icon::new(IconName::Pause))
            .label("Stop")
            // The button cancels; Enter queues. Said here because this is the
            // one moment the two gestures do different things, and the label
            // can only name one of them.
            .tooltip("Stop this turn · Enter queues what you write next")
            .on_click(on_click);
    }
    let send = crate::controls::action("send")
        .primary()
        .icon(Icon::new(IconName::ArrowUp))
        .label("Send");
    match blocked {
        // No pointer over a button that refuses: the cursor is a promise a
        // press will do something, and this one is here to say it will not.
        Some(reason) => crate::controls::resting(send)
            .disabled(true)
            .tooltip(SharedString::from(reason.hint())),
        // What Enter does, on the control it does it to. Standing in the row as
        // its own line instead, it was a fixed width competing with the
        // selector chips for a narrow panel's last inch -- and the chips say
        // something that changes, where this says the same thing forever.
        None => send
            .tooltip("Enter sends · Shift+Enter for a newline")
            .on_click(on_click),
    }
}
