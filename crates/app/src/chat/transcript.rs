//! The transcript: one element per [`ChatItem`].
//!
//! Follows the design language's **structure** — the block types, the user's
//! right-hand bubble against the agent's full-width lane —
//! while every colour, radius and size comes from
//! `cx.theme()`. That split is deliberate: the component library's theme is
//! this app's look, so no token table is carried anywhere and nothing here can
//! drift away from the rest of the window.
//!
//! Rendering stays **bounded**. Diffs and command output
//! draw one element per line, so an unbounded result would freeze the frame.
//! The caps below are correctness, not tuning.

use super::session::ChatSession;
use gpui::{App, Entity, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::{ChatItem, NoticeLevel, TranscriptItemId};

mod ask;
mod metrics;
mod parts;
mod permission;
mod plan;
mod prose;
mod strip;
mod tool;
mod user;
pub(in crate::chat) use ask::ask;
pub(in crate::chat) use metrics::{
    BLOCK_GAP, CONTENT_COLUMN, LEADING, TEXT, TEXT_SM, TIGHT_GAP, TURN_GAP,
};
use metrics::{LINE_H, STATE_TINT};
pub(in crate::chat) use parts::{accent, line_counts};
pub(in crate::chat) use permission::permission;
use plan::plan;
use prose::{agent, thought};
pub(in crate::chat) use strip::turn_summary;
pub use strip::{Run, activity_group, activity_summary, cluster, runs, section_group};
pub(in crate::chat) use tool::diff_rows;
use tool::tool;
use user::user;

/// Render one transcript item.
///
/// `target` addresses fold toggles back into the model, and carries whether the
/// item came from the read-only resumed history or the live tail: history and
/// live items are two collections, so a fold addressed by render position lands
/// in the wrong one.
/// How much room the block is being drawn in, and whether it is one of a
/// group's own rows.
///
/// One value rather than three arguments threaded through a dozen signatures,
/// and the two facts travel together because they are read off the same frame:
/// a caller that passed a fresh width beside a stale panel height would be
/// sizing one half of a block against a layout the other half never saw.
#[derive(Clone)]
pub struct Room {
    /// The panel's own height, for the blocks that bound themselves against the
    /// room they have rather than against the window.
    pub well: Option<gpui::Pixels>,
    /// A column too short to hold a block off its edges and still leave a line
    /// worth reading.
    pub narrow: bool,
}

impl Room {
    pub fn new(well: Option<gpui::Pixels>, narrow: bool) -> Self {
        Self { well, narrow }
    }
}

pub fn item(
    session: &Entity<ChatSession>,
    it: &ChatItem,
    target: TranscriptItemId,
    room: Room,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let chat = &session.read(cx).chat;
    // Only an answer has a footer, and only its turn's last one draws it.
    let turn = matches!(it, ChatItem::Agent(_))
        .then(|| chat.turn_answer(target))
        .flatten();
    let body = match it {
        ChatItem::User(u) => user(u, fold_key(target), room, cx).into_any_element(),
        ChatItem::Agent(md) => agent(session, md, target, turn, window, cx).into_any_element(),
        ChatItem::Thought(th) => thought(session, th, target, window, cx).into_any_element(),
        ChatItem::Tool(t) => tool(session, t, target, cx).into_any_element(),
        ChatItem::Plan(p) => plan(session, p, target, cx).into_any_element(),
        ChatItem::Permission(p) => permission(session, p, target, room.well, cx).into_any_element(),
        ChatItem::Ask(a) => ask(session, a, target, cx).into_any_element(),
        ChatItem::Notice { text, level } => notice(text, *level, cx).into_any_element(),
    };

    // Width is owned by the pane's run so an activity summary drawn by the
    // pane and the steps rendered here always share the same two edges.
    div().w_full().min_w_0().child(body)
}

// ── notice ──────────────────────────────────────────────────────────────────

/// A line the session says about itself.
///
/// A remark stays a caption. A **failure does not**: the two loudest things
/// this app can say — the turn errored, the agent is gone and here is the key
/// that brings it back — used to draw at the smallest size in the palest
/// colour the theme has, quieter than the file path in the tool card above
/// them. Something that ends the conversation cannot be whispered, so a
/// failure takes the alert icon, the danger tint and the body size.
///
/// It is **not** one of the muted machine wells, and deliberately so: those say
/// "a machine produced this", and the tint is what carries the meaning here. A
/// failure sits on a wash of its own danger colour, in the body face, which is
/// what stops it reading as one more line of the answer without claiming to be
/// quoted output.
fn notice(text: &str, level: NoticeLevel, cx: &App) -> impl IntoElement + use<> {
    if level != NoticeLevel::Error {
        // **A line down the middle of the column, with nothing around it.** A
        // remark is the transcript speaking about itself rather than either
        // side speaking, and the centre is the one lane neither of them uses —
        // so it needs no box, no rule and no mark to be told apart from the
        // conversation running past it.
        return div()
            .h_flex()
            .items_center()
            .justify_center()
            .w_full()
            .h(LINE_H)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().min_w_0().truncate().child(text.to_string()))
            .into_any_element();
    }

    // A failure is a banner: the full width of the column, its own edge, and
    // the mark anchored to the first line so a message that wraps to three
    // lines does not carry the icon down the middle of itself.
    let danger = crate::theme::status_ink(cx).danger;
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .w_full()
        .min_w_0()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(danger)
        .bg(cx.theme().danger.opacity(STATE_TINT))
        .text_size(TEXT_SM)
        .text_color(danger)
        .child(
            // The glyph is held to the first line's height, so a message that
            // wraps keeps it beside its first words.
            div()
                .h(metrics::LINE_SM)
                .flex_none()
                .h_flex()
                .items_center()
                .child(Icon::new(IconName::TriangleAlert).xsmall()),
        )
        .child(div().flex_1().min_w_0().child(text.to_string()))
        .into_any_element()
}

/// A stable element id per item. History and live indices overlap, so the
/// source has to be part of the key or two items share one id.
pub(super) fn fold_key(target: TranscriptItemId) -> usize {
    match target {
        TranscriptItemId::History(i) => i * 2,
        TranscriptItemId::Live(i) => i * 2 + 1,
    }
}

/// The live-items index, or `None` for a history item.
///
/// Only live items are answerable: a permission replayed from the archive
/// carries an rpc id that no running adapter ever issued.
fn live_index(target: TranscriptItemId) -> Option<usize> {
    match target {
        TranscriptItemId::Live(i) => Some(i),
        TranscriptItemId::History(_) => None,
    }
}

#[cfg(test)]
mod tests;
