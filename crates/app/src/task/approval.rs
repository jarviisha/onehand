//! The two dialogs that answer a run waiting on approval, wherever it is
//! offered: the answer it kept, to read and go on from, and the note that
//! sends it back. The conversation's step strip and a task's detail open
//! the same ones.

use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_area;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, InteractiveElement as _, IntoElement, ParentElement, Rems, Styled,
    Window, div, rems,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::TextareaState;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::task::Approval;
use onehand_core::task::work::{ANSWER_CHANGED, UnderReview};
use std::cell::Cell;
use std::rc::Rc;

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
pub(crate) fn press_continue(approval: &Approval, window: &mut Window, cx: &mut App) {
    match waits(approval, cx) {
        Waits::There => {
            // Deferred: the run reaches into its session, which may be
            // what offers this.
            let approval = approval.clone();
            cx.defer(move |cx| super::approve(approval, cx));
        }
        Waits::Elsewhere(now) => open_review(&approval.task, *now, true, window, cx),
        // It no longer waits on anybody; where it went is drawn already.
        Waits::Nowhere => {}
    }
}

/// How tall a review's answer grows before it scrolls, so the buttons that
/// answer it stay on screen.
const REVIEW_MAX_H: Rems = rems(28.);
/// How tall the note sent back stands: a few sentences in view.
const NOTE_H: Rems = rems(10.);

/// Put up what task `task`'s run waits on approval for, as `review` read it:
/// the answer the step kept, as the markdown it was written in, saying first
/// when it is not the answer a press was drawn from (`changed`).
///
/// Read from the run rather than the transcript: a run resumed in a new
/// session has no transcript holding it, and its approval would otherwise be
/// asked for blind.
pub(crate) fn open_review(
    task: &str,
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
        let body = match review.answer.trim().is_empty() {
            true => div()
                .text_color(cx.theme().muted_foreground)
                .child("The step kept no answer")
                .into_any_element(),
            false => onehand_ui::no_code_label(gpui_component::text::TextView::markdown(
                "workflow-review-body",
                review.answer.clone(),
            ))
            .style(onehand_ui::unedged_code())
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
                                .text_size(crate::tokens::CHROME_TEXT)
                                .text_color(crate::theme::status_ink(cx).warning)
                                .child(ANSWER_CHANGED),
                        )
                    })
                    .child(
                        div()
                            .id("workflow-review-scroll")
                            .max_h(REVIEW_MAX_H)
                            .child(body)
                            // With a bar of its own, so it still scrolls when the dialog body
                            // around it overflows too.
                            .overflow_y_scrollbar(),
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
                            .short()
                            .quiet(cx)
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("workflow-review-continue")
                            .short()
                            .primary()
                            .label("Continue")
                            .on_click(move |_, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                                press_continue(&approval, window, cx);
                            }),
                    ),
            )
    });
}

/// Put up the window that sends what the run waits on, as `approval` read
/// it, back to be done again, with what to change. Once the run no longer
/// waits there, sending is refused in place, keeping the note.
pub(crate) fn open_revise(approval: Approval, window: &mut Window, cx: &mut App) {
    let note =
        cx.new(|cx| TextareaState::new(window, cx).placeholder("What should be done differently?"));
    note.update(cx, |input, cx| input.focus(window, cx));
    let changed = Rc::new(Cell::new(false));
    window.open_dialog(cx, move |dialog, _, cx| {
        // Built again on every frame, so the refusal follows the typing.
        let empty = note.read(cx).value().trim().is_empty();
        let send = {
            let (note, approval, changed) = (note.clone(), approval.clone(), changed.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let text = note.read(cx).value().trim().to_string();
                if text.is_empty() {
                    return;
                }
                if !matches!(waits(&approval, cx), Waits::There) {
                    changed.set(true);
                    window.refresh();
                    return;
                }
                window.close_dialog(cx);
                let approval = approval.clone();
                cx.defer(move |cx| super::revise(approval, text, cx));
            }
        };
        dialog
            .title("Send it back")
            // A press beside it would throw the note being written away
            // without asking.
            .overlay_closable(false)
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
                                .text_size(crate::tokens::CHROME_TEXT)
                                .text_color(crate::theme::status_ink(cx).warning)
                                .child(format!(
                                    "{ANSWER_CHANGED} Read it again with Review… first."
                                )),
                        )
                    })
                    .child(text_area(&note, cx).h(NOTE_H)),
            )
            .footer(
                div()
                    .h_flex()
                    .gap_2()
                    .w_full()
                    .justify_end()
                    .child(
                        crate::controls::action("workflow-revise-cancel")
                            .short()
                            .quiet(cx)
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("workflow-revise-send")
                            .short()
                            .primary()
                            .label("Send back")
                            .refuses(empty.then_some("Say what to change first"))
                            .on_click(send),
                    ),
            )
    });
}
