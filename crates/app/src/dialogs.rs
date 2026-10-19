//! The modal windows: the conversation rename, the worktree split, the branch
//! rename, the issue picker and the workflow launcher.
//!
//! The component library owns overlays, focus traps and Escape handling.

use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_area;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, relative,
};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::{Dialog, DialogClose, DialogTitle};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

mod branch;
mod issue;

pub use branch::{new_worktree, rename_branch};

pub use issue::pick_issue;
pub(crate) use issue::{PickJudged, issue_row, issue_workflow_menu, page_row};

/// How tall a form's details field stands: a paragraph in view.
const DETAILS_H: gpui::Rems = gpui::rems(8.);
/// How tall a form's instructions field stands: a line or two, since they are
/// optional and added to every step.
const INSTRUCTIONS_H: gpui::Rems = gpui::rems(4.);
/// How tall the first prompt's preview grows before it scrolls, so the form's
/// fields stay in reach under it.
const PREVIEW_MAX_H: gpui::Rems = gpui::rems(12.);
/// One line of a preflight finding: the extra small text size (0.75rem) at
/// the reading leading, so its mark sits on the first line's centre.
const FINDING_LINE: gpui::Rems = gpui::rems(0.75 * crate::tokens::LEADING);

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
pub(super) fn title_row(name: impl Into<SharedString>, cx: &App) -> impl IntoElement {
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
                .line_height(relative(crate::tokens::LEADING_TIGHT))
                .truncate()
                .child(name.into()),
        )
        .child(
            div().flex_none().size_6().child(
                DialogClose::new().child(
                    crate::controls::action("dialog-close")
                        .small()
                        .quiet_icon(false, cx)
                        .icon(Icon::new(IconName::Close))
                        .tooltip("Close"),
                ),
            ),
        )
}

/// A dialog's footer: a third action at its start, where there is one, then
/// a spacer, then the dismissing button and the primary at its end.
pub(crate) fn footer_row() -> gpui::Div {
    div().h_flex().items_center().gap_2().w_full()
}

/// One field of a dialog's form: the plugin host's, as Settings' are.
use onehand_plugin_host::field;

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
    let empty = input.read(cx).value().trim().is_empty();

    Dialog::new(cx)
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            content
                .child(title_row("Rename conversation", cx))
                .child(field("Title", None, text_input(&input, cx).short(), cx))
        })
        .footer(
            footer_row()
                // Offered only when there is an override to drop. On a
                // conversation that never had one this button would look like
                // it clears the title, and it does not: the title comes back,
                // derived from the first prompt. A third action, so at the
                // footer's start, apart from the two that close it.
                .when(resettable, |row| {
                    row.child(
                        crate::controls::action("reset-title")
                            .short()
                            .secondary()
                            .label("Use the automatic title")
                            .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                                shell.reset_conversation_title(cx);
                            })),
                    )
                })
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-rename")
                        .short()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_rename(cx);
                        })),
                )
                .child(
                    crate::controls::action("save-rename")
                        .short()
                        .primary()
                        .label("Rename")
                        .refuses(empty.then_some("Name it first"))
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
    let found = shell.launcher_preflight(cx).unwrap_or_default();
    let blocking = found.iter().filter(|f| f.blocks).count();
    let (muted, danger) = (
        cx.theme().muted_foreground,
        crate::theme::status_ink(cx).danger,
    );
    let about = match picked.as_ref().map(|entry| &entry.template) {
        Some(Ok(template)) => format!(
            "{} {}",
            template.description.trim(),
            match template.place {
                onehand_core::workflow::Place::Checkout => "It works in this checkout",
                onehand_core::workflow::Place::Worktree => {
                    "It works on a new branch, in a worktree of its own"
                }
            }
        ),
        Some(Err(why)) => format!("This workflow cannot be read: {why}"),
        None => "No workflow is on offer yet".to_string(),
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
        .w(form_width(window))
        .margin_top(margin)
        .p_4()
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
            let lines: Vec<_> = found
                .iter()
                .enumerate()
                .map(|(at, finding)| finding_line(at, finding, danger, muted, &handle, None, cx))
                .collect();
            let handle = handle.clone();
            let names = names.clone();
            let picker = crate::controls::menu_below(
                "workflow-template",
                onehand_plugin_host::picker(
                    "workflow-template-trigger",
                    picker_name.clone(),
                    true,
                    onehand_plugin_host::ControlSize::Short,
                    cx,
                ),
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
            // The column sits in the scrolling box rather than being it, or
            // its fields would shrink to fit instead of scrolling.
            content.child(title_row(heading.clone(), cx)).child(
                div()
                    .id("workflow-launcher-body")
                    .w_full()
                    .max_h(room)
                    .overflow_y_scroll()
                    .child(
                        div()
                            .v_flex()
                            .gap_4()
                            .w_full()
                            // What it runs, what it is asked, and what stands
                            // in the way: each a section of its own.
                            .child(sections(
                                [
                                    Some(
                                        div()
                                            .v_flex()
                                            .gap_4()
                                            .child(field(
                                                "Workflow",
                                                Some(about.clone().into_any_element()),
                                                div().h_flex().child(picker),
                                                cx,
                                            ))
                                            .children(shown)
                                            .into_any_element(),
                                    ),
                                    Some(
                                        div()
                                            .v_flex()
                                            .gap_4()
                                            .child(field(
                                                "Title",
                                                None,
                                                text_input(&title, cx).short(),
                                                cx,
                                            ))
                                            .child(field(
                                                "Details",
                                                None,
                                                text_area(&body, cx).h(DETAILS_H),
                                                cx,
                                            ))
                                            .child(field(
                                                "Instructions",
                                                None,
                                                text_area(&instructions, cx).h(INSTRUCTIONS_H),
                                                cx,
                                            ))
                                            .into_any_element(),
                                    ),
                                    // What blocks it, above the footer's primary.
                                    (!lines.is_empty() || error.is_some()).then(|| {
                                        div()
                                            .v_flex()
                                            .gap_2()
                                            .children(lines)
                                            .when_some(error.clone(), |col, why| {
                                                col.child(
                                                    div().text_xs().text_color(danger).child(why),
                                                )
                                            })
                                            .into_any_element()
                                    }),
                                ]
                                .into_iter()
                                .flatten(),
                                cx,
                            ))
                            // How many of them block it, last, standing over
                            // the footer's primary.
                            .children(blocking_note(blocking, danger)),
                    ),
            )
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-workflow")
                        .short()
                        .quiet(cx)
                        .label("Cancel")
                        .refuses(busy.then_some(STARTING))
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_workflow(cx);
                        })),
                )
                .child({
                    let run = crate::controls::action("run-workflow")
                        .short()
                        .primary()
                        .label("Run")
                        .loading(busy);
                    let why = match busy {
                        true => Some(STARTING.to_string()),
                        false => blocked(blocking),
                    };
                    match why {
                        Some(why) => run.refuses(Some(why)),
                        None => run.on_click(cx.listener(
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

/// How wide a dialog that starts a run stands: [`crate::tokens::DIALOG_FORM`],
/// or the window less its margin on each side when that is narrower, since the
/// library centres a dialog by its width and never shrinks it.
fn form_width(window: &Window) -> gpui::Pixels {
    let rem = window.rem_size();
    let frame = gpui_component::window_paddings(window);
    let room = window.viewport_size().width - frame.left - frame.right - rem * LAUNCHER_MARGIN * 2.;
    (rem * crate::tokens::DIALOG_FORM.0)
        .min(room)
        .max(gpui::px(0.))
}

/// A form's sections, a hairline between each two: what a run is, where it
/// works and what blocks it run together as one paragraph on space alone.
/// The line sits on a wrapper of the section's own, as a box's rows' does.
fn sections(groups: impl IntoIterator<Item = AnyElement>, cx: &App) -> gpui::Div {
    let rule = cx.theme().border;
    div()
        .v_flex()
        .gap_4()
        .w_full()
        .children(groups.into_iter().enumerate().map(|(i, group)| {
            div()
                .w_full()
                .when(i > 0, |section| {
                    section.border_t_1().border_color(rule).pt_4()
                })
                .child(group)
        }))
}

/// One thing the preflight found: in the danger ink when it blocks, muted
/// when it only says, with where it is changed, and *Retry…* for an earlier
/// task worth retrying instead, which opens its Retry dialog.
pub(crate) fn finding_line(
    at: usize,
    finding: &onehand_core::preflight::Finding,
    danger: gpui::Hsla,
    muted: gpui::Hsla,
    handle: &Entity<Shell>,
    ask: Option<&crate::agent_check::Ask>,
    cx: &App,
) -> impl IntoElement {
    let text = match finding
        .change
        .and_then(onehand_core::preflight::Change::said)
    {
        Some(change) => format!("{}{change}", finding.text),
        None => finding.text.clone(),
    };
    let shell = handle.clone();
    let (mark, ink) = match finding.blocks {
        true => (IconName::TriangleAlert, danger),
        false => (IconName::Info, muted),
    };
    div()
        .h_flex()
        .gap_2()
        .items_center()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_flex()
                .items_start()
                .gap_2()
                .text_xs()
                .line_height(relative(crate::tokens::LEADING))
                .text_color(ink)
                // The mark says whether it blocks the start or only tells,
                // centred on the first line's height when the text wraps.
                .child(onehand_plugin_host::mark_slot(Icon::new(mark).xsmall()).h(FINDING_LINE))
                .child(div().flex_1().min_w_0().child(text)),
        )
        .children(finding.task.clone().map(|task| {
            // Filled, as every action beside a finding's sentence is: a ghost
            // there reads as more of the text.
            crate::controls::action(("pick-retry-task", at))
                .secondary()
                .short()
                .label("Retry…")
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    shell.update(cx, |shell, cx| {
                        shell.cancel_pick(cx);
                        shell.begin_retry(task.clone(), window, cx);
                    });
                })
        }))
        // Who holds each slot, as the slots stand now, each opening its
        // task in place of the start.
        .children(
            (finding.check == onehand_core::preflight::Check::Slot)
                .then(|| crate::unattended::slots(cx))
                .flatten()
                // Only a full slot is refused for its holders; at the cap on
                // waiting runs they are not what stands in the way.
                .filter(|slots| slots.taken())
                .into_iter()
                .flat_map(|slots| slots.holders)
                .take(onehand_core::unattended::HOLDERS_SAID)
                .enumerate()
                .map(|(i, holder)| {
                    let (shell, task) = (handle.clone(), holder.task);
                    // The line above names each holder; the button opens it.
                    crate::controls::action(SharedString::from(format!("slot-holder-{at}-{i}")))
                        .secondary()
                        .short()
                        .label(format!("Open {}", holder.named))
                        .tooltip("Open the task holding this slot")
                        .on_click(move |_, window: &mut Window, cx: &mut App| {
                            gpui_component::WindowExt::close_dialog(window, cx);
                            shell.update(cx, |shell, cx| {
                                shell.cancel_pick(cx);
                                shell.show_task(&task, window, cx);
                            });
                        })
                }),
        )
        // A mode not known yet can be learned now, at the cost of an agent
        // start, and only on a press.
        .children(
            ask.filter(|_| {
                finding.check == onehand_core::preflight::Check::Mode && !finding.blocks
            })
            .map(|ask| crate::agent_check::button(at, ask, cx)),
        )
}

/// Why *Run* is spent, said once, at the end of the form and so above the
/// footer: how many things above block the start. What each is, and where it
/// is changed, is in the form.
fn blocking_note(blocking: usize, danger: gpui::Hsla) -> Option<impl IntoElement> {
    let why = match blocking {
        0 => return None,
        1 => "One thing above blocks the start".to_string(),
        n => format!("{n} things above block the start"),
    };
    Some(div().text_xs().text_color(danger).child(why))
}

/// *Run*'s refusal while something above blocks the start: what answers it,
/// leaving the count to the note over the footer rather than saying it twice.
fn blocked(blocking: usize) -> Option<String> {
    (blocking > 0).then(|| BLOCKED.to_string())
}

// Why a dialog's buttons refuse while its work runs: the work cannot be called
// back, nor asked for a second time.
const STARTING: &str = "The run is starting";
const BLOCKED: &str = "Clear what blocks the start first";

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
    let muted = cx.theme().muted_foreground;
    let toggle = {
        let handle = handle.clone();
        onehand_plugin_host::fold_toggle("workflow-preview", "Preview", open).on_click(
            move |_, _, cx: &mut App| {
                handle.update(cx, toggle);
            },
        )
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
        .unwrap_or_else(|| "No step prompts the agent".to_string());
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
        // The prompt as the agent receives it: machine text, in a well.
        .child(
            onehand_plugin_host::well(cx)
                .id("workflow-preview-prompt")
                .max_h(PREVIEW_MAX_H)
                .overflow_y_scroll()
                .p_2()
                .child(prompt),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
