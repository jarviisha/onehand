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
use gpui::prelude::FluentBuilder as _;
use gpui::{App, Entity, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::bubble::{Bubble, BubbleVariant};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::{ChatItem, NoticeLevel, TranscriptItemId};

mod ask;
mod code_view;
mod image_view;
mod metrics;
mod parts;
mod permission;
mod plan;
mod prose;
mod strip;
mod subagent;
mod table_view;
mod tool;
mod user;
pub(in crate::chat) use ask::{advance_pinned, ask, pick_pinned, skip_pinned};
pub(in crate::chat) use permission::answer_pinned;

// The blocking cards' keys, which the keymap binds in the cards' own contexts:
// a question's choices walked, taken, numbered or skipped, and a permission
// allowed or denied.
gpui::actions!(
    question_card,
    [QuestionUp, QuestionDown, QuestionTake, QuestionSkip]
);
gpui::actions!(permission_card, [PermissionAllow, PermissionDeny]);
// The pinned card's answers with a modifier, which the keymap binds over the
// whole conversation so they reach the card from the composer: a permission
// allowed once or always or denied, a question moved on or skipped.
gpui::actions!(
    pinned_card,
    [
        PinnedAllow,
        PinnedAlways,
        PinnedDeny,
        PinnedNext,
        PinnedSkip
    ]
);

/// The one card pinned over the composer: the oldest of the parked
/// permissions and questions, whichever kind it is, by its live index.
pub(in crate::chat) enum Pinned {
    Permission(usize),
    Question(usize),
}

pub(in crate::chat) fn pinned(chat: &onehand_core::chat::Chat) -> Option<Pinned> {
    let perm = chat.pending_permissions().first().map(|(idx, _)| *idx);
    let ask = chat.pending_asks().first().map(|(idx, _)| *idx);
    match (perm, ask) {
        (Some(perm), Some(ask)) if ask < perm => Some(Pinned::Question(ask)),
        (Some(perm), _) => Some(Pinned::Permission(perm)),
        (None, ask) => ask.map(Pinned::Question),
    }
}

/// The key bound to `action` as it stands, overrides included: the binding
/// added last, which is the one that wins. Where a command has a bare key on
/// its card and an Alt one anywhere, the Alt one is registered after it.
fn bound_key(action: &dyn gpui::Action, cx: &App) -> Option<gpui::Keystroke> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).last()?;
    let stroke = binding.keystrokes().first()?;
    Some(gpui::AsKeystroke::as_keystroke(stroke).clone())
}

/// The key bound to `action` as it stands, overrides included, drawn as plain
/// words in the ink of the control it sits in: on a button, beside its label,
/// so the key is in sight without hovering for it.
fn key_label(action: &dyn gpui::Action, cx: &App) -> Option<gpui::AnyElement> {
    let stroke = bound_key(action, cx)?;
    Some(
        div()
            .flex_none()
            .child(gpui_component::kbd::Kbd::new(stroke).appearance(false))
            .into_any_element(),
    )
}

/// Take the question card's row numbered `index + 1`, the free-text box
/// included; a number nobody offered does nothing.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = question_card, no_json)]
pub struct QuestionPick {
    pub index: usize,
}
pub(in crate::chat) use image_view::open as show_image;
use metrics::LINE_H;
pub(in crate::chat) use metrics::{
    BLOCK_GAP, CONTENT_COLUMN, PART_GAP, TEXT, TEXT_SM, TIGHT_GAP, TUCK, TURN_GAP,
};
pub(in crate::chat) use parts::accent;
pub(crate) use parts::line_counts;
pub(in crate::chat) use permission::permission;
use plan::plan;
use prose::{agent, thought};
pub(in crate::chat) use strip::turn_summary;
pub use strip::{activity_group, cluster, section_group};
pub(crate) use tool::diff_rows;
use tool::tool;
use user::user;
pub(in crate::chat) use user::{opens, plate};

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
        ChatItem::User(u) => {
            let fork = chat.fork_point(target).map(|_| (session.clone(), target));
            user(u, fold_key(target), room, fork, cx).into_any_element()
        }
        ChatItem::Agent(md) => agent(session, md, target, turn, window, cx).into_any_element(),
        ChatItem::Thought(th) => thought(session, th, target, window, cx).into_any_element(),
        ChatItem::Tool(t) => tool(session, t, target, cx).into_any_element(),
        ChatItem::Plan(p) => plan(session, p, target, cx).into_any_element(),
        ChatItem::Permission(p) => {
            permission(session, p, target, room.well, 0, cx).into_any_element()
        }
        ChatItem::Ask(a) => ask(session, a, target, 0, cx).into_any_element(),
        ChatItem::Subagent(card) => subagent::subagent(session, card, window, cx),
        ChatItem::Notice {
            text,
            level,
            detail,
        } => {
            // The adapter is gone and this is what said so: the way back is
            // named by the key the keymap binds now.
            let lost = chat.link == onehand_core::chat::Link::Lost
                && matches!(target, TranscriptItemId::Live(i) if i + 1 == chat.items.len());
            let restart = lost
                .then(|| {
                    gpui_component::kbd::Kbd::binding_for_action(
                        &crate::shell::RestartSession,
                        None,
                        window,
                    )
                })
                .flatten();
            // Live by the rule the rail reads: a failed turn until a prompt
            // follows it, a lost agent while it is lost. Any other failure (an
            // error answering a Stop, a resume that fell back) is past at once.
            let past = match level {
                NoticeLevel::Failed => prompted_after(chat, target),
                NoticeLevel::Error => !lost,
                NoticeLevel::Info => false,
            };
            let detail = detail.as_deref().map(|detail| (session, target, detail));
            notice(text, *level, past, restart, detail, cx).into_any_element()
        }
    };

    // Width is owned by the pane's run so an activity summary drawn by the
    // pane and the steps rendered here always share the same two edges.
    div().w_full().min_w_0().child(body)
}

// ── notice ──────────────────────────────────────────────────────────────────

/// A line the session says about itself.
///
/// A remark or an error is a divider across the column (see
/// [`notice_shape`]); an error that still matters takes the danger tone and,
/// when the agent is gone, the key that brings it back. The failure a turn
/// ended on is the loudest thing this app says and stands as its own block,
/// with the alert icon at the body size.
///
/// **Only while nothing has been asked since.** A failure a later prompt has
/// moved past no longer needs the person: it keeps its mark and its words, in
/// the muted ink, as the record of what happened, so a conversation scrolled
/// back through is not a column of alarms already answered.
fn notice(
    text: &str,
    level: NoticeLevel,
    past: bool,
    restart: Option<gpui_component::kbd::Kbd>,
    detail: Option<(&Entity<ChatSession>, TranscriptItemId, &str)>,
    cx: &App,
) -> impl IntoElement + use<> {
    // What the agent attached, folded, in the ink of the line it sits in.
    let detail = |ink| {
        detail.map(|(session, target, detail)| notice_detail(session, target, detail, ink, cx))
    };
    let past = match notice_shape(level, past) {
        NoticeShape::Divider { alarm } => {
            return notice_divider(text, alarm, restart, detail, cx);
        }
        NoticeShape::Failed { past } => past,
    };
    if past {
        return div()
            .h_flex()
            .items_start()
            .gap_2()
            .w_full()
            .min_w_0()
            .text_size(TEXT_SM)
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .h(metrics::LINE_SM)
                    .flex_none()
                    .h_flex()
                    .items_center()
                    .child(Icon::new(IconName::TriangleAlert).xsmall()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .v_flex()
                    .gap_1()
                    .child(text.to_string())
                    .children(detail(cx.theme().muted_foreground)),
            )
            .into_any_element();
    }
    // A failure is the library's destructive bubble across the column, the
    // mark anchored to the first line so a message that wraps to three lines
    // does not carry the icon down the middle of itself.
    let ink = cx.theme().danger;
    Bubble::new()
        .with_variant(BubbleVariant::Destructive)
        .w_full()
        .max_w_full()
        .child(
            div()
                .h_flex()
                .items_start()
                .gap_2()
                .w_full()
                .min_w_0()
                .text_size(TEXT_SM)
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
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .h_flex()
                                .flex_wrap()
                                .items_center()
                                .gap_x_2()
                                // In a box that may shrink: bare text in a flex row
                                // measures its whole line as its least width, so an
                                // adapter's long error ran out past the column.
                                .child(div().min_w_0().child(text.to_string()))
                                .children(restart.map(|key| {
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child("Restart the agent with")
                                        .child(key)
                                })),
                        )
                        .children(detail(ink)),
                ),
        )
        .into_any_element()
}

/// How a notice draws.
#[derive(Debug, PartialEq)]
enum NoticeShape {
    /// Words between two rules across the column; `alarm` in the danger
    /// tone, while an error still matters.
    Divider { alarm: bool },
    /// The failure a turn ended on, a card; `past` once a prompt followed.
    Failed { past: bool },
}

/// A remark and an error are the session speaking about itself, so both are
/// a line across the column rather than a block of either side's; only the
/// failure a turn ended on stands as a card. An error rings while it is live
/// and is a quiet line once past.
fn notice_shape(level: NoticeLevel, past: bool) -> NoticeShape {
    match level {
        NoticeLevel::Info => NoticeShape::Divider { alarm: false },
        NoticeLevel::Error => NoticeShape::Divider { alarm: !past },
        NoticeLevel::Failed => NoticeShape::Failed { past },
    }
}

/// A notice as a line across the column: its words centred between two
/// hairlines, the key that brings a lost agent back after them, and what the
/// agent attached folded underneath.
fn notice_divider(
    text: &str,
    alarm: bool,
    restart: Option<gpui_component::kbd::Kbd>,
    detail: impl Fn(gpui::Hsla) -> Option<gpui::AnyElement>,
    cx: &App,
) -> gpui::AnyElement {
    let (ink, rule) = match alarm {
        true => {
            let ink = crate::theme::hue_ink(cx.theme().danger, cx);
            (ink, ink.opacity(0.4))
        }
        false => (cx.theme().muted_foreground, cx.theme().border),
    };
    let line = || div().flex_1().min_w(gpui::rems(1.)).h_px().bg(rule);
    div()
        .v_flex()
        .items_center()
        .gap_1()
        .w_full()
        .min_w_0()
        .text_xs()
        .text_color(ink)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .min_h(LINE_H)
                .child(line())
                .child(div().min_w_0().flex_shrink(1.).child(text.to_string()))
                .children(restart.map(|key| {
                    div()
                        .h_flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .child("Restart the agent with")
                        .child(key)
                }))
                .child(line()),
        )
        .children(detail(ink))
        .into_any_element()
}

/// What the agent attached to a failure beside its message: machine text,
/// folded under the words a person reads first, and opened on request.
fn notice_detail(
    session: &Entity<ChatSession>,
    target: TranscriptItemId,
    detail: &str,
    ink: gpui::Hsla,
    cx: &App,
) -> gpui::AnyElement {
    let open = session.read(cx).section_is_open(target);
    let session = session.clone();
    let toggle = parts::show_more(
        ("notice-detail", fold_key(target)),
        if open { "Hide details" } else { "Show details" },
        cx,
    )
    .text_color(ink)
    .on_click(move |_, _, cx: &mut App| {
        session.update(cx, |session, cx| {
            session.toggle_section(target);
            cx.notify();
        });
    });
    div()
        .v_flex()
        .items_start()
        .gap_1()
        .child(toggle)
        .when(open, |column| {
            column.child(
                div()
                    .w_full()
                    .min_w_0()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(crate::tokens::CODE_TEXT)
                    .child(detail.to_string()),
            )
        })
        .into_any_element()
}

/// Whether a prompt was sent after `target`, which moves a failure there into
/// the record.
fn prompted_after(chat: &onehand_core::chat::Chat, target: TranscriptItemId) -> bool {
    let prompt = |item: &ChatItem| matches!(item, ChatItem::User(_));
    match target {
        TranscriptItemId::History(i) => {
            chat.history.iter().skip(i + 1).any(prompt) || chat.items.iter().any(prompt)
        }
        TranscriptItemId::Live(i) => chat.items.iter().skip(i + 1).any(prompt),
    }
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
