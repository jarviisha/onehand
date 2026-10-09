//! A session's row, its state's mark and words, and its menu.

use super::model::{Item, meta};
use super::row::{
    DragGhost, SessionDrag, ellipsize, faded, hover_fill, labelled, menu_button, on_hover,
    rail_control, row_surfaces,
};
use crate::chat::pane::SessionSignal;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

/// What a state says on hover: what it means, and what to do about it.
///
/// The words are the half of a mark that needs no learning, and the only half
/// that works for a reader who does not separate red from green.
pub(super) fn signal_hint(signal: Option<SessionSignal>) -> &'static str {
    match signal {
        Some(SessionSignal::Lost) => "The agent went away — restart it from the session menu",
        Some(SessionSignal::Failed) => "The last turn ended on an error",
        Some(SessionSignal::AwaitingUser) => "Waiting for your answer",
        Some(SessionSignal::Busy) => "Working",
        Some(SessionSignal::UnseenTurn) => "Finished while you were away",
        None => "Nothing waiting on you",
    }
}

/// What a state is called where there is room for a name but not a sentence:
/// the rail's meta line, the overview, the remote bridge's session listing.
/// One place, so no two readers of one state call it two things.
pub(crate) fn signal_word(signal: SessionSignal) -> &'static str {
    match signal {
        SessionSignal::Lost => "Disconnected",
        SessionSignal::Failed => "Failed",
        SessionSignal::AwaitingUser => "Needs input",
        SessionSignal::Busy => "Running",
        SessionSignal::UnseenTurn => "Done",
    }
}

/// A state's mark: a shape of its own for each, as well as an ink, so none is
/// told by colour alone. None of them moves: a list of turning glyphs pulls
/// the eye from what waits on the person. A lost agent is the failure shape,
/// and its word says which failure.
pub(super) fn status_icon(signal: Option<SessionSignal>, cx: &App) -> Icon {
    let ink = crate::theme::status_ink(cx);
    let (icon, color) = match signal {
        Some(SessionSignal::Lost | SessionSignal::Failed) => {
            (Icon::new(IconName::TriangleAlert), ink.danger)
        }
        Some(SessionSignal::AwaitingUser) => (Icon::new(crate::icons::Icon::Hand), ink.warning),
        Some(SessionSignal::Busy) => (Icon::new(IconName::LoaderCircle), cx.theme().link),
        Some(SessionSignal::UnseenTurn) => (Icon::new(IconName::CircleCheck), ink.success),
        None => (
            Icon::new(crate::icons::Icon::Circle),
            cx.theme().muted_foreground,
        ),
    };
    icon.xsmall().text_color(color)
}

/// A state's mark in its stable column, so titles never shift beside it,
/// naming itself on hover and to assistive technology.
pub(super) fn status_mark(
    id: gpui::ElementId,
    signal: Option<SessionSignal>,
    label: SharedString,
    cx: &App,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .flex_none()
        .w_4()
        .flex()
        .justify_center()
        .role(gpui::accesskit::Role::Image)
        .aria_label(label.clone())
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .child(status_icon(signal, cx))
}

/// A signal's mark where a page names a session, outside the rail.
pub(crate) fn signal_mark(signal: SessionSignal, cx: &App) -> impl IntoElement + use<> {
    status_mark(
        "signal".into(),
        Some(signal),
        signal_hint(Some(signal)).into(),
        cx,
    )
}

/// More characters than the widest rail can draw, fewer than a paste.
///
/// A cost bound and not a fit rule: what fits is decided in pixels by the
/// fade, but the label is shaped on every frame and a conversation's derived
/// title is free text somebody can open with a paragraph.
pub(super) const LABEL_SHAPE_CAP: usize = 120;

/// What a session row is called: the conversation's own name once it has
/// one, otherwise the agent that runs it.
pub(super) fn session_label(title: Option<&str>, agent: &str) -> SharedString {
    ellipsize(title.unwrap_or(agent), LABEL_SHAPE_CAP)
}

/// Everything a session row offers, as its `⋯` and on right-click.
///
/// Restart and Export select the session first: both act on the conversation
/// on screen, and an entry that restarts an agent nobody can see is worse than
/// one that takes them there on the way.
fn session_menu(
    root_idx: usize,
    session_idx: usize,
    uid: u64,
    resend: bool,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, cx: &mut App| {
        let danger = crate::theme::status_ink(cx).danger;
        let (again, rename, restart, export, close) = (
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
        );
        menu.when(resend, |menu| {
            menu.item(
                crate::controls::menu_item(RESEND)
                    .icon(Icon::new(IconName::Redo))
                    .on_click(move |_, _, cx: &mut App| {
                        again
                            .update(cx, |shell: &mut Shell, cx| {
                                shell.resend_last_prompt(uid, cx)
                            })
                            .ok();
                    }),
            )
        })
        .item(
            crate::controls::menu_item("Rename…")
                .icon(Icon::new(crate::icons::Icon::SquarePen))
                .on_click(move |_, window, cx: &mut App| {
                    rename
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.begin_rename(uid, window, cx);
                        })
                        .ok();
                }),
        )
        .item(
            crate::controls::menu_item("Restart the agent")
                .icon(Icon::new(IconName::Redo))
                .on_click(move |_, window, cx: &mut App| {
                    restart
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.restart_session_at(root_idx, session_idx, window, cx);
                        })
                        .ok();
                }),
        )
        .item(
            crate::controls::menu_item("Export as Markdown…")
                .icon(Icon::new(IconName::ExternalLink))
                .on_click(move |_, window, cx: &mut App| {
                    export
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.export_session_at(root_idx, session_idx, window, cx);
                        })
                        .ok();
                }),
        )
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| div().text_color(danger).child("Close session"))
                .icon(Icon::new(IconName::Close).text_color(danger))
                .on_click(move |_, window, cx: &mut App| {
                    close
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.close_session(root_idx, session_idx, window, cx);
                        })
                        .ok();
                }),
        )
    }
}

/// The words of the action that sends a failed turn's prompt again.
const RESEND: &str = "Send the last prompt again";

/// How a session row stands: the one on screen, the one the keyboard is on.
pub(super) struct Mark {
    /// The session the conversation shows.
    pub(super) shown: bool,
    /// The keyboard's row, while the list has focus.
    pub(super) at: bool,
    /// Its turn failed and its last prompt can go again.
    pub(super) resend: bool,
}

/// A session: its mark, its title and `⋯`, and under the title
/// `state · who · age`, who being the agent, or the project in a flat list.
/// Over the end of that line while the pointer is on the row: Stop while it
/// runs, Send again once it failed, and Close.
///
/// Only the tree's rows are dragged: a flat list is in an order this app
/// keeps nowhere, so there would be nothing for a drop to write into.
pub(super) fn session_row(
    item: &Item,
    who: &str,
    flat: bool,
    mark: Mark,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (root, session, uid, signal) = (item.root, item.session, item.uid, item.signal);
    let chosen = mark.shown || mark.at;
    let (rest, hovered) = row_surfaces(chosen, cx);
    let group = SharedString::from(format!("rail-session-{uid}"));
    let title = SharedString::from(item.title.clone());
    let facts = SharedString::from(meta(signal, who, item.age));
    let (shell, hover) = (cx.entity().downgrade(), hover_fill(cx));
    let muted = cx.theme().muted_foreground;
    let label = SharedString::from(format!(
        "{}: {}",
        super::model::word(signal),
        signal_hint(signal)
    ));
    let tip = title.clone();
    let meta_tip = facts.clone();
    div()
        .id(gpui::ElementId::Name(group.clone()))
        .group(group.clone())
        .v_flex()
        .py_1()
        .when(flat, |d| d.pl_1p5())
        // A session's text starts under its project's name: the project's
        // fold chevron and folder, and the gap after them.
        .when(!flat, |d| d.pl_7())
        .pr_1()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .when(chosen, |d| d.bg(super::row::chosen_fill(cx)))
        .when(!chosen, |d| d.hover(move |d| d.bg(hover)))
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.rail_open_session(root, session, window, cx);
            }),
        )
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(status_mark(("status", uid).into(), signal, label, cx))
                .child(
                    faded(("title", uid), title, group.clone(), rest, hovered)
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .text_color(cx.theme().foreground)
                        .when(
                            mark.shown || signal == Some(SessionSignal::UnseenTurn),
                            |d| d.font_medium(),
                        ),
                )
                .child(on_hover(
                    &group,
                    mark.shown,
                    labelled(
                        ("session-menu-name", uid),
                        "Session actions",
                        menu_button(
                            rail_control(("session-menu", uid), IconName::Ellipsis, cx),
                            "What can be done with this session",
                            session_menu(root, session, uid, mark.resend, shell.clone()),
                        ),
                    ),
                )),
        )
        .child(
            div()
                .h_flex()
                .relative()
                // Past the mark's column and the gap after it, so the facts
                // start under the title.
                .pl_6()
                .text_xs()
                .child(
                    faded(("meta", uid), facts, group.clone(), rest, hovered)
                        .text_color(muted)
                        .tooltip(move |window, cx| {
                            Tooltip::new(meta_tip.clone()).build(window, cx)
                        }),
                )
                .child(actions(item, mark.resend, &group, hovered, cx)),
        )
        .when(!flat, |row| {
            let (target, ghost) = (shell.clone(), SharedString::from(item.title.clone()));
            row.on_drag(
                SessionDrag {
                    root,
                    from: session,
                },
                move |_, _, _, cx| {
                    let ghost = ghost.clone();
                    cx.new(|_| DragGhost(ghost))
                },
            )
            // The same refusal as the drop below: this one stops the row
            // promising a drop it will not take, that one stops it taking one.
            .drag_over::<SessionDrag>(move |style, drag, _, cx| match drag.root == root {
                true => style.bg(hover_fill(cx)),
                false => style,
            })
            .on_drop(move |drag: &SessionDrag, _, cx| {
                if drag.root != root {
                    return;
                }
                let from = drag.from;
                let _ = target.update(cx, |shell: &mut Shell, cx| {
                    shell.move_session(root, from, session, cx);
                });
            })
        })
        .context_menu(move |menu, window, cx| {
            session_menu(root, session, uid, mark.resend, shell.clone())(menu, window, cx)
        })
        .into_any_element()
}

/// A session's quick actions, over the end of its meta line while the pointer
/// is on the row, on the row's own hover fill so what is under them does not
/// show through.
fn actions(
    item: &Item,
    resend: bool,
    group: &SharedString,
    fill: gpui::Hsla,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (root, session, uid, signal) = (item.root, item.session, item.uid, item.signal);
    let button = |id: &'static str, icon: Icon, tip: &'static str, cx: &App| {
        rail_control((id, uid), icon, cx).tooltip(tip)
    };
    div()
        .h_flex()
        .absolute()
        .right_0()
        .top_0()
        .bottom_0()
        .bg(fill)
        .invisible()
        .group_hover(group.clone(), |s| s.visible())
        .when(signal == Some(SessionSignal::Busy), |d| {
            d.child(labelled(
                ("session-stop-name", uid),
                "Stop the turn",
                button(
                    "session-stop",
                    Icon::new(crate::icons::Icon::Square),
                    "Stop the turn",
                    cx,
                )
                .on_click(cx.listener(move |shell: &mut Shell, _, _, cx| {
                    shell.stop_turn(uid, cx);
                })),
            ))
        })
        .when(resend, |d| {
            d.child(labelled(
                ("session-again-name", uid),
                RESEND,
                button("session-again", Icon::new(IconName::Redo), RESEND, cx).on_click(
                    cx.listener(move |shell: &mut Shell, _, _, cx| {
                        shell.resend_last_prompt(uid, cx);
                    }),
                ),
            ))
        })
        .child(labelled(
            ("session-close-name", uid),
            "Close the session",
            button(
                "session-close",
                Icon::new(crate::icons::Icon::Archive),
                "Close the session; its conversation is kept",
                cx,
            )
            .on_click(cx.listener(move |shell: &mut Shell, _, window, cx| {
                shell.close_session(root, session, window, cx);
            })),
        ))
}
