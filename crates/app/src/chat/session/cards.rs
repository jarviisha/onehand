//! The inputs a parked card is answered in: a free-text box per question
//! field, where the reader is on each question card, the focus a
//! permission's buttons are reached by, and the answers they send through
//! the drive.

use super::{AskBox, AskInput, ChatSession};
use gpui::{AppContext, Context, Entity, Window};
use gpui_component::input::{InputEvent, TextareaState};
use onehand_core::acp::ElicitKind;
use onehand_core::chat::AskPlace;
use std::collections::HashSet;

/// The rows the typed answer to a question holds at rest, and the most it
/// grows to before it scrolls.
const ASK_MIN_ROWS: usize = 2;
const ASK_MAX_ROWS: usize = 6;

impl ChatSession {
    /// Make sure every parked question showing a free-text box has one, and
    /// drop the boxes of questions that have since been answered.
    ///
    /// **Built here and not where the question arrives**, because an input needs
    /// a window and the event pump has none — it runs off the agent's stream,
    /// which belongs to no window. So the pane calls this on its way to drawing
    /// the card, which is the first moment both exist.
    ///
    /// Only the question on screen gets a box built; a form's other fields wait
    /// until the user opens their tab. The box is seeded from the field's own
    /// stored text, so one rebuilt after a rail switch comes back with what was
    /// typed into it rather than empty over a tab that says it was answered.
    pub fn sync_ask_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // One walk of the transcript, on a path that runs every frame: what is
        // parked and what each parked question wants come out of the same pass.
        let mut live: HashSet<usize> = HashSet::new();
        let mut wanted: Vec<AskBox> = Vec::new();
        for (idx, a) in self.chat.pending_asks() {
            live.insert(idx);
            let field = self.ask_place(idx).active_field(a);
            if !a.has_custom(field) {
                continue;
            }
            let f = &a.req.fields[field];
            // A text field *is* its box, so the question's own wording is the
            // invitation; a select's box is the way past the choices above it
            // and has to say so. On a form of several questions that wording is
            // already the line over the box, so the box does not say it again.
            let worded = (a.req.fields.len() == 1)
                .then(|| f.description.clone().or_else(|| f.title.clone()))
                .flatten();
            let hint = match &f.kind {
                ElicitKind::Text => worded.unwrap_or_else(|| "Type your answer".into()),
                _ => "Or type your own answer".into(),
            };
            let typed = a.custom.get(field).cloned().unwrap_or_default();
            wanted.push((idx, field, hint, typed));
        }
        if live.is_empty() && self.ask_inputs.is_empty() && self.ask_focus.is_empty() {
            return;
        }

        // A question that has been answered keeps no box: the card it belonged
        // to is a record now and draws no controls at all.
        self.ask_inputs.retain(|(idx, _), _| live.contains(idx));
        self.ask_places.retain(|idx, _| live.contains(idx));
        self.ask_focus.retain(|idx, _| live.contains(idx));
        // Handles only: which card takes the caret is settled once both
        // kinds are known, in [`Self::sync_perm_focus`].
        for &idx in &live {
            if self.ask_focus.contains_key(&idx) {
                continue;
            }
            let handle = cx.focus_handle();
            self.ask_focus.insert(idx, handle);
        }

        for (idx, field, hint, typed) in wanted {
            if self.ask_inputs.contains_key(&(idx, field)) {
                continue;
            }
            let state = cx.new(|cx| {
                // Two lines at rest, so the typed answer stands as tall as a
                // choice with its description beside it, growing with what is
                // written up to a bound and scrolling past it.
                TextareaState::new(window, cx)
                    .auto_grow(ASK_MIN_ROWS, ASK_MAX_ROWS)
                    // Enter answers, as it did on one line; Shift+Enter is the
                    // newline.
                    .submit_on_enter(true)
                    .placeholder(hint)
                    .default_value(typed)
            });
            let typing = cx.subscribe(&state, move |s: &mut Self, state, ev: &InputEvent, cx| {
                match ev {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        s.ask_type(idx, field, text, cx);
                        cx.notify();
                    }
                    // Enter is the only key that finishes the card: the choices above it answer on a
                    // click, so somebody who writes their own answer instead
                    // would otherwise have to leave the box to send it. Nothing
                    // typed and nothing picked sends nothing -- an empty accept
                    // is an answer the agent cannot read.
                    InputEvent::PressEnter { shift: false, .. } => {
                        s.ask_submit(idx, cx);
                        cx.notify();
                    }
                    _ => {}
                }
            });
            self.ask_inputs.insert(
                (idx, field),
                AskInput {
                    state,
                    _typing: typing,
                },
            );
        }
    }

    /// The free-text box for `field` of the question at live index `idx`, once
    /// [`Self::sync_ask_inputs`] has built one.
    pub fn ask_input(&self, idx: usize, field: usize) -> Option<&Entity<TextareaState>> {
        self.ask_inputs.get(&(idx, field)).map(|i| &i.state)
    }

    /// The card of the question at live index `idx`, for the keys it answers.
    pub fn ask_focus(&self, idx: usize) -> Option<&gpui::FocusHandle> {
        self.ask_focus.get(&idx)
    }

    /// Give every parked permission a handle to scope its keys to, and drop
    /// the handles of the cards that have since been answered.
    ///
    /// Alongside [`Self::sync_ask_inputs`] rather than inside it: a permission
    /// has no boxes to build, and the two lists are what the agent parked on
    /// rather than one list with a flag.
    ///
    /// `composer_idle`: the caret is in the composer and nothing is written
    /// there, so a card that has just been pinned may take it.
    pub fn sync_perm_focus(
        &mut self,
        composer_idle: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let live: HashSet<usize> = self
            .chat
            .pending_permissions()
            .into_iter()
            .map(|(idx, _)| idx)
            .collect();
        self.perm_focus.retain(|idx, _| live.contains(idx));
        for &idx in &live {
            self.perm_focus
                .entry(idx)
                .or_insert_with(|| cx.focus_handle());
        }
        // Only the oldest card is pinned, whichever kind it is, so only it may
        // take the caret. The card and never a button on it, so Enter lands on
        // the card's own listener rather than on whichever grant happened to
        // be focused.
        //
        // **Offered once, as it is pinned**, and taken only where it costs
        // nothing: the keyboard is nowhere, the composer is empty, or the card
        // before it held the keys as it was answered, so answering one card
        // after another stays on the cards rather than falling back to the
        // composer between them. A card parks when the agent chose to, so the
        // caret is never taken out of a half-typed prompt: from there the
        // choices answer to their number with Alt. Offered every frame, the
        // card would take back a caret the user had just clicked away.
        let oldest = self
            .perm_focus
            .iter()
            .chain(&self.ask_focus)
            .min_by_key(|(idx, _)| **idx)
            .map(|(idx, handle)| (*idx, handle.clone()));
        let (seen, held) = self.pinned_keys;
        self.pinned_keys = match oldest {
            Some((idx, handle)) => {
                if takes_keys(
                    window.focused(cx).is_none(),
                    seen != Some(idx),
                    composer_idle,
                    held,
                ) {
                    window.focus(&handle, cx);
                    // Held from this frame on, though the frame the card is
                    // first drawn in is one gpui cannot yet say so about: it
                    // answers from the frame before, which had no card in it.
                    (Some(idx), true)
                } else {
                    (Some(idx), handle.contains_focused(window, cx))
                }
            }
            None => (None, false),
        };
    }

    /// The card of the permission at live index `idx`, for Enter and Esc.
    pub fn perm_focus(&self, idx: usize) -> Option<&gpui::FocusHandle> {
        self.perm_focus.get(&idx)
    }

    /// Empty `field`'s free-text box — the model's slot and the widget showing
    /// it, together.
    ///
    /// **Both, because they are cleared from two different directions.** The
    /// model drops a typed answer by itself whenever a choice is picked, since
    /// the two cannot both be the answer; and the widget's own `set_value`
    /// raises no change event, so neither half tells the other. Cleared one at
    /// a time, the box goes on showing words nothing will send — which is the
    /// one thing a form must never do.
    pub fn clear_ask_input(
        &mut self,
        idx: usize,
        field: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask_type(idx, field, String::new(), cx);
        let state = self.ask_inputs.get(&(idx, field)).map(|i| i.state.clone());
        if let Some(state) = state {
            state.update(cx, |state, cx| state.set_value("", window, cx));
        }
    }
}

impl ChatSession {
    /// Where the reader is on the open question at `idx`.
    pub fn ask_place(&self, idx: usize) -> AskPlace {
        self.ask_places.get(&idx).copied().unwrap_or_default()
    }

    /// Put the reader at `place` on the open question at `idx`.
    pub fn set_ask_place(&mut self, idx: usize, place: AskPlace) {
        self.ask_places.insert(idx, place);
    }

    /// Answer the permission card at `idx` with `option_id`.
    pub fn answer_permission(&mut self, idx: usize, option_id: &str, cx: &mut Context<Self>) {
        let fx = self.chat.answer_permission(idx, option_id);
        self.perform(fx, cx);
    }

    /// Pick `option` of `field` on the question at `idx`, the keyboard landing
    /// on it; see [`onehand_core::chat::SessionDrive::ask_pick`].
    pub fn ask_pick(&mut self, idx: usize, field: usize, option: usize, cx: &mut Context<Self>) {
        let mut place = self.ask_place(idx);
        place.point_at(option);
        self.set_ask_place(idx, place);
        let (_, fx) = self.chat.ask_pick(idx, field, option);
        self.perform(fx, cx);
    }

    /// Type `text` as the answer to `field` of the question at `idx`.
    pub fn ask_type(&mut self, idx: usize, field: usize, text: String, cx: &mut Context<Self>) {
        let fx = self.chat.ask_type(idx, field, text);
        self.perform(fx, cx);
    }

    /// Pass on `field` of the question at `idx`, showing the next one if the
    /// form goes on; see [`onehand_core::chat::SessionDrive::ask_skip`].
    pub fn ask_skip(&mut self, idx: usize, field: usize, cx: &mut Context<Self>) {
        let (next, fx) = self.chat.ask_skip(idx, field);
        self.show_question(idx, next);
        self.perform(fx, cx);
    }

    /// Move the question at `idx` on from `field`; see
    /// [`onehand_core::chat::SessionDrive::ask_advance`].
    pub fn ask_advance(&mut self, idx: usize, field: usize, cx: &mut Context<Self>) {
        let (next, fx) = self.chat.ask_advance(idx, field);
        self.show_question(idx, next);
        self.perform(fx, cx);
    }

    /// Send the question at `idx` as it stands, if it holds an answer.
    pub fn ask_submit(&mut self, idx: usize, cx: &mut Context<Self>) {
        let fx = self.chat.ask_submit(idx);
        self.perform(fx, cx);
    }

    fn show_question(&mut self, idx: usize, next: Option<usize>) {
        if let Some(field) = next {
            let mut place = self.ask_place(idx);
            place.go_to(field);
            self.set_ask_place(idx, place);
        }
    }
}

/// Whether the pinned card takes the keyboard this frame: always where the
/// keyboard is nowhere, else only the frame it is pinned, and then only from an
/// empty composer or from the card answered before it.
fn takes_keys(nothing_focused: bool, fresh: bool, composer_idle: bool, held: bool) -> bool {
    nothing_focused || (fresh && (composer_idle || held))
}

#[cfg(test)]
mod tests {
    use super::takes_keys;

    #[test]
    fn a_card_takes_the_keys_once_and_never_from_a_written_prompt() {
        assert!(
            takes_keys(true, false, false, false),
            "nowhere is always taken"
        );
        assert!(
            takes_keys(false, true, true, false),
            "a new card, an empty composer"
        );
        assert!(
            takes_keys(false, true, false, true),
            "the card before held them"
        );
        assert!(
            !takes_keys(false, true, false, false),
            "a half-typed prompt keeps them"
        );
        assert!(
            !takes_keys(false, false, true, false),
            "a card already offered them does not take back a caret clicked away"
        );
    }
}
