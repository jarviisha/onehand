//! The inputs a parked card is answered in: a free-text box per question
//! field, and the focus a permission's buttons are reached by.

use super::{AskBox, AskInput, ChatSession};
use gpui::{App, AppContext, Context, Entity, Window};
use gpui_component::input::{InputEvent, InputState};
use std::collections::HashSet;

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
            let field = a.active_field();
            if !a.has_custom(field) {
                continue;
            }
            // Just "Answer": the question and its description are said on the
            // card already, and said again as the placeholder they were the
            // longest line on it, cut off on a narrow pane.
            let hint = "Answer".to_string();
            let typed = a.custom.get(field).cloned().unwrap_or_default();
            wanted.push((idx, field, hint, typed));
        }
        if live.is_empty() && self.ask_inputs.is_empty() && self.ask_focus.is_empty() {
            return;
        }

        // A question that has been answered keeps no box: the card it belonged
        // to is a record now and draws no controls at all.
        self.ask_inputs.retain(|(idx, _), _| live.contains(idx));
        self.ask_focus.retain(|idx, _| live.contains(idx));
        for idx in &live {
            if self.ask_focus.contains_key(idx) {
                continue;
            }
            let handle = cx.focus_handle();
            // A card that has just stopped the turn is where the keyboard
            // belongs -- but **only where the keyboard is nowhere**. Taking the
            // caret out of a composer somebody is mid-sentence in is worse than
            // a card that has to be clicked before its number keys work, and a
            // question can park at any moment because the agent chose it, not
            // because the user asked for it.
            if window.focused(cx).is_none() {
                window.focus(&handle, cx);
            }
            self.ask_focus.insert(*idx, handle);
        }

        for (idx, field, hint, typed) in wanted {
            if self.ask_inputs.contains_key(&(idx, field)) {
                continue;
            }
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(hint)
                    .default_value(typed)
            });
            let typing = cx.subscribe(&state, move |s: &mut Self, state, ev: &InputEvent, cx| {
                match ev {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        if let Some(a) = s.chat.ask_at_mut(idx) {
                            a.set_custom(field, text);
                        }
                        cx.notify();
                    }
                    // A one-line field means Enter, and here it is the only key
                    // that finishes the card: the choices above it answer on a
                    // click, so somebody who writes their own answer instead
                    // would otherwise have to leave the box to send it. Nothing
                    // typed and nothing picked sends nothing -- an empty accept
                    // is an answer the agent cannot read.
                    InputEvent::PressEnter { .. } => {
                        if s.chat.ask_at_mut(idx).is_some_and(|a| a.has_answer()) {
                            s.chat.answer_ask(idx, false);
                        }
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
    pub fn ask_input(&self, idx: usize, field: usize) -> Option<&Entity<InputState>> {
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
    pub fn sync_perm_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let live: HashSet<usize> = self
            .chat
            .pending_permissions()
            .into_iter()
            .map(|(idx, _)| idx)
            .collect();
        if live.is_empty() && self.perm_focus.is_empty() {
            return;
        }
        self.perm_focus.retain(|idx, _| live.contains(idx));
        for idx in live {
            if self.perm_focus.contains_key(&idx) {
                continue;
            }
            let handle = cx.focus_handle();
            // The card and never a button on it, so Enter lands on the card's
            // own listener rather than on whichever grant happened to be
            // focused -- and **only where the keyboard is nowhere**, because a
            // permission parks when the agent chose to, not when the user
            // asked, and taking the caret out of a half-typed prompt is worse
            // than a card that has to be clicked before its keys work.
            if window.focused(cx).is_none() {
                window.focus(&handle, cx);
            }
            self.perm_focus.insert(idx, handle);
        }
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
    pub fn clear_ask_input(&mut self, idx: usize, field: usize, window: &mut Window, cx: &mut App) {
        if let Some(a) = self.chat.ask_at_mut(idx) {
            a.set_custom(field, String::new());
        }
        let state = self.ask_inputs.get(&(idx, field)).map(|i| i.state.clone());
        if let Some(state) = state {
            state.update(cx, |state, cx| state.set_value("", window, cx));
        }
    }
}
