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
    let kinds = WhenKind::ALL.iter().enumerate().map(|(n, kind)| {
        let kind = *kind;
        onehand_ui::choice(
            ("schedule-when", n),
            kind.label(),
            draft.when == kind,
            ControlSize::Tall,
            cx,
        )
        .on_click({
            let handle = handle.clone();
            move |_, window: &mut Window, cx: &mut App| {
                edit(&handle, move |d| d.when = kind)(window, cx)
            }
        })
    });
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
                .child(format!("{expr}: {why}"))
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
        .child(onehand_ui::choice_track(kinds, cx))
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

/// The day a weekly time falls on, one of seven.
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
