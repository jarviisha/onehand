//! A subagent's card: one box where the agent handed work off, its head the
//! subagent's name and how it ended, its body the steps it took.
//!
//! **Its steps are drawn as rows that open into nothing.** A step's own fold
//! lives on the item in core and is addressed by its place in the transcript,
//! which a step inside a card does not have; so a tool is its row as the
//! transcript draws it, a thought and a plan a row of their own, and the
//! subagent's words its prose, parsed by the same cache as the agent's.

use super::metrics::{FOLD_INDENT, TEXT_SM};
use super::parts::{ActivityRow, Object, RowMark, activity_row, chevron_slot, show_more};
use super::prose::md_view;
use super::tool::step_row;
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme, IconName, IconNamed as _, StyledExt};
use onehand_core::acp::PlanStatus;
use onehand_core::chat::{ChatItem, SubagentItem, SubagentState, activity};

/// How many of a working card's newest steps show while it is folded: enough
/// to see what it is doing, few enough that several working at once do not
/// push the conversation off screen.
const SUBAGENT_LIVE_STEPS: usize = 3;

/// Steps an opened card draws before it says how many more it holds.
const SUBAGENT_STEP_CAP: usize = 200;

/// The card for one subagent, nested cards drawn inside it the same way.
pub(super) fn subagent(
    session: &Entity<ChatSession>,
    card: &SubagentItem,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    let open = session.read(cx).subagent_is_open(&card.id);
    let working = card.state == SubagentState::Working;
    let steps = card.items.len();
    let toggle = {
        let session = session.clone();
        let id = card.id.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |session, cx| {
                session.toggle_subagent(&id);
                cx.notify();
            });
        }
    };

    // Opened: every step up to the cap. Working: the newest few. Ended and
    // folded: none, the head says how many there were.
    let shown = match (open, working) {
        (true, _) => 0..steps.min(SUBAGENT_STEP_CAP),
        (false, true) => steps.saturating_sub(SUBAGENT_LIVE_STEPS)..steps,
        (false, false) => steps..steps,
    };
    let earlier = shown.start;
    let more = steps - shown.end;
    let root = session.read(cx).chat.root.clone();
    let rows: Vec<gpui::AnyElement> = shown
        .clone()
        .filter_map(|ix| {
            let id = ElementId::NamedInteger(
                SharedString::from(format!("subagent-step-{}", card.id)),
                ix as u64,
            );
            step(session, &card.items[ix], id, &root, window, cx)
        })
        .collect();

    onehand_ui::card_box(cx)
        .w_full()
        .child(head(card, open, steps, toggle.clone(), cx))
        // The task says what the name cannot; a folded card that has ended is
        // the one line its head is.
        .children(card.task.as_ref().filter(|_| open || working).map(|task| {
            div()
                .w_full()
                .min_w_0()
                .pl(FOLD_INDENT)
                .truncate()
                .text_size(TEXT_SM)
                .text_color(crate::theme::meta_ink(cx))
                .child(task.clone())
        }))
        .when(!rows.is_empty() || earlier > 0 || more > 0, |card_box| {
            card_box.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    .pl(FOLD_INDENT)
                    .gap_2()
                    .when(earlier > 0, |body| {
                        body.child(
                            div().h_flex().child(
                                show_more(
                                    ElementId::Name(format!("subagent-earlier-{}", card.id).into()),
                                    counted(earlier, "earlier step", "earlier steps"),
                                    cx,
                                )
                                .on_click(toggle),
                            ),
                        )
                    })
                    .children(rows)
                    .when(more > 0, |body| {
                        body.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{more} more")),
                        )
                    }),
            )
        })
        .into_any_element()
}

/// The card's head, which is also what opens and folds it: the arrow, the
/// mark (a spinner while it works), the name, and once it has ended the word
/// for how, with the count of its steps while it is folded.
fn head(
    card: &SubagentItem,
    open: bool,
    steps: usize,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let lit = cx.theme().foreground;
    let (mark, word) = match card.state {
        SubagentState::Working => (RowMark::Running, None),
        SubagentState::Completed => (RowMark::Done, Some(("done", muted))),
        SubagentState::Failed => (
            RowMark::Done,
            Some(("failed", crate::theme::status_ink(cx).danger)),
        ),
        // Neither failed: one was stopped, the other's end was never seen.
        SubagentState::Cancelled => (
            RowMark::Done,
            Some(("cancelled", crate::theme::meta_ink(cx))),
        ),
        SubagentState::Interrupted => (
            RowMark::Done,
            Some(("interrupted", crate::theme::meta_ink(cx))),
        ),
    };
    div()
        .id(ElementId::Name(format!("subagent-{}", card.id).into()))
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .cursor_pointer()
        .text_size(TEXT_SM)
        .text_color(muted)
        // Ink, and no plate, as every row of the transcript answers.
        .hover(move |row| row.text_color(lit))
        .on_click(toggle)
        .child(chevron_slot(Some(open)))
        .child(mark.draw(IconName::Bot.path(), cx))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(lit)
                .child(card.name.clone()),
        )
        .children(word.map(|(word, ink)| {
            div()
                .flex_none()
                .h_flex()
                .gap_2()
                .whitespace_nowrap()
                .child(div().text_color(ink).child(word))
                .when(!open, |line| {
                    line.child("·").child(counted(steps, "step", "steps"))
                })
        }))
}

/// One of a card's steps, as the transcript draws it; `None` for what a
/// subagent never sends.
fn step(
    session: &Entity<ChatSession>,
    item: &ChatItem,
    id: ElementId,
    root: &std::path::Path,
    window: &Window,
    cx: &App,
) -> Option<gpui::AnyElement> {
    // A row with nothing to open takes no press.
    let row = |row: ActivityRow| activity_row(row, |_, _, _| {}, cx);
    Some(match item {
        ChatItem::Agent(md) => div()
            .w_full()
            .min_w_0()
            .child(md_view(session, md, window, cx))
            .into_any_element(),
        ChatItem::Tool(t) => row(step_row(t, &activity::presentation(t), root, id, cx)),
        ChatItem::Thought(th) => row(ActivityRow::new(
            id,
            match th.is_running() {
                true => RowMark::Running,
                false => RowMark::Done,
            },
            IconName::Info.path(),
            match (th.is_running(), th.elapsed_secs) {
                (true, _) => "Thinking…".to_string(),
                (false, Some(secs)) => format!("Thought for {}", onehand_core::duration(secs)),
                (false, None) => "Thought".to_string(),
            },
        )),
        ChatItem::Plan(p) => {
            let done = p
                .entries
                .iter()
                .filter(|e| e.status == PlanStatus::Completed)
                .count();
            row(
                ActivityRow::new(id, RowMark::Done, IconName::Check.path(), "Plan")
                    .object(Some(Object::words(format!("{done}/{}", p.entries.len())))),
            )
        }
        ChatItem::Subagent(card) => subagent(session, card, window, cx),
        // A card's reducer takes none of these: the parent holds a subagent's
        // requests, and its prompts and notices are the parent's own.
        ChatItem::User(_)
        | ChatItem::Permission(_)
        | ChatItem::Ask(_)
        | ChatItem::Notice { .. } => return None,
    })
}

/// `n` and the word for it, singular for one.
fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}
