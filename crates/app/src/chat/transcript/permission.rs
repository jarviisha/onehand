use super::ask::user_icon;
use super::metrics::{FRAME_PAD, STACK_GAP};
use super::parts::{
    ActivityRow, Object, RowMark, activity_row, detail_well, floating_card, grows, pill, plain_box,
    tool_label,
};
use super::tool::{CommandBlock, command_cwd};
use super::{fold_key, live_index};
use crate::chat::session::ChatSession;
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
    let status = crate::theme::status_ink(cx);
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
    .meta(Some(
        pill(
            choice.to_string(),
            match denied {
                true => status.danger,
                false => cx.theme().muted_foreground,
            },
            cx,
        )
        .into_any_element(),
    ))
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
        command: SharedString::from(p.command().to_string()),
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

    floating_card(cx)
        .v_flex()
        // The card's own padding goes to the rows inside it, because the
        // footer's rule has to run edge to edge and a rule inside a padded box
        // stops short of the corners it is squaring off. The corners clip for
        // the other half of that: without it the rule ends in a square nib a
        // pixel outside the rounded edge above it.
        .p_0()
        .overflow_hidden()
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                .px_4()
                .pt_3()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .flex_1()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size_4()
                                .flex_none()
                                .text_color(crate::theme::status_ink(cx).warning),
                        )
                        // **The weight and the mark carry it, not the size.**
                        // This and the question card are the two blocks where
                        // nothing proceeds until the user acts, and they were
                        // set at the answer's own size to say so. What that
                        // bought was a heading that was the largest text on
                        // screen over the largest controls on screen, floating
                        // an inch above a composer where everything had come
                        // down a step.
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_semibold()
                                .child("Permission required"),
                        ),
                )
                // What kind of work it is, opposite the heading — the same
                // slot and the same voice the question card puts its counter
                // in, because the two cards are read as one family and this is
                // the corner a reader has already learned to check.
                .children(kind.map(|word| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(word)
                })),
        )
        .child(
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .px_4()
                .pt_3()
                .pb_3p5()
                .child(block)
                .children(cwd.map(|(shown, full)| {
                    div()
                        .id(("perm-cwd", fold_key(target)))
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("in {shown}"))
                        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
                })),
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
        .track_focus(&focus)
        .on_key_down(move |event, window, cx| {
            let keystroke = &event.keystroke;
            // A modified key is somebody else's: Ctrl+1 switches sessions and
            // Shift+Enter is a newline in whatever holds the caret.
            if keystroke.modifiers.modified() {
                return;
            }
            let chosen = match keystroke.key.as_str() {
                // **Only while the card itself holds the caret**, which is the
                // question card's rule and matters more here. Every button in
                // the footer is a library `Button`, and a focused one already
                // turns Enter into its own click -- so answering here as well
                // races it, and this listener runs first because a click is
                // settled on the key going *up*. Somebody who has tabbed to
                // Deny and pressed Enter would have granted the call: the grant
                // lands, and Deny's own click arrives afterwards to find the
                // permission already answered and is dropped. Unguarded, the
                // key that means no is how yes gets said.
                "enter" if card_focus.is_focused(window) => allow.as_deref(),
                "enter" => None,
                // Esc is safe in the other direction and needs no such guard:
                // no button on this card denies by being focused, and the worst
                // it can do is refuse a call twice.
                "escape" => deny.as_deref(),
                _ => None,
            };
            if let Some(option) = chosen {
                answer_permission(&session, idx, option, cx);
            }
        })
        .into_any_element()
}

/// The strip that answers the card.
///
/// **A strip of its own over a rule that spans the card**, which is the
/// question card's footer drawn again and deliberately so: both end the one
/// block everything is waiting on, and left inside the body's padding the
/// buttons read as the last row of the card rather than as what closes it.
///
/// **The buttons wrap before a label is cut.** A narrow pane drops the key
/// hints first — they name keys that still work — and then lets the row fold
/// onto a second, still right-aligned line. A truncated *Always allow* is a
/// grant nobody can read the reach of, which is the one thing on this card
/// that must never happen.
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

    div()
        .v_flex()
        .w_full()
        .child(div().w_full().h_px().bg(cx.theme().border))
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .w_full()
                .px_4()
                .py_2p5()
                .child(
                    div()
                        // The first thing to give way when the row runs out of
                        // room, because the keys it names keep working whether
                        // or not they are printed. Squeezed to nothing, the
                        // buttons wrap onto a line of their own rather than
                        // losing a character of a label.
                        .flex_shrink(1.)
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Enter allow · Esc deny"),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_end()
                        .flex_wrap()
                        .gap_2()
                        .flex_none()
                        .children(options.into_iter().map(|(i, option)| {
                            let (id, session) = (option.id.clone(), session.clone());
                            let weight = option.weight();
                            grows(crate::controls::action(("perm", i)))
                                // One primary per card, and it is the grant
                                // that expires with this call. "Always allow"
                                // is the same word with a far longer reach:
                                // drawn as loudly as "allow once" it is the
                                // one that gets clicked by muscle memory, and
                                // it is the one that cannot be taken back from
                                // the card. It stays reachable, in the neutral
                                // outline — a decision, not a reflex.
                                .map(|b| match weight {
                                    PermissionWeight::AllowOnce => b.primary(),
                                    PermissionWeight::AllowAlways => b.outline(),
                                    PermissionWeight::Deny => b.ghost(),
                                })
                                .label(option.name.clone())
                                .on_click(move |_, _, cx: &mut App| {
                                    answer_permission(&session, idx, &id, cx);
                                })
                        })),
                ),
        )
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
