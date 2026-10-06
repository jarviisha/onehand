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
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

/// A project's open issues, to pick one to work now.
///
/// **No trigger**, for the rename's reason: it is opened from a menu entry that
/// is gone by the time the list arrives, so the shell decides whether it exists.
///
/// **Who opened each issue is on its row.** The issue's body goes into the
/// agent's prompt word for word, and the agent works with the user's
/// credentials; the automatic search takes only the user's own issues for that
/// reason, and a person picking by hand from everybody's is shown whose text
/// they are about to hand over.
pub fn pick_issue(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let Some(picker) = shell.issue_picker() else {
        return Dialog::new(cx);
    };
    let heading = match picker.only {
        Some(number) => format!("Run a workflow on issue {number} in {}", picker.project),
        None => format!("Open issues in {}", picker.project),
    };
    let found = picker.found.clone();
    let chosen = picker.workflow.clone();
    let handle = cx.entity();
    Dialog::new(cx)
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            let shell = handle.clone();
            let menu = issue_workflow_menu(
                "pick-workflow",
                chosen.as_deref(),
                Some("By the issue's labels"),
                move |id, _, cx| shell.update(cx, |shell, cx| shell.pick_issue_workflow(id, cx)),
                cx,
            );
            content
                .child(title_row(heading.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            "Picking one claims it where it lives, starts an agent on it in \
                             a worktree of its own with the workflow below, and shows you the \
                             session.",
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().text_sm().child("Workflow"))
                        .child(menu),
                )
                .child(issue_list(found.as_deref(), &handle, cx))
        })
        .footer(
            div().h_flex().justify_end().w_full().child(
                crate::controls::action("cancel-pick")
                    .ghost()
                    .label("Cancel")
                    .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                        shell.cancel_pick(cx);
                    })),
            ),
        )
        // Esc and the close button both put the list away, and both have to
        // clear what is putting it on screen, or it renders straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_pick(cx);
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
    handle: &Entity<Shell>,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
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
                    .on_click(
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| shell.pick_issue(i, window, cx));
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
