//! The strip under the conversation header while a run drives the session,
//! and the two dialogs its approval controls open.

use super::{ChatPane, ChatPaneEvent};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, WeakEntity, Window, div, rems,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::task::Approval;
use onehand_core::task::work::{ANSWER_CHANGED, UnderReview};
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
                            let task = shown.task.to_string();
                            let pressed = Approval {
                                task: task.clone(),
                                at: review.at.clone(),
                            };
                            let (read, revised) = (review, pressed.clone());
                            row.child(
                                crate::controls::action("workflow-review")
                                    .xsmall()
                                    .ghost()
                                    .label("Review…")
                                    .tooltip("Read what is waiting for approval")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        let pane = cx.entity().downgrade();
                                        open_review(&task, pane, read.clone(), false, window, cx)
                                    })),
                            )
                            .child(
                                crate::controls::action("workflow-revise")
                                    .xsmall()
                                    .ghost()
                                    .label("Revise…")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        open_revise(revised.clone(), window, cx)
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
                                        press_continue(pane, &pressed, window, cx)
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

/// Where the run waits now, against where a press was drawn from.
enum Waits {
    /// Where the press was read.
    There,
    /// At another answer, which the press did not read.
    Elsewhere(Box<UnderReview>),
    /// On nobody.
    Nowhere,
}

/// Where the run `approval` was read from waits now.
fn waits(approval: &Approval, cx: &App) -> Waits {
    match crate::task::review_of(&approval.task, cx) {
        Some(now) if now.at == approval.at => Waits::There,
        Some(now) => Waits::Elsewhere(Box::new(now)),
        None => Waits::Nowhere,
    }
}

/// *Continue* on what was read, as `approval` names it: approved when the run
/// still waits there. Otherwise the answer changed under the reader, and what
/// it waits on now is put up to be read, rather than approved unread.
pub(super) fn press_continue(
    pane: WeakEntity<ChatPane>,
    approval: &Approval,
    window: &mut Window,
    cx: &mut App,
) {
    match waits(approval, cx) {
        Waits::There => {
            let approval = approval.clone();
            let _ = pane.update(cx, |_, cx| {
                cx.emit(ChatPaneEvent::ContinueWorkflow(approval))
            });
        }
        Waits::Elsewhere(now) => open_review(&approval.task, pane, *now, true, window, cx),
        // It no longer waits on anybody; where it went is drawn already.
        Waits::Nowhere => {}
    }
}

/// Put up what task `task`'s run waits on approval for, as `review` read it:
/// the answer the step kept, as the markdown it was written in, saying first
/// when it is not the answer a press was drawn from (`changed`).
///
/// Read from the run rather than the transcript: a run resumed in a new
/// session has no transcript holding it, and its approval would otherwise be
/// asked for blind.
pub(super) fn open_review(
    task: &str,
    pane: WeakEntity<ChatPane>,
    review: UnderReview,
    changed: bool,
    window: &mut Window,
    cx: &mut App,
) {
    window.close_dialog(cx);
    let approval = Approval {
        task: task.to_string(),
        at: review.at.clone(),
    };
    window.open_dialog(cx, move |dialog, _, cx| {
        let pane = pane.clone();
        let body = match review.answer.trim().is_empty() {
            true => div()
                .text_color(cx.theme().muted_foreground)
                .child("The step kept no answer.")
                .into_any_element(),
            false => gpui_component::text::TextView::markdown(
                "workflow-review-body",
                review.answer.clone(),
            )
            .selectable(true)
            .into_any_element(),
        };
        let approval = approval.clone();
        dialog
            .title(format!("{}: waiting for approval", review.of))
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .when(changed, |col| {
                        col.child(
                            div()
                                .text_sm()
                                .text_color(crate::theme::status_ink(cx).warning)
                                .child(ANSWER_CHANGED),
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
                                press_continue(pane.clone(), &approval, window, cx);
                            }),
                    ),
            )
    });
}

/// Put up the window that sends what the run waits on, as `approval` read
/// it, back to be done again, with what to change. Once the run no longer
/// waits there, sending is refused in place, keeping the note.
pub(super) fn open_revise(approval: Approval, window: &mut Window, cx: &mut Context<ChatPane>) {
    let pane = cx.entity().downgrade();
    let note =
        cx.new(|cx| TextareaState::new(window, cx).placeholder("What should be done differently?"));
    note.update(cx, |input, cx| input.focus(window, cx));
    let changed = Rc::new(Cell::new(false));
    window.open_dialog(cx, move |dialog, _, cx| {
        let send = {
            let (note, pane, approval, changed) = (
                note.clone(),
                pane.clone(),
                approval.clone(),
                changed.clone(),
            );
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let text = note.read(cx).value().trim().to_string();
                if text.is_empty() {
                    window.push_notification("Say what to change", cx);
                    return;
                }
                if !matches!(waits(&approval, cx), Waits::There) {
                    changed.set(true);
                    window.refresh();
                    return;
                }
                window.close_dialog(cx);
                let approval = approval.clone();
                let _ = pane.update(cx, |_, cx| {
                    cx.emit(ChatPaneEvent::ReviseWorkflow {
                        approval,
                        note: text,
                    })
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
                                .child(format!(
                                    "{ANSWER_CHANGED} Read it again with Review… first."
                                )),
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
