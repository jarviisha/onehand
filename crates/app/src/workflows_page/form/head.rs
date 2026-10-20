//! The editor's head: the workflow's name over what it is, *Run…*, and what
//! is done to the workflow itself; and what stands in for the editor when a
//! file of the person's own cannot be read.

use super::super::pick::{CardActs, Pick};
use super::super::{WorkflowDraft, WorkflowsPage, WorkflowsPageEvent, click};
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use gpui::{
    AnyElement, App, ElementId, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{IconName, StyledExt};
use onehand_plugin_host::Tall as _;
use std::path::{Path, PathBuf};

/// *Run…*: which project to start the workflow of id `workflow` on, the
/// rail's selection first, then the launcher. It refuses with `refuses`,
/// since a run starts from what is saved. `trigger` is the button it opens
/// under, and `id` the menu's.
pub(in crate::workflows_page) fn run_menu(
    handle: &Entity<WorkflowsPage>,
    workflow: &str,
    choices: &[(PathBuf, SharedString)],
    refuses: Option<String>,
    id: impl Into<ElementId>,
    trigger: Button,
) -> AnyElement {
    if let Some(why) = refuses {
        return trigger.refuses(Some(why)).into_any_element();
    }
    let (handle, choices, workflow) = (handle.clone(), choices.to_vec(), workflow.to_string());
    crate::controls::menu_below(id, trigger, move |mut menu, _, _| {
        menu = menu.label("Run on");
        for (root, label) in &choices {
            let (handle, root, workflow) = (handle.clone(), root.clone(), workflow.clone());
            menu = menu.item(crate::controls::menu_item(label.clone()).on_click(
                move |_, _: &mut Window, cx: &mut App| {
                    let (root, workflow) = (root.clone(), workflow.clone());
                    handle.update(cx, |_, cx| {
                        cx.emit(WorkflowsPageEvent::Run { workflow, root })
                    });
                },
            ));
        }
        if choices.is_empty() {
            menu = menu.label("Add a project first");
        }
        menu
    })
    .into_any_element()
}

/// A workflow's ⋯ under `trigger`: *Duplicate*, *Export…* and *Delete…*
/// on the workflow `pick` names, each where `acts` offers it.
pub(in crate::workflows_page) fn more_menu(
    handle: &Entity<WorkflowsPage>,
    pick: Pick,
    acts: CardActs,
    id: impl Into<ElementId>,
    trigger: Button,
    cx: &App,
) -> AnyElement {
    let handle = handle.clone();
    let danger = crate::theme::status_ink(cx).danger;
    crate::controls::menu_below(id, trigger, move |mut menu, _, _| {
        let act =
            |act: fn(&mut WorkflowsPage, &Pick, &mut Window, &mut gpui::Context<WorkflowsPage>)| {
                let (handle, pick) = (handle.clone(), pick.clone());
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                    handle.update(cx, |page, cx| act(page, &pick, window, cx))
                }
            };
        if acts.duplicate {
            menu = menu.item(
                crate::controls::menu_item("Duplicate").on_click(act(WorkflowsPage::duplicate)),
            );
        }
        if acts.export {
            menu = menu
                .item(crate::controls::menu_item("Export…").on_click(act(WorkflowsPage::export)));
        }
        if acts.delete {
            menu = menu.separator().item(
                crate::controls::menu_row(move |_, _| div().text_color(danger).child("Delete…"))
                    .on_click(act(WorkflowsPage::confirm_delete)),
            );
        }
        menu
    })
    .into_any_element()
}

/// The key the page opened `draft` by: its file, a shipped one's id, or the
/// new one being written.
fn pick_of_draft(draft: &WorkflowDraft) -> Pick {
    match (&draft.file, draft.shipped) {
        (Some(file), _) => Pick::Own(file.clone()),
        (None, true) => Pick::Shipped(draft.original.id.clone()),
        (None, false) => Pick::New,
    }
}

/// The editor's head: the name, a line of what it is, then *Run…* and the
/// ⋯; a shipped workflow's leads with *Duplicate to edit*, its primary.
pub(in crate::workflows_page) fn editor_head(
    handle: &Entity<WorkflowsPage>,
    draft: &WorkflowDraft,
    choices: &[(PathBuf, SharedString)],
    cx: &App,
) -> AnyElement {
    let name = draft.name.read(cx).value().trim().to_string();
    let title = if name.is_empty() {
        "Untitled".to_string()
    } else {
        name
    };
    let steps = match draft.steps.len() {
        1 => "1 step".to_string(),
        n => format!("{n} steps"),
    };
    let mut facts = format!("{steps} · {}", draft.place.label());
    if draft.shipped {
        facts.push_str(" · Built in, read-only: duplicate it to change it");
    }
    let refuses = (!draft.shipped && (draft.file.is_none() || draft.dirty(cx)))
        .then(|| "Save it first: a run starts from what is saved".to_string());
    let mut actions = vec![run_menu(
        handle,
        &draft.original.id,
        choices,
        refuses,
        "run-workflow",
        crate::controls::action("run-workflow-trigger")
            .tall()
            .secondary()
            .label("Run…"),
    )];
    if draft.shipped {
        actions.push(
            crate::controls::action("duplicate-to-edit")
                .tall()
                .primary()
                .label("Duplicate to edit")
                .on_click({
                    let pick = pick_of_draft(draft);
                    click(handle, move |page, window, cx| {
                        page.duplicate(&pick, window, cx)
                    })
                })
                .into_any_element(),
        );
    }
    actions.push(more_menu(
        handle,
        pick_of_draft(draft),
        CardActs {
            duplicate: !draft.shipped && draft.file.is_some(),
            export: draft.shipped || draft.file.is_some(),
            delete: draft.file.is_some(),
        },
        "workflow-more",
        crate::controls::action("workflow-more-trigger")
            .tall()
            .quiet_icon(false, cx)
            .icon(IconName::Ellipsis)
            .tooltip("More"),
        cx,
    ));
    crate::chat::pane::page_parts::page_head(title, Some(facts.into_any_element()), actions, cx)
        .into_any_element()
}

/// What stands in for the editor when a file of the person's own cannot be
/// read: its name, why, and *Delete…*, so a broken file can still go.
pub(in crate::workflows_page) fn unreadable(
    handle: &Entity<WorkflowsPage>,
    why: &str,
    file: &Path,
    cx: &App,
) -> AnyElement {
    let name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    div()
        .v_flex()
        .gap_6()
        .child(crate::chat::pane::page_parts::page_head(
            name,
            Some(
                div()
                    .text_color(crate::theme::status_ink(cx).warning)
                    .child(format!("Cannot be read: {why}"))
                    .into_any_element(),
            ),
            vec![
                crate::controls::action("delete-unreadable-workflow")
                    .tall()
                    .quiet(cx)
                    .text_color(crate::theme::status_ink(cx).danger)
                    .label("Delete…")
                    .on_click({
                        let pick = Pick::Own(file.to_path_buf());
                        click(handle, move |page, window, cx| {
                            page.confirm_delete(&pick, window, cx)
                        })
                    })
                    .into_any_element(),
            ],
            cx,
        ))
        .into_any_element()
}
