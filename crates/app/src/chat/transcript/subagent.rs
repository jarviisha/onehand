//! A subagent, where the agent handed work off: one line in the conversation.
//!
//! **It never opens in place.** What the subagent did is a transcript of its
//! own, and drawn inside the parent's it read as two stories told at once;
//! pressed, the line asks the pane to show that transcript in a float beside
//! the conversation instead.

use super::metrics::TEXT_SM;
use super::parts::{WorkingLine, working_text};
use crate::chat::session::{ChatEvent, ChatSession};
use gpui::{
    App, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::{ChatItem, SubagentItem, SubagentState};

/// The line for one subagent: its mark, its name, what it is doing or how it
/// ended, and its steps and time at the end.
pub(super) fn subagent(
    session: &Entity<ChatSession>,
    card: &SubagentItem,
    cx: &App,
) -> gpui::AnyElement {
    let chat = session.read(cx).chat();
    let key = card.key();
    let muted = cx.theme().muted_foreground;
    let lit = cx.theme().foreground;
    // Waiting on the person is said, not shimmered: nothing moves until they
    // answer, and the strip over the composer and the float say the same.
    let waiting = chat.standing(card) == onehand_core::chat::Standing::Waiting;
    let working = card.state == SubagentState::Working && !waiting;
    // What it is doing now, as its own steps say it.
    let doing = working.then(|| {
        let items: Vec<&ChatItem> = card.items.iter().collect();
        onehand_core::chat::cluster_summary(&items, &chat.root)
            .line(false)
            .in_hand
            .map_or_else(
                || "Working".to_string(),
                |step| format!("{}{}", step.verb, step.rest),
            )
    });
    let ended = match card.state {
        SubagentState::Working if waiting => {
            Some(("waiting on you", crate::theme::status_ink(cx).warning))
        }
        SubagentState::Working => None,
        SubagentState::Failed => Some(("failed", crate::theme::status_ink(cx).danger)),
        // Neither cancelled nor interrupted failed: one was stopped, the
        // other's end was never seen.
        SubagentState::Completed => Some(("done", muted)),
        SubagentState::Cancelled => Some(("cancelled", muted)),
        SubagentState::Interrupted => Some(("interrupted", muted)),
    };
    let session = session.clone();
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
        // **Ink, not a plate**, as every line in the transcript answers.
        .hover(move |line| line.text_color(lit))
        .on_click(move |_, _, cx: &mut App| {
            session.update(cx, |_, cx| cx.emit(ChatEvent::OpenSubagent(key)));
        })
        // In the arrow's slot, a mark of its own: nothing opens here.
        .child(
            div()
                .w_3()
                .flex_none()
                .child(Icon::new(IconName::Bot).xsmall()),
        )
        .child(
            div()
                .flex_none()
                .max_w_1_2()
                .truncate()
                .child(card.name.clone()),
        )
        .children(doing.map(|doing| {
            working_text(
                ElementId::Name(format!("subagent-doing-{}", card.id).into()),
                WorkingLine::words(doing),
                cx,
            )
        }))
        .children(ended.map(|(word, ink)| {
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_color(ink)
                .child(word)
        }))
        .child(div().flex_1())
        .child(div().flex_none().whitespace_nowrap().child(facts(card)))
        .into_any_element()
}

/// A card's steps and its time: how long it ran once it has ended, how long
/// it has been going while it works.
pub(in crate::chat) fn facts(card: &SubagentItem) -> String {
    let steps = counted(card.steps(), "step", "steps");
    let secs = card.secs.or_else(|| {
        (card.state == SubagentState::Working)
            .then_some(card.started)
            .flatten()
            .map(|started| onehand_core::chat::now_secs().saturating_sub(started))
    });
    match secs {
        Some(secs) => format!("{steps} · {}", onehand_core::duration(secs)),
        None => steps,
    }
}

/// `n` and the word for it, singular for one.
fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

#[cfg(test)]
mod tests {
    use super::counted;

    #[test]
    fn a_count_takes_its_number() {
        assert_eq!(counted(1, "step", "steps"), "1 step");
        assert_eq!(counted(3, "step", "steps"), "3 steps");
    }
}
