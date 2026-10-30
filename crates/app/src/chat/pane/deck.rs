//! The deck: a card tucked behind the lead's composer, its top edge showing
//! above it, that counts the lead's workers and says how they stand.
//! Pressed, it slides up over the transcript to a row per worker; a row
//! opens the workers' region at that worker.
//!
//! **The pointer leaves the caret where it was**, in the lead's composer as
//! a rule: the edge's press is kept from the pane, which would take focus.
//! *Show the workers* opens it from the keyboard with focus on its first
//! row; Esc folds it and gives the caret back to the composer.

use super::{ChatPane, ChatPaneEvent, SessionSignal};
use crate::control::Worker;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, FocusHandle, InteractiveElement, IntoElement, MouseButton, ParentElement, Rems,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Instant;

/// A worker's row: its state on one line, what it does or waits on under.
const DECK_ROW_H: Rems = rems(2.75);

gpui::actions!(workers_deck, [DeckUp, DeckDown, DeckOpen, DeckFold]);

/// The deck's own state.
pub(super) struct Deck {
    open: bool,
    /// Presses so far, so each plays the slide once.
    turns: usize,
    /// The row the keys are on.
    row: usize,
    focus: FocusHandle,
    /// The lead it was last drawn for: another one finds it folded.
    lead: Option<u64>,
    /// Each worker's state and since when it has stood so.
    since: HashMap<u64, (Option<SessionSignal>, Instant)>,
}

impl Deck {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            turns: 0,
            row: 0,
            focus,
            lead: None,
            since: HashMap::new(),
        }
    }
}

/// Which of two states is the more urgent, by the signal's own order; no
/// state at all is the least.
fn urgency(a: Option<SessionSignal>, b: Option<SessionSignal>) -> Ordering {
    match a == b {
        true => Ordering::Equal,
        false if SessionSignal::most_urgent(a.into_iter().chain(b)) == a => Ordering::Less,
        false => Ordering::Greater,
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

/// The row a key moves to among `len`, stopping at either end.
fn step(row: usize, len: usize, down: bool) -> usize {
    match down {
        true => (row + 1).min(len.saturating_sub(1)),
        false => row.saturating_sub(1),
    }
}

impl ChatPane {
    /// *Show the workers*: the deck open, focus on its first row.
    pub fn show_deck(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.lead_workers().is_empty() {
            return;
        }
        if !self.deck.open {
            self.deck.open = true;
            self.deck.turns += 1;
        }
        self.deck.row = 0;
        self.deck.focus.focus(window, cx);
        cx.notify();
    }

    /// Fold the deck; the caret back in the lead's composer when it was in
    /// the deck.
    fn fold_deck(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had_focus = self.deck.focus.contains_focused(window, cx);
        if self.deck.open {
            self.deck.open = false;
            self.deck.turns += 1;
        }
        if had_focus {
            self.reclaim_focus(window, cx);
        }
        cx.notify();
    }

    fn deck_open_row(&mut self, cx: &mut Context<Self>) {
        let workers = self.lead_workers();
        let Some(worker) = workers.get(self.deck.row) else {
            return;
        };
        cx.emit(ChatPaneEvent::ShowWorker(worker.uid));
        // The caret goes into the worker's block, so none is left here.
        if self.deck.open {
            self.deck.open = false;
            self.deck.turns += 1;
        }
        cx.notify();
    }

    /// The deck over the lead's composer, while the lead on screen has
    /// workers.
    pub(super) fn deck(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let workers = self.lead_workers();
        // Another lead finds the deck folded, standing still.
        if self.deck.lead != self.active {
            self.deck.lead = self.active;
            self.deck.open = false;
            self.deck.row = 0;
        }
        if workers.is_empty() {
            // Leaving the frame with the caret in it would take the keymap.
            if self.deck.focus.contains_focused(window, cx) {
                self.reclaim_focus(window, cx);
            }
            self.deck.open = false;
            self.deck.since.clear();
            return None;
        }
        let now = Instant::now();
        let signals: Vec<Option<SessionSignal>> =
            workers.iter().map(|w| self.signal(w.uid, cx)).collect();
        for (worker, signal) in workers.iter().zip(&signals) {
            let entry = self.deck.since.entry(worker.uid).or_insert((*signal, now));
            if entry.0 != *signal {
                *entry = (*signal, now);
            }
        }
        self.deck
            .since
            .retain(|uid, _| workers.iter().any(|w| w.uid == *uid));
        self.deck.row = self.deck.row.min(workers.len() - 1);
        let age = |uid: u64| {
            self.deck
                .since
                .get(&uid)
                .map_or(0, |(_, at)| at.elapsed().as_secs() / 60)
        };
        let urgent = workers
            .iter()
            .zip(&signals)
            .min_by(|(_, a), (_, b)| urgency(**a, **b))
            .map(|(worker, signal)| (worker.uid, *signal));
        let edge = self.deck_edge(&summary(&signals), urgent, &age, cx);
        // The keys' row is ringed only while the keys are in the deck.
        let keyed = self
            .deck
            .focus
            .contains_focused(window, cx)
            .then_some(self.deck.row);
        let rows: Vec<gpui::AnyElement> = workers
            .iter()
            .zip(&signals)
            .enumerate()
            .map(|(i, (worker, signal))| {
                self.deck_row(worker, *signal, age(worker.uid), (i, keyed == Some(i)), cx)
                    .into_any_element()
            })
            .collect();
        let rem = onehand_ui::tokens::ROW_H;
        let full = rems(rem.0 + DECK_ROW_H.0 * workers.len() as f32);
        let card = div()
            .v_flex()
            .w_full()
            .bg(crate::theme::raised(cx))
            .border_1()
            .border_b_0()
            .border_color(cx.theme().border)
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
                    self.deck.turns,
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

    /// A worker's row: its state, age, turn and background count, and under
    /// them its latest activity or the question it waits on.
    fn deck_row(
        &self,
        worker: &Worker,
        signal: Option<SessionSignal>,
        age: u64,
        (i, keyed): (usize, bool),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let uid = worker.uid;
        let muted = cx.theme().muted_foreground;
        let chat = self.session_of(uid).map(|session| &session.read(cx).chat);
        let waiting = chat.and_then(|chat| chat.waiting_line());
        let line = waiting
            .clone()
            .or_else(|| chat.and_then(|chat| chat.activity_status()));
        let in_turn = chat.is_some_and(|chat| chat.busy);
        let (background, _, _) = self.activity(uid, cx);
        let mut facts = vec![crate::rail::age_label(age)];
        facts.extend(worker.turn.map(|turn| format!("turn {turn}")));
        if background > 0 {
            facts.push(format!("{background} in background"));
        }
        div()
            .relative()
            .h_flex()
            .items_center()
            .gap_2()
            .h(DECK_ROW_H)
            .px_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(keyed, |row| row.child(crate::rail::key_ring(cx)))
            .child(
                div()
                    .id(("workers-deck-row", uid))
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|row| row.text_color(cx.theme().foreground))
                    .on_click(cx.listener(move |pane: &mut Self, _, _, cx| {
                        pane.deck.row = i;
                        pane.deck_open_row(cx);
                    }))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(onehand_ui::mark_slot(crate::rail::status_icon(signal, cx)))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().foreground)
                                    .child(self.worker_name(uid, cx)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(crate::rail::word_ink(signal, cx).unwrap_or(muted))
                                    .child(crate::rail::word(signal)),
                            )
                            .child(div().flex_none().text_color(muted).child(facts.join(" · "))),
                    )
                    .children(line.map(|line| {
                        let ink = match waiting.is_some() {
                            true => crate::theme::status_ink(cx).warning,
                            false => muted,
                        };
                        // Under the name, past the mark's slot and its gap.
                        div()
                            .pl_6()
                            .min_w_0()
                            .truncate()
                            .text_color(ink)
                            .child(line)
                    })),
            )
            .when(in_turn, |row| {
                row.child(
                    crate::controls::action(("workers-deck-stop", uid))
                        .quiet(cx)
                        .short()
                        .label("Stop")
                        .on_click(cx.listener(move |pane: &mut Self, _, _, cx| {
                            if let Some(session) = pane.session_of(uid).cloned() {
                                Self::cancel(&session, cx);
                            }
                        })),
                )
            })
    }
}

#[cfg(test)]
mod tests;
