use super::fold_key;
use super::metrics::{FOLD_INDENT, TEXT_SM};
use super::parts::{WorkingLine, fold_box, fold_line, working_text};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Entity, HighlightStyle, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    StyledText, div,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::PlanStatus;
use onehand_core::chat::{PlanItem, TranscriptItemId};

/// Plan entries drawn before the list is truncated.
const MAX_TODO_ITEMS: usize = 50;

// ── plan / TodoWrite ────────────────────────────────────────────────────────

/// The agent's checklist: *Plan* and how much of it is done, opening into a
/// step per line, each marked by its state.
///
/// Open until the user closes it: it is what the turn is working through, so
/// it stays in sight.
pub(super) fn plan(
    session: &Entity<ChatSession>,
    p: &PlanItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let open = session.read(cx).item_is_open(target, true);
    let done = p
        .entries
        .iter()
        .filter(|e| e.status == PlanStatus::Completed)
        .count();
    let hidden = p.entries.len().saturating_sub(MAX_TODO_ITEMS);
    let muted = cx.theme().muted_foreground;
    // Only the running turn's plan is worked through now.
    let busy = session.read(cx).chat().in_running_turn(target);

    let line = fold_line(("plan", fold_key(target)), open, cx)
        .on_click({
            let session = session.clone();
            move |_, _, cx: &mut App| {
                session.update(cx, |s, cx| {
                    s.toggle_item(target, true);
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

    fold_box().child(line).when(open, |block| {
        block.child(
            div()
                .v_flex()
                .gap_1()
                .pl(FOLD_INDENT)
                .text_size(TEXT_SM)
                .children(
                    p.entries
                        .iter()
                        .take(MAX_TODO_ITEMS)
                        .enumerate()
                        .map(|(ix, entry)| {
                            // **The glyph says where a step stands and never
                            // turns**: a muted check once done, a ring for the
                            // one in hand whether or not the turn is running,
                            // a dash for one not begun. That the one in hand is
                            // being worked on now is told by its words.
                            let icon = match entry.status {
                                PlanStatus::Completed => IconName::Check,
                                PlanStatus::InProgress => IconName::LoaderCircle,
                                PlanStatus::Pending => IconName::Minus,
                            };
                            let mark = Icon::new(icon).xsmall().text_color(muted);
                            let words = match (entry.status, busy) {
                                (PlanStatus::InProgress, true) => {
                                    let (text, code) = code_spans(&entry.content);
                                    // On the quiet ink, so the band has
                                    // something to rise from.
                                    div().min_w_0().text_color(muted).child(working_text(
                                        ("plan-in-hand", ix),
                                        WorkingLine::words(text).tinted(code, code_ink(cx)),
                                        cx,
                                    ))
                                }
                                // Finished work is struck: the eye skips it
                                // and lands on what is left.
                                (PlanStatus::Completed, _) => div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(muted)
                                    .line_through()
                                    .child(label(&entry.content, cx)),
                                (PlanStatus::InProgress | PlanStatus::Pending, _) => div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().foreground)
                                    .child(label(&entry.content, cx)),
                            };
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .child(mark)
                                .child(words)
                        }),
                )
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
        color: Some(code_ink(cx)),
        ..Default::default()
    };
    StyledText::new(text).with_highlights(spans.into_iter().map(|r| (r, ink)))
}

/// The ink of what a step sets in backticks.
fn code_ink(cx: &App) -> gpui::Hsla {
    crate::theme::hue_ink(cx.theme().blue, cx)
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
