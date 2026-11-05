//! The lead's strip: one line on a surface of its own above the lead's
//! composer, narrower than it, while the lead on screen has workers. It
//! counts them, a count per state, and names the most urgent one waiting on
//! the person. Pressed, the most urgent worker shows in the pane, as its row
//! under the lead in the rail would show it.

use super::over_composer::over_composer;
use super::{ChatPane, ChatPaneEvent, SessionSignal};
use crate::control::Worker;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement as _,
    Styled, div,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};
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

    /// The strip over the lead's composer, while the lead on screen has
    /// workers.
    pub(super) fn lead_strip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let workers: Vec<(u64, Option<SessionSignal>)> = self
            .lead_workers()
            .iter()
            .map(|worker| (worker.uid, self.signal(worker.uid, cx)))
            .collect();
        let most_urgent = workers
            .iter()
            .min_by(|(_, a), (_, b)| urgency(*a, *b))
            .map(|(uid, _)| *uid)?;
        let signals: Vec<Option<SessionSignal>> =
            workers.iter().map(|(_, signal)| *signal).collect();
        let waiting = waiting_first(&workers).map(|uid| self.worker_name(uid, cx));
        let muted = cx.theme().muted_foreground;
        let line =
            over_composer("lead-strip", cx)
                .text_color(muted)
                .cursor_pointer()
                // A press answers the pointer as a line row does: its ink lifts.
                .hover(|line| line.text_color(cx.theme().foreground))
                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                    cx.emit(ChatPaneEvent::ShowSession(most_urgent));
                }))
                .child(div().flex_none().text_color(cx.theme().foreground).child(
                    match workers.len() {
                        1 => "1 worker".to_string(),
                        n => format!("{n} workers"),
                    },
                ))
                .children(summary(&signals).into_iter().map(|(signal, n)| {
                    div()
                        .h_flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .text_color(crate::rail::word_ink(signal, cx).unwrap_or(muted))
                        .child(crate::rail::status_icon(signal, cx))
                        .child(n.to_string())
                }))
                .child(div().flex_1())
                .children(waiting.map(|name| {
                    // The name gives way before the words that say what the ink
                    // means.
                    div()
                        .h_flex()
                        .min_w_0()
                        .text_color(crate::theme::status_ink(cx).warning)
                        .child(div().min_w_0().truncate().child(name))
                        .child(div().flex_none().child("\u{a0}needs you"))
                }));
        // Narrower than the composer it stands on.
        Some(div().w_full().px_3().child(line).into_any_element())
    }
}

#[cfg(test)]
mod tests;
