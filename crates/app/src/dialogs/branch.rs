//! The worktree split and the branch rename: the two dialogs a branch chip
//! and a project's menu open.

use super::{field, footer_row, title_row};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ClickEvent, Context, IntoElement, ParentElement, SharedString, Styled, div};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use gpui_component::{ActiveTheme, StyledExt};

// Why a dialog's buttons refuse while its work runs.
const MAKING_WORKTREE: &str = "The worktree is being made";
const RENAMING_BRANCH: &str = "The branch is being renamed";

/// Split a project onto a branch of its own, as a git worktree.
///
/// The folder is **shown, not typed**. It is derived from the branch name, so a
/// second field holding it would be a copy of a reading -- one that either
/// fights every keystroke in the field above it or silently stops following it.
/// What is left for the user to decide is the part the derivation cannot know:
/// which folder to put it under, and that is a picker rather than a path to
/// spell out.
pub fn new_worktree(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let input = shell.worktree_branch().clone();
    let Some(draft) = shell.worktree_draft() else {
        return Dialog::new(cx);
    };
    let (label, error, busy) = (draft.label.clone(), draft.error.clone(), draft.busy);
    let target = shell
        .worktree_target(cx)
        .map(|dir| SharedString::from(dir.display().to_string()));
    let named = target.is_some();
    let (muted, danger) = (
        cx.theme().muted_foreground,
        crate::theme::status_ink(cx).danger,
    );

    Dialog::new(cx)
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            content.child(title_row("New worktree", cx)).child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .child(field(
                        "Branch",
                        Some(
                            format!("A second checkout of {label}, on its own branch")
                                .into_any_element(),
                        ),
                        text_input(&input, cx).short(),
                        cx,
                    ))
                    // Where it lands, in full, in the face a path is set in. A
                    // worktree is a folder that appears on disk without anyone
                    // browsing to it, so the one thing this form owes the reader
                    // is the path it is about to create -- before it exists, not
                    // in a toast afterwards.
                    .child(match target.clone() {
                        Some(path) => div()
                            .text_xs()
                            .text_color(muted)
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(path),
                        None => div()
                            .text_xs()
                            .text_color(muted)
                            .child("Name the branch to see where its folder goes"),
                    })
                    .when_some(error.clone(), |col, why| {
                        col.child(div().text_xs().text_color(danger).child(why))
                    }),
            )
        })
        .footer(
            footer_row()
                // A third action, at the footer's start, apart from the two
                // that close it.
                .child(
                    crate::controls::action("worktree-parent")
                        .short()
                        .secondary()
                        .label("Put it somewhere else…")
                        .refuses(busy.then_some(MAKING_WORKTREE))
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.pick_worktree_parent(cx);
                        })),
                )
                .child(div().flex_1())
                .child(
                    // Spent while git is working, for the same reason Create is
                    // and one more: the command cannot be called back, so a
                    // Cancel that still offered itself would be promising to
                    // undo something already happening on disk.
                    crate::controls::action("cancel-worktree")
                        .short()
                        .quiet(cx)
                        .label("Cancel")
                        .refuses(busy.then_some(MAKING_WORKTREE))
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_worktree(cx);
                        })),
                )
                .child({
                    // Spent while git is working and while there is no name to
                    // work from: cloning a working tree takes long enough that
                    // a button still offering itself invites the second press
                    // that would ask for the same folder twice.
                    let create = crate::controls::action("create-worktree")
                        .short()
                        .primary()
                        .label("Create worktree")
                        .loading(busy);
                    let why = match (busy, named) {
                        (true, _) => Some(MAKING_WORKTREE),
                        (false, false) => Some("Name the branch first"),
                        (false, true) => None,
                    };
                    match why {
                        Some(why) => create.refuses(Some(why)),
                        None => create.on_click(cx.listener(
                            |shell: &mut Shell, _: &ClickEvent, _, cx| {
                                shell.commit_worktree(cx);
                            },
                        )),
                    }
                }),
        )
        // Esc and the close button have to clear what is putting this on screen,
        // or the dialog dismisses itself and the next frame renders it back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_worktree(cx);
        }))
}

/// The rename-this-branch form.
///
/// One field and a sentence, where the worktree form beside it has a field, a
/// derived folder and a picker — because this makes nothing and lands nowhere.
/// The field opens on the name as it stands, so the part being kept does not
/// have to be retyped to change the part that is not.
pub fn rename_branch(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let input = shell.branch_input().clone();
    let Some(draft) = shell.branch_draft() else {
        return Dialog::new(cx);
    };
    let (from, error, busy) = (draft.from.clone(), draft.error.clone(), draft.busy);
    let danger = crate::theme::status_ink(cx).danger;

    Dialog::new(cx)
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            content.child(title_row("Rename branch", cx)).child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .child(field(
                        "New name",
                        Some(format!("The branch checked out here is {from}").into_any_element()),
                        text_input(&input, cx).short(),
                        cx,
                    ))
                    // git's own words where git refused, and the name rule's
                    // where it never got that far. Shown against the name that
                    // caused it, which is why the form is still up.
                    .children(
                        error
                            .clone()
                            .map(|why| div().text_xs().text_color(danger).child(why)),
                    ),
            )
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-branch-rename")
                        .short()
                        .quiet(cx)
                        .label("Cancel")
                        .refuses(busy.then_some(RENAMING_BRANCH))
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_branch_rename(cx);
                        })),
                )
                .child({
                    let rename = crate::controls::action("commit-branch-rename")
                        .short()
                        .primary()
                        .label("Rename")
                        .loading(busy);
                    match busy {
                        true => rename.refuses(Some(RENAMING_BRANCH)),
                        false => rename.on_click(cx.listener(
                            |shell: &mut Shell, _: &ClickEvent, _, cx| {
                                shell.commit_branch_rename(cx);
                            },
                        )),
                    }
                }),
        )
        // Esc and the close button have to clear what is putting this on screen,
        // or the dialog dismisses itself and the next frame renders it back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_branch_rename(cx);
        }))
}
