use super::fold_key;
use super::metrics::{
    BUTTON_H, FRAME_PAD, LINE_H, PART_GAP, PLAN_BAR_H, PLAN_BOX, PLAN_DOT, STACK_GAP, TEXT_PAD_X,
    WORK_TEXT, radius_block, radius_tag,
};
use super::parts::chevron_slot;
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, Entity, IntoElement, ParentElement, Styled, div, relative};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::acp::PlanStatus;
use onehand_core::chat::{PlanItem, TranscriptItemId};

/// Plan entries drawn before the list is truncated.
const MAX_TODO_ITEMS: usize = 50;

// ── plan / TodoWrite ────────────────────────────────────────────────────────

/// The agent's checklist.
///
/// Folds like every other card, and force-opens while an entry is in progress
/// — a plan is only worth the space it takes while it is being worked through,
/// and a finished twenty-item list between two answers is twenty rows of
/// history nobody is reading. Both rules are the model's (`PlanItem::is_open`),
/// the same pair a tool card follows.
pub(super) fn plan(
    session: &Entity<ChatSession>,
    p: &PlanItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let open = p.is_open();
    let done = p
        .entries
        .iter()
        .filter(|e| e.status == PlanStatus::Completed)
        .count();
    let rows = p
        .entries
        .iter()
        .take(if open { MAX_TODO_ITEMS } else { 0 })
        .map(|entry| {
            // Pending draws a dot rather than an icon: there is no glyph for
            // "not started" that is not just noise, and the row still needs to
            // occupy the marker column so the contents stay aligned.
            // **The box never changes size, only what is in it.** A plan is
            // worked through while it is on screen, so an entry moving from
            // pending to running to done is the one thing here guaranteed to
            // happen under the reader's eye — and a marker that grew or shrank
            // as it changed would reflow every line beneath it each time.
            let ink = match entry.status {
                PlanStatus::Completed => crate::theme::status_ink(cx).success,
                PlanStatus::InProgress => crate::theme::status_ink(cx).warning,
                PlanStatus::Pending => cx.theme().muted_foreground,
            };

            div()
                .h_flex()
                .items_center()
                .gap(PART_GAP)
                .w_full()
                .min_w_0()
                .h(BUTTON_H)
                .child(
                    div()
                        .flex_none()
                        .size(PLAN_BOX)
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius_tag(cx))
                        .border_1()
                        .border_color(ink)
                        .text_color(ink)
                        .map(|box_| match entry.status {
                            PlanStatus::Completed => {
                                box_.child(Icon::new(IconName::Check).size_3())
                            }
                            // Running is the border alone, because a second
                            // glyph in this column would have to be learned
                            // before the column could be read at all.
                            PlanStatus::InProgress => box_,
                            // A dot rather than an empty box: nothing at all
                            // reads as a box that failed to draw its tick.
                            PlanStatus::Pending => {
                                box_.child(div().size(PLAN_DOT).rounded_full().bg(ink))
                            }
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        // A struck-through line is finished work: the eye skips
                        // it and lands on what is left, which is the only part
                        // of a plan anyone is reading it for.
                        .when(entry.status == PlanStatus::Completed, |row| {
                            row.line_through().text_color(cx.theme().muted_foreground)
                        })
                        .child(entry.content.clone()),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let hidden = if open {
        p.entries.len().saturating_sub(rows.len())
    } else {
        0
    };
    let total = p.entries.len().max(1);
    // The header and checklist are separate regions of the card. The list is
    // intentionally denser inside itself, but it must not pull its first row
    // closer to the title than a tool card pulls detail to its header.
    let body = (!rows.is_empty() || hidden > 0).then(|| {
        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .pt(PART_GAP)
            .children(rows)
            .when(hidden > 0, |list| {
                list.child(
                    div()
                        .h(BUTTON_H)
                        .h_flex()
                        .items_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("+{hidden} more")),
                )
            })
    });

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .py(FRAME_PAD)
        .px(TEXT_PAD_X)
        .rounded(radius_block(cx))
        .border_1()
        .border_color(cx.theme().border)
        // A checklist is the agent's working notes, drawn at the size the rest
        // of its working steps are: the entries are read down as a list, not
        // across as prose, and at the answer's size a twenty-item plan is a
        // wall between two paragraphs.
        .text_size(WORK_TEXT)
        .child(
            crate::controls::action(("plan", fold_key(target)))
                .ghost()
                .w_full()
                .min_w_0()
                .h(LINE_H)
                .p_0()
                .on_click({
                    let session = session.clone();
                    move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            s.chat.toggle_tool(target);
                            cx.notify();
                        });
                    }
                })
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(PART_GAP)
                        .w_full()
                        .min_w_0()
                        .child(chevron_slot(Some(open), cx))
                        .child(
                            div()
                                .flex_none()
                                .font_semibold()
                                .text_color(cx.theme().foreground)
                                .child("Plan"),
                        )
                        .child(div().flex_1())
                        // Collapsed, the header is the whole card, so it has to
                        // say what the list said: how much of it is done.
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{done}/{}", p.entries.len())),
                        ),
                ),
        )
        // **The same count as a length.** The figure beside the title is exact
        // and is read a digit at a time; this is the one that answers "nearly
        // done or barely started" without being read at all. A rule's weight
        // rather than a control's, because it is not one — nothing here can be
        // dragged, and a bar thick enough to look draggable says it can.
        .child(
            div()
                .w_full()
                .h(PLAN_BAR_H)
                .mt(STACK_GAP)
                .rounded(cx.theme().radius)
                .bg(cx.theme().border)
                .child(
                    div()
                        .h_full()
                        .w(relative(done as f32 / total as f32))
                        .rounded(cx.theme().radius)
                        .bg(crate::theme::status_ink(cx).success),
                ),
        )
        .children(body)
}
