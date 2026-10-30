use super::fold_key;
use super::metrics::{FOLD_INDENT, TEXT_SM};
use super::parts::{accent, fold_box, fold_line};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Entity, HighlightStyle, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    StyledText, div,
};
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::PlanStatus;
use onehand_core::chat::{PlanItem, TranscriptItemId};

/// Plan entries drawn before the list is truncated.
const MAX_TODO_ITEMS: usize = 50;

// ── plan / TodoWrite ────────────────────────────────────────────────────────

/// The agent's checklist: *Plan* and how much of it is done, opening into a
/// step per line, each marked by its state.
///
/// Open until the user closes it (`PlanItem::is_open`): it is what the turn is
/// working through, so it stays in sight.
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
    let hidden = p.entries.len().saturating_sub(MAX_TODO_ITEMS);
    let muted = cx.theme().muted_foreground;
    // Only the running turn's plan is worked through now.
    let busy = session.read(cx).chat.in_running_turn(target);

    let line = fold_line(("plan", fold_key(target)), open, false, cx)
        .on_click({
            let session = session.clone();
            move |_, _, cx: &mut App| {
                session.update(cx, |s, cx| {
                    s.chat.toggle_tool(target);
                    cx.notify();
                });
            }
        })
        .child("Plan")
        .child(
            div()
                .text_color(muted)
                .child(format!("{done}/{}", p.entries.len())),
        );

    fold_box(cx).child(line).when(open, |block| {
        block.child(
            div()
                .v_flex()
                .gap_1()
                .pl(FOLD_INDENT)
                .text_size(TEXT_SM)
                .children(p.entries.iter().take(MAX_TODO_ITEMS).map(|entry| {
                    // **The glyph changes, never the row**: a plan is
                    // worked through while it is on screen.
                    // A finished step is calm, a muted check; the one in
                    // hand turns while the turn runs and rests after.
                    let (icon, struck) = match entry.status {
                        PlanStatus::Completed => (Some(IconName::Check), true),
                        PlanStatus::InProgress if busy => (None, false),
                        PlanStatus::InProgress => (Some(IconName::LoaderCircle), false),
                        PlanStatus::Pending => (Some(IconName::Minus), false),
                    };
                    let mark = match icon {
                        Some(icon) => Icon::new(icon)
                            .xsmall()
                            .text_color(muted)
                            .into_any_element(),
                        None => Spinner::new().xsmall().color(accent(cx)).into_any_element(),
                    };
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(mark)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(match struck {
                                    true => muted,
                                    false => cx.theme().foreground,
                                })
                                // Finished work is struck: the eye skips
                                // it and lands on what is left.
                                .when(struck, |row| row.line_through())
                                .child(label(&entry.content, cx)),
                        )
                }))
                .when(hidden > 0, |list| {
                    list.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("+{hidden} more")),
                    )
                }),
        )
    })
}

/// A step's words with what it set in backticks in code's ink, as prose sets
/// it, the marks themselves taken out: inline code in a label too short to be
/// markdown.
fn label(text: &str, cx: &App) -> StyledText {
    let (text, spans) = code_spans(text);
    let ink = HighlightStyle {
        color: Some(crate::theme::hue_ink(cx.theme().blue, cx)),
        ..Default::default()
    };
    StyledText::new(text).with_highlights(spans.into_iter().map(|r| (r, ink)))
}

/// `text` without its backticks, and where each backticked span fell in what
/// is left.
fn code_spans(text: &str) -> (String, Vec<std::ops::Range<usize>>) {
    let mut out = String::new();
    let mut spans = Vec::new();
    for (i, part) in text.split('`').enumerate() {
        let start = out.len();
        out.push_str(part);
        if i % 2 == 1 {
            spans.push(start..out.len());
        }
    }
    (out, spans)
}

#[cfg(test)]
mod tests {
    #[test]
    fn backticks_mark_code_and_leave_the_text() {
        let (text, spans) = super::code_spans("call `wait` then `now()`.");
        assert_eq!(text, "call wait then now().");
        assert_eq!(spans, [5..9, 15..20]);
        assert!(super::code_spans("plain").1.is_empty());
    }
}
