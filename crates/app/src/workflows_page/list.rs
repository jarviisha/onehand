//! The Workflows page's list column: its head, cut down to the column, then
//! the person's own workflows and the shipped ones, each slot named, a row
//! each that opens its workflow beside the list.

use super::pick::{self, Pick};
use super::{WorkflowsPage, click};
use crate::controls::Quiet as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::workflow as core;
use onehand_plugin_host::Tall as _;

/// The list column's head: the page's name over how many there are, then
/// *New*, the page's one primary at rest, which steps down while a workflow
/// is open beside it, and a ⋯ holding *Import…*.
fn head(handle: &Entity<WorkflowsPage>, open: bool, cx: &App) -> AnyElement {
    let count = crate::workflow::templates(cx).len();
    let facts = match count {
        1 => "1 workflow".to_string(),
        n => format!("{n} workflows"),
    };
    let more = {
        let handle = handle.clone();
        crate::controls::menu_below(
            "workflows-more",
            crate::controls::action("workflows-more-trigger")
                .tall()
                .quiet_icon(false, cx)
                .icon(IconName::Ellipsis)
                .tooltip("More"),
            move |menu, _, _| {
                let handle = handle.clone();
                menu.item(crate::controls::menu_item("Import…").on_click(
                    move |_, window: &mut Window, cx: &mut App| {
                        handle.update(cx, |page, cx| page.import_workflow(window, cx))
                    },
                ))
            },
        )
    };
    crate::chat::pane::page_parts::page_head(
        "Workflows",
        Some(facts.into_any_element()),
        vec![
            crate::controls::action("new-workflow")
                .tall()
                .when(open, |button| button.secondary())
                .when(!open, |button| button.primary())
                .label("New")
                .tooltip("New workflow")
                .on_click(click(handle, |page, window, cx| {
                    page.new_workflow(window, cx)
                }))
                .into_any_element(),
            more.into_any_element(),
        ],
        cx,
    )
    .into_any_element()
}

/// The list column: the head, then the workflows on offer in their slots,
/// capped and saying what was left out. `draft_name` is the name typed into
/// a new workflow not saved yet, whose row leads the person's own.
pub(super) fn list_column(
    handle: &Entity<WorkflowsPage>,
    picked: Option<&Pick>,
    draft_name: Option<String>,
    focus: &[FocusHandle],
    cx: &App,
) -> AnyElement {
    let entries = crate::workflow::templates(cx);
    let (muted, ring) = (crate::theme::meta_ink(cx), cx.theme().ring);
    let warning = crate::theme::status_ink(cx).warning;
    let listed = pick::listed(&entries);
    let left_out = entries.len().saturating_sub(listed.len());

    let row = |i: usize, name: String, why: Option<String>, key: Option<Pick>| {
        let selected = key.is_some() && key.as_ref() == picked;
        onehand_plugin_host::row(selected, cx)
            .id(("workflow-row", i))
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(|row| row.border_color(ring))
            .v_flex()
            .justify_center()
            .py_1()
            .when_some(focus.get(i), |row, focus| row.track_focus(focus))
            .child(div().truncate().child(name))
            .children(why.map(|why| {
                div()
                    .text_xs()
                    .text_color(warning)
                    .truncate()
                    .child(format!("Cannot be read: {why}"))
            }))
            .when_some(key, |row, key| {
                row.on_click(click(handle, move |page, window, cx| {
                    page.pick(key.clone(), window, cx)
                }))
            })
            .into_any_element()
    };
    let slot = |label: &'static str| {
        div()
            .px_2()
            .pt_2()
            .pb_1()
            .text_xs()
            .text_color(muted)
            .child(label)
    };

    let mut own = Vec::new();
    if picked == Some(&Pick::New) {
        let name = draft_name.filter(|name| !name.trim().is_empty());
        own.push(row(
            usize::MAX,
            name.unwrap_or_else(|| "Untitled".to_string()),
            None,
            Some(Pick::New),
        ));
    }
    let mut shipped = Vec::new();
    for (place, at) in listed.iter().copied().enumerate() {
        let entry = &entries[at];
        let why = entry.template.as_ref().err().cloned();
        let drawn = row(place, entry.name(), why, pick::pick_of(entry));
        match entry.file.is_some() {
            true => own.push(drawn),
            false => shipped.push(drawn),
        }
    }

    let rows = crate::chat::pane::page_parts::card_box(cx)
        .w_full()
        .gap_0()
        .when(!own.is_empty(), |list| {
            list.child(slot("Yours")).children(own)
        })
        .when(!shipped.is_empty(), |list| {
            list.child(slot("Built in")).children(shipped)
        })
        .when(left_out > 0, |list| {
            list.child(
                div()
                    .px_2()
                    .pt_2()
                    .h_flex()
                    .flex_wrap()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!(
                        "{left_out} more workflows not shown; remove some from"
                    ))
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(core::store::dir().display().to_string()),
                    ),
            )
        })
        .when(entries.is_empty(), |list| {
            list.child(slot("Reading the workflows…"))
        });

    div()
        .v_flex()
        .gap_6()
        .child(head(handle, picked.is_some(), cx))
        .child(rows)
        .into_any_element()
}
