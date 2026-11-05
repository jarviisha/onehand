//! The window's frame around the rail and the dock: the title bar, holding the
//! app's mark, its menus, the search over the rail's sessions and the docks'
//! toggles, and the status bar along the foot, holding the branch.
//!
//! **The menus hold only the keymap's commands.** Each item is a command the
//! window already answers by a key, named for the menu and dispatched as that
//! command's action, so a menu never offers something no key reaches and the
//! key it shows is the one the keymap binds, overrides included.

use super::{Shell, ToggleTerminal, ToggleWorkbenchVisibility};
use crate::controls::{Named as _, Quiet as _, action};
use crate::keymap::{CLOSE_SESSION, COMMANDS, NEW_SESSION, RESTART};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, Menu, MenuItem, ParentElement as _,
    Styled as _, div,
};
use gpui_component::dock::DockPlacement;
use gpui_component::menu::AppMenuBar;
use gpui_component::status_bar::StatusBar;
use gpui_component::{
    ActiveTheme as _, GlobalState, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, h_flex,
};
use onehand_ui::ShortIcon as _;

/// The title bar's menus at the chrome size: `CHROME_TEXT` over the library's
/// `text_sm` (0.875rem).
const MENU_TEXT: f32 = crate::tokens::CHROME_TEXT.0 / 0.875;

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
    /// The title bar: the app's mark and name, then its menus, at the start;
    /// the search over the rail's sessions and projects centred in the room
    /// left; the terminal's and the Workbench's toggles at the end. The
    /// window's own controls are the library's, drawn where the window is
    /// decorated by the app.
    pub(super) fn title_bar(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement + use<> {
        let guard = self.close_guard.clone();
        TitleBar::new()
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_1p5()
                    .mr_3()
                    .child(gpui::img(crate::assets::LOGO).size_4().flex_none())
                    .child(
                        div()
                            .text_size(crate::tokens::CHROME_TEXT)
                            .font_medium()
                            .child("onehand"),
                    ),
            )
            // The library letters its menu buttons at its control size (`text_sm`)
            // with no way in, so the bar is laid out a step down to the chrome
            // size. The open menus come along: a deferred draw keeps the rem in
            // force where it was made, so their rows letter at the chrome size
            // too, as every other menu's do.
            // Its buttons also take the arrow cursor, which every other button
            // here does not, so a pane painted over them asks for the pointer:
            // drawn last, its cursor wins, and with nothing to occlude a press
            // still reaches the menu under it.
            .child(crate::zoom::scaled(
                MENU_TEXT,
                h_flex()
                    .relative()
                    .items_center()
                    .child(self.menu_bar.clone())
                    .child(div().absolute().inset_0().cursor_pointer()),
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .px_2()
                    .child(crate::rail::search_field(self.rail_state(), cx)),
            )
            .child(self.dock_toggles(cx))
            // The library's close removes the window outright; this one asks
            // what the compositor's close asks first.
            .on_close_window(move |_, window, cx| {
                if guard(window, cx) {
                    window.remove_window();
                }
            })
    }

    /// The status bar: the branch of the project on screen. The rail's own
    /// *Hide the rail*, the agent pane's way back and the key toggle the rail.
    pub(super) fn status_bar(&self) -> impl IntoElement + use<> {
        let branch = self
            .window
            .workspace
            .active_root()
            .and_then(|root| self.window.git_of(root))
            .map(|status| status.branch.clone());
        StatusBar::new().left(h_flex().gap_1().children(branch.map(|branch| {
            h_flex()
                .gap_1()
                .child(Icon::new(crate::icons::Icon::GitBranch).small())
                .child(branch)
        })))
    }

    /// The terminal's and the Workbench's toggles, at the title bar's end. The
    /// title bar drags the window from a press and opens the window's menu
    /// from a right press, so either stops here before the bar: a nudge
    /// presses the toggle rather than moving the window.
    fn dock_toggles(&self, cx: &App) -> impl IntoElement + use<> {
        let terminal = self.dock.read(cx).is_dock_open(DockPlacement::Bottom);
        let workbench = self.workbench_open(cx);
        let live = self.terminal_live;
        h_flex()
            .flex_none()
            .gap_1()
            .pr_2()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
            // A command still running behind a closed dock is the one fact
            // the icon cannot carry, so a dot in the running ink rides its
            // corner while the dock is closed; an open terminal shows the
            // command itself.
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(panel_toggle(
                        "title-terminal",
                        IconName::PanelBottom,
                        match live {
                            true => {
                                "A command is running in a shell here — show or hide the terminal"
                            }
                            false => "Show or hide the terminal",
                        },
                        terminal,
                        Box::new(ToggleTerminal),
                        cx,
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
            .child(panel_toggle(
                "title-workbench",
                IconName::PanelRight,
                "Show or hide the Workbench",
                workbench,
                Box::new(ToggleWorkbenchVisibility),
                cx,
            ))
    }
}

/// A panel's toggle: an icon alone, its glyph in the foreground ink while
/// what it shows is open and in the muted ink while it is closed.
///
/// Dispatched as the command is, from where the keys are, so the shell
/// answers a press here exactly as it answers the key. The ink alone says
/// which are open, with no fill at rest: a fill sat heavy beside the window's
/// own controls, and the hover fill stays the press's. Its tooltip shows the
/// key, as every control a key reaches does.
fn panel_toggle(
    id: &'static str,
    icon: IconName,
    name: &'static str,
    on: bool,
    act: Box<dyn gpui::Action>,
    cx: &App,
) -> impl IntoElement + use<> {
    let ink = match on {
        true => cx.theme().foreground,
        false => cx.theme().muted_foreground,
    };
    action(id)
        .quiet(cx)
        .short_icon()
        .icon(Icon::new(icon).text_color(ink))
        .named_with_action(name, &*act, None)
        .on_click(move |_, window, cx| window.dispatch_action(act.boxed_clone(), cx))
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
