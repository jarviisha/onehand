//! The lead's strip: one line on a surface of its own above the lead's
//! composer, narrower than it, while the lead on screen has workers. It
//! counts them, a count per state, and names the most urgent one waiting on
//! the person. Pressed, the workers' region shows with that worker picked.
//!
//! **It stays while the region shows**: one line, which says what waits
//! without the person turning to the region. **The pointer leaves the caret
//! where it was**: the press is kept from the pane, which would take focus.

use super::workers::urgency;
use super::{ChatPane, ChatPaneEvent, SessionSignal};
use gpui::{
    Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement as _, Styled, div,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};

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
            div()
                .id("lead-strip")
                .h_flex()
                .items_center()
                .gap_2()
                .h(onehand_ui::tokens::ROW_H)
                .px_3()
                // Set off by its fill and the composer's lift, as the card it
                // stands on: no edge to add to its height.
                .bg(crate::theme::raised(cx))
                .shadow(crate::theme::composer_lift(cx))
                .rounded_t(cx.theme().radius)
                .text_size(onehand_ui::tokens::CHROME_TEXT)
                .text_color(muted)
                .cursor_pointer()
                // Kept from the pane, which takes focus on a press: the caret
                // stays where it was.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                    cx.emit(ChatPaneEvent::ShowWorker(most_urgent));
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
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(crate::theme::status_ink(cx).warning)
                        .child(format!("{name} needs you"))
                }));
        // Narrower than the composer it stands on.
        Some(div().w_full().px_3().child(line).into_any_element())
    }
}

#[cfg(test)]
mod tests;
