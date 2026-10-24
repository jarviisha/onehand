//! A settled question as the transcript keeps it: what was asked, and the answers.

use super::*;

/// The settled record of a question.
pub(super) fn ask_record(
    session: &Entity<ChatSession>,
    a: &AskItem,
    answer: &str,
    target: TranscriptItemId,
    cx: &App,
) -> gpui::AnyElement {
    let pairs = ask_pairs(a);
    // One question is already said whole by the row; several are a list worth
    // opening, and the count is what says there is one.
    let many = pairs.len() > 1;
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_ask(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("ask-record", fold_key(target)).into(),
        RowMark::Done,
        user_icon(),
        "Asked",
    )
    .object(Some(Object::words(match many {
        // The prompt that introduced the form, with the answers in the
        // column beside it -- a strip of question titles would be the
        // form's own chrome rather than what came of it.
        true => a.req.message.to_string(),
        false => pairs
            .first()
            .map(|(question, _)| question.to_string())
            .unwrap_or_else(|| a.req.message.to_string()),
    })))
    .meta(Some(match many {
        true => {
            pill(a.outcome_words().unwrap_or_else(|| answer.to_string()), cx).into_any_element()
        }
        false => pill(answer.to_string(), cx).into_any_element(),
    }))
    .fold(many.then_some(a.expanded));

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(many && a.expanded, |block| {
            block.child(detail_well(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    // **No rules between the pairs.** They are one answer given
                    // in several parts, not several records -- ruled apart they
                    // read as separate exchanges with the agent, which is the
                    // one thing a form is not.
                    .children(pairs.into_iter().map(|(question, chosen)| {
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap(PART_GAP)
                            .w_full()
                            .min_w_0()
                            .h(CONTROL_H)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(question),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(relative(0.5))
                                    .child(pill(chosen, cx)),
                            )
                    })),
            ))
        })
        .into_any_element()
}

/// Each question of a settled form paired with what was chosen for it.
///
/// A typed answer and a picked one come back the same shape, because from the
/// far side of the exchange they are the same thing: what the user said. Which
/// of the two it was is carried by weight, not by a different control.
pub(super) fn ask_pairs(a: &AskItem) -> Vec<(SharedString, SharedString)> {
    a.req
        .fields
        .iter()
        .enumerate()
        .map(|(f, field)| {
            let question = field
                .title
                .clone()
                .or_else(|| field.description.clone())
                .unwrap_or_else(|| format!("Question {}", f + 1));
            let typed = a.custom.get(f).map(String::as_str).unwrap_or("").trim();
            let chosen = match typed.is_empty() {
                false => typed.to_string(),
                true => {
                    let picks = a.picked.get(f).cloned().unwrap_or_default();
                    let choices = match &field.kind {
                        ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c.as_slice(),
                        ElicitKind::Text => &[],
                    };
                    picks
                        .iter()
                        .filter_map(|&i| choices.get(i).map(|c| c.label.to_string()))
                        .collect::<Vec<_>>()
                        .join(" · ")
                }
            };
            (
                SharedString::from(question),
                SharedString::from(match (chosen.is_empty(), a.field_answered(f)) {
                    // A multi-select moved on from with nothing picked: none
                    // was the answer, which a skip is not.
                    (true, true) => "none".to_string(),
                    (true, false) => "skipped".to_string(),
                    (false, _) => chosen,
                }),
            )
        })
        .collect()
}

/// The mark on a row that is about the person rather than the agent.
pub(in crate::chat::transcript) fn user_icon() -> SharedString {
    use gpui_component::IconNamed as _;
    IconName::User.path()
}
