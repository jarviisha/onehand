//! The modal windows: the conversation rename, the worktree split, the branch
//! rename, the issue picker and the workflow launcher.
//!
//! The component library owns overlays, focus traps and Escape handling.

use crate::controls::Refuses as _;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, relative,
};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::{Dialog, DialogClose, DialogTitle};
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Disableable, Icon, IconName, Sizable as _, StyledExt};

mod issue;

pub use issue::pick_issue;
pub(crate) use issue::{issue_row, issue_workflow_menu, page_row};

/// A dialog's name, and the ✕ that closes it.
///
/// Both sit in the dialog's *content* rather than in its own `title` and
/// `close_button` slots, for two separate reasons that happen to share one
/// answer.
///
/// **The name.** A dialog opened from a trigger rebuilds itself from nothing on
/// every press, and what survives that is its content builder, its style and its
/// props — not its title, header or footer, which are elements and so cannot be
/// cloned into a closure that runs again on each open. A name set through the
/// slot is therefore dropped in silence on any dialog the rail opens, and the
/// content builder is the only slot left to put it in. Every dialog here goes
/// through this one, trigger or not: a rule half the call sites follow is the
/// rule the next call site forgets.
///
/// **The ✕.** The library builds its own out of a plain library button inside
/// the dialog element, so it never passes through the app's action wrapper and
/// ends up the single control on a dialog drawing the arrow cursor while
/// everything inside it answers the pointer. Turning that one off and drawing
/// ours puts it on the line that already carries the name.
///
/// It still closes through the library's own `DialogClose`, which dispatches the
/// dialog's cancel action — the same path the built-in took, so the handlers
/// that clear a half-finished rename or worktree still run. The fixed box around
/// it is what contains that element's `size_full`, which would otherwise take
/// the whole row away from the name beside it.
pub(super) fn title_row(name: impl Into<SharedString>) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .w_full()
        .child(
            DialogTitle::new()
                .min_w_0()
                // The library sets this title's line height to exactly one em,
                // and `truncate` clips to the box -- so every descender is cut
                // off at the baseline, which is the "g" in *Settings* losing
                // its tail. The refinement lands after the library's own, so
                // asking for the room back here is enough.
                .line_height(relative(1.3))
                .truncate()
                .child(name.into()),
        )
        .child(
            div().flex_none().size_6().child(
                DialogClose::new().child(
                    crate::controls::action("dialog-close")
                        .small()
                        .ghost()
                        .icon(Icon::new(IconName::Close)),
                ),
            ),
        )
}

/// The conversation-rename window.
///
/// **No trigger.** Every other dialog here is opened by a control that can
/// carry one, so `Dialog::trigger` ties the two together and "at most one open"
/// is structural. This one is opened from a menu entry, which is gone by the
/// time the dialog would appear — a `Dialog` built without a trigger renders
/// already open, so the shell decides whether it exists at all.
///
/// Until this existed the rename was unreachable: core could name a
/// conversation and the archive could store the name, and nothing anywhere
/// called either, so a conversation was stuck with the summary guessed from its
/// first prompt for good.
pub fn rename_session(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let input = shell.rename_input().clone();
    let resettable = shell.rename_is_override(cx);

    Dialog::new(cx)
        .close_button(false)
        .content(move |content, _, _: &mut App| {
            content
                .child(title_row("Rename conversation"))
                .child(div().v_flex().gap_1().w_full().child(Input::new(&input)))
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                // Offered only when there is an override to drop. On a
                // conversation that never had one this button would look like
                // it clears the title, and it does not: the title comes back,
                // derived from the first prompt.
                .when(resettable, |row| {
                    row.child(
                        crate::controls::action("reset-title")
                            .ghost()
                            .label("Use the automatic title")
                            .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                                shell.reset_conversation_title(cx);
                            })),
                    )
                })
                .child(
                    crate::controls::action("cancel-rename")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_rename(cx);
                        })),
                )
                .child(
                    crate::controls::action("save-rename")
                        .primary()
                        .label("Rename")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.commit_rename(cx);
                        })),
                ),
        )
        // Esc and the close button both mean the same thing here, and both have
        // to clear the state that is putting this on screen -- otherwise the
        // dialog dismisses itself and the next frame renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_rename(cx);
        }))
}

/// The workflow launcher: which template, what to do, and what to ask of
/// every step.
///
/// **No trigger**, for the rename's reason: it is opened from a menu entry or
/// a key, so the shell decides whether it exists.
///
/// Where the run works is the template's to say, and said under its name, so
/// nobody presses *Run* expecting a checkout and gets a new worktree.
pub fn run_workflow(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(launcher) = shell.workflow_launcher() else {
        return Dialog::new(cx);
    };
    let entries = crate::workflow::templates(cx);
    let picked = entries.get(launcher.template).cloned();
    let heading = format!("Run a workflow on {}", launcher.project);
    let (title, body, instructions) = (
        launcher.title.clone(),
        launcher.body.clone(),
        launcher.instructions.clone(),
    );
    let (error, busy, preview) = (launcher.error.clone(), launcher.busy, launcher.preview);
    let (muted, danger) = (
        cx.theme().muted_foreground,
        crate::theme::status_ink(cx).danger,
    );
    let about = match picked.as_ref().map(|entry| &entry.template) {
        Some(Ok(template)) => format!(
            "{} {}.",
            template.description.trim(),
            match template.place {
                onehand_core::workflow::Place::Checkout => "It works in this checkout",
                onehand_core::workflow::Place::Worktree => {
                    "It works on a new branch, in a worktree of its own"
                }
            }
        ),
        Some(Err(why)) => format!("This workflow cannot be read: {why}"),
        None => "No workflow is on offer yet.".to_string(),
    };
    let handle = cx.entity();
    let left_out = entries
        .len()
        .saturating_sub(crate::workflow::TEMPLATES_SHOWN);
    let names: Vec<(usize, String, bool)> = entries
        .iter()
        .take(crate::workflow::TEMPLATES_SHOWN)
        .enumerate()
        .map(|(at, entry)| (at, entry.name(), entry.file.is_none()))
        .collect();
    let current = launcher.template;
    let runnable = picked
        .as_ref()
        .and_then(|entry| entry.template.clone().ok());
    let picker_name = picked.map_or_else(|| "Pick a workflow".to_string(), |e| e.name());
    let (margin, room) = form_room(window);

    Dialog::new(cx)
        .margin_top(margin)
        .p(gpui::rems(1.))
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            // Read here, as it is typed, so the preview follows the brief.
            let shown = runnable.as_ref().map(|template| {
                let brief = crate::shell::brief(&title, &body, &instructions, cx);
                workflow_preview(
                    template,
                    &brief,
                    preview,
                    Shell::toggle_workflow_preview,
                    true,
                    &handle,
                    cx,
                )
            });
            let handle = handle.clone();
            let names = names.clone();
            let picker = crate::controls::menu_below(
                "workflow-template",
                crate::controls::action("workflow-template-trigger")
                    .outline()
                    .small()
                    .label(picker_name.clone())
                    .icon(Icon::new(IconName::ChevronDown)),
                move |mut menu, _, _| {
                    for (at, name, shipped) in &names {
                        let (at, handle) = (*at, handle.clone());
                        let label = match shipped {
                            true => format!("{name} (built in)"),
                            false => name.clone(),
                        };
                        menu = menu.item(
                            crate::controls::menu_item(label)
                                .checked(at == current)
                                .on_click(move |_, _, cx: &mut App| {
                                    handle.update(cx, |shell: &mut Shell, cx| {
                                        shell.pick_workflow_template(at, cx)
                                    });
                                }),
                        );
                    }
                    if left_out > 0 {
                        menu = menu.label(format!("{left_out} more workflows not shown"));
                    }
                    menu
                },
            );
            let label = |text: &'static str| div().text_sm().child(text);
            // The column sits in the scrolling box rather than being it, or
            // its fields would shrink to fit instead of scrolling.
            content.child(title_row(heading.clone())).child(
                div()
                    .id("workflow-launcher-body")
                    .w_full()
                    .max_h(room)
                    .overflow_y_scroll()
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .child(label("Workflow"))
                            .child(div().h_flex().child(picker))
                            .child(div().text_xs().text_color(muted).child(about.clone()))
                            .children(shown)
                            .child(label("Title"))
                            .child(Input::new(&title))
                            .child(label("Details"))
                            .child(gpui_component::input::Textarea::new(&body).h(gpui::rems(8.)))
                            .child(label("Instructions"))
                            .child(
                                gpui_component::input::Textarea::new(&instructions)
                                    .h(gpui::rems(4.)),
                            )
                            .when_some(error.clone(), |col, why| {
                                col.child(div().text_xs().text_color(danger).child(why))
                            }),
                    ),
            )
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                .child(
                    crate::controls::action("cancel-workflow")
                        .ghost()
                        .label("Cancel")
                        .refuses(busy)
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_workflow(cx);
                        })),
                )
                .child({
                    let run = crate::controls::action("run-workflow")
                        .primary()
                        .label(if busy {
                            "Making the worktree…"
                        } else {
                            "Run"
                        });
                    match busy {
                        true => crate::controls::resting(run).disabled(true),
                        false => run.on_click(cx.listener(
                            |shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.commit_workflow(window, cx);
                            },
                        )),
                    }
                }),
        )
        // Esc and the close button have to clear what is putting this on
        // screen, or it renders straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_workflow(cx);
        }))
}

/// A form dialog's margin from the top of the window, and the room its form
/// has under the heading and over the footer.
///
/// The dialog sits its margin down from the top of the window, inside any
/// frame the window draws, and keeps at least that margin under it; the form
/// takes what is left and scrolls past that, so an open preview never pushes
/// *Run* off screen. The padding is in rems, so the room set aside for the
/// rest is too.
fn form_room(window: &Window) -> (gpui::Pixels, gpui::Pixels) {
    let rem = window.rem_size();
    let frame = gpui_component::window_paddings(window);
    let margin = rem * LAUNCHER_MARGIN;
    let room = (window.viewport_size().height
        - frame.top
        - frame.bottom
        - margin * 2.
        - rem * LAUNCHER_CHROME)
        .max(gpui::px(0.));
    (margin, room)
}

/// The launcher's margin above it, and the least below it, in rems.
const LAUNCHER_MARGIN: f32 = 3.;

/// What the launcher takes besides its form, in rems: its heading, its
/// footer, its padding and the gaps between them, with room to spare.
const LAUNCHER_CHROME: f32 = 10.;

/// How many steps the launcher's preview lists before saying how many more
/// there are.
const PREVIEW_STEPS: usize = 12;

/// What a run of `template` on `brief` starts with, behind a toggle that
/// calls `toggle`: where it works and its limits when `head` asks for them
/// (a form that shows them above leaves them out), a line per step, and the
/// first prompt exactly as the agent would receive it.
fn workflow_preview(
    template: &onehand_core::workflow::Template,
    brief: &onehand_core::workflow::Brief,
    open: bool,
    toggle: fn(&mut Shell, &mut Context<Shell>),
    head: bool,
    handle: &Entity<Shell>,
    cx: &App,
) -> AnyElement {
    let (muted, border, radius) = (
        cx.theme().muted_foreground,
        cx.theme().border,
        cx.theme().radius,
    );
    let toggle = {
        let handle = handle.clone();
        crate::controls::action("workflow-preview")
            .ghost()
            .small()
            .label("Preview")
            .icon(Icon::new(match open {
                true => IconName::ChevronDown,
                false => IconName::ChevronRight,
            }))
            .on_click(move |_, _, cx: &mut App| {
                handle.update(cx, toggle);
            })
    };
    let column = div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(div().h_flex().child(toggle));
    if !open {
        return column.into_any_element();
    }
    let head = head.then(|| {
        format!(
            "{} · times out after {} · {} misses allowed · version {}",
            template.place.label(),
            template.timeout,
            template.misses,
            template.version
        )
    });
    let left_out = template.steps.len().saturating_sub(PREVIEW_STEPS);
    let steps = template
        .steps
        .iter()
        .take(PREVIEW_STEPS)
        .enumerate()
        .map(|(at, step)| {
            div()
                .text_xs()
                .child(format!("{}. {}: {}", at + 1, step.label, step.summary()))
        });
    let prompt = onehand_core::workflow::first_prompt(template, brief)
        .unwrap_or_else(|| "No step prompts the agent.".to_string());
    column
        .children(head.map(|head| div().text_xs().text_color(muted).child(head)))
        .children(steps)
        .when(left_out > 0, |col| {
            col.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{left_out} more steps not shown")),
            )
        })
        .child(div().text_xs().text_color(muted).child("The first prompt"))
        .child(
            div()
                .id("workflow-preview-prompt")
                .w_full()
                .max_h(gpui::rems(12.))
                .overflow_y_scroll()
                .p_2()
                .border_1()
                .border_color(border)
                .rounded(radius)
                .text_xs()
                .child(prompt),
        )
        .into_any_element()
}

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
        .content(move |content, _, _: &mut App| {
            content.child(title_row("New worktree")).child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("A second checkout of {label}, on its own branch.")),
                    )
                    .child(Input::new(&input))
                    // Where it lands, in full. A worktree is a folder that
                    // appears on disk without anyone browsing to it, so the one
                    // thing this form owes the reader is the path it is about
                    // to create -- before it exists, not in a toast afterwards.
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(target.clone().unwrap_or_else(|| {
                                SharedString::from("Name the branch to see where its folder goes.")
                            })),
                    )
                    .when_some(error.clone(), |col, why| {
                        col.child(div().text_xs().text_color(danger).child(why))
                    }),
            )
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                .child(
                    crate::controls::action("worktree-parent")
                        .ghost()
                        .label("Put it somewhere else…")
                        .refuses(busy)
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.pick_worktree_parent(cx);
                        })),
                )
                .child(
                    // Spent while git is working, for the same reason Create is
                    // and one more: the command cannot be called back, so a
                    // Cancel that still offered itself would be promising to
                    // undo something already happening on disk.
                    crate::controls::action("cancel-worktree")
                        .ghost()
                        .label("Cancel")
                        .refuses(busy)
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_worktree(cx);
                        })),
                )
                .child({
                    // Spent while git is working and while there is no name to
                    // work from: cloning a working tree takes long enough that
                    // a button still offering itself invites the second press
                    // that would ask for the same folder twice.
                    let create =
                        crate::controls::action("create-worktree")
                            .primary()
                            .label(if busy {
                                "Creating…"
                            } else {
                                "Create worktree"
                            });
                    match busy || !named {
                        true => crate::controls::resting(create).disabled(true),
                        false => create.on_click(cx.listener(
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
    let (muted, danger) = (
        cx.theme().muted_foreground,
        crate::theme::status_ink(cx).danger,
    );

    Dialog::new(cx)
        .close_button(false)
        .content(move |content, _, _: &mut App| {
            content.child(title_row("Rename branch")).child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("The branch checked out here is {from}.")),
                    )
                    .child(Input::new(&input))
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
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .child(
                    crate::controls::action("cancel-branch-rename")
                        .label("Cancel")
                        .refuses(busy)
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_branch_rename(cx);
                        })),
                )
                .child({
                    let rename = crate::controls::action("commit-branch-rename")
                        .primary()
                        .label(if busy { "Renaming…" } else { "Rename" });
                    match busy {
                        true => crate::controls::resting(rename).disabled(true),
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

#[cfg(test)]
mod tests;
