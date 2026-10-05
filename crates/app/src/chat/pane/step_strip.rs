//! The strip under the conversation header while a run drives the session,
//! and the two dialogs its approval controls open.

use super::{ChatPane, ChatPaneEvent};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

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
                        .when_some(shown.review, |row, (of, answer)| {
                            row.child(
                                crate::controls::action("workflow-review")
                                    .xsmall()
                                    .ghost()
                                    .label("Review…")
                                    .tooltip("Read what is waiting for approval")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        open_review(uid, of.clone(), answer.clone(), window, cx)
                                    })),
                            )
                            .child(
                                crate::controls::action("workflow-revise")
                                    .xsmall()
                                    .ghost()
                                    .label("Revise…")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        open_revise(uid, window, cx)
                                    })),
                            )
                            .child(
                                crate::controls::action("workflow-continue")
                                    .xsmall()
                                    .primary()
                                    .icon(Icon::new(IconName::Check))
                                    .label("Continue")
                                    .tooltip("Approve it and go on to the next step")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(ChatPaneEvent::ContinueWorkflow(uid))
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

/// Put up what session `uid`'s run waits on approval for: `answer`, the
/// answer step `of` kept, as the markdown it was written in.
///
/// Read from the run rather than the transcript: a run resumed in a new
/// session has no transcript holding it, and its approval would otherwise be
/// asked for blind.
fn open_review(
    uid: u64,
    of: SharedString,
    answer: SharedString,
    window: &mut Window,
    cx: &mut Context<ChatPane>,
) {
    let pane = cx.entity().downgrade();
    window.open_dialog(cx, move |dialog, _, cx| {
        let pane = pane.clone();
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
                    .id("workflow-review-scroll")
                    .max_h(rems(28.))
                    .overflow_y_scroll()
                    .child(body),
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
                                let _ = pane.update(cx, |_, cx| {
                                    cx.emit(ChatPaneEvent::ContinueWorkflow(uid))
                                });
                            }),
                    ),
            )
    });
}

/// Put up the window that sends what session `uid`'s run waits on back to
/// be done again, with what to change.
fn open_revise(uid: u64, window: &mut Window, cx: &mut Context<ChatPane>) {
    let pane = cx.entity().downgrade();
    let note =
        cx.new(|cx| TextareaState::new(window, cx).placeholder("What should be done differently?"));
    note.update(cx, |input, cx| input.focus(window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let send = {
            let (note, pane) = (note.clone(), pane.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let text = note.read(cx).value().trim().to_string();
                if text.is_empty() {
                    window.push_notification("Say what to change", cx);
                    return;
                }
                window.close_dialog(cx);
                let _ = pane.update(cx, |_, cx| {
                    cx.emit(ChatPaneEvent::ReviseWorkflow { uid, note: text })
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
