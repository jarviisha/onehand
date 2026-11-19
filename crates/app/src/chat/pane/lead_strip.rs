//! The lead's chip: in the lead's composer row, while the lead on screen has
//! workers. It counts them a count per state, in marks, and names the worker
//! waiting in its tooltip. Pressed, it lists them, and a worker picked there
//! shows in the pane, as its row under the lead in the rail would show it.

use super::{ChatPane, ChatPaneEvent, SessionSignal};
use crate::control::Worker;
use gpui::{App, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div};
use gpui_component::{Icon, StyledExt as _};
use std::cmp::Ordering;

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

/// The most urgent worker waiting on the person, the first of a tie; none
/// when none waits.
fn waiting_first(workers: &[(u64, Option<SessionSignal>)]) -> Option<u64> {
    workers
        .iter()
        .filter(|(_, signal)| crate::rail::needs_attention(*signal))
        .min_by(|(_, a), (_, b)| urgency(*a, *b))
        .map(|(uid, _)| *uid)
}

impl ChatPane {
    /// The workers of the lead on screen that this pane holds, opening or
    /// open, in the order the lead opened them.
    fn lead_workers(&self) -> Vec<Worker> {
        let Some(lead) = self.active else {
            return Vec::new();
        };
        crate::control::workers_of(lead)
            .into_iter()
            .filter(|worker| self.conversations.contains_key(&worker.uid))
            .collect()
    }

    /// What a worker, or the lead, is called: as the rail names it.
    pub(super) fn worker_name(&self, uid: u64, cx: &App) -> String {
        let agent = self
            .conversations
            .get(&uid)
            .map(|conv| conv.spec.name.clone())
            .unwrap_or_default();
        let title = self.title_for(uid, cx);
        crate::rail::session_label(title.as_deref(), &agent).to_string()
    }

    /// The lead's chip in the composer's row, while the lead on screen has
    /// workers: a count per state, most urgent first, in marks. Pressed, it
    /// lists them above it, each in its state's mark and named as the rail
    /// names it, in the order the lead opened them; a row pressed shows that
    /// worker in the pane. The list is held to the cap on what one lead opens,
    /// so it needs no cap of its own.
    pub(super) fn lead_chip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let lead = self.active?;
        let workers: Vec<(u64, Option<SessionSignal>)> = self
            .lead_workers()
            .iter()
            .map(|worker| (worker.uid, self.signal(worker.uid, cx)))
            .collect();
        if workers.is_empty() {
            return None;
        }
        let signals: Vec<Option<SessionSignal>> =
            workers.iter().map(|(_, signal)| *signal).collect();
        let workers_said = match workers.len() {
            1 => "1 worker".to_string(),
            n => format!("{n} workers"),
        };
        let name = match waiting_first(&workers).map(|uid| self.worker_name(uid, cx)) {
            Some(who) => format!("{workers_said}; {who} needs you"),
            None => workers_said,
        };
        let rows: Vec<(u64, Option<SessionSignal>, SharedString)> = workers
            .iter()
            .map(|&(uid, signal)| (uid, signal, self.worker_name(uid, cx).into()))
            .collect();
        let trigger = crate::chat::composer::status_chip(
            "lead-chip",
            Icon::new(crate::icons::Icon::Users),
            summary(&signals),
            name,
            cx,
        );
        let pane = cx.entity().downgrade();
        Some(
            crate::controls::menu_below(("lead-workers", lead), trigger, move |menu, _, _| {
                rows.iter().fold(menu, |menu, (uid, signal, name)| {
                    let (uid, signal, name, pane) = (*uid, *signal, name.clone(), pane.clone());
                    menu.item(
                        crate::controls::menu_row(move |_, cx| {
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .child(onehand_ui::mark_slot(crate::rail::status_icon(signal, cx)))
                                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        })
                        .on_click(move |_, _, cx: &mut App| {
                            _ = pane.update(cx, |_: &mut Self, cx| {
                                cx.emit(ChatPaneEvent::ShowSession(uid));
                            });
                        }),
                    )
                })
            })
            // Above the chip, its right edge on the chip's: the row is the
            // composer's foot, with no room under it.
            .anchor(gpui::Anchor::BottomRight)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;
