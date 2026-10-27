//! The form's *When*: the quick choices, each writing a cron expression, or
//! the expression typed; the expression it comes to and the next times it
//! names; and what a missed or an overlapping time comes to.

use super::SchedulesPage;
use super::draft::{DAYS, ScheduleDraft, WhenKind};
use super::form::edit;
use crate::controls::text_input;
use crate::settings::{about, field, section};
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::{ActiveTheme as _, StyledExt as _};
use onehand_core::schedule::{self, Missed, Overlap};
use onehand_ui::{ControlSize, Tall as _};

/// How many coming times the form shows.
const NEXT_SHOWN: usize = 3;

pub(super) fn when_section(
    handle: &Entity<SchedulesPage>,
    draft: &ScheduleDraft,
    cx: &App,
) -> AnyElement {
    // Every kind at one width across the form, as the two choices under it
    // are, so the section's rows end on one edge.
    let labels = WhenKind::ALL
        .iter()
        .map(|kind| SharedString::from(kind.label()))
        .collect::<Vec<_>>();
    let active = WhenKind::ALL
        .iter()
        .position(|kind| *kind == draft.when)
        .unwrap_or(0);
    let kinds = onehand_ui::switch(
        "schedule-when",
        &labels,
        active,
        ControlSize::Tall,
        {
            let handle = handle.clone();
            move |at: &usize, window, cx| {
                let kind = WhenKind::ALL[*at];
                handle.update(cx, |page, cx| {
                    if let Some(d) = page.draft.as_mut() {
                        // The expression field starts from what the quick
                        // choice said, not from the schedule as opened.
                        if kind == WhenKind::Expression
                            && let Ok(expr) = d.when_expr(cx)
                        {
                            d.expr
                                .update(cx, |state, cx| state.set_value(expr, window, cx));
                        }
                        d.when = kind;
                    }
                    cx.notify();
                })
            }
        },
        cx,
    );
    let input = |state| text_input(state, cx).tall();
    let fields: AnyElement = match draft.when {
        WhenKind::EveryHours => div()
            .h_flex()
            .gap_4()
            .child(field(
                "Every",
                about("Hours, 1 to 23"),
                input(&draft.hours),
                cx,
            ))
            .child(field(
                "At minute",
                about("0 to 59"),
                input(&draft.minute),
                cx,
            ))
            .into_any_element(),
        WhenKind::Daily | WhenKind::Weekdays => field(
            "At",
            about("HH:MM, in this machine's time"),
            input(&draft.time),
            cx,
        )
        .into_any_element(),
        WhenKind::Weekly => div()
            .h_flex()
            .items_end()
            .gap_4()
            .child(field("On", None, weekday_track(handle, draft, cx), cx))
            .child(field("At", about("HH:MM"), input(&draft.time), cx))
            .into_any_element(),
        WhenKind::Expression => field(
            "Expression",
            about("Five fields: minute, hour, day of month, month, day of week"),
            input(&draft.expr),
            cx,
        )
        .into_any_element(),
    };
    let ink = crate::theme::status_ink(cx);
    let meta = crate::theme::meta_ink(cx);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let said: AnyElement = match draft.when_expr(cx) {
        Err(why) => div()
            .text_xs()
            .text_color(ink.warning)
            .child(why)
            .into_any_element(),
        Ok(expr) => match schedule::parse(&expr) {
            Err(why) => div()
                .text_xs()
                .text_color(ink.warning)
                .child(match expr.trim().is_empty() {
                    true => "Say when it runs".to_string(),
                    false => format!("{expr}: {why}"),
                })
                .into_any_element(),
            Ok(_) => {
                let next: Vec<String> =
                    schedule::next_times(&expr, now, NEXT_SHOWN, &chrono::Local)
                        .into_iter()
                        .map(|t| schedule::next_said(t, now, &chrono::Local))
                        .collect();
                let next = match next.is_empty() {
                    true => "It names no time in the coming year".to_string(),
                    false => format!("Next: {}", next.join(" · ")),
                };
                div()
                    .v_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(meta)
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(expr),
                    )
                    .child(next)
                    .into_any_element()
            }
        },
    };

    let missed = onehand_ui::switch(
        "schedule-missed",
        &[
            "Run once when onehand opens".into(),
            "Skip to the next time".into(),
        ],
        usize::from(draft.missed == Missed::Skip),
        ControlSize::Tall,
        {
            let handle = handle.clone();
            move |at: &usize, window, cx| {
                let to = if *at == 0 {
                    Missed::RunOnce
                } else {
                    Missed::Skip
                };
                edit(&handle, move |d| d.missed = to)(window, cx)
            }
        },
        cx,
    );
    let overlap = onehand_ui::switch(
        "schedule-overlap",
        &["Skip this time".into(), "Queue behind it".into()],
        usize::from(draft.overlap == Overlap::Queue),
        ControlSize::Tall,
        {
            let handle = handle.clone();
            move |at: &usize, window, cx| {
                let to = if *at == 0 {
                    Overlap::Skip
                } else {
                    Overlap::Queue
                };
                edit(&handle, move |d| d.overlap = to)(window, cx)
            }
        },
        cx,
    );

    section(Some("When"), None, cx)
        .child(kinds)
        .child(fields)
        .child(said)
        .child(field(
            "When it was missed",
            about("A time that came while onehand was closed"),
            missed,
            cx,
        ))
        .child(field("When the last run is still going", None, overlap, cx))
        .into_any_element()
}

/// The day a weekly time falls on, one of seven, each at its own width:
/// split evenly over the half of the row it shares with *At*, a day's name
/// would not fit its share.
fn weekday_track(handle: &Entity<SchedulesPage>, draft: &ScheduleDraft, cx: &App) -> AnyElement {
    let days = DAYS.iter().enumerate().map(|(n, day)| {
        let handle = handle.clone();
        onehand_ui::choice(
            ("schedule-weekday", n),
            SharedString::from(&day[..3]),
            usize::from(draft.weekday) == n,
            ControlSize::Tall,
            cx,
        )
        .on_click(move |_, window: &mut Window, cx: &mut App| {
            let day = n as u8;
            edit(&handle, move |d| d.weekday = day)(window, cx)
        })
    });
    onehand_ui::choice_track(days, cx).into_any_element()
}
