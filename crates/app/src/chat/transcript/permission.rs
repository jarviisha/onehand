use super::ask::user_icon;
use super::metrics::{FRAME_PAD, STACK_GAP};
use super::parts::{
    ActivityRow, Object, RowMark, activity_row, card_body, card_foot, card_head, copy_button,
    detail_well, grows, pill, pinned_card, plain_box, tool_label,
};
use super::tool::{CommandBlock, command_cwd};
use super::{fold_key, live_index};
use crate::chat::session::ChatSession;
use crate::controls::Quiet as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::acp::{PermissionWeight, ToolKind};
use onehand_core::chat::activity;
use onehand_core::chat::{PermItem, TranscriptItemId};

// ── the record a settled exchange leaves ────────────────────────────────────
//
// **What is left of a question or a grant once it is answered is one line.**
// While either is waiting it is the block everything stops for, and it is not
// drawn here at all -- it is pinned above the composer, where the answer is
// given. Afterwards there is nothing left to decide, and what the transcript
// owes a reader is the fact: the agent asked this, the user said that.
//
// Drawn as a card it was the loudest thing in the conversation for the rest of
// the conversation's life -- a heading, a rule, a body, a footer and a shadow,
// repeated per exchange, so three questions in a row cost most of a screen to
// say three sentences. As rows they join the block of steps around them, which
// is also where they belong: answering a question is one of the things that
// happened during that piece of work.

/// The settled record of a grant.
fn permission_record(
    session: &Entity<ChatSession>,
    p: &PermItem,
    choice: &str,
    target: TranscriptItemId,
    cx: &App,
) -> gpui::AnyElement {
    let denied = p
        .req
        .options
        .iter()
        .find(|option| option.name == choice)
        .is_some_and(|option| option.weight() == PermissionWeight::Deny);
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_permission(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("perm-record", fold_key(target)).into(),
        // A refusal is not a failure -- nothing went wrong, somebody decided
        // -- so it is not the danger cross a failed step carries. It is the
        // struck-out words and the pill that say what happened.
        match denied {
            true => RowMark::Refused,
            false => RowMark::Done,
        },
        user_icon(),
        match denied {
            true => "Denied",
            false => "Allowed",
        },
    )
    .object(Some(Object::plain(activity::first_line_trunc(
        p.command(),
        160,
    ))))
    .meta(Some(pill(choice.to_string(), cx).into_any_element()))
    .fold(Some(p.expanded));
    let mut row = row;
    row.struck = denied;

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(p.expanded, |block| {
            // The whole command and where it would have run: the two facts the
            // row cut down to one line, in the one place that has room for
            // them.
            block.child(detail_well(
                plain_box(cx)
                    .v_flex()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .py(STACK_GAP)
                            .px(FRAME_PAD)
                            .text_color(cx.theme().foreground)
                            .child(onehand_core::chat::redact(p.command())),
                    )
                    .children(command_cwd(&session.read(cx).chat.root).map(|(shown, _)| {
                        div()
                            .w_full()
                            .min_w_0()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .py(STACK_GAP)
                            .px(FRAME_PAD)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("in {shown}"))
                    })),
            ))
        })
        .into_any_element()
}

pub(in crate::chat) fn permission(
    session: &Entity<ChatSession>,
    p: &PermItem,
    target: TranscriptItemId,
    well: Option<gpui::Pixels>,
    cx: &App,
) -> impl IntoElement + use<> {
    if let Some(choice) = p.resolved.as_deref() {
        return permission_record(session, p, choice, target, cx);
    }
    let idx = live_index(target);
    let lines = p.command_lines();
    let block = CommandBlock {
        target,
        lines: lines
            .into_iter()
            .map(|l| SharedString::from(l.to_string()))
            .collect(),
        well,
    };
    // The only word the protocol offers about *what* is being asked for. An
    // unrecognised kind has no word, so the slot stays empty rather than
    // printing one this build made up.
    let kind = (p.req.kind != ToolKind::Other).then(|| tool_label(p.req.kind));
    let cwd = command_cwd(&session.read(cx).chat.root);

    pinned_card(idx.is_some(), cx)
        // The one card that keeps its mark: a grant is the block where a
        // misread costs most.
        .child(card_head(
            Some(
                Icon::new(IconName::TriangleAlert).text_color(crate::theme::status_ink(cx).warning),
            ),
            "Permission required",
            // The only word the protocol offers about *what* is asked for,
            // opposite the heading where the question card keeps its count.
            kind.map(SharedString::from),
            cx,
        ))
        .child(
            card_body()
                .gap_2()
                .child(block)
                // Where it would run, and the copy, on the line under the
                // command: in the well, a copy took a column the whole
                // command's height for one icon, and its scrollbar ran over
                // the text. Drawn always, never waiting to be hovered: this
                // is the text most likely wanted elsewhere, and a command
                // that scrolls cannot be selected out by dragging.
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(div().flex_1().min_w_0().children(cwd.map(|(shown, full)| {
                            div()
                                .id(("perm-cwd", fold_key(target)))
                                .truncate()
                                .text_xs()
                                .text_color(crate::theme::meta_ink(cx))
                                .child(format!("in {shown}"))
                                .tooltip(move |window, cx| {
                                    Tooltip::new(full.clone()).build(window, cx)
                                })
                        })))
                        .child(copy_button(
                            ("perm-copy", fold_key(target)),
                            p.command().to_string(),
                            "Copy the whole command",
                            cx,
                        )),
                ),
        )
        // A card replayed from the archive carries an rpc id no running adapter
        // issued: showing controls would invite an answer nobody is waiting
        // for. An answered one never reaches here -- it is a row.
        .map(|card| match idx {
            None => card.into_any_element(),
            Some(idx) => permission_keys(
                card.child(permission_footer(session, p, idx, cx)),
                session,
                p,
                idx,
                cx,
            ),
        })
        .into_any_element()
}

/// Enter allows once, Esc denies — and only while the card holds the caret.
///
/// **Taken on the card's own handle rather than bound as app actions**, which
/// is what the question card does and for the same reason: both keys are ones
/// somebody is as likely to be pressing in the composer an inch below, and an
/// app binding would reach over it exactly the way the panel shortcuts are
/// meant to.
///
/// Which option either key means is [`onehand_core::acp::PermissionWeight`]
/// and never the position in the list: an agent is free to send its grants in
/// any order, and a key that answered by position would grant *always* on a
/// card that happened to list it first. An option no card offers is no key at
/// all, since there is nothing to send.
fn permission_keys(
    card: gpui::Div,
    session: &Entity<ChatSession>,
    p: &PermItem,
    idx: usize,
    cx: &App,
) -> gpui::AnyElement {
    let Some(focus) = session.read(cx).perm_focus(idx).cloned() else {
        return card.into_any_element();
    };
    let pick = |weight: PermissionWeight| {
        p.req
            .options
            .iter()
            .find(|option| option.weight() == weight)
            .map(|option| option.id.clone())
    };
    let (allow, deny) = (
        pick(PermissionWeight::AllowOnce),
        pick(PermissionWeight::Deny),
    );
    let session = session.clone();
    let card_focus = focus.clone();
    card.id(("perm-card", idx))
        .key_context("PermissionCard")
        .track_focus(&focus)
        // **Enter only while the card itself holds the caret**, which is the
        // question card's rule and matters more here. Every button in the
        // footer is a library `Button`, and a focused one already turns Enter
        // into its own click -- so answering here as well races it. Somebody
        // who has tabbed to Deny and pressed Enter would have granted the call:
        // the grant lands, and Deny's own click arrives afterwards to find the
        // permission already answered and is dropped. Unguarded, the key that
        // means no is how yes gets said.
        .on_action({
            let session = session.clone();
            move |_: &super::PermissionAllow, window, cx| {
                if let Some(option) = allow.as_deref().filter(|_| card_focus.is_focused(window)) {
                    answer_permission(&session, idx, option, cx);
                }
            }
        })
        // Esc is safe in the other direction and needs no such guard: no
        // button on this card denies by being focused, and the worst it can do
        // is refuse a call twice.
        .on_action(move |_: &super::PermissionDeny, _, cx| {
            if let Some(option) = deny.as_deref() {
                answer_permission(&session, idx, option, cx);
            }
        })
        .into_any_element()
}

/// The answers that close the card, the keys that reach them named in their
/// tooltips.
fn permission_footer(
    session: &Entity<ChatSession>,
    p: &PermItem,
    idx: usize,
    cx: &App,
) -> impl IntoElement + use<> {
    // Deny first and the narrowest grant last, against the reading direction
    // of the sentence: the button under the thumb at the end of the row is the
    // one that expires with this call, and the widest grant never sits there.
    let mut options: Vec<(usize, &onehand_core::acp::PermissionOption)> =
        p.req.options.iter().enumerate().collect();
    options.sort_by_key(|(_, option)| match option.weight() {
        PermissionWeight::Deny => 0,
        PermissionWeight::AllowAlways => 1,
        PermissionWeight::AllowOnce => 2,
    });

    card_foot(cx).children(options.into_iter().map(|(i, option)| {
        let (id, session) = (option.id.clone(), session.clone());
        let name = option.name.clone();
        grows(crate::controls::action(("perm", i)))
            // One primary per card, and it is the grant that expires with this
            // call. "Always allow" is the same word with a far longer reach:
            // drawn as loudly it is the one clicked by muscle memory, and the
            // one that cannot be taken back from the card. It stays reachable
            // on the neutral secondary fill -- a decision, not a reflex. The
            // answer that dismisses is a quiet ghost, as Skip is.
            .map(|b| match option.weight() {
                PermissionWeight::AllowOnce => b.primary().tooltip_with_action(
                    name.clone(),
                    &super::PermissionAllow,
                    Some("PermissionCard"),
                ),
                PermissionWeight::AllowAlways => b.secondary(),
                PermissionWeight::Deny => b.quiet(cx).tooltip_with_action(
                    name.clone(),
                    &super::PermissionDeny,
                    Some("PermissionCard"),
                ),
            })
            .label(name)
            .on_click(move |_, _, cx: &mut App| {
                answer_permission(&session, idx, &id, cx);
            })
    }))
}

/// Settle the card at `idx` on `option`.
///
/// One function behind the buttons and the keys for the reason the question
/// card has one: they are the same decision, and a second copy is where a key
/// comes to answer a card the click would have refused.
fn answer_permission(session: &Entity<ChatSession>, idx: usize, option: &str, cx: &mut App) {
    let option = option.to_string();
    session.update(cx, |s, cx| {
        s.chat.answer_permission(idx, &option);
        cx.notify();
    });
}
