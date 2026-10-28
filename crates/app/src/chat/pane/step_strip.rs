//! The strip under the conversation header while a run drives the session.

use super::{ChatPane, ChatPaneEvent};
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use crate::task::approval::{open_review, open_revise, press_continue};
use gpui::prelude::FluentBuilder as _;
use gpui::{Context, IntoElement, ParentElement, Styled, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::task::Approval;

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
                    row.child(Icon::new(IconName::ChevronRight).xsmall().text_color(muted))
                })
                .when(i < at_step, |row| {
                    row.child(Icon::new(IconName::CircleCheck).xsmall().text_color(muted))
                })
                .child(
                    div()
                        .child(label)
                        .text_color(if i == at_step { foreground } else { muted }),
                )
        });
        Some(
            onehand_ui::strip(cx)
                .child(div().flex_none().child(shown.name))
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
                        .gap_2()
                        .when_some(shown.review, |row, review| {
                            let task = shown.task.to_string();
                            let pressed = Approval {
                                task: task.clone(),
                                at: review.at.clone(),
                            };
                            let (read, revised) = (review, pressed.clone());
                            row.child(
                                crate::controls::action("workflow-review")
                                    .short()
                                    .quiet(cx)
                                    .label("Review…")
                                    .tooltip("Read what is waiting for approval")
                                    .on_click(move |_, window, cx| {
                                        open_review(&task, read.clone(), false, window, cx)
                                    }),
                            )
                            .child(
                                crate::controls::action("workflow-revise")
                                    .short()
                                    .quiet(cx)
                                    .label("Revise…")
                                    .on_click(move |_, window, cx| {
                                        open_revise(revised.clone(), window, cx)
                                    }),
                            )
                            .child(
                                crate::controls::action("workflow-continue")
                                    .short()
                                    .primary()
                                    .label("Continue")
                                    .tooltip("Approve it and go on to the next step")
                                    .on_click(move |_, window, cx| {
                                        press_continue(&pressed, window, cx)
                                    }),
                            )
                        })
                        .child(
                            crate::controls::action("workflow-stop")
                                .short()
                                .quiet(cx)
                                .label("Stop")
                                .loading(shown.stopping)
                                .tooltip("Cancel the turn and end the run here")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(ChatPaneEvent::StopWorkflow(uid))
                                })),
                        ),
                ),
        )
    }
}
