//! The picked worker's detail, under the list: its head, the card it waits
//! on, its whole transcript following the newest output, and its composer
//! only once *Write to …* asks for it.

use super::short_name;
use crate::chat::pane::{ChatPane, ChatPaneEvent, SessionSignal};
use crate::chat::transcript;
use crate::chat::viewport;
use crate::control::Worker;
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, list, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::spinner::Spinner;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, IconName, Sizable as _, StyledExt as _};

impl ChatPane {
    /// The picked worker's detail, filling what the list leaves.
    pub(super) fn worker_detail(
        &mut self,
        worker: &Worker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let uid = worker.uid;
        // A worker coming up has nothing to show yet but that it is coming.
        let connecting = !self.worker_ready(uid, cx);
        let head = self.worker_head(worker, connecting, cx);
        let measure = self.workers.detail_height.clone();
        let detail = div()
            .id(("worker-detail", uid))
            .relative()
            .v_flex()
            .flex_1()
            .min_h_0()
            .track_focus(&self.workers.detail_focus)
            .child(
                gpui::canvas(
                    move |bounds, _, _| measure.set(bounds.size.height),
                    |_, _: (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(head);
        let session = self.session_of(uid).cloned();
        let Some(session) = session.filter(|_| !connecting) else {
            return detail.into_any_element();
        };
        let Some(list_state) = self.reproject_for(
            uid,
            viewport::TopRoom {
                head: px(0.),
                floor: px(0.),
            },
            cx,
        ) else {
            return detail.into_any_element();
        };
        let well = (list_state.viewport_bounds().size.height > px(0.))
            .then(|| list_state.viewport_bounds().size.height);
        // The detail is a dock's width: narrow whatever the window is.
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
        let writing = self.workers.writing == Some(uid);
        let composer = writing.then(|| self.worker_composer(uid, window, cx));
        // Only an open composer the person left idle hands its card the
        // caret: with none open the card takes nothing, so a worker starting
        // to wait never moves the caret.
        let idle = composer
            .as_ref()
            .is_some_and(|composer| super::super::body::composer_idle(composer, window, cx));
        // Bounded by the detail's own height, not the transcript's under it,
        // which is what the card leaves: bounded by that, the two would
        // shrink each other frame by frame.
        let height = self.workers.detail_height.get();
        let bound = (height > px(0.)).then_some(height);
        let cards = Self::cards(&session, bound, idle, window, cx);
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
        detail
            // The oldest card it waits on first, where the eye lands, and how
            // many more wait after it.
            .children(
                cards
                    .into_iter()
                    .map(|card| inset(div().pt_2()).child(card)),
            )
            .child(
                // The scrollbar lies over the transcript alone.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(transcript)
                    .child(onehand_ui::scrollbar(&list_state)),
            )
            .children(card.map(|card| {
                inset(div())
                    // Esc closes it, the draft kept, when the input has no
                    // use for the key itself.
                    .on_action(cx.listener(
                        |pane: &mut Self, _: &gpui_component::input::Escape, window, cx| {
                            pane.close_writing(window, cx);
                        },
                    ))
                    .child(card)
            }))
            .into_any_element()
    }

    /// The detail's head: its mark, name and state, its folder, who holds
    /// it, *Write to …*, *Stop* while a turn runs, and `⋯`.
    fn worker_head(
        &self,
        worker: &Worker,
        connecting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let uid = worker.uid;
        let signal = self.signal(uid, cx);
        let name = self.worker_name(uid, cx);
        let short = SharedString::from(format!("Write to {}…", short_name(&name)));
        let chat = self.session_of(uid).map(|session| &session.read(cx).chat);
        let held = worker
            .sent
            .zip(chat)
            .is_some_and(|(sent, chat)| chat.prompted_since(sent));
        let prompts = chat.map_or(0, |chat| chat.prompts_sent);
        let in_turn = chat.is_some_and(|chat| chat.busy);
        let give_back = crate::control::give_back_offered(uid, prompts, cx);
        let resend = self.can_resend(uid, cx);
        let writing = self.workers.writing == Some(uid);
        let folder = self.conversations.get(&uid).map(|conv| conv.root.clone());
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
                                let _ = back.update(cx, |pane: &mut Self, cx| {
                                    // Read now: the menu may have stood open
                                    // while a prompt went.
                                    let sent = pane
                                        .session_of(uid)
                                        .map_or(0, |session| session.read(cx).chat.prompts_sent);
                                    crate::control::give_back(uid, sent, cx);
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
            .border_b_1()
            .border_color(cx.theme().border)
            // Chrome, as the rail's rows: the transcript under it reads larger.
            .text_size(onehand_ui::tokens::CHROME_TEXT)
            .child(onehand_ui::mark_slot(mark))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_medium()
                    .text_color(cx.theme().foreground)
                    .child(name),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(word_ink)
                    .child(match connecting {
                        true => "Connecting",
                        false => crate::rail::word(signal),
                    }),
            )
            .children(folder.map(|folder| {
                let full = SharedString::from(folder.display().to_string());
                let last = folder.file_name().map_or_else(
                    || full.to_string(),
                    |last| last.to_string_lossy().into_owned(),
                );
                div()
                    .id(("worker-folder", uid))
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .child(last)
                    .tooltip(onehand_ui::tip(move |window, cx| {
                        Tooltip::new(full.clone()).build(window, cx)
                    }))
            }))
            .when(held, |head| {
                head.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child("held by you"),
                )
            })
            .child(div().flex_1())
            .when(!connecting && !writing, |head| {
                head.child(
                    crate::controls::action(("worker-write", uid))
                        .secondary()
                        .short()
                        .label(short)
                        .named_with_action(
                            "Write to this worker",
                            &super::WorkersWrite,
                            Some("WorkersList"),
                        )
                        .on_click(cx.listener(move |pane: &mut Self, _, window, cx| {
                            pane.write_to(uid, window, cx);
                        })),
                )
            })
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
