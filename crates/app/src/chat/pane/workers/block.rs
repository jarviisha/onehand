//! One worker's block in the region: its head, and under it the
//! transcript's tail, or, open, the whole of it with the card it waits on
//! and a composer of its own.

use super::{WORKER_OPEN_MIN, WORKER_TAIL_H};
use crate::chat::pane::{ChatPane, ChatPaneEvent, SessionSignal, body};
use crate::chat::transcript;
use crate::chat::viewport;
use crate::control::Worker;
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement as _,
    Styled, Window, div, list, px,
};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme as _, IconName, Sizable as _, StyledExt as _};
use onehand_core::chat::Link;

impl ChatPane {
    /// Worker `worker`'s block; `tails` whether a folded one keeps its tail.
    pub(super) fn worker_block(
        &mut self,
        worker: &Worker,
        tails: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let uid = worker.uid;
        let open = self.workers.open == Some(uid);
        let session = self.session_of(uid).cloned();
        let was_live = self
            .conversations
            .get(&uid)
            .is_some_and(|conv| conv.was_live);
        // A worker coming up has nothing to show yet but that it is coming.
        let connecting = session.as_ref().is_none_or(|session| {
            let chat = &session.read(cx).chat;
            chat.link == Link::Connecting && !was_live
        });
        let head = self.worker_head(worker, connecting, cx);
        let block = div()
            .v_flex()
            .w_full()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(head);
        let Some(session) = session.filter(|_| !connecting) else {
            return block.flex_none().into_any_element();
        };
        let list_state = self.reproject_for(
            uid,
            viewport::TopRoom {
                head: px(0.),
                floor: px(0.),
            },
            cx,
        );
        let Some(list_state) = list_state else {
            return block.flex_none().into_any_element();
        };
        let well = (list_state.viewport_bounds().size.height > px(0.))
            .then(|| list_state.viewport_bounds().size.height);
        // A block is a dock's width: narrow whatever the window is.
        let room = transcript::Room::new(well, true);
        let this = cx.entity();
        let for_render = session.clone();
        let transcript = list(
            list_state.clone(),
            move |ix, window: &mut Window, cx: &mut App| {
                this.read(cx)
                    .run_element(ix, &for_render, room.clone(), window, cx)
            },
        )
        .size_full();
        let composer = open.then(|| self.worker_composer(uid, window, cx));
        let idle = composer
            .as_ref()
            .is_some_and(|composer| body::composer_idle(composer, window, cx));
        let cards = Self::cards(&session, well, idle, window, cx);
        let card = composer.map(|composer| {
            let blocked = {
                let composer = composer.read(cx);
                session
                    .read(cx)
                    .chat
                    .submit_blocker(&composer.text(cx), &composer.attachments)
            };
            composer.update(cx, |composer, cx| {
                composer.card(&session, blocked, false, cx)
            })
        });
        let inset = |el: gpui::Div| el.px_3().pb_2();
        match open {
            true => block
                .flex_1()
                .min_h(WORKER_OPEN_MIN)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(transcript)
                        .child(onehand_ui::scrollbar(&list_state)),
                )
                .children(cards.into_iter().map(|card| inset(div()).child(card)))
                .children(card.map(|card| inset(div()).child(card)))
                .into_any_element(),
            false => block
                .flex_none()
                .when(tails, |block| {
                    block.child(div().h(WORKER_TAIL_H).overflow_hidden().child(transcript))
                })
                .children(
                    cards
                        .into_iter()
                        .take(1)
                        .map(|card| inset(div()).child(card)),
                )
                .into_any_element(),
        }
    }

    /// A block's head: its mark, name and state, who holds it, *Stop* while
    /// a turn runs, and `⋯`. Pressed, the block opens with the caret in its
    /// composer.
    fn worker_head(
        &self,
        worker: &Worker,
        connecting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let uid = worker.uid;
        let signal = self.signal(uid, cx);
        let name = self.worker_name(uid, cx);
        let chat = self.session_of(uid).map(|session| &session.read(cx).chat);
        let held = worker
            .sent
            .zip(chat)
            .is_some_and(|(sent, chat)| chat.prompted_since(sent));
        let prompts = chat.map_or(0, |chat| chat.prompts_sent);
        let in_turn = chat.is_some_and(|chat| chat.busy);
        let give_back = crate::control::give_back_offered(uid, prompts, cx);
        let resend = self.can_resend(uid, cx);
        let muted = cx.theme().muted_foreground;
        let word_ink = crate::rail::word_ink(signal, cx).unwrap_or(muted);
        let mark = match connecting {
            true => Spinner::new().xsmall().color(muted).into_any_element(),
            false => crate::rail::status_icon(signal, cx),
        };
        let pane = cx.entity().downgrade();
        let more = crate::controls::action(("worker-more", uid))
            .quiet_icon(false, cx)
            .small()
            .icon(IconName::Ellipsis)
            .named("More for this worker")
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                let (back, again, close) = (pane.clone(), pane.clone(), pane.clone());
                menu.when(give_back, |menu| {
                    menu.item(
                        crate::controls::menu_item("Give back to the lead").on_click(
                            move |_, _, cx: &mut App| {
                                let _ = back.update(cx, |_: &mut Self, cx| {
                                    crate::control::give_back(uid, prompts, cx);
                                    cx.notify();
                                });
                            },
                        ),
                    )
                })
                .when(resend, |menu| {
                    menu.item(crate::controls::menu_item(crate::rail::RESEND).on_click(
                        move |_, _, cx: &mut App| {
                            let _ = again.update(cx, |pane: &mut Self, cx| {
                                pane.resend_last_prompt(uid, cx);
                            });
                        },
                    ))
                })
                .item(
                    crate::controls::menu_item("Close").on_click(move |_, _, cx: &mut App| {
                        let _ = close.update(cx, |_: &mut Self, cx| {
                            cx.emit(ChatPaneEvent::CloseWorker(uid));
                        });
                    }),
                )
            });
        div()
            .h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(onehand_ui::tokens::ROW_H)
            .px_3()
            .child(
                div()
                    .id(("worker-head", uid))
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .cursor_pointer()
                    .on_click(cx.listener(move |pane: &mut Self, _, window, cx| {
                        pane.open_worker(uid, true, window, cx);
                    }))
                    .child(onehand_ui::mark_slot(mark))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_medium()
                            .text_color(cx.theme().foreground)
                            .child(name),
                    )
                    .child(div().flex_none().text_xs().text_color(word_ink).child(
                        match connecting {
                            true => "Connecting",
                            false => crate::rail::word(signal),
                        },
                    ))
                    .children(worker.turn.map(|turn| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("turn {turn}"))
                    }))
                    .when(held, |head| {
                        head.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child("held by you"),
                        )
                    }),
            )
            .when(in_turn && signal != Some(SessionSignal::Lost), |head| {
                head.child(
                    crate::controls::action(("worker-stop", uid))
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
            .child(more)
    }
}
