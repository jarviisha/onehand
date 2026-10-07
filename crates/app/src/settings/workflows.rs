use super::{APP, field, list_row, page_head, section};
use crate::shell::Shell;
use crate::workflows_page::{click, form, row_action, row_delete, row_icon};
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, div};
use gpui_component::button::ButtonVariants;
use gpui_component::input::Input;
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::workflow as core;

/// The workflows page: the templates on offer, the form that edits one, and
/// each project's check command.
pub(super) fn workflows_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let entries = crate::workflow::templates(cx);
    let muted = cx.theme().muted_foreground;
    let warning = crate::theme::status_ink(cx).warning;

    let left_out = entries
        .len()
        .saturating_sub(crate::workflow::TEMPLATES_SHOWN);
    let rows = entries
        .iter()
        .take(crate::workflow::TEMPLATES_SHOWN)
        .enumerate()
        .map(|(i, entry)| {
            let shipped = entry.file.is_none();
            let about = match &entry.template {
                Ok(template) => div()
                    .text_xs()
                    .child(template.description.clone())
                    .into_any_element(),
                Err(why) => div()
                    .text_xs()
                    .text_color(warning)
                    .child(format!("Cannot be read: {why}"))
                    .into_any_element(),
            };
            let readable = entry.template.is_ok();
            list_row(
                entry.name(),
                Some(about),
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(shipped, |row| {
                        row.child(Tag::secondary().small().child("Built in"))
                    })
                    .when(readable, |row| {
                        row.child(row_action(
                            handle,
                            ("duplicate-workflow", i),
                            "Duplicate",
                            move |shell, window, cx| shell.duplicate_workflow(i, window, cx),
                        ))
                    })
                    .when(readable, |row| {
                        row.child(row_action(
                            handle,
                            ("export-workflow", i),
                            "Export…",
                            move |shell, window, cx| shell.export_workflow(i, window, cx),
                        ))
                    })
                    .when(readable && !shipped, |row| {
                        row.child(
                            row_icon(("edit-workflow", i), crate::icons::Icon::SquarePen, "Edit")
                                .on_click(click(handle, move |shell, window, cx| {
                                    shell.edit_workflow(i, window, cx)
                                })),
                        )
                    })
                    .when(!shipped, |row| {
                        row.child(row_delete(
                            handle,
                            ("delete-workflow", i),
                            move |shell, window, cx| shell.confirm_delete_workflow(i, window, cx),
                            cx,
                        ))
                    }),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();

    let list = section(None, None, cx)
        .gap_3()
        .children(rows)
        .when(left_out > 0, |list| {
            list.child(div().text_sm().text_color(muted).child(format!(
                "{left_out} more workflows not shown; remove some from {}",
                core::store::dir().display()
            )))
        })
        .when(entries.is_empty(), |list| {
            list.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Reading the workflows…"),
            )
        })
        .child(
            div()
                .h_flex()
                .gap_1()
                .child(
                    crate::controls::action("new-workflow")
                        .ghost()
                        .icon(Icon::new(IconName::Plus))
                        .label("New workflow")
                        .on_click(click(handle, |shell, window, cx| {
                            shell.new_workflow(window, cx)
                        })),
                )
                .child(
                    crate::controls::action("import-workflow")
                        .ghost()
                        .icon(Icon::new(IconName::FolderOpen))
                        .label("Import…")
                        .on_click(click(handle, |shell, window, cx| {
                            shell.import_workflow(window, cx)
                        })),
                ),
        );

    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(page_head(
            "Workflows",
            "The workflows a run starts from, shared by every workspace. The ones \
             onehand ships are read-only: duplicate one to change it.",
            APP,
            cx,
        ))
        .child(list)
        .children(shell.workflow_draft().map(|draft| form(handle, draft, cx)))
        .child(checks_section(handle, cx))
        .into_any_element()
}

/// How many projects' check command fields the page draws before it says how
/// many more there are.
const CHECKS_SHOWN: usize = 30;

/// Each project's check command: what a command step that names none runs.
fn checks_section(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let rows = handle.read(cx).check_inputs();
    let empty = rows.is_empty();
    let left_out = rows.len().saturating_sub(CHECKS_SHOWN);
    section(
        Some("Check commands"),
        Some(SharedString::from(
            "What a command step that names no command runs, per project of this workspace. \
             onehand runs it itself, so passing is never the agent's word.",
        )),
        cx,
    )
    .children(
        rows.into_iter()
            .take(CHECKS_SHOWN)
            .map(|(name, input)| field(name, None, Input::new(&input), cx).into_any_element()),
    )
    .when(left_out > 0, |group| {
        group.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{left_out} more projects not shown")),
        )
    })
    .when(empty, |group| {
        group.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("No projects in this workspace."),
        )
    })
    .into_any_element()
}
