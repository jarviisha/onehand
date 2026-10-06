//! The issue picker, and the rows every list of issues is drawn with.

use super::title_row;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::{ActiveTheme, Disableable, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::preflight::{Check, Finding};
use onehand_core::unattended::{self as core, IssueRow, Tracker};
use onehand_core::workflow::Template;

/// A project's open issues, and the form that starts a run on the one chosen.
///
/// **No trigger**, for the rename's reason: it is opened from a menu entry that
/// is gone by the time the list arrives, so the shell decides whether it exists.
///
/// **Who opened each issue is on its row.** The issue's body goes into the
/// agent's prompt word for word, and the agent works with the user's
/// credentials; the automatic search takes only the user's own issues for that
/// reason, and a person picking by hand from everybody's is shown whose text
/// they are about to hand over.
///
/// Opened on one issue, from that issue, there is no row to pick: the form
/// reads top to bottom in the order a person decides, and *Run* is the one
/// thing left to press.
pub fn pick_issue(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(picker) = shell.issue_picker() else {
        return Dialog::new(cx);
    };
    let heading = match picker.only {
        Some(number) => format!("Run a workflow on issue {number} in {}", picker.project),
        None => format!("Open issues in {}", picker.project),
    };
    let found = picker.found.clone();
    let (at, narrowed, preview) = (picker.chosen, picker.only.is_some(), picker.preview);
    let instructions = picker.instructions.clone();
    let picked = picker.workflow.clone();
    let chosen = picker.chosen();
    // Judged on every frame from what the app holds, so a block fixed
    // elsewhere (an agent come up, a slot let go) clears as it happens.
    let judged = shell.pick_preflight(cx);
    let blocking = judged
        .as_ref()
        .map_or(0, |(found, _)| found.iter().filter(|f| f.blocks).count());
    let runnable = judged.is_some() && blocking == 0;
    let danger = crate::theme::status_ink(cx).danger;
    let (margin, room) = super::form_room(window);
    let handle = cx.entity();
    Dialog::new(cx)
        .margin_top(margin)
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            let list = (!narrowed || chosen.is_none()).then(|| {
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .when(!narrowed, |col| {
                        col.child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                            "Choose an issue to start a run on. Starting claims it where it \
                             lives and starts an agent on it in a worktree of its own.",
                        ))
                    })
                    .child(issue_list(found.as_deref(), at, &handle, cx))
            });
            let form = chosen
                .as_ref()
                .zip(judged.as_ref())
                .map(|((tracker, row), judged)| {
                    start_form(
                        tracker,
                        row,
                        picked.as_deref(),
                        judged,
                        &instructions,
                        preview,
                        &handle,
                        cx,
                    )
                });
            // The column sits in the scrolling box rather than being it, or
            // its fields would shrink to fit instead of scrolling.
            content.child(title_row(heading.clone())).child(
                div()
                    .id("issue-picker-body")
                    .w_full()
                    .max_h(room)
                    .overflow_y_scroll()
                    .child(
                        div()
                            .v_flex()
                            .gap_3()
                            .w_full()
                            .children(list)
                            .children(form),
                    ),
            )
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                // Why *Run* is spent, beside it; what each block is, and
                // where it is changed, is in the form above.
                .when(blocking > 0, |row| {
                    row.child(div().text_xs().text_color(danger).child(match blocking {
                        1 => "One thing above blocks the start.".to_string(),
                        n => format!("{n} things above block the start."),
                    }))
                })
                .child(
                    crate::controls::action("cancel-pick")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_pick(cx);
                        })),
                )
                .child({
                    let run = crate::controls::action("run-pick").primary().label("Run");
                    match runnable {
                        false => crate::controls::resting(run).disabled(true),
                        true => run.on_click(cx.listener(
                            |shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.commit_pick(window, cx);
                            },
                        )),
                    }
                }),
        )
        // Esc and the close button both put the list away, and both have to
        // clear what is putting it on screen, or it renders straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_pick(cx);
        }))
}

/// The form that starts a run on `row`: the workflow and what it is, where it
/// works, what the preflight `judged` blocks or says, what the person adds to
/// the brief, the limits, and the preview of the first prompt.
#[allow(clippy::too_many_arguments)]
fn start_form(
    tracker: &Tracker,
    row: &IssueRow,
    picked: Option<&str>,
    judged: &(Vec<Finding>, Result<Template, String>),
    instructions: &Entity<TextareaState>,
    preview: bool,
    handle: &Entity<Shell>,
    cx: &App,
) -> AnyElement {
    let (findings, template) = judged;
    let (muted, danger) = (
        cx.theme().muted_foreground,
        crate::theme::status_ink(cx).danger,
    );
    let label = |text: &'static str| div().text_sm().child(text);
    let shell = handle.clone();
    let menu = issue_workflow_menu(
        "pick-workflow",
        picked,
        Some("By the issue's labels"),
        move |id, _, cx| shell.update(cx, |shell, cx| shell.pick_issue_workflow(id, cx)),
        cx,
    );
    let about = template.as_ref().ok().map(|template| {
        div()
            .text_xs()
            .text_color(muted)
            .child(template.description.trim().to_string())
    });
    let agent = crate::unattended::run_agent(cx).map_or_else(
        || "no agent configured".to_string(),
        |name| format!("with {name}"),
    );
    let place = format!(
        "On a new branch, {}, in a worktree of its own, {agent}.",
        core::branch_for(tracker, &row.issue)
    );
    let base = findings
        .iter()
        .filter(|f| f.check == Check::Base)
        .map(|f| div().text_xs().text_color(muted).child(f.text.clone()));
    let said: Vec<&Finding> = findings.iter().filter(|f| f.check != Check::Base).collect();
    let lines: Vec<_> = said
        .iter()
        .enumerate()
        .map(|(at, finding)| finding_line(at, finding, danger, muted, handle))
        .collect();
    let column = div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(label("Workflow"))
        .child(div().h_flex().child(menu))
        .children(about)
        .child(label("Where it works"))
        .child(div().text_xs().text_color(muted).child(place))
        .children(base)
        .when(!lines.is_empty(), |col| {
            col.child(label("Before it starts")).children(lines)
        })
        .child(label("Instructions for this run"))
        .child(Textarea::new(instructions).h(gpui::rems(4.)));
    let Ok(template) = template else {
        return column.into_any_element();
    };
    let brief = core::brief_for(tracker, &row.issue, instructions.read(cx).value().as_ref());
    column
        .child(div().text_xs().text_color(muted).child(format!(
            "Times out after {} · {} misses allowed",
            template.timeout, template.misses
        )))
        .child(super::workflow_preview(
            template,
            &brief,
            preview,
            Shell::toggle_pick_preview,
            false,
            handle,
            cx,
        ))
        .into_any_element()
}

/// One thing the preflight found: in the danger ink when it blocks, muted
/// when it only says, with where it is changed, and *Show task* for an
/// earlier task worth retrying instead.
fn finding_line(
    at: usize,
    finding: &Finding,
    danger: gpui::Hsla,
    muted: gpui::Hsla,
    handle: &Entity<Shell>,
) -> impl IntoElement {
    let text = match finding.change {
        Some(change) => format!("{} Changed in {change}.", finding.text),
        None => finding.text.clone(),
    };
    let shell = handle.clone();
    div()
        .h_flex()
        .gap_2()
        .items_center()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(if finding.blocks { danger } else { muted })
                .child(text),
        )
        .children(finding.task.clone().map(|task| {
            crate::controls::action(("pick-show-task", at))
                .ghost()
                .small()
                .label("Show task")
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    shell.update(cx, |shell, cx| {
                        shell.cancel_pick(cx);
                        shell.show_task(&task, window, cx);
                    });
                })
        }))
}

/// A menu of the workflows an issue can be worked with, its trigger naming
/// `picked`. With `any` it starts with an entry for no workflow in particular,
/// picked when `picked` is `None`; `on_pick` hears the id, or `None` for it.
///
/// A workflow that works in the checkout is not offered: an issue's run is cut
/// a worktree of its own. One named that is no longer in the library shows its
/// id, so a choice gone stale is seen rather than shown as another.
pub(crate) fn issue_workflow_menu<F>(
    id: &'static str,
    picked: Option<&str>,
    any: Option<&'static str>,
    on_pick: F,
    cx: &App,
) -> impl IntoElement + use<F>
where
    F: Fn(Option<String>, &mut Window, &mut App) + 'static,
{
    let all = crate::unattended::issue_workflows(cx);
    let left_out = all.len().saturating_sub(crate::workflow::TEMPLATES_SHOWN);
    let shown: Vec<_> = all
        .iter()
        .take(crate::workflow::TEMPLATES_SHOWN)
        .cloned()
        .collect();
    let name = match picked {
        None => any.unwrap_or_default().to_string(),
        Some(picked) => all
            .iter()
            .find(|(id, _, _)| id == picked)
            .map_or_else(|| picked.to_string(), |(_, name, _)| name.clone()),
    };
    let picked = picked.map(str::to_string);
    let on_pick = std::rc::Rc::new(on_pick);
    crate::controls::menu_below(
        id,
        crate::controls::action((id, 0usize))
            .outline()
            .small()
            .label(name)
            .icon(Icon::new(IconName::ChevronDown)),
        move |mut menu, _, _| {
            if let Some(any) = any {
                let on_pick = on_pick.clone();
                menu = menu.item(
                    crate::controls::menu_item(any)
                        .checked(picked.is_none())
                        .on_click(move |_, window, cx: &mut App| on_pick(None, window, cx)),
                );
            }
            for (id, name, shipped) in &shown {
                let (on_pick, id) = (on_pick.clone(), id.clone());
                let label = match shipped {
                    true => format!("{name} (built in)"),
                    false => name.clone(),
                };
                menu = menu.item(
                    crate::controls::menu_item(label)
                        .checked(picked.as_ref() == Some(&id))
                        .on_click(move |_, window, cx: &mut App| {
                            on_pick(Some(id.clone()), window, cx)
                        }),
                );
            }
            if left_out > 0 {
                menu = menu.label(format!("{left_out} more workflows not shown"));
            }
            menu
        },
    )
}

/// One issue as a pressable row: how it is shown muted — the forge's number, or
/// *Draft* — then the title, a few labels as pills, and a muted word at the end
/// saying where it lives or whose it is.
///
/// One builder for every list of issues, so an issue reads the same in the
/// picker and on the workspace page.
pub(crate) fn issue_row(
    id: impl Into<gpui::ElementId>,
    shown: String,
    title: String,
    labels: &[String],
    trailing: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let (muted, radius) = (cx.theme().muted_foreground, cx.theme().radius);
    let (pill_bg, pill_fg) = (cx.theme().secondary, cx.theme().secondary_foreground);
    row_shell(id, shown, title, cx)
        // A few labels, not all: the row is for telling issues apart, and the
        // title is what does most of that.
        .children(labels.iter().take(3).map(|label| {
            div()
                .flex_none()
                .px_1()
                .rounded(radius)
                .text_xs()
                .bg(pill_bg)
                .text_color(pill_fg)
                .child(label.clone())
        }))
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(trailing),
        )
}

/// A row in the issue row's shape with something other than a number at its
/// head — a session's mark, a project's folder, a conversation's age — and no
/// labels. What the workspace page lists that is not an issue.
pub(crate) fn page_row(
    id: impl Into<gpui::ElementId>,
    lead: impl IntoElement,
    title: String,
    trailing: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let muted = cx.theme().muted_foreground;
    row_shell(id, lead, title, cx).child(
        div()
            .flex_none()
            .text_xs()
            .text_color(muted)
            .child(trailing),
    )
}

/// What both row shapes share: the pressable line, its muted head and its
/// truncated title.
fn row_shell(
    id: impl Into<gpui::ElementId>,
    lead: impl IntoElement,
    title: String,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let (muted, radius, hover) = (
        cx.theme().muted_foreground,
        cx.theme().radius,
        cx.theme().list_hover,
    );
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .px_2()
        .py_1()
        .rounded(radius)
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(div().flex_none().text_color(muted).child(lead))
        .child(div().flex_1().min_w_0().truncate().child(title))
}

/// The picker's body: a wait, a failure, an empty answer, or the rows.
fn issue_list(
    found: Option<&crate::shell::PickerAnswer>,
    chosen: Option<usize>,
    handle: &Entity<Shell>,
    cx: &App,
) -> AnyElement {
    let (muted, active) = (cx.theme().muted_foreground, cx.theme().list_active);
    let (rows, cut, unread) = match found {
        None => {
            return div()
                .text_color(muted)
                .child("Reading the open issues…")
                .into_any_element();
        }
        Some(Err(why)) => {
            return div()
                .text_color(crate::theme::status_ink(cx).warning)
                .child(format!("Nothing to pick: {why}"))
                .into_any_element();
        }
        Some(Ok((rows, _, _))) if rows.is_empty() => {
            return div()
                .text_color(muted)
                .child("There are no open issues.")
                .into_any_element();
        }
        Some(Ok((rows, cut, unread))) => (rows, *cut, unread.clone()),
    };
    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(
            div()
                .id("issue-list")
                .v_flex()
                .w_full()
                .max_h(gpui::rems(24.))
                .overflow_y_scroll()
                .children(rows.iter().enumerate().map(|(i, (tracker, row))| {
                    let handle = handle.clone();
                    // Who wrote it on a forge; an issue kept in onehand is the
                    // user's own, and what is worth saying is where it lives.
                    let trailing = match tracker {
                        onehand_core::unattended::Tracker::Local(_) => "in onehand".to_string(),
                        // Kept in step: the forge's number is already the
                        // row's head, so the end says which forge.
                        onehand_core::unattended::Tracker::Synced { forge, .. } => {
                            match row.issue.forge_ref() {
                                Some(_) => forge.name().to_string(),
                                None => "in onehand".to_string(),
                            }
                        }
                        onehand_core::unattended::Tracker::Forge(_) => {
                            format!("by {}", row.author)
                        }
                    };
                    issue_row(
                        ("issue", i),
                        tracker.shown(&row.issue),
                        row.issue.title_text().to_string(),
                        &row.labels,
                        trailing,
                        cx,
                    )
                    .when(chosen == Some(i), |row| row.bg(active))
                    .on_click(
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| shell.choose_issue(i, cx));
                        },
                    )
                })),
        )
        // The forge's half could not be read while the project's own could:
        // said, since its issues are missing from a list that looks whole.
        .when_some(unread, |list, why| {
            list.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::status_ink(cx).warning)
                    .child(format!("The forge's issues could not be read: {why}")),
            )
        })
        // Said, not hidden: a list cut silently reads as the whole of it.
        .when(cut, |list| {
            list.child(div().text_xs().text_color(muted).child(format!(
                "Showing the newest {}; close some to reach the rest.",
                onehand_core::unattended::ISSUES_SHOWN
            )))
        })
        .into_any_element()
}
