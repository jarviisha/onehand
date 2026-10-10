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
use gpui::{
    App, AppContext, Context, Entity, IntoElement, Rems, Render, Subscription, Window, div, rems,
};
use gpui_component::input::{InputEvent, TextareaState};
use onehand_core::attachment::StagedAttachment;
use onehand_core::completion::ActiveTrigger;

mod presentation;

mod attachments;
mod card;
mod complete;
mod popup;
mod rows;
pub(in crate::chat) use card::COMPOSER_SPLIT;
pub(in crate::chat) use popup::POPUP_STACK_PEEK;
pub use popup::popup_room;

/// How much of the selected fill is let through.
///
/// **The fill is the only thing saying where `Enter` will land**, so this is the
/// one value here that cannot simply be tuned down until it looks calm. Full
/// strength it was a slab of mid-grey the width of the popup with a small
/// radius on it, which reads as a text field that has the caret rather than as
/// a row that is picked out — and a row that looks like an input in a list you
/// are arrowing through is saying the wrong thing about what the keyboard is
/// doing.
///
/// What bounds it from below is the hover step. A row can be hovered and
/// selected at once, and if those two fills converge the reader cannot tell
/// which of them is telling them where Enter goes. **The light palette is what
/// sets this number**, not the dark one: there is only 1.15 between white and
/// the well to divide up in the first place, so the selected fill and the hover
/// fill start much closer together there and thinning the selected one closes
/// the gap far faster. `theme::tests::the_selection_stays_clear_of_hover`
/// holds it, blending the fill the way the compositor does rather than
/// asserting on the token it came from.
pub(crate) const SELECTED_ALPHA: f32 = 0.75;

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

/// What is showing above the composer. Mutually exclusive **by construction**:
/// one `Option` makes that structural, where a flag per overlay needs a
/// "close the others" call on every path that opens one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// The `@`/`/` candidate list.
    Completion,
    /// The session mode's choices.
    Mode,
    /// The model and every other agent-advertised config choice, effort
    /// aside, in one directly selectable list.
    Options,
    /// What the model runs at, from its own chip beside the model's.
    Effort,
    /// The fast group's choices, where they are not a plain switch: the chip
    /// toggles a group whose two values say which is on, and opens this for
    /// anything else, so both values are named and the one in force ticked.
    Fast,
    /// The `+` menu: what can be put into the prompt from a control.
    Add,
    /// What can be done about the branch on the strip under the card.
    Branch,
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
    /// Something done to the project's branch from its chip. The pane owns the
    /// way to the shell, which carries it out.
    Branch(BranchAct),
}

/// What the branch chip's menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchAct {
    Rename,
    Worktree,
    Refresh,
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
    /// How tall the open popup stands, in rows and in headings, so it does not
    /// resize under the hand that is aiming at it.
    ///
    /// **A popup that grows upward moves every row when it changes size.** The
    /// list is anchored above the composer, so shrinking it walks the whole
    /// block down toward the field — including the highlighted row, which is
    /// the one thing on screen the user is currently pointing at. Two ordinary
    /// events do that. Typing narrows the query, which is the common one: a
    /// list of eight becomes a list of two and the row under the pointer is
    /// somewhere else by the time the click lands. And the `@` candidates are
    /// scanned off the UI loop at session start, so a mention typed in the
    /// first moments of a conversation opens on nothing and is filled in a
    /// frame later — growing from one row to eight under a reader who has just
    /// started reading it.
    ///
    /// So it is measured once, when the popup opens, and held until it closes —
    /// and measured against an **empty** query, so it is the height of the list
    /// rather than of whatever happens to match. Taken from what was on screen
    /// it was a floor rather than a height, and held in one direction only:
    /// narrowing was stable because the floor was already above it, while
    /// deleting a character broadened the list and the popup grew. Growing is
    /// the common one, and it is the one that moves a row out from under the
    /// pointer.
    ///
    /// In rows rather than pixels, because a panel's zoom overrides the rem
    /// base for its subtree and a height snapshotted in pixels would be the one
    /// thing in the popup that did not scale with the text it is completing.
    ///
    /// **Rows and headings counted apart**, because they are not the same
    /// height. Held as one number, a query that narrowed until a whole group
    /// stopped matching still shrank the popup by that group's label — the
    /// filler replaced the rows it lost and had nothing to say about the line
    /// above them.
    opened_rows: Option<(usize, usize)>,
    /// Files staged with 📎, sent with the next prompt.
    pub attachments: Vec<StagedAttachment>,
    /// The popup's scroll, so the highlight can be kept on screen.
    rows_scroll: gpui::ScrollHandle,
    /// The attachment manager's scroll, for its scrollbar.
    attachments_scroll: gpui::ScrollHandle,
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
            opened_rows: None,
            attachments: Vec::new(),
            rows_scroll: gpui::ScrollHandle::new(),
            attachments_scroll: gpui::ScrollHandle::new(),
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
        self.set_overlay(None);
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
        match self.overlay.clone() {
            None => false,
            // **Through the same router a click goes through**, and not
            // straight into `accept`. Every row in this list used to complete
            // something, so the two paths happened to agree; a row that opens a
            // control instead is one `accept` has nothing to do with, and it
            // answered by doing nothing at all — the mouse worked and the
            // keyboard did not, on the list whose whole point is the keyboard.
            Some(Overlay::Completion) => {
                let rows = self.matches(session, cx).0;
                let Some(pick) = highlight(self.selected, rows.len())
                    .and_then(|row| rows.into_iter().nth(row))
                    .map(|row| row.pick)
                else {
                    return false;
                };
                self.apply_pick(&pick, session, window, cx)
            }
            Some(
                picker @ (Overlay::Fast
                | Overlay::Mode
                | Overlay::Options
                | Overlay::Effort
                | Overlay::Add
                | Overlay::Branch),
            ) => {
                let rows = complete::picker_rows(&picker, session, cx).unwrap_or_default();
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

    fn toggle_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_overlay(
            (self.overlay != Some(Overlay::Attachments)).then_some(Overlay::Attachments),
        );
        self.selected = 0;
        self.state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// Open, close or swap the popup, and retire the height snapshot with it.
    ///
    /// **Guarded on the value actually changing, which is the whole of why this
    /// is a method.** One of the callers runs on every keystroke — the trigger
    /// is recomputed per edit and re-asserts `Completion` each time — so a
    /// setter that cleared the snapshot on every assignment would retake it on
    /// every letter typed, which is the state the snapshot exists to prevent,
    /// reached by the code meant to prevent it.
    fn set_overlay(&mut self, next: Option<Overlay>) {
        if self.overlay == next {
            return;
        }
        self.overlay = next;
        self.opened_rows = None;
    }

    /// Keep the highlighted row on screen.
    ///
    /// A list taller than its box scrolls, and the highlight is the only thing
    /// saying what Enter takes -- walked past the fold it left the user pressing
    /// a key with nothing on screen changing.
    fn reveal_selected(&self) {
        self.rows_scroll.scroll_to_item(self.selected);
    }

    /// Whether a popup is on screen, so Esc and a click elsewhere have
    /// something to dismiss.
    pub fn overlay_open(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.set_overlay(None);
        cx.notify();
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The composer is drawn by the pane, which owns the layout it sits in.
        div()
    }
}

#[cfg(test)]
mod tests;
