//! The project page's *Schedules* box: the project's schedules, each leading
//! to the Schedules page narrowed to the project.

use super::super::page_parts::page_card;
use super::super::{ChatPane, ChatPaneEvent};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Div, InteractiveElement as _, ParentElement, StatefulInteractiveElement as _, Styled,
    div,
};
use gpui_component::{ActiveTheme, StyledExt as _};
use std::path::Path;

/// The project's schedules, each by its name and when it runs next, each
/// opening the Schedules page narrowed to the project; capped as the page is.
pub(super) fn schedules_box(root: &Path, cx: &mut Context<ChatPane>) -> Div {
    const SHOWN: usize = 5;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mine: Vec<_> = crate::schedules::listed(cx)
        .iter()
        .filter(|s| s.project == root)
        .cloned()
        .collect();
    let muted = cx.theme().muted_foreground;
    let left_out = mine.len().saturating_sub(SHOWN);
    let rows = mine.iter().take(SHOWN).enumerate().map(|(i, s)| {
        let root = root.to_path_buf();
        let next = match s.on {
            false => "off".to_string(),
            true => onehand_core::schedule::next_times(&s.when, now, 1, &chrono::Local)
                .first()
                .map_or_else(String::new, |t| {
                    onehand_core::schedule::next_said(*t, now, &chrono::Local)
                }),
        };
        onehand_ui::row(false, cx)
            .id(("project-schedule", i))
            .h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(div().min_w_0().truncate().child(s.name.clone()))
            .child(div().flex_none().text_xs().text_color(muted).child(next))
            .tooltip(onehand_ui::tip(|window, cx| {
                gpui_component::tooltip::Tooltip::new("Show it on the Schedules page")
                    .build(window, cx)
            }))
            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                cx.emit(ChatPaneEvent::ShowSchedules(Some(root.clone())));
            }))
    });
    let read = crate::schedules::read(cx);
    page_card("Schedules", Some(mine.len()), None, cx)
        .when(read.is_none(), |card| {
            card.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Reading the schedules…"),
            )
        })
        .when(read.as_ref().is_some_and(Result::is_err), |card| {
            card.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::status_ink(cx).warning)
                    .child("The schedules could not be read; the Schedules page says why"),
            )
        })
        .when(
            mine.is_empty() && read.is_some_and(|read| read.is_ok()),
            |card| card.child(div().text_xs().text_color(muted).child("No schedules here")),
        )
        .children(rows)
        .when(left_out > 0, |card| {
            card.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{left_out} more on the Schedules page")),
            )
        })
}
