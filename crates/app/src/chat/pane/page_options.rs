//! The page's chips for what its agent offers: the model, the effort, the
//! fast switch and the mode, read from what a session of that agent last
//! said. What is picked here is set on the session before its first prompt.

use super::ChatPane;
use gpui::{AnyElement, App, Context, IntoElement, ParentElement, SharedString, Styled, div};
use gpui_component::ActiveTheme;
use gpui_component::button::ButtonVariants as _;
use onehand_core::acp::Role;
use onehand_core::chat::MODE_SELECTOR;
use onehand_core::config::AgentFacts;
use onehand_ui::ShortIcon as _;
use std::path::Path;

/// A group's choices as a chip draws them: its id, the label over its menu,
/// each value with its label, and the one in force.
struct Group {
    id: String,
    name: String,
    choices: Vec<(String, String)>,
    current: Option<String>,
}

impl ChatPane {
    /// The chips for `agent`'s offer on the page of `root`: one muted line
    /// while nothing is known of it yet.
    pub(super) fn page_options(
        &self,
        root: &Path,
        agent: &str,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(facts) = crate::state::Shared::global(cx)
            .settings
            .facts_of(agent)
            .cloned()
        else {
            return vec![
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Agent's default")
                    .into_any_element(),
            ];
        };
        let picks = self.page_picks(root);
        let picked = |id: &str| picks.iter().find(|(g, _)| g == id).map(|(_, v)| v.clone());
        let mut chips = Vec::new();
        for option in &facts.options {
            let current = picked(&option.id).or_else(|| option.current.clone());
            match option.role() {
                Some(Role::Fast) => {
                    let forced = onehand_core::acp::ConfigOption {
                        current: current.clone(),
                        ..option.clone()
                    };
                    if let Some(switch) = forced.switch() {
                        chips.push(self.fast_chip(root, &option.id, switch, cx));
                    }
                }
                Some(Role::Model | Role::Effort) => {
                    let group = Group {
                        id: option.id.clone(),
                        name: option.name.clone(),
                        choices: option
                            .choices
                            .iter()
                            .map(|c| (c.value.clone(), c.name.clone()))
                            .collect(),
                        current,
                    };
                    chips.push(self.group_chip(root, group, cx));
                }
                None => {}
            }
        }
        if let Some(mode) = mode_group(&facts, picked(MODE_SELECTOR)) {
            chips.push(self.group_chip(root, mode, cx));
        }
        chips
    }

    /// What the chips for `agent` on `root`'s page show, as picks: each
    /// group's value picked here, else the one the agent last had in force,
    /// where it is one of its choices. The model first, since what else is
    /// offered may follow it.
    pub(super) fn page_shown_picks(
        &self,
        root: &Path,
        agent: &str,
        cx: &App,
    ) -> Vec<(String, String)> {
        let Some(facts) = crate::state::Shared::global(cx).settings.facts_of(agent) else {
            return Vec::new();
        };
        let picks = self.page_picks(root);
        let picked = |id: &str| picks.iter().find(|(g, _)| g == id).map(|(_, v)| v.clone());
        let mut shown: Vec<(String, String)> = Vec::new();
        for option in &facts.options {
            let role = option.role();
            if role.is_none() {
                continue;
            }
            let value = picked(&option.id).or_else(|| option.current.clone());
            let offered = value.filter(|value| option.choices.iter().any(|c| &c.value == value));
            if let Some(value) = offered {
                let pick = (option.id.clone(), value);
                match role {
                    Some(Role::Model) => shown.insert(0, pick),
                    Some(Role::Effort | Role::Fast) | None => shown.push(pick),
                }
            }
        }
        let mode = picked(MODE_SELECTOR)
            .or_else(|| facts.current_mode.clone())
            .filter(|mode| facts.modes.iter().any(|m| &m.id == mode));
        if let Some(mode) = mode {
            shown.push((MODE_SELECTOR.to_string(), mode));
        }
        shown
    }

    /// A picker over one group's choices.
    fn group_chip(&self, root: &Path, group: Group, cx: &mut Context<Self>) -> AnyElement {
        let label = group
            .current
            .as_ref()
            .and_then(|value| group.choices.iter().find(|(v, _)| v == value))
            .map_or_else(|| group.name.clone(), |(_, label)| label.clone());
        let pane = cx.entity().downgrade();
        let at = root.to_path_buf();
        let id = SharedString::from(format!("page-group-{}", group.id));
        let trigger = SharedString::from(format!("page-group-{}-trigger", group.id));
        crate::controls::menu_below(
            id,
            onehand_ui::picker(trigger, label, false, onehand_ui::ControlSize::Short, cx),
            move |mut menu, _, _| {
                menu = menu.item(crate::controls::menu_label(group.name.clone()));
                for (value, label) in &group.choices {
                    let (pane, at, gid, value) =
                        (pane.clone(), at.clone(), group.id.clone(), value.clone());
                    menu = menu.item(
                        crate::controls::menu_item(label.clone())
                            .checked(group.current.as_ref() == Some(&value))
                            .on_click(move |_, _, cx: &mut App| {
                                let _ = pane.update(cx, |pane: &mut Self, cx| {
                                    pane.pick_on_page(&at, &gid, value.clone(), cx);
                                });
                            }),
                    );
                }
                menu
            },
        )
        .into_any_element()
    }

    /// The fast switch: its bolt, lit while on, a press flipping it.
    fn fast_chip(
        &self,
        root: &Path,
        id: &str,
        switch: onehand_core::acp::Switch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (pane, at, gid) = (cx.entity().downgrade(), root.to_path_buf(), id.to_string());
        crate::controls::action("page-fast")
            .short_icon()
            .ghost()
            .icon(crate::chat::composer::bolt(switch.on, cx))
            .tooltip("Fast")
            .on_click(move |_, _, cx: &mut App| {
                let _ = pane.update(cx, |pane: &mut Self, cx| {
                    pane.pick_on_page(&at, &gid, switch.flip_to.clone(), cx);
                });
            })
            .into_any_element()
    }
}

/// The mode as a group, the one picked or else the one in force.
fn mode_group(facts: &AgentFacts, picked: Option<String>) -> Option<Group> {
    (!facts.modes.is_empty()).then(|| Group {
        id: MODE_SELECTOR.to_string(),
        name: "Mode".to_string(),
        choices: facts
            .modes
            .iter()
            .map(|m| (m.id.clone(), m.name.clone()))
            .collect(),
        current: picked.or_else(|| facts.current_mode.clone()),
    })
}
