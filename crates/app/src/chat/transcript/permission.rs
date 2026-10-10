use super::ask::{Footer, Pinned, Settled, pinned_card, settled_detail, settled_row};
use super::parts::{grows, tool_label};
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

/// The settled record of a grant: one line, opening onto the whole command and
/// where it would have run.
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
    let row = Settled {
        id: ("perm-record", fold_key(target)).into(),
        verb: match denied {
            true => "Denied",
            false => "Allowed",
        },
        asked: activity::first_line_trunc(p.command(), 160).into(),
        mono: true,
        refused: denied,
        answer: choice.to_string().into(),
        fold: Some(p.expanded),
    };

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(settled_row(row, toggle, cx))
        .when(p.expanded, |block| {
            // The whole command and where it would have run: the two facts the
            // line cut down to one, in the one place that has room for them.
            block.child(settled_detail(
                div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().foreground)
                            .child(onehand_core::chat::redact(p.command())),
                    )
                    .children(command_cwd(&session.read(cx).chat.root).map(|(shown, _)| {
                        div()
                            .w_full()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("in {shown}"))
                    })),
                cx,
            ))
        })
        .into_any_element()
}

/// What a permission card asks, in the card's own title.
///
/// **The request carries a kind and a title, and no tool name or path.** Where
/// the agent's title already reads as the question for its kind (`Edit
/// src/x.rs`, one line), it is asked back; otherwise the kind's own question,
/// and the title is in the body below. An unrecognised kind asks plainly
/// rather than printing a word this build made up.
fn permission_title(kind: ToolKind, title: &str) -> String {
    let title = title.trim().replace('`', "");
    let says_itself = !title.contains('\n')
        && title.chars().count() <= TITLE_MAX
        && kind != ToolKind::Other
        && kind != ToolKind::Execute
        && title
            .to_lowercase()
            .starts_with(&format!("{} ", tool_label(kind).to_lowercase()));
    if says_itself {
        return format!("{title}?");
    }
    match kind {
        ToolKind::Execute => "Run a command?",
        ToolKind::Edit => "Edit a file?",
        ToolKind::Delete => "Delete a file?",
        ToolKind::Move => "Move a file?",
        ToolKind::Read => "Read a file?",
        ToolKind::Search => "Search the project?",
        ToolKind::Fetch => "Fetch from the web?",
        ToolKind::Think | ToolKind::Other => "Allow this?",
    }
    .to_string()
}

/// The longest agent title asked back as the card's question; past it the
/// kind's own question is shorter to read.
const TITLE_MAX: usize = 80;

/// The mark of what is being asked for, in the ink of what waits on the person.
fn permission_icon(kind: ToolKind) -> Icon {
    Icon::new(match kind {
        ToolKind::Execute => IconName::SquareTerminal,
        ToolKind::Edit | ToolKind::Delete | ToolKind::Move | ToolKind::Read => IconName::File,
        ToolKind::Search => IconName::Search,
        ToolKind::Fetch => IconName::Globe,
        ToolKind::Think | ToolKind::Other => IconName::TriangleAlert,
    })
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
    let (shown, hidden) = p.shown_lines();
    let block = CommandBlock {
        session: session.clone(),
        target,
        command: SharedString::from(p.command().to_string()),
        lines: shown
            .into_iter()
            .map(|l| SharedString::from(l.to_string()))
            .collect(),
        hidden,
        well,
        long: p.is_long(),
        total: p.command_lines().len().max(1),
        expanded: p.expanded,
    };
    let chat = &session.read(cx).chat;
    // Who asks, and with what: the agent's name, and the protocol's one word
    // about the kind of work where it has one.
    let meta = match p.req.kind {
        ToolKind::Other => chat.agent.clone(),
        kind => format!("{} · {}", chat.agent, tool_label(kind)),
    };
    let cwd = command_cwd(&chat.root);
    let body = vec![
        block.into_any_element(),
        div()
            .v_flex()
            .children(cwd.map(|(shown, full)| {
                div()
                    .id(("perm-cwd", fold_key(target)))
                    .w_full()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("in {shown}"))
                    .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            }))
            .into_any_element(),
    ];

    let card = pinned_card(
        Pinned {
            icon: permission_icon(p.req.kind),
            title: permission_title(p.req.kind, p.command()).into(),
            meta: meta.into(),
            body,
            // A card replayed from the archive carries an rpc id no running
            // adapter issued: controls would invite an answer nobody waits for.
            footer: idx.map(|idx| Footer {
                actions: permission_actions(session, p, idx),
            }),
        },
        cx,
    );
    match idx {
        None => card.into_any_element(),
        Some(idx) => permission_keys(card, session, p, idx, cx),
    }
}

/// Enter allows once, Esc denies — and only while the card holds the caret.
///
/// **Taken on the card's own handle rather than bound as app actions**: both
/// keys are ones somebody is as likely to be pressing in the composer an inch
/// below. Which option either key means is [`PermissionWeight`] and never the
/// position in the list, so a key cannot grant *always* on a card that happened
/// to list it first.
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
                // **Only while the card itself holds the caret.** A focused
                // button turns Enter into its own click, and this listener runs
                // first: somebody who tabbed to Deny and pressed Enter would
                // otherwise have granted the call.
                "enter" if card_focus.is_focused(window) => allow.as_deref(),
                "enter" => None,
                // Esc is safe the other way: no button here denies by being
                // focused, and the worst it can do is refuse a call twice.
                "escape" => deny.as_deref(),
                _ => None,
            };
            if let Some(option) = chosen {
                answer_permission(&session, idx, option, cx);
            }
        })
        .into_any_element()
}

/// The answers, Deny first and the narrowest grant last: the button at the end
/// of the row is the one that expires with this call, and the one primary.
/// *Always* is an outline — a decision, not a reflex.
fn permission_actions(
    session: &Entity<ChatSession>,
    p: &PermItem,
    idx: usize,
) -> Vec<gpui::AnyElement> {
    let mut options: Vec<(usize, &onehand_core::acp::PermissionOption)> =
        p.req.options.iter().enumerate().collect();
    options.sort_by_key(|(_, option)| match option.weight() {
        PermissionWeight::Deny => 0,
        PermissionWeight::AllowAlways => 1,
        PermissionWeight::AllowOnce => 2,
    });
    options
        .into_iter()
        .map(|(i, option)| {
            let (id, session) = (option.id.clone(), session.clone());
            // Grows, because the wording is the agent's and may wrap.
            grows(crate::controls::action(("perm", i)))
                .map(|b| match option.weight() {
                    PermissionWeight::AllowOnce => b.primary(),
                    PermissionWeight::AllowAlways => b.outline(),
                    PermissionWeight::Deny => b.ghost(),
                })
                .label(option.name.clone())
                .on_click(move |_, _, cx: &mut App| {
                    answer_permission(&session, idx, &id, cx);
                })
                .into_any_element()
        })
        .collect()
}

/// Settle the card at `idx` on `option`: one function behind the buttons and
/// the keys, because they are the same decision.
fn answer_permission(session: &Entity<ChatSession>, idx: usize, option: &str, cx: &mut App) {
    let option = option.to_string();
    session.update(cx, |s, cx| {
        s.chat.answer_permission(idx, &option);
        cx.notify();
    });
}

#[cfg(test)]
mod tests;
