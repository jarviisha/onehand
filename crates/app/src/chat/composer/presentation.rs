//! Derived, render-ready composer data.
//!
//! The protocol remains the source of truth. This module gives the view stable
//! selector keys and human labels without making the card renderer know how
//! modes and agent config groups are stored separately.

use super::super::session::ChatSession;
use gpui::{App, Entity, ParentElement, SharedString, Styled, div};
use gpui_component::{Icon, IconName, StyledExt};
use onehand_core::chat::SubmitBlock;

pub(super) struct Row {
    pub(super) label: SharedString,
    pub(super) detail: Option<SharedString>,
    pub(super) checked: bool,
    /// What taking this row does, or `None` where it is a heading naming the
    /// group under it.
    ///
    /// An `Option` rather than a variant of [`Pick`], because the difference is
    /// structural and not another kind of picking: a heading is drawn as plain
    /// text instead of a button, the keys step over it, and Enter on one settles
    /// nothing. Carried as a variant, each of those three would have been a
    /// match arm doing nothing, and the fourth place that forgot to write one
    /// would have been a heading the user could click.
    pub(super) pick: Option<Pick>,
}

#[derive(Clone)]
pub(super) enum Pick {
    Complete,
    Mode(String),
    Config { config_id: String, value: String },
}

pub(super) fn mode_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let chat = &session.read(cx).chat;
    (!chat.modes.is_empty()).then(|| {
        let current = chat
            .current_mode
            .as_ref()
            .and_then(|id| chat.modes.iter().find(|mode| &mode.id == id))
            .map(|mode| mode.name.as_str())
            .unwrap_or("Not set");
        SharedString::from(format!("Mode · {current}"))
    })
}

pub(super) fn options_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let options = &session.read(cx).chat.config_options;
    if options.is_empty() {
        return None;
    }
    let model = options.iter().find(|option| {
        option.id.eq_ignore_ascii_case("model") || option.name.eq_ignore_ascii_case("model")
    });
    Some(SharedString::from(match model {
        Some(model) => format!(
            "Model · {}",
            model
                .current
                .as_ref()
                .and_then(|value| model.choices.iter().find(|choice| &choice.value == value))
                .map(|choice| choice.name.as_str())
                .unwrap_or("Not set")
        ),
        None => "Model".to_string(),
    }))
}

pub(super) fn mode_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    let chat = &session.read(cx).chat;
    chat.modes
        .iter()
        .map(|mode| Row {
            label: SharedString::from(mode.name.clone()),
            detail: None,
            checked: chat.current_mode.as_ref() == Some(&mode.id),
            pick: Some(Pick::Mode(mode.id.clone())),
        })
        .collect()
}

pub(super) fn options_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    let chat = &session.read(cx).chat;
    let mut options: Vec<_> = chat.config_options.iter().enumerate().collect();
    // The trigger promises Model, then the popup expands the scope to Effort
    // and the remaining options. Preserve agent order within the last group.
    options.sort_by_key(|(index, option)| config_rank(&option.id, &option.name, *index));
    options
        .into_iter()
        .map(|(_, option)| option)
        .flat_map(|option| {
            // **Each group is announced once, and its choices then stand
            // alone.** This list runs across every group the agent advertises —
            // model, effort, and whatever else it offers — so a row has to be
            // placeable in one of them. Naming the group on every row put the
            // same word in the position the eye lands on, several rows running,
            // ahead of the thing actually being picked; naming it once above
            // them costs a row per group and says it in the place a reader
            // already looks for it.
            //
            // It is also what makes several rows marked in force at once read
            // correctly. One per group is the truth in this list, but the mark
            // is weight and primary ink — a convention built for the mode
            // picker, which is a list of a single choice — so three rows
            // carrying it in one undivided column read as three answers to one
            // question.
            let heading = Row {
                label: SharedString::from(option.name.clone()),
                detail: None,
                checked: false,
                pick: None,
            };
            std::iter::once(heading).chain(option.choices.iter().map(move |choice| Row {
                label: SharedString::from(choice.name.clone()),
                detail: None,
                checked: option.current.as_ref() == Some(&choice.value),
                pick: Some(Pick::Config {
                    config_id: option.id.clone(),
                    value: choice.value.clone(),
                }),
            }))
        })
        .collect()
}

fn config_rank(id: &str, name: &str, index: usize) -> (u8, usize) {
    let rank = if id.eq_ignore_ascii_case("model") || name.eq_ignore_ascii_case("model") {
        0
    } else if id.eq_ignore_ascii_case("effort") || name.eq_ignore_ascii_case("effort") {
        1
    } else {
        2
    };
    (rank, index)
}

pub(super) fn composer_status(blocked: Option<SubmitBlock>, cx: &App) -> Option<gpui::Div> {
    let reason = match blocked? {
        SubmitBlock::UnreadableAttachment(name) => {
            format!("{name} could not be read — remove it before sending")
        }
        SubmitBlock::NotConnected => "Agent disconnected — waiting to reconnect".to_string(),
        SubmitBlock::Empty | SubmitBlock::Busy => return None,
    };
    Some(
        div()
            .h_flex()
            .gap_1()
            .text_xs()
            .text_color(crate::theme::status_ink(cx).danger)
            .child(Icon::new(IconName::Info).size_3())
            .child(reason),
    )
}

#[cfg(test)]
mod tests {
    use super::config_rank;

    #[test]
    fn model_then_effort_lead_the_combined_popup() {
        let mut groups = [
            config_rank("agent", "Sub-agent", 0),
            config_rank("effort", "Effort", 1),
            config_rank("model", "Model", 2),
        ];
        groups.sort();
        assert_eq!(groups, [(0, 2), (1, 1), (2, 0)]);
    }
}
