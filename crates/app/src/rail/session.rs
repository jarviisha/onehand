//! A session's row, its state's mark and words, and its menu.

use super::model::{Item, meta};
use super::row::{
    DragGhost, SessionDrag, ellipsize, faded, hover_fill, key_ring, labelled, menu_button,
    meta_control, on_hover, row_control, row_inks, row_surfaces,
};
use crate::chat::pane::SessionSignal;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, HighlightStyle, Hsla,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, StyledText, WeakEntity, Window, div,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_ui::mark_slot;

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

/// A state's mark: a shape of its own for each, as well as an ink, so none is
/// told by colour alone. None of them moves: a list of turning glyphs pulls
/// the eye from what waits on the person, so running is a filled dot, the one
/// solid shape among outlines. A lost agent is the failure shape, and its word
/// says which failure. A step under the row's text, so the mark reads as a
/// mark and the title as the thing.
pub(super) fn status_icon(signal: Option<SessionSignal>, cx: &App) -> gpui::AnyElement {
    let ink = crate::theme::status_ink(cx);
    let (icon, color) = match signal {
        Some(SessionSignal::Busy) => {
            return div()
                .size_2()
                .rounded_full()
                .bg(cx.theme().link)
                .into_any_element();
        }
        Some(SessionSignal::Lost | SessionSignal::Failed) => {
            (Icon::new(IconName::TriangleAlert), ink.danger)
        }
        Some(SessionSignal::AwaitingUser) => (Icon::new(crate::icons::Icon::Hand), ink.warning),
        Some(SessionSignal::UnseenTurn) => (Icon::new(IconName::CircleCheck), ink.success),
        None => (
            Icon::new(crate::icons::Icon::Circle),
            cx.theme().muted_foreground,
        ),
    };
    icon.xsmall().text_color(color).into_any_element()
}

/// The ink a state's word takes on the meta line: its own, for a state that
/// waits on the person; the line's, for every other.
fn word_ink(signal: Option<SessionSignal>, cx: &App) -> Option<Hsla> {
    let ink = crate::theme::status_ink(cx);
    match signal {
        Some(SessionSignal::Lost | SessionSignal::Failed) => Some(ink.danger),
        Some(SessionSignal::AwaitingUser) => Some(ink.warning),
        Some(SessionSignal::Busy | SessionSignal::UnseenTurn) | None => None,
    }
}

/// A state's mark in its stable column, so titles never shift beside it,
/// naming itself on hover and to assistive technology.
pub(super) fn status_mark(
    id: gpui::ElementId,
    signal: Option<SessionSignal>,
    label: SharedString,
    cx: &App,
) -> impl IntoElement + use<> {
    mark_slot(status_icon(signal, cx))
        .id(id)
        .role(gpui::accesskit::Role::Image)
        .aria_label(label.clone())
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
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
    move |menu, _, _: &mut App| {
        let (again, rename, restart, export, close) = (
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
        );
        menu.when(resend, |menu| {
            menu.item(
                crate::controls::menu_item(RESEND).on_click(move |_, _, cx: &mut App| {
                    again
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.resend_last_prompt(uid, cx)
                        })
                        .ok();
                }),
            )
        })
        .item(
            crate::controls::menu_item("Rename…").on_click(move |_, window, cx: &mut App| {
                rename
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.begin_rename(uid, window, cx);
                    })
                    .ok();
            }),
        )
        .item(crate::controls::menu_item("Restart the agent").on_click(
            move |_, window, cx: &mut App| {
                restart
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.restart_session_at(root_idx, session_idx, window, cx);
                    })
                    .ok();
            },
        ))
        .item(crate::controls::menu_item("Export as Markdown…").on_click(
            move |_, window, cx: &mut App| {
                export
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.export_session_at(root_idx, session_idx, window, cx);
                    })
                    .ok();
            },
        ))
        .separator()
        // A plain word: the conversation is kept and can be resumed, so
        // closing takes nothing that cannot come back.
        .item(crate::controls::menu_item("Close session").on_click(
            move |_, window, cx: &mut App| {
                close
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.close_session(root_idx, session_idx, window, cx);
                    })
                    .ok();
            },
        ))
    }
}

/// The words of the action that sends a failed turn's prompt again.
const RESEND: &str = "Send the last prompt again";

/// How a session row stands: the one on screen, the one the keyboard is on.
pub(super) struct Mark {
    /// The session the conversation shows: the chosen fill.
    pub(super) shown: bool,
    /// The keyboard's row, while the list holds the keys and the keys put it
    /// there: the ring, and its actions as if it were hovered.
    pub(super) at: bool,
    /// Its turn failed and its last prompt can go again.
    pub(super) resend: bool,
}

/// A session: its mark, its title and `⋯`, and under the title
/// `state · who · age`, who being the agent, or the project in a flat list,
/// the state's word in its own ink while it waits on the person. Over the end
/// of that line while the pointer or the keyboard is on the row: Stop while it
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
    let chosen = mark.shown;
    let (rest, hovered) = row_surfaces(chosen, cx);
    let (name_ink, meta_ink) = row_inks(chosen, cx);
    let group = SharedString::from(format!("rail-session-{uid}"));
    let title = SharedString::from(item.title.clone());
    let facts = SharedString::from(meta(signal, who, item.age));
    // The state's word leads the line, so its ink runs over its own bytes.
    let words = StyledText::new(facts.clone()).with_highlights(word_ink(signal, cx).map(|ink| {
        let color = HighlightStyle {
            color: Some(ink),
            ..HighlightStyle::default()
        };
        (0..super::model::word(signal).len(), color)
    }));
    let (shell, hover) = (cx.entity().downgrade(), hover_fill(cx));
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
        .relative()
        .py_1()
        // A session's mark stands under its project's folder and its title
        // under the name. The row's fill starts past the fold control's
        // column, so a chosen row carries no empty band on its left.
        .when(!flat, |d| d.ml(super::INDENT))
        .px_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .when(chosen, |d| d.bg(super::row::chosen_fill(cx)))
        .when(!chosen, |d| d.hover(move |d| d.bg(hover)))
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.rail_open_session(root, session, false, window, cx);
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
                        .text_color(name_ink),
                )
                .child(on_hover(
                    &group,
                    mark.shown || mark.at,
                    labelled(
                        ("session-menu-name", uid),
                        "Session actions",
                        menu_button(
                            row_control(("session-menu", uid), IconName::Ellipsis, cx),
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
                    faded(("meta", uid), words, group.clone(), rest, hovered)
                        .text_color(meta_ink)
                        .tooltip(move |window, cx| {
                            Tooltip::new(meta_tip.clone()).build(window, cx)
                        }),
                )
                .child(actions(
                    item,
                    mark.resend,
                    &group,
                    (rest, hovered),
                    mark.at && !mark.shown,
                    cx,
                )),
        )
        .when(mark.at, |row| row.child(key_ring(cx)))
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
/// is on the row, and while the keyboard is (`keyed`) unless it is the
/// session on screen, whose line they would hide for as long as it shows. On
/// the row's own fill, at rest or hovered, so what is under them does not show
/// through.
fn actions(
    item: &Item,
    resend: bool,
    group: &SharedString,
    (rest, hovered): (Hsla, Hsla),
    keyed: bool,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let (root, session, uid, signal) = (item.root, item.session, item.uid, item.signal);
    let button = |id: &'static str, icon: Icon, tip: &'static str, cx: &App| {
        meta_control((id, uid), icon, cx).tooltip(tip)
    };
    div()
        .h_flex()
        .absolute()
        .right_0()
        .top_0()
        .bottom_0()
        .bg(rest)
        .when(!keyed, |d| d.invisible())
        .group_hover(group.clone(), move |s| s.visible().bg(hovered))
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
