//! The workers' list: a row per worker, pinned at the top of the region, and
//! the keys that move along it. The deck over the lead's composer draws its
//! rows through the same row.

use super::super::ChatPane;
use crate::chat::pane::SessionSignal;
use crate::control::Worker;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Rems,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};

/// A worker's row: its state on one line, what it does or waits on under.
pub(in crate::chat::pane) const WORKER_ROW_H: Rems = rems(2.75);

gpui::actions!(
    workers_list,
    [WorkersUp, WorkersDown, WorkersWrite, WorkersBack]
);

/// The row a key moves to among `len`, stopping at either end.
pub(in crate::chat::pane) fn step(row: usize, len: usize, down: bool) -> usize {
    match down {
        true => (row + 1).min(len.saturating_sub(1)),
        false => row.saturating_sub(1),
    }
}

impl ChatPane {
    /// The list: a row per worker in the frozen order, the picked one
    /// filled, ringed while the keys are on it.
    pub(super) fn workers_list(
        &mut self,
        workers: &[Worker],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // A lead opens at most this many, so the list never needs a cut.
        debug_assert!(workers.len() <= crate::control::OPENED_CAP);
        let keyed = self.workers.focus.is_focused(window);
        let rows: Vec<gpui::AnyElement> = self
            .workers
            .order
            .iter()
            .filter_map(|uid| workers.iter().find(|worker| worker.uid == *uid))
            .map(|worker| {
                let uid = worker.uid;
                let picked = self.workers.picked == Some(uid);
                let signal = self.signal(uid, cx);
                let age = self.worker_age(uid);
                self.worker_row(
                    worker,
                    signal,
                    age,
                    (picked, keyed && picked),
                    "workers-list-row",
                    move |pane, window, cx| pane.open_worker(uid, false, window, cx),
                    cx,
                )
            })
            .collect();
        div()
            .v_flex()
            .flex_none()
            .gap_px()
            .p_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .track_focus(&self.workers.focus)
            .key_context("WorkersList")
            .on_action(cx.listener(|pane: &mut Self, _: &WorkersUp, window, cx| {
                pane.step_pick(false, window, cx);
            }))
            .on_action(cx.listener(|pane: &mut Self, _: &WorkersDown, window, cx| {
                pane.step_pick(true, window, cx);
            }))
            .on_action(
                cx.listener(|pane: &mut Self, _: &WorkersWrite, window, cx| {
                    if let Some(uid) = pane.workers.picked {
                        pane.write_to(uid, window, cx);
                    }
                }),
            )
            .on_action(cx.listener(|pane: &mut Self, _: &WorkersBack, window, cx| {
                pane.reclaim_focus(window, cx);
            }))
            .children(rows)
    }

    /// The keys' move along the list: the row above or below picked, the
    /// caret staying on the list. The order stands: rows moving under the
    /// keys would send the next press somewhere the person did not aim.
    fn step_pick(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let order = &self.workers.order;
        let at = self
            .workers
            .picked
            .and_then(|picked| order.iter().position(|uid| *uid == picked))
            .unwrap_or(0);
        if let Some(&uid) = order.get(step(at, order.len(), down)) {
            self.pick(uid, window, cx);
        }
    }

    /// A worker's row: its mark, name, state, age, the lead's turn and its
    /// background count, and under them the card it waits on in the warning
    /// ink, else what it is doing; *Stop* while a turn runs. `(picked,
    /// keyed)`: filled, and ringed for the keys. A press runs `on_press` and
    /// leaves the caret where it was.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::chat::pane) fn worker_row(
        &self,
        worker: &Worker,
        signal: Option<SessionSignal>,
        age: u64,
        (picked, keyed): (bool, bool),
        id: &'static str,
        on_press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let uid = worker.uid;
        let muted = match picked {
            true => {
                onehand_ui::middle_ink(cx.theme().accent_foreground, cx.theme().muted_foreground)
            }
            false => cx.theme().muted_foreground,
        };
        let name_ink = match picked {
            true => cx.theme().accent_foreground,
            false => cx.theme().foreground,
        };
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
        onehand_ui::row(picked, cx)
            .relative()
            .h_flex()
            .items_center()
            .gap_2()
            .h(WORKER_ROW_H)
            .text_size(onehand_ui::tokens::CHROME_TEXT)
            // A press here leaves the caret where it was.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .when(keyed, |row| row.child(crate::rail::key_ring(cx)))
            .child(
                div()
                    .id((id, uid))
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .justify_center()
                    .on_click(cx.listener(move |pane: &mut Self, _, window, cx| {
                        on_press(pane, window, cx);
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
                                    .text_color(name_ink)
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
                    crate::controls::action(gpui::SharedString::from(format!("{id}-stop-{uid}")))
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
            .into_any_element()
    }
}
