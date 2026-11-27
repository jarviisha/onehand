//! The page's chips for what its agent offers: the model, the effort, the
//! fast switch and the mode, read from what a session of that agent last
//! said. What is picked here is set on the session before its first prompt.

use super::ChatPane;
use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    Styled, div,
};
use gpui_component::ActiveTheme;
use gpui_component::button::ButtonVariants as _;
use onehand_core::acp::Role;
use onehand_core::chat::Cards;
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
            // In the room the agent's own name leaves, and gone when that
            // cannot hold it whole.
            return vec![
                super::project_page::facts::facts(
                    div()
                        .debug_selector(|| "page-agent-default".into())
                        .flex_none()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Agent's default")
                        .into_any_element(),
                    div().into_any_element(),
                    false,
                )
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
        super::page_draft::page_menu(
            id,
            onehand_ui::picker(
                trigger,
                label.clone(),
                false,
                onehand_ui::ControlSize::Short,
                cx,
            ),
            label,
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

impl ChatPane {
    /// The level a session sent from `root`'s page answers its cards at: the
    /// project's default; *Manual* on a host's page, whose host answers the
    /// folder's cards itself (its chip is the host's switch).
    pub(super) fn page_cards(&self, root: &Path, cx: &App) -> onehand_core::chat::Cards {
        let here = self
            .empty
            .as_ref()
            .is_some_and(|project| project.host.is_none());
        match here {
            true => crate::projects::of(root, cx)
                .map(|project| project.cards)
                .unwrap_or_default(),
            false => onehand_core::chat::Cards::Manual,
        }
    }

    /// The page's *Cards* chip and what the level costs: on this machine,
    /// the project's default; on a host, the host's own answering of the
    /// folder's cards, once the host has said how it stands.
    pub(super) fn page_cards_chip(
        &self,
        root: &Path,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Option<String>)> {
        let project = self.empty.as_ref()?;
        let (level, choices, heading, cost) = match &project.host {
            None => {
                let level = self.page_cards(root, cx);
                let cost = level.cost().map(str::to_string);
                (
                    level,
                    &Cards::ALL[..],
                    "Cards, this project's default".to_string(),
                    cost,
                )
            }
            Some(host) => {
                let level = host_cards(project.bypass)?;
                let cost = (level == Cards::Bypass).then(|| host_cost(host));
                (
                    level,
                    &Cards::ON_A_HOST[..],
                    format!("Cards, this folder on {host}"),
                    cost,
                )
            }
        };
        let on_host = project.host.is_some();
        let at = root.to_path_buf();
        let pane = cx.entity().downgrade();
        let choices = choices.to_vec();
        let chip = super::page_draft::page_menu(
            "page-cards",
            onehand_ui::picker(
                "page-cards-trigger",
                format!("Cards: {}", level.label()),
                false,
                onehand_ui::ControlSize::Short,
                cx,
            ),
            format!("Cards: {}", level.label()),
            move |mut menu, _, _| {
                menu = menu.item(crate::controls::menu_label(heading.clone()));
                for choice in &choices {
                    let (at, pane, choice) = (at.clone(), pane.clone(), *choice);
                    menu = menu.item(
                        crate::controls::menu_item(format!(
                            "{} · {}",
                            choice.label(),
                            choice.about()
                        ))
                        .checked(choice == level)
                        .on_click(move |_, window, cx: &mut App| match on_host {
                            // The host keeps the folder's switch.
                            true => {
                                let _ = pane.update(cx, |pane: &mut Self, cx| {
                                    pane.set_host_bypass(choice == Cards::Bypass, window, cx)
                                });
                            }
                            false => crate::projects::set_cards(&at, choice, cx),
                        }),
                    );
                }
                menu
            },
        )
        // At the strip's end, on whichever line it stands when the strip
        // wraps.
        .trigger_style(gpui::StyleRefinement::default().min_w_0().ml_auto())
        .into_any_element();
        Some((chip, cost))
    }
}

/// The host's switch for a folder as a level; none until the host has said.
pub(super) fn host_cards(bypass: Option<bool>) -> Option<Cards> {
    bypass.map(Cards::of_host_switch)
}

/// What the host answering a folder's cards costs.
pub(super) fn host_cost(host: &onehand_core::workspace::HostName) -> String {
    format!(
        "The host answers every card itself: an agent can do anything the host's user can on \
         {host}, git push and deleting files included"
    )
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
