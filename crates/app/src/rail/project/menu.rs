//! What a project row offers behind its `⋯`, and on a right-click.

use super::{pick_item, unattended_item};
use crate::chat::pane::ProjectFacts;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ParentElement as _, Styled as _, WeakEntity, Window, div};
use gpui_component::menu::PopupMenu;

/// Everything a project row offers, behind one button.
///
/// **Not a ✕ any more.** A remove control sitting on the row that also *selects*
/// the project put the one irreversible action in the rail under its smallest
/// target, a few pixels from the thing users click most -- and next to a
/// session row's ✕, which closes one session, it read as "close this tab"
/// rather than "drop this project from the workspace". Behind a menu the
/// removal gets a full-width label that says what it removes, sits last, is
/// separated from the harmless entries above it, and is drawn in the danger
/// tint. The other four entries are things that were either buried or reachable
/// only by first selecting the project.
///
/// The button shows on the row under the pointer, over the row's end, so a
/// rail at rest is a rail of names rather than of controls; right-click
/// reaches the same entries, exactly as on a session row — that parity is the
/// point. While this menu existed only as
/// a dropdown, *Remove from workspace* and *New worktree…* could be reached on
/// one row in the rail and nowhere else, so acting on a project always meant
/// selecting it first and tearing down whatever was on screen on the way.
///
/// Written against `&mut App` rather than `&mut Context<PopupMenu>` for the
/// reason `session::session_menu` is: the dropdown host and the context-menu host
/// disagree about that argument, and `Context` derefs to `App`.
///
/// `asks`: removing it asks first, which the word's ellipsis says.
pub(super) fn project_menu(
    root_idx: usize,
    ProjectFacts {
        pinned,
        is_repo,
        unattended,
        check: _,
        asks: _,
        offload,
    }: ProjectFacts,
    asks: bool,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, cx: &mut App| {
        let danger = crate::theme::status_ink(cx).danger;
        let away = shell.clone();
        let (pin, auto, pick, start, split, terminal, copy, refresh, remove) = (
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
        );
        menu.item(
            // The label is the state readout as well as the action: with no
            // pin marker of its own a row would otherwise only say it is
            // pinned by *where* it is, which reads as an accident.
            crate::controls::menu_item(if pinned { "Unpin" } else { "Pin to top" }).on_click(
                move |_, window, cx: &mut App| {
                    pin.update(cx, |shell: &mut Shell, cx| {
                        shell.toggle_pin(root_idx, window, cx);
                    })
                    .ok();
                },
            ),
        )
        .when_some(unattended, |menu, on| {
            menu.item(unattended_item(on, move |_, window, cx: &mut App| {
                auto.update(cx, |shell: &mut Shell, cx| {
                    shell.toggle_unattended(root_idx, window, cx);
                })
                .ok();
            }))
        })
        .when(is_repo && unattended.is_some(), |menu| {
            menu.item(pick_item(move |_, window, cx: &mut App| {
                pick.update(cx, |shell: &mut Shell, cx| {
                    shell.begin_pick(root_idx, None, window, cx)
                })
                .ok();
            }))
        })
        .item(
            crate::controls::menu_item("New session").on_click(move |_, window, cx: &mut App| {
                start
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.new_session_in(root_idx, window, cx);
                    })
                    .ok();
            }),
        )
        // Only where there is a repository to split. On a plain folder the
        // entry could not do anything but report that git said no, and an
        // entry whose whole job is to fail is one the eye has to learn to
        // skip.
        .when(is_repo, |menu| {
            menu.item(crate::controls::menu_item("New worktree…").on_click(
                move |_, window, cx: &mut App| {
                    split
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.begin_worktree(root_idx, window, cx);
                        })
                        .ok();
                },
            ))
        })
        .item(crate::controls::menu_item("Open terminal").on_click(
            move |_, window, cx: &mut App| {
                terminal
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.open_terminal_in(root_idx, window, cx);
                    })
                    .ok();
            },
        ))
        .item(crate::controls::menu_item("Copy project path").on_click(
            move |_, window, cx: &mut App| {
                copy.update(cx, |shell: &mut Shell, cx| {
                    shell.copy_root_path(root_idx, window, cx);
                })
                .ok();
            },
        ))
        // A host's status is read over its attach, as the sweep does.
        .item(crate::controls::menu_item("Refresh Git status").on_click(
            move |_, _, cx: &mut App| {
                refresh
                    .update(cx, |shell: &mut Shell, cx| shell.refresh_git(cx))
                    .ok();
            },
        ))
        .when_some(offload, |menu, back| {
            let label = match back {
                true => "Run here again",
                false => "Offload to a host…",
            };
            menu.item(
                crate::controls::menu_item(label).on_click(move |_, window, cx: &mut App| {
                    away.update(cx, |shell: &mut Shell, cx| match back {
                        true => shell.run_here_again(root_idx, window, cx),
                        false => shell.begin_offload(root_idx, window, cx),
                    })
                    .ok();
                }),
            )
        })
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| {
                div().text_color(danger).child(match asks {
                    true => "Remove from workspace…",
                    false => "Remove from workspace",
                })
            })
            .on_click(move |_, window, cx: &mut App| {
                remove
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.remove_root(root_idx, window, cx);
                    })
                    .ok();
            }),
        )
    }
}
