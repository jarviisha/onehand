//! The window's frame around the rail and the dock: the title bar, holding the
//! app's menus, and the status bar along the foot.
//!
//! **The menus hold only the keymap's commands.** Each item is a command the
//! window already answers by a key, named for the menu and dispatched as that
//! command's action, so a menu never offers something no key reaches and the
//! key it shows is the one the keymap binds, overrides included.

use super::{Shell, ToggleRail, ToggleTerminal, ToggleWorkbenchVisibility};
use crate::controls::{Named as _, Quiet as _, action};
use crate::keymap::{CLOSE_SESSION, COMMANDS, NEW_SESSION, RESTART};
use gpui::prelude::FluentBuilder as _;
use gpui::{App, Entity, IntoElement, Menu, MenuItem, ParentElement as _, Styled as _, div};
use gpui_component::dock::DockPlacement;
use gpui_component::menu::AppMenuBar;
use gpui_component::status_bar::StatusBar;
use gpui_component::{
    ActiveTheme as _, GlobalState, Icon, IconName, Sizable as _, TitleBar, h_flex,
};

/// A menu item: a command's keymap id and the name it takes in the menu, or
/// `None` for a separator.
type Item = Option<(&'static str, &'static str)>;

/// Each menu, and the commands it holds.
const MENUS: &[(&str, &[Item])] = &[
    (
        "File",
        &[
            Some((NEW_SESSION, "New session")),
            Some(("run_workflow", "Run a workflow on this project…")),
            None,
            Some(("save", "Save the open file")),
            None,
            Some(("settings", "Open Settings")),
        ],
    ),
    (
        "View",
        &[
            Some(("toggle_rail", "Show or hide the rail")),
            Some(("toggle_workbench", "Show or hide the Workbench")),
            Some(("terminal", "Show or hide the terminal")),
            None,
            Some(("editor", "Open Editor")),
            Some(("markdown", "Open Markdown")),
            Some(("neovim", "Open Neovim")),
            None,
            Some(("maximize", "Maximize or restore the focused panel")),
            Some(("zoom_in", "Zoom the focused panel in")),
            Some(("zoom_out", "Zoom the focused panel out")),
            Some(("zoom_reset", "Reset the focused panel's zoom")),
        ],
    ),
    (
        "Go",
        &[
            Some(("focus_rail_search", "Search sessions and projects")),
            Some(("composer", "Go to the composer")),
            None,
            Some(("show_tasks", "Tasks")),
            Some(("show_schedules", "Schedules")),
            None,
            Some(("next_session", "Next session")),
            Some(("previous_session", "Previous session")),
            Some(("rail_next_session", "Next session in the rail")),
            Some(("rail_previous_session", "Previous session in the rail")),
        ],
    ),
    (
        "Session",
        &[
            Some(("show_workers", "Show the workers")),
            None,
            Some((RESTART, "Restart the agent")),
            Some((CLOSE_SESSION, "Close the session")),
        ],
    ),
];

fn menus() -> Vec<Menu> {
    MENUS
        .iter()
        .map(|(name, items)| Menu {
            name: (*name).into(),
            items: items
                .iter()
                .map(|item| match item {
                    None => MenuItem::Separator,
                    Some((id, label)) => {
                        let command = COMMANDS
                            .iter()
                            .find(|command| command.id == *id)
                            .expect("a menu names a command the keymap holds");
                        MenuItem::Action {
                            name: (*label).into(),
                            action: command.action(),
                            os_action: None,
                            checked: false,
                            disabled: false,
                        }
                    }
                })
                .collect(),
            disabled: false,
        })
        .collect()
}

/// Give the app its menus, once, before the first window: the platform's menu
/// bar where there is one, and the library's, which each window draws in its
/// title bar.
pub(super) fn install_menus(cx: &mut App) {
    cx.set_menus(menus());
    let owned = menus().into_iter().map(|menu| menu.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
}

/// The menu bar a window draws in its title bar.
pub(super) fn menu_bar(cx: &mut App) -> Entity<AppMenuBar> {
    AppMenuBar::new(cx)
}

impl Shell {
    /// The title bar: the app's menus. The window's own controls are the
    /// library's, drawn where the window is decorated by the app.
    pub(super) fn title_bar(&self) -> impl IntoElement + use<> {
        let guard = self.close_guard.clone();
        TitleBar::new()
            .child(h_flex().items_center().child(self.menu_bar.clone()))
            // The library's close removes the window outright; this one asks
            // what the compositor's close asks first.
            .on_close_window(move |_, window, cx| {
                if guard(window, cx) {
                    window.remove_window();
                }
            })
    }

    /// The status bar: the rail's toggle and the branch of the project on
    /// screen at the start, the terminal's and the Workbench's toggles at the end.
    pub(super) fn status_bar(&self, cx: &App) -> impl IntoElement + use<> {
        let branch = self
            .window
            .workspace
            .active_root()
            .and_then(|root| self.window.git.get(&root.path))
            .map(|status| status.branch.clone());
        // Dispatched as the command is, from where the keys are, so the
        // shell answers a press here exactly as it answers the key. A closed
        // panel's glyph takes the muted ink: on the dark bar the selected
        // fill alone is too near the bar's own to say which are open.
        let muted = cx.theme().muted_foreground;
        let toggle = |id: &'static str,
                      icon: IconName,
                      name: &'static str,
                      on: bool,
                      act: Box<dyn gpui::Action>| {
            action(id)
                .quiet_icon(on, cx)
                .small()
                .icon(Icon::new(icon).when(!on, |icon| icon.text_color(muted)))
                .named(name)
                .on_click(move |_, window, cx| window.dispatch_action(act.boxed_clone(), cx))
        };
        let terminal = self.dock.read(cx).is_dock_open(DockPlacement::Bottom);
        let workbench = self.workbench_open(cx);
        let live = self.terminal_live;
        StatusBar::new()
            .left(toggle(
                "status-rail",
                IconName::PanelLeft,
                "Show or hide the rail",
                !self.rail_hidden,
                Box::new(ToggleRail),
            ))
            .left(h_flex().gap_1().children(branch.map(|branch| {
                h_flex()
                    .gap_1()
                    .child(Icon::new(crate::icons::Icon::GitBranch).small())
                    .child(branch)
            })))
            // A command still running behind a closed dock is the one fact
            // the icon cannot carry, so a dot in the running ink rides its
            // corner while the dock is closed; an open terminal shows the
            // command itself.
            .right(
                div()
                    .relative()
                    .flex_none()
                    .child(toggle(
                        "status-terminal",
                        IconName::PanelBottom,
                        match live {
                            true => {
                                "A command is running in a shell here — show or hide the terminal"
                            }
                            false => "Show or hide the terminal",
                        },
                        terminal,
                        Box::new(ToggleTerminal),
                    ))
                    .when(live && !terminal, |toggle| {
                        toggle.child(
                            div()
                                .absolute()
                                .top_0p5()
                                .right_0p5()
                                .size(crate::chat::pane::LIVE_DOT)
                                .rounded_full()
                                .bg(cx.theme().info),
                        )
                    }),
            )
            .right(toggle(
                "status-workbench",
                IconName::PanelRight,
                "Show or hide the Workbench",
                workbench,
                Box::new(ToggleWorkbenchVisibility),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::MENUS;
    use crate::keymap::COMMANDS;

    /// A menu item names a command the keymap holds, or the menus cannot be
    /// built at all.
    #[test]
    fn every_menu_item_is_a_command() {
        for (menu, items) in MENUS {
            for (id, _) in items.iter().flatten() {
                assert!(
                    COMMANDS.iter().any(|command| command.id == *id),
                    "{menu} names `{id}`, which the keymap does not hold"
                );
            }
        }
    }
}
