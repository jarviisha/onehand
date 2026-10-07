//! The strip under the conversation header while a run drives the session,
//! and the two dialogs its approval controls open.

use super::{ChatPane, ChatPaneEvent};
use crate::task::Review;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, WeakEntity, Window, div, rems,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow::ApprovalAt;
use std::cell::Cell;
use std::rc::Rc;

impl ChatPane {
    /// Where the run driving this session stands: its template's
    /// name, then every step in order, the ones behind it checked off and the
    /// one it is at in full ink, and *Stop* at the far end. `None` for a
    /// session no run drives.
    ///
    /// The transcript says each step as it starts, but a line scrolled past is
    /// not an answer to "how far along is it", and the steps are few enough to
    /// be read in one glance. A run waiting for approval is approved here, in
    /// its own session, so *Revise…* and *Continue* come before *Stop*.
    pub(super) fn workflow_strip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let uid = self.active?;
        let shown = crate::task::shown(uid, cx)?;
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let at_step = shown.at;
        let steps = shown.steps.into_iter().enumerate().map(move |(i, label)| {
            div()
                .h_flex()
                .flex_none()
                .items_center()
                .gap_1()
                .when(i > 0, |row| {
                    row.child(Icon::new(IconName::ChevronRight).size_3().text_color(muted))
                })
                .when(i < at_step, |row| {
                    row.child(Icon::new(IconName::CircleCheck).size_3().text_color(muted))
                })
                .child(
                    div()
                        .child(label)
                        .text_color(if i == at_step { foreground } else { muted })
                        .when(i == at_step, |label| label.font_semibold()),
                )
        });
        Some(
            div()
                .h_flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_4()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(div().flex_none().mr_1().child(shown.name))
                .child(
                    div()
                        .h_flex()
                        .min_w_0()
                        .overflow_hidden()
                        .gap_1()
                        .children(steps),
                )
                .child(
                    div()
                        .h_flex()
                        .flex_none()
                        .ml_auto()
                        .gap_1()
                        .when_some(shown.review, |row, review| {
                            let (read, revised, pressed) =
                                (review.clone(), review.at.clone(), review.at);
                            row.child(
                                crate::controls::action("workflow-review")
                                    .xsmall()
                                    .ghost()
                                    .label("Review…")
                                    .tooltip("Read what is waiting for approval")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        let pane = cx.entity().downgrade();
                                        open_review(uid, pane, read.clone(), false, window, cx)
                                    })),
                            )
                            .child(
                                crate::controls::action("workflow-revise")
                                    .xsmall()
                                    .ghost()
                                    .label("Revise…")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        open_revise(uid, revised.clone(), window, cx)
                                    })),
                            )
                            .child(
                                crate::controls::action("workflow-continue")
                                    .xsmall()
                                    .primary()
                                    .icon(Icon::new(IconName::Check))
                                    .label("Continue")
                                    .tooltip("Approve it and go on to the next step")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        let pane = cx.entity().downgrade();
                                        press_continue(uid, pane, &pressed, window, cx)
                                    })),
                            )
                        })
                        .child(
                            crate::controls::action("workflow-stop")
                                .xsmall()
                                .ghost()
                                .label("Stop")
                                .tooltip("Cancel the turn and end the run here")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(ChatPaneEvent::StopWorkflow(uid))
                                })),
                        ),
                ),
        )
    }
}

/// What the run on session `uid` waits for approval on now, when it still
/// waits at `at`; otherwise what it waits on instead, which a press drawn
/// from `at` would not have read.
fn still_at(uid: u64, at: &ApprovalAt, cx: &App) -> Result<(), Option<Review>> {
    match crate::task::shown(uid, cx).and_then(|shown| shown.review) {
        Some(now) if &now.at == at => Ok(()),
        now => Err(now),
    }
}

/// *Continue* on what was read at `at`: approved when the run still waits
/// there. Otherwise the answer changed under the reader, and what it waits
/// on now is put up to be read, rather than approved unread.
fn press_continue(
    uid: u64,
    pane: WeakEntity<ChatPane>,
    at: &ApprovalAt,
    window: &mut Window,
    cx: &mut App,
) {
    match still_at(uid, at, cx) {
        Ok(()) => {
            let at = at.clone();
            let _ = pane.update(cx, |_, cx| {
                if let Some(task) = crate::task::shown(uid, cx).map(|shown| shown.task) {
                    cx.emit(ChatPaneEvent::ContinueWorkflow {
                        task: task.to_string(),
                        at,
                    })
                }
            });
        }
        Err(Some(now)) => open_review(uid, pane, now, true, window, cx),
        // It no longer waits on anybody; the strip says where it went.
        Err(None) => {}
    }
}

/// Put up what session `uid`'s run waits on approval for: the answer the
/// step kept, as the markdown it was written in, saying first when it is not
/// the answer a press was drawn from.
///
/// Read from the run rather than the transcript: a run resumed in a new
/// session has no transcript holding it, and its approval would otherwise be
/// asked for blind.
fn open_review(
    uid: u64,
    pane: WeakEntity<ChatPane>,
    review: Review,
    changed: bool,
    window: &mut Window,
    cx: &mut App,
) {
    window.close_dialog(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let pane = pane.clone();
        let Review { of, answer, at } = review.clone();
        let body = match answer.trim().is_empty() {
            true => div()
                .text_color(cx.theme().muted_foreground)
                .child("The step kept no answer.")
                .into_any_element(),
            false => {
                gpui_component::text::TextView::markdown("workflow-review-body", answer.clone())
                    .selectable(true)
                    .into_any_element()
            }
        };
        dialog
            .title(format!("{of}: waiting for approval"))
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .when(changed, |col| {
                        col.child(
                            div()
                                .text_sm()
                                .text_color(crate::theme::status_ink(cx).warning)
                                .child(CHANGED),
                        )
                    })
                    .child(
                        div()
                            .id("workflow-review-scroll")
                            .max_h(rems(28.))
                            .overflow_y_scroll()
                            .child(body),
                    ),
            )
            .footer(
                div()
                    .h_flex()
                    .gap_2()
                    .w_full()
                    .justify_end()
                    .child(
                        crate::controls::action("workflow-review-close")
                            .small()
                            .ghost()
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("workflow-review-continue")
                            .small()
                            .primary()
                            .icon(Icon::new(IconName::Check))
                            .label("Continue")
                            .on_click(move |_, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                                press_continue(uid, pane.clone(), &at, window, cx);
                            }),
                    ),
            )
    });
}

/// What a refused press says, where the reader is.
const CHANGED: &str = "The answer changed since you opened it.";

/// Put up the window that sends what session `uid`'s run waits on, as read at
/// `at`, back to be done again, with what to change. Once the run no longer
/// waits there, sending is refused in place, keeping the note.
fn open_revise(uid: u64, at: ApprovalAt, window: &mut Window, cx: &mut Context<ChatPane>) {
    let pane = cx.entity().downgrade();
    let note =
        cx.new(|cx| TextareaState::new(window, cx).placeholder("What should be done differently?"));
    note.update(cx, |input, cx| input.focus(window, cx));
    let changed = Rc::new(Cell::new(false));
    window.open_dialog(cx, move |dialog, _, cx| {
        let send = {
            let (note, pane, at, changed) =
                (note.clone(), pane.clone(), at.clone(), changed.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let text = note.read(cx).value().trim().to_string();
                if text.is_empty() {
                    window.push_notification("Say what to change", cx);
                    return;
                }
                if still_at(uid, &at, cx).is_err() {
                    changed.set(true);
                    window.refresh();
                    return;
                }
                window.close_dialog(cx);
                let at = at.clone();
                let _ = pane.update(cx, |_, cx| {
                    if let Some(task) = crate::task::shown(uid, cx).map(|shown| shown.task) {
                        cx.emit(ChatPaneEvent::ReviseWorkflow {
                            task: task.to_string(),
                            at,
                            note: text,
                        })
                    }
                });
            }
        };
        dialog
            .title("Send it back")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "The step it approves runs again, with this note and the \
                                 answer it gave before.",
                            ),
                    )
                    .when(changed.get(), |col| {
                        col.child(
                            div()
                                .text_sm()
                                .text_color(crate::theme::status_ink(cx).warning)
                                .child(format!("{CHANGED} Read it again with Review… first.")),
                        )
                    })
                    .child(Textarea::new(&note).h(rems(10.))),
            )
            .footer(
                div()
                    .h_flex()
                    .gap_2()
                    .w_full()
                    .justify_end()
                    .child(
                        crate::controls::action("workflow-revise-cancel")
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("workflow-revise-send")
                            .small()
                            .primary()
                            .label("Send back")
                            .on_click(send),
                    ),
            )
    });
}
