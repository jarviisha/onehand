//! A setting's row of choices: what an agent offers for one of its
//! settings (a mode, a model, an effort), the one taken filled, with a first
//! choice for leaving it to whatever runs. A workflow's agent step and a
//! schedule pick from it alike.

use crate::controls::Refuses as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, IntoElement, ParentElement, SharedString, Styled, Window, div};
use gpui_component::StyledExt as _;
use std::rc::Rc;

/// The most of an agent's values a setting's row draws before it counts
/// the rest.
const CHOICES_SHOWN: usize = 12;

/// What picking a choice does: `None` for the first choice, leaving the
/// setting to what runs.
pub(crate) type SettingPick = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

/// A setting's row of choices: `run` (the step names none) first, then the
/// values offered, the one taken filled; a value taken that the agent does
/// not offer is marked: in the warning ink when `strict` (what was learned
/// decides it), muted otherwise (the run's check at the step does).
/// `offered` is `None` while what the agent offers is not known, and nothing
/// is marked.
/// In one edged track that wraps on a narrow panel; past [`CHOICES_SHOWN`]
/// a line says how many more.
#[allow(clippy::too_many_arguments)]
pub(crate) fn setting_row(
    id: String,
    run: &'static str,
    offered: Option<&[String]>,
    current: Option<&str>,
    strict: bool,
    refused: Option<&'static str>,
    pick: SettingPick,
    cx: &App,
) -> AnyElement {
    let known = offered.is_some();
    let (shown, more) = chips(offered.unwrap_or_default(), current);
    let warning = crate::theme::status_ink(cx).warning;
    let chip = |n: usize, label: SharedString, to: Option<String>, on: bool| {
        let pick = pick.clone();
        onehand_ui::choice(
            SharedString::from(format!("{id}-{n}")),
            label,
            on,
            // A page form's pickers stand at its inputs' height.
            onehand_ui::ControlSize::Tall,
            cx,
        )
        .refuses(refused)
        .on_click(move |_, window: &mut Window, cx: &mut App| pick(to.clone(), window, cx))
    };
    let chips = shown.into_iter().enumerate().map(|(n, (value, offered))| {
        let on = current == Some(value.as_str());
        match offered || !known {
            true => chip(n + 1, value.clone().into(), Some(value), on),
            false if strict => chip(
                n + 1,
                format!("{value} · not offered").into(),
                Some(value),
                on,
            )
            .text_color(warning)
            // A refusal keeps its own tooltip.
            .when(refused.is_none(), |chip| {
                chip.tooltip("The agent does not offer it: a run fails at this step")
            }),
            // An option's values were learned under whatever model ran
            // then: one missing may be offered under the step's.
            false => chip(n + 1, format!("{value} · not seen").into(), Some(value), on)
                .text_color(crate::theme::meta_ink(cx))
                .when(refused.is_none(), |chip| {
                    chip.tooltip(
                        "Not among what the agent offered under the model it last ran; \
                     the run checks it when the step starts",
                    )
                }),
        }
    });
    div()
        .v_flex()
        .gap_1()
        .child(onehand_ui::choice_track(
            std::iter::once(chip(0, run.into(), None, current.is_none())).chain(chips),
            cx,
        ))
        .when(more > 0, |col| {
            col.child(
                div()
                    .text_xs()
                    .text_color(crate::theme::meta_ink(cx))
                    .child(format!("{more} more")),
            )
        })
        .into_any_element()
}

/// The values a row draws after its first choice, each with whether the
/// agent offers it: the first [`CHOICES_SHOWN`] offered, the one taken
/// even past them, and a value taken that is not offered last; and how many
/// offered values are left out.
fn chips(offered: &[String], current: Option<&str>) -> (Vec<(String, bool)>, usize) {
    let mut shown: Vec<(String, bool)> = offered
        .iter()
        .take(CHOICES_SHOWN)
        .map(|value| (value.clone(), true))
        .collect();
    if let Some(current) = current.filter(|current| !shown.iter().any(|(v, _)| v == current)) {
        shown.push((current.to_string(), offered.iter().any(|v| v == current)));
    }
    let drawn = shown.iter().filter(|(_, known)| *known).count();
    (shown, offered.len() - drawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(n: usize) -> Vec<String> {
        (0..n).map(|v| format!("v{v}")).collect()
    }

    #[test]
    fn chips_show_what_is_offered() {
        let (shown, more) = chips(&values(2), Some("v1"));
        assert_eq!(
            shown,
            vec![("v0".to_string(), true), ("v1".to_string(), true)]
        );
        assert_eq!(more, 0);
    }

    #[test]
    fn a_value_not_offered_stays_marked() {
        let (shown, more) = chips(&values(1), Some("opus"));
        assert_eq!(
            shown,
            vec![("v0".to_string(), true), ("opus".to_string(), false)]
        );
        assert_eq!(more, 0);
        assert_eq!(
            chips(&[], Some("plan")).0,
            vec![("plan".to_string(), false)]
        );
    }

    #[test]
    fn chips_are_capped_and_the_one_taken_kept() {
        let offered = values(CHOICES_SHOWN + 3);
        let (shown, more) = chips(&offered, None);
        assert_eq!((shown.len(), more), (CHOICES_SHOWN, 3));
        let last = offered.last().unwrap();
        let (shown, more) = chips(&offered, Some(last));
        assert_eq!(shown.last(), Some(&(last.clone(), true)));
        assert_eq!((shown.len(), more), (CHOICES_SHOWN + 1, 2));
    }
}
