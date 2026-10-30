//! The deck: a card tucked behind the lead's composer, its top edge showing
//! above it, that counts the lead's workers and says how they stand.
//! Pressed, it slides up over the transcript to a row per worker; a row
//! opens the workers' region at that worker.
//!
//! **The pointer leaves the caret where it was**, in the lead's composer as
//! a rule: the edge's press is kept from the pane, which would take focus.
//! The keyboard's way to the workers is the region's list.

use super::workers::{WORKER_ROW_H, step, urgency};
use super::{ChatPane, ChatPaneEvent, SessionSignal};
use gpui::{
    Context, FocusHandle, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};
use std::collections::HashMap;
use std::time::Instant;

gpui::actions!(workers_deck, [DeckUp, DeckDown, DeckOpen, DeckFold]);

/// The deck's own state.
pub(super) struct Deck {
    open: bool,
    /// Presses so far, so each plays the slide once.
    turns: usize,
    /// When the last press was and how far open the deck stood then, so the
    /// slide plays only right after it, from there, and never when the deck
    /// is only drawn again.
    pressed: Option<(Instant, f32)>,
    /// The row the keys are on, among the workers last drawn.
    row: usize,
    shown: Vec<u64>,
    focus: FocusHandle,
    /// The lead it was last drawn for: another one finds it folded.
    lead: Option<u64>,
    /// Since when each session has stood as it is, by the rail's clock.
    since: HashMap<u64, Instant>,
}

impl Deck {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            turns: 0,
            pressed: None,
            row: 0,
            shown: Vec::new(),
            focus,
            lead: None,
            since: HashMap::new(),
        }
    }

    /// The press still sliding the card, if one is.
    fn sliding(&self) -> Option<(usize, f32)> {
        self.pressed
            .filter(|(at, _)| at.elapsed() < onehand_ui::TUCKED_SLIDE)
            .map(|(_, from)| (self.turns, from))
    }

    /// How far open the deck stands now, mid-slide or at rest.
    fn openness(&self) -> f32 {
        match self.pressed {
            Some((at, from)) => onehand_ui::tucked_openness(from, self.open, at.elapsed()),
            None => f32::from(u8::from(self.open)),
        }
    }

    /// Folded at once, with no slide: the deck went out of sight.
    fn reset(&mut self) {
        self.open = false;
        self.pressed = None;
        self.row = 0;
    }
}

/// One count per state that has any, most urgent first.
fn summary(signals: &[Option<SessionSignal>]) -> Vec<(Option<SessionSignal>, usize)> {
    let mut counts: Vec<(Option<SessionSignal>, usize)> = Vec::new();
    for signal in signals {
        match counts.iter_mut().find(|(seen, _)| seen == signal) {
            Some((_, n)) => *n += 1,
            None => counts.push((*signal, 1)),
        }
    }
    counts.sort_by(|(a, _), (b, _)| urgency(*a, *b));
    counts
}

impl ChatPane {
    /// Ages by the rail's clock, pushed by the shell as they change.
    pub fn set_since(&mut self, since: HashMap<u64, Instant>) {
        self.deck.since = since;
    }

    /// Whole minutes worker `uid` has stood as it is, by the rail's clock.
    pub(super) fn worker_age(&self, uid: u64) -> u64 {
        self.deck
            .since
            .get(&uid)
            .map_or(0, |at| at.elapsed().as_secs() / 60)
    }

    pub(super) fn deck_open(&self) -> bool {
        self.deck.open
    }

    /// Open or fold the deck as a press does: sliding.
    pub(super) fn press_deck(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.deck.open != open {
            let from = self.deck.openness();
            self.deck.open = open;
            self.deck.turns += 1;
            self.deck.pressed = Some((Instant::now(), from));
        }
        cx.notify();
    }

    /// Fold the deck; the caret back in the lead's composer when it was in
    /// the deck.
    fn fold_deck(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had_focus = self.deck.focus.contains_focused(window, cx);
        self.press_deck(false, cx);
        if had_focus {
            self.reclaim_focus(window, cx);
        }
    }

    /// The row the keys are on, as it was drawn, opened in the region.
    fn deck_open_row(&mut self, cx: &mut Context<Self>) {
        let Some(&uid) = self.deck.shown.get(self.deck.row) else {
            return;
        };
        cx.emit(ChatPaneEvent::ShowWorker(uid));
        self.press_deck(false, cx);
    }

    /// The deck over the lead's composer, while the lead on screen has
    /// workers.
    pub(super) fn deck(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let workers = self.lead_workers();
        let focused = self.deck.focus.contains_focused(window, cx);
        // Another lead, or none to show, finds the deck folded and standing
        // still; a caret in it goes back to the composer first, as a
        // focused element leaving the frame would take the keymap.
        let gone = workers.is_empty() || self.deck.lead != self.active;
        if gone {
            self.deck.lead = self.active;
            self.deck.reset();
            if focused {
                self.reclaim_focus(window, cx);
            }
        }
        // The composer's own list opens where the deck would cover it.
        if self.deck.open && self.composer.read(cx).overlay_open() {
            self.deck.reset();
        }
        if workers.is_empty() {
            return None;
        }
        self.deck.shown = workers.iter().map(|w| w.uid).collect();
        let signals: Vec<Option<SessionSignal>> =
            workers.iter().map(|w| self.signal(w.uid, cx)).collect();
        self.deck.row = self.deck.row.min(workers.len() - 1);
        let age = |uid: u64| self.worker_age(uid);
        let urgent = workers
            .iter()
            .zip(&signals)
            .min_by(|(_, a), (_, b)| urgency(**a, **b))
            .map(|(worker, signal)| (worker.uid, *signal));
        let edge = self.deck_edge(&summary(&signals), urgent, &age, cx);
        // The keys' row is ringed only while the keys are in the deck.
        let keyed = (focused && !gone).then_some(self.deck.row);
        let rows: Vec<gpui::AnyElement> = workers
            .iter()
            .zip(&signals)
            .enumerate()
            .map(|(i, (worker, signal))| {
                self.worker_row(
                    worker,
                    *signal,
                    age(worker.uid),
                    (false, keyed == Some(i)),
                    "workers-deck-row",
                    move |pane, _, cx| {
                        pane.deck.row = i;
                        pane.deck_open_row(cx);
                    },
                    cx,
                )
            })
            .collect();
        let rem = onehand_ui::tokens::ROW_H;
        let full = rems(rem.0 + WORKER_ROW_H.0 * workers.len() as f32);
        let card = div()
            .v_flex()
            .w_full()
            // Set off by its fill and the composer's lift, as the card it is
            // tucked behind: no edge to add to its height.
            .bg(crate::theme::raised(cx))
            .shadow(crate::theme::composer_lift(cx))
            .rounded_t(cx.theme().radius)
            .text_size(onehand_ui::tokens::CHROME_TEXT)
            .track_focus(&self.deck.focus)
            .key_context("WorkersDeck")
            .on_action(cx.listener(|pane: &mut Self, _: &DeckUp, _, cx| {
                let len = pane.lead_workers().len();
                pane.deck.row = step(pane.deck.row, len, false);
                cx.notify();
            }))
            .on_action(cx.listener(|pane: &mut Self, _: &DeckDown, _, cx| {
                let len = pane.lead_workers().len();
                pane.deck.row = step(pane.deck.row, len, true);
                cx.notify();
            }))
            .on_action(cx.listener(|pane: &mut Self, _: &DeckOpen, _, cx| {
                pane.deck_open_row(cx);
            }))
            .on_action(cx.listener(|pane: &mut Self, _: &DeckFold, window, cx| {
                pane.fold_deck(window, cx);
            }))
            // A wheel over the open deck scrolls nothing under it.
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(edge)
            .children(rows);
        Some(
            div()
                .w_full()
                // Narrower than the composer it is tucked behind.
                .px_3()
                .child(onehand_ui::tucked(
                    "workers-deck",
                    self.deck.open,
                    self.deck.sliding(),
                    rem,
                    full,
                    card,
                ))
                .into_any_element(),
        )
    }

    /// The edge: how many workers, a count per state, and the most urgent
    /// one by name and age. Pressed, the deck opens or folds.
    fn deck_edge(
        &self,
        counts: &[(Option<SessionSignal>, usize)],
        urgent: Option<(u64, Option<SessionSignal>)>,
        age: &dyn Fn(u64) -> u64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let total: usize = counts.iter().map(|(_, n)| n).sum();
        let muted = cx.theme().muted_foreground;
        let open = self.deck.open;
        div()
            .id("workers-deck-edge")
            .h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(onehand_ui::tokens::ROW_H)
            .px_3()
            .text_color(muted)
            .cursor_pointer()
            // Kept from the pane, which takes focus on a press: the caret
            // stays where it was.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(
                cx.listener(|pane: &mut Self, _, window, cx| match pane.deck.open {
                    true => pane.fold_deck(window, cx),
                    false => {
                        pane.deck.open = true;
                        pane.deck.turns += 1;
                        cx.notify();
                    }
                }),
            )
            .child(
                Icon::new(match open {
                    true => IconName::ChevronDown,
                    false => IconName::ChevronUp,
                })
                .xsmall(),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(cx.theme().foreground)
                    .child(match total {
                        1 => "1 worker".to_string(),
                        n => format!("{n} workers"),
                    }),
            )
            .children(counts.iter().map(|(signal, n)| {
                div()
                    .h_flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .text_color(crate::rail::word_ink(*signal, cx).unwrap_or(muted))
                    .child(crate::rail::status_icon(*signal, cx))
                    .child(n.to_string())
            }))
            .child(div().flex_1())
            .children(urgent.map(|(uid, signal)| {
                div().min_w_0().truncate().child(format!(
                    "{} · {} {}",
                    self.worker_name(uid, cx),
                    crate::rail::word(signal),
                    crate::rail::age_label(age(uid)),
                ))
            }))
    }
}

#[cfg(test)]
mod tests;
