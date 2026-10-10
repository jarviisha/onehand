//! The agent asking the person something: a question it parked on, and what is
//! left of a question or a grant once it is answered.

use super::metrics::TEXT_SM;
use super::{fold_key, live_index};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::ElicitKind;
use onehand_core::chat::{AskItem, TranscriptItemId};

mod form;
mod pinned;
pub(super) use pinned::{Footer, Pinned, pinned_card};

// ── the record a settled exchange leaves ────────────────────────────────────
//
// **What is left of a question or a grant once it is answered is one line.**
// While either is waiting it is pinned above the composer, where the answer is
// given. Afterwards what the transcript owes a reader is the fact: the agent
// asked this, the person said that.

/// One settled exchange, as its line says it.
pub(super) struct Settled {
    pub(super) id: ElementId,
    /// *Allowed*, *Denied* or *Asked*.
    pub(super) verb: &'static str,
    /// What was asked: a command, in the machine face, or a question.
    pub(super) asked: SharedString,
    pub(super) mono: bool,
    /// A grant refused: what was asked is struck out, and the answer is in the
    /// danger ink.
    pub(super) refused: bool,
    pub(super) answer: SharedString,
    /// Whether the line opens onto more, and whether it is open.
    pub(super) fold: Option<bool>,
}

/// The line a settled exchange leaves: the person's glyph, the verb, what was
/// asked, a muted `·`, then the answer as plain words.
pub(super) fn settled_row(
    row: Settled,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let muted = cx.theme().muted_foreground;
    let line = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .text_size(TEXT_SM)
        .text_color(muted)
        .child(Icon::new(IconName::User).xsmall().flex_none())
        .child(div().flex_none().child(row.verb))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(match row.refused {
                    true => muted,
                    false => cx.theme().foreground,
                })
                .when(row.refused, |asked| asked.line_through())
                .when(row.mono, |asked| {
                    asked.font_family(cx.theme().mono_font_family.clone())
                })
                .child(row.asked),
        )
        .child(div().flex_none().child("·"))
        .child(
            div()
                .flex_none()
                .when(row.refused, |answer| {
                    answer.text_color(crate::theme::status_ink(cx).danger)
                })
                .child(row.answer),
        )
        .children(row.fold.map(|open| {
            Icon::new(match open {
                true => IconName::ChevronDown,
                false => IconName::ChevronRight,
            })
            .xsmall()
            .flex_none()
        }));
    match row.fold {
        // Something to open, so something to press: the ink lifts under the
        // pointer, with no plate behind the line.
        Some(_) => line
            .id(row.id)
            .cursor_pointer()
            .hover(|line| line.text_color(crate::theme::meta_ink(cx)))
            .on_click(on_click)
            .into_any_element(),
        None => line.into_any_element(),
    }
}

/// What a settled line opens onto, set in under its words.
pub(super) fn settled_detail(body: impl IntoElement, cx: &App) -> gpui::Div {
    div().w_full().min_w_0().pl_5().pt_1().child(
        div()
            .v_flex()
            .gap_1()
            .w_full()
            .min_w_0()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .text_size(TEXT_SM)
            .child(body),
    )
}

/// The settled record of a question.
fn ask_record(
    session: &Entity<ChatSession>,
    a: &AskItem,
    answer: &str,
    target: TranscriptItemId,
    cx: &App,
) -> gpui::AnyElement {
    let pairs = ask_pairs(a);
    // One question is already said whole by the line; several are a list worth
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
    let row = Settled {
        id: ("ask-record", fold_key(target)).into(),
        verb: "Asked",
        // The prompt that introduced the form where there were several
        // questions: a strip of their titles would be the form's chrome rather
        // than what came of it.
        asked: match many {
            true => a.req.message.clone().into(),
            false => pairs
                .first()
                .map(|(question, _)| question.clone())
                .unwrap_or_else(|| a.req.message.clone().into()),
        },
        mono: false,
        refused: false,
        answer: match many {
            true => format!("{} answers", pairs.len()).into(),
            false => answer.to_string().into(),
        },
        fold: many.then_some(a.expanded),
    };

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(settled_row(row, toggle, cx))
        .when(many && a.expanded, |block| {
            // One answer given in several parts, not several records, so no
            // rules between them.
            block.child(settled_detail(
                div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .children(pairs.into_iter().map(|(question, chosen)| {
                        div()
                            .h_flex()
                            .justify_between()
                            .gap_2()
                            .w_full()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(question),
                            )
                            .child(div().flex_none().child(chosen))
                    })),
                cx,
            ))
        })
        .into_any_element()
}

/// Each question of a settled form paired with what was chosen for it.
///
/// A typed answer and a picked one come back the same shape, because from the
/// far side of the exchange they are the same thing: what the person said.
fn ask_pairs(a: &AskItem) -> Vec<(SharedString, SharedString)> {
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
                SharedString::from(match chosen.is_empty() {
                    true => "skipped".to_string(),
                    false => chosen,
                }),
            )
        })
        .collect()
}

// ── the agent asking a question (`AskUserQuestion` / an MCP form) ───────────

pub(in crate::chat) fn ask(
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    if let Some(answer) = a.resolved.as_deref() {
        return ask_record(session, a, answer, target, cx);
    }
    form::ask_card(session, a, target, live_index(target), cx)
}
