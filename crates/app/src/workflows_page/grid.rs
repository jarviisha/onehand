//! The Workflows page's grid: its head and search, then a card per workflow
//! in rows of as many as the page holds, the person's own under *Yours* and
//! the shipped under *Built in*. A card opens its workflow on a page of its
//! own; its *Run…* and ⋯ act from where it stands.

use super::form::{more_menu, run_menu};
use super::pick::{self, Pick};
use super::{WorkflowsPage, click};
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Entity, FocusHandle, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, Rems, SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::workflow::{self as core, StepKind};
use onehand_plugin_host::Tall as _;
use std::path::PathBuf;

/// The least a card takes before the row holds one fewer: as wide as a
/// project tile on the overview, room for a name and its two lines.
const CARD_MIN: Rems = rems(14.);

/// The most cards a row holds, however wide the page.
const MAX_COLUMNS: usize = 4;

/// The gap between cards, across and down (`gap_4`).
const CARD_GAP: Rems = rems(1.);

/// A step's mark on a card: a short bar, one per step, in its kind's ink.
const MARK_W: Rems = rems(1.);
const MARK_H: Rems = rems(0.375);

/// The widest the search stands in the head.
const SEARCH_MAX: Rems = rems(20.);

/// How many cards a row of a grid `width` wide holds: as many as fit at
/// [`CARD_MIN`] with [`CARD_GAP`] between them, from one to [`MAX_COLUMNS`].
pub(super) fn grid_columns(width: Rems) -> usize {
    let fit = ((width.0 + CARD_GAP.0) / (CARD_MIN.0 + CARD_GAP.0)).floor();
    (fit.max(1.) as usize).min(MAX_COLUMNS)
}

/// What a step's mark says of it: the agent's turns, onehand's own
/// commands, a person's approval, and what goes to the forge.
pub(super) enum Tone {
    Agent,
    Command,
    Approval,
    Forge,
}

/// The tone of a step of kind `kind`.
pub(super) fn tone(kind: &StepKind) -> Tone {
    match kind {
        StepKind::Agent { .. } => Tone::Agent,
        StepKind::Command { .. } => Tone::Command,
        StepKind::Approval { .. } => Tone::Approval,
        StepKind::Push => Tone::Forge,
        StepKind::PullRequest => Tone::Forge,
        StepKind::StatusChecks { .. } => Tone::Forge,
    }
}

/// The ink a mark of `tone` is drawn in.
fn tone_ink(tone: Tone, cx: &App) -> gpui::Hsla {
    match tone {
        Tone::Agent => cx.theme().primary,
        Tone::Command => cx.theme().muted_foreground,
        Tone::Approval => crate::theme::status_ink(cx).warning,
        Tone::Forge => crate::theme::status_ink(cx).success,
    }
}

/// The page's head: its name over how many there are, *New*, its one
/// primary, and a ⋯ holding *Import…*; then the search.
fn head(handle: &Entity<WorkflowsPage>, query: &Entity<InputState>, cx: &App) -> AnyElement {
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
    div()
        .v_flex()
        .gap_4()
        .child(crate::chat::pane::page_parts::page_head(
            "Workflows",
            Some(facts.into_any_element()),
            vec![
                crate::controls::action("new-workflow")
                    .tall()
                    .primary()
                    .label("New")
                    .tooltip("New workflow")
                    .on_click(click(handle, |page, window, cx| {
                        page.new_workflow(window, cx)
                    }))
                    .into_any_element(),
                more.into_any_element(),
            ],
            cx,
        ))
        .child(
            div()
                .w_full()
                .max_w(SEARCH_MAX)
                .child(crate::controls::search_input(query, cx).tall()),
        )
        .into_any_element()
}

/// One workflow's card, the `i`th the page shows: pressed on its body it
/// opens the workflow; its foot holds *Run…* and its ⋯. It never takes a
/// fill: hover raises its edge, and the keyboard's focus rings it.
fn card(
    handle: &Entity<WorkflowsPage>,
    i: usize,
    entry: &crate::workflow::Entry,
    focus: Option<&FocusHandle>,
    choices: &[(PathBuf, SharedString)],
    cx: &App,
) -> AnyElement {
    let (ring, raised) = (cx.theme().ring, cx.theme().muted_foreground);
    let muted = cx.theme().muted_foreground;
    let key = pick::pick_of(entry);
    let acts = pick::card_acts(entry);
    let more = more_menu(
        handle,
        key.clone().unwrap_or(Pick::New),
        acts,
        ("workflow-card-more", i),
        crate::controls::action(("workflow-card-more-trigger", i))
            .short()
            .quiet_icon(false, cx)
            .icon(IconName::Ellipsis)
            .tooltip("More"),
        cx,
    );
    let body = match &entry.template {
        Ok(template) => {
            let steps = match template.steps.len() {
                1 => "1 step".to_string(),
                n => format!("{n} steps"),
            };
            let marks = template.steps.iter().map(|step| {
                div()
                    .flex_none()
                    .w(MARK_W)
                    .h(MARK_H)
                    .rounded_full()
                    .bg(tone_ink(tone(&step.kind), cx))
            });
            let run = run_menu(
                handle,
                &template.id,
                choices,
                None,
                ("workflow-card-run", i),
                crate::controls::action(("workflow-card-run-trigger", i))
                    .short()
                    .secondary()
                    .label("Run…"),
            );
            div()
                .v_flex()
                .flex_1()
                .gap_2()
                .child(
                    div()
                        .truncate()
                        .font_semibold()
                        .child(template.name.clone()),
                )
                .when(!template.description.is_empty(), |body| {
                    body.child(
                        div()
                            .text_color(crate::theme::meta_ink(cx))
                            .line_clamp(2)
                            .child(template.description.clone()),
                    )
                })
                .when(!template.steps.is_empty(), |body| {
                    body.child(div().h_flex().gap_1().overflow_hidden().children(marks))
                })
                .child(
                    div()
                        .mt_auto()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{} · {steps}", template.place.label())),
                        )
                        .child(actions_slot(div().h_flex().gap_1().child(run).child(more))),
                )
        }
        Err(why) => div()
            .v_flex()
            .flex_1()
            .gap_2()
            .child(div().truncate().text_color(muted).child(entry.name()))
            .child(
                div()
                    .text_xs()
                    .text_color(crate::theme::status_ink(cx).warning)
                    .line_clamp(2)
                    .child(format!("Cannot be read: {why}")),
            )
            .child(
                div()
                    .mt_auto()
                    .h_flex()
                    .justify_end()
                    .when(acts.delete, |foot| foot.child(actions_slot(more))),
            ),
    };
    onehand_plugin_host::card_box(cx)
        .id(("workflow-card", i))
        .flex_1()
        .min_h_full()
        .when_some(focus, |card, focus| card.track_focus(focus))
        .focus_visible(|card| card.border_color(ring))
        .when_some(key, |card, key| {
            card.cursor_pointer()
                .hover(|card| card.border_color(raised))
                .on_click(click(handle, move |page, window, cx| {
                    page.pick(key.clone(), window, cx)
                }))
        })
        .child(body)
        .into_any_element()
}

/// What holds a card's own controls: a press on them stays theirs, never
/// reaching the card, which would open the workflow under the menu.
fn actions_slot(child: impl IntoElement) -> impl IntoElement {
    div()
        .flex_none()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(child)
}

/// A slot's cards in rows of `columns`, the last row's empty places held
/// so its cards keep the column's width.
fn rows(cards: Vec<AnyElement>, columns: usize) -> impl IntoElement {
    let mut grid = div().v_flex().gap_4().w_full();
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let mut row = div().h_flex().items_stretch().gap_4().w_full();
        for _ in 0..columns {
            row = match cards.next() {
                Some(card) => row.child(div().flex_1().min_w_0().h_flex().child(card)),
                None => row.child(div().flex_1()),
            };
        }
        grid = grid.child(row);
    }
    grid
}

/// The page while no workflow is open: the head and search, then the cards
/// answering the search in their slots, capped, the foot saying why the
/// grid is short. `width` is the grid's own, as measured.
pub(super) fn grid_page(
    handle: &Entity<WorkflowsPage>,
    query: &Entity<InputState>,
    focus: &[FocusHandle],
    choices: &[(PathBuf, SharedString)],
    width: Rems,
    cx: &App,
) -> AnyElement {
    let entries = crate::workflow::templates(cx);
    let text = query.read(cx).value().to_string();
    let listed = pick::listed(&entries, &text);
    let columns = grid_columns(width);
    let muted = crate::theme::meta_ink(cx);

    let (mut own, mut shipped) = (Vec::new(), Vec::new());
    for (place, at) in listed.shown.iter().copied().enumerate() {
        let entry = &entries[at];
        let drawn = card(handle, place, entry, focus.get(place), choices, cx);
        match entry.file.is_some() {
            true => own.push(drawn),
            false => shipped.push(drawn),
        }
    }
    let slot = |label: &'static str, cards: Vec<AnyElement>| {
        div()
            .v_flex()
            .gap_2()
            .child(div().text_xs().text_color(muted).child(label))
            .child(rows(cards, columns))
    };
    let left_out = listed.matched.saturating_sub(listed.shown.len());
    let foot = if entries.is_empty() {
        Some("Reading the workflows…".to_string())
    } else if listed.matched == 0 {
        Some("No workflows match".to_string())
    } else if !text.trim().is_empty() {
        Some(format!(
            "Showing {} of {} workflows",
            listed.shown.len(),
            entries.len()
        ))
    } else {
        None
    };
    div()
        .v_flex()
        .gap_6()
        .w_full()
        .child(head(handle, query, cx))
        .when(!own.is_empty(), |page| page.child(slot("Yours", own)))
        .when(!shipped.is_empty(), |page| {
            page.child(slot("Built in", shipped))
        })
        .when(left_out > 0, |page| {
            page.child(
                div()
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
        .children(foot.map(|foot| div().text_xs().text_color(muted).child(foot)))
        .into_any_element()
}

#[cfg(test)]
mod tests;
