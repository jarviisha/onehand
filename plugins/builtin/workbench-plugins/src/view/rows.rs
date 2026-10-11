use super::PluginsView;
use super::details::{compact, component_chips, counted, note, open_details, tally};
use crate::cli::{self, Available, Catalog, Change, Plugin, Scope, Verb};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Entity, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::switch::Switch;
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Sizable as _, StyledExt, WindowExt as _,
};
use onehand_plugin_host::{Request, action, menu_below, menu_item, menu_row, status_ink};

impl PluginsView {
    /// One installed plugin.
    ///
    /// Its name, then a quieter line saying where it came from and where it
    /// is installed, then what it carries — one chip per kind, *hooks* in the
    /// warning ink since a hook is code that runs on its own. The version is
    /// in the monospace face, a commit cut to the seven characters a person
    /// reads with the whole of it on hover.
    ///
    /// **The switch is "on in this project", and it writes to Local** — this
    /// project on this machine, the one file that reaches nobody else. The
    /// other two scopes each reach somebody else, so they are in the ••• menu,
    /// chosen on purpose rather than hit on the way past.
    ///
    /// The whole row opens the plugin's details and is a tab stop: Enter
    /// opens, Space flips the switch. The switch, the update chip and the menu
    /// keep their presses to themselves, so reaching for one never opens the
    /// details as well.
    pub(super) fn installed_row(
        &self,
        i: usize,
        plugin: &Plugin,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (muted, ring, hover) = (theme.muted_foreground, theme.ring, theme.list_hover);
        let on_here = plugin.in_force(Scope::Local);
        let busy = self.busy.is_some();
        let here = plugin.flip(Scope::Local);

        let switch = Switch::new(("plugin-on", i))
            .checked(on_here)
            .disabled(busy)
            .tooltip(if on_here {
                "On in this project — turn it off on this machine only"
            } else {
                "Off in this project — turn it on on this machine only"
            })
            .on_click({
                let here = here.clone();
                cx.listener(move |view, _: &bool, _, cx| view.change(here.clone(), cx))
            });

        let trigger = action(SharedString::from(format!(
            "plugin-menu-button-{}",
            plugin.id
        )))
        .ghost()
        .xsmall()
        .icon(Icon::new(IconName::Ellipsis))
        .disabled(busy)
        .tooltip("Where it is on, and removing it");
        let menu = self.row_menu(plugin, trigger, cx);

        // The version: monospace, a commit cut to seven characters with what
        // is known of it on hover.
        let version = plugin.version.clone().map(|version| {
            let shown = if cli::is_hash(&version) {
                version[..7].to_string()
            } else {
                version.clone()
            };
            // On hover, the whole commit it came from where the install record
            // names one, else whatever was cut from the label.
            let full = plugin
                .commit
                .clone()
                .or_else(|| (shown != version).then(|| version.clone()));
            div()
                .id(("plugin-version", i))
                .flex_none()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(muted)
                .child(shown)
                .when_some(full, |v, full| {
                    v.tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
                })
        });
        let update = plugin.update.clone().map(|update| {
            // To the scope whose copy is behind, which is not always the
            // first a plugin is installed at.
            let change = Change {
                id: plugin.id.clone(),
                scope: update.scope,
                verb: Verb::Update,
            };
            action(("plugin-update", i))
                .xsmall()
                .outline()
                .label(format!("Update {}", update.to))
                .loading(self.running(&change))
                .disabled(busy)
                .on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| view.change(change.clone(), cx)),
                )
        });

        let scopes = plugin
            .installed
            .iter()
            .map(|scope| scope.label())
            .collect::<Vec<_>>()
            .join(", ");
        let origin = div()
            .h_flex()
            .items_center()
            .gap_1p5()
            .text_xs()
            .text_color(muted)
            .child(format!("{} · {scopes}", plugin.marketplace()))
            .when(plugin.official(), |line| {
                line.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_0p5()
                        .child(Icon::new(IconName::CircleCheck).xsmall())
                        .child("Official"),
                )
            })
            .when(plugin.decided_elsewhere(), |line| {
                line.child(
                    div()
                        .text_color(status_ink(cx).warning)
                        .child("· set elsewhere"),
                )
            });

        let chips = component_chips(plugin, cx);
        let handle = self.rows.get(&plugin.id).cloned();
        let open = plugin.clone();
        let key_plugin = plugin.clone();
        let key_flip = here;

        div()
            .id(SharedString::from(format!("plugin-row-{}", plugin.id)))
            .when_some(handle, |row, handle| row.track_focus(&handle))
            .v_flex()
            .gap_1()
            .w_full()
            .px_3()
            .py_2p5()
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .hover(|row| row.bg(hover))
            .focus(|row| row.border_color(ring))
            .on_click(move |_, window, cx| open_details(&open, window, cx))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => open_details(&key_plugin, window, cx),
                    "space" => view.change(key_flip.clone(), cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .when(!on_here, |name| name.text_color(muted))
                            .child(plugin.name().to_string()),
                    )
                    .children(version)
                    .child(
                        // The controls keep their presses: without this, the
                        // press that flips the switch also opens the details.
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(update)
                            .child(
                                div()
                                    .flex_none()
                                    .when(!busy, |d| d.cursor_pointer())
                                    .child(switch),
                            )
                            .child(menu),
                    ),
            )
            .child(origin)
            .children(chips)
            .into_any_element()
    }

    /// The ••• menu on an installed row, in five groups — where it lives,
    /// its version, where it applies, its files, and removing it — each
    /// separated from the next, with anything that does not apply to this
    /// plugin left out rather than drawn refusing.
    fn row_menu(
        &self,
        plugin: &Plugin,
        trigger: gpui_component::button::Button,
        cx: &mut Context<Self>,
    ) -> gpui_component::popover::Popover {
        // Named by what it acts on, so a menu held open across a re-list is
        // one for this plugin and never for whichever row took its place.
        let menu_id = SharedString::from(format!("plugin-menu-{}", plugin.id));
        let plugin = plugin.clone();
        let view = cx.entity();
        let ask = self.ask.clone();
        menu_below(menu_id, trigger, move |menu, window, cx| {
            let danger = status_ink(cx).danger;
            let mut groups: Vec<Vec<PopupMenuItem>> = Vec::new();

            // Where it lives.
            let mut about = Vec::new();
            if let Some(url) = plugin.inventory.repository.clone() {
                about.push(
                    menu_item("Open repository")
                        .on_click(move |_, _, cx: &mut App| cx.open_url(&url)),
                );
            }
            if let Some(path) = plugin.inventory.changelog.clone() {
                let ask = ask.clone();
                about.push(menu_item("View changelog").on_click(
                    move |_, window: &mut Window, cx: &mut App| {
                        ask(&Request::OpenFile(&path), window, cx)
                    },
                ));
            }
            groups.push(about);

            // Its version. *Update to* only where a newer one is known;
            // *Check for updates* always, since fetching the catalog is how
            // one becomes known.
            let mut version = Vec::new();
            if let Some(update) = &plugin.update {
                let to = if cli::is_hash(&update.to) || update.to.starts_with('v') {
                    update.to.clone()
                } else {
                    format!("v{}", update.to)
                };
                let update = Change {
                    id: plugin.id.clone(),
                    scope: update.scope,
                    verb: Verb::Update,
                };
                version.push(menu_item(format!("Update to {to}")).on_click(act(&view, update)));
            }
            // The scope is not read for a catalog fetch; any install's will do.
            let check = Change {
                id: plugin.id.clone(),
                scope: plugin.installed[0],
                verb: Verb::CheckUpdates,
            };
            version.push(menu_item("Check for updates").on_click(act(&view, check)));
            groups.push(version);

            // Its files.
            let mut files = Vec::new();
            if let Some(path) = plugin.install_path.clone() {
                files.push(
                    menu_item("Open install folder")
                        .on_click(move |_, _, cx: &mut App| cx.open_with_system(&path)),
                );
            }
            let id = plugin.id.clone();
            files.push(
                menu_item("Copy plugin ID").on_click(move |_, _, cx: &mut App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(id.clone()));
                }),
            );

            // Removing it: last, in the danger ink, and asked about first.
            let removes: Vec<PopupMenuItem> = plugin
                .installed
                .iter()
                .map(|scope| {
                    let label = if plugin.installed.len() > 1 {
                        format!("Uninstall from {}…", scope.label())
                    } else {
                        "Uninstall…".to_string()
                    };
                    let (view, plugin, scope) = (view.clone(), plugin.clone(), *scope);
                    menu_row(move |_, _| div().text_color(danger).child(label.clone())).on_click(
                        move |_, window: &mut Window, cx: &mut App| {
                            confirm_uninstall(&view, &plugin, scope, window, cx)
                        },
                    )
                })
                .collect();

            let mut menu = menu;
            let mut first = true;
            let mut divide = |menu: PopupMenu| {
                let menu = if first { menu } else { menu.separator() };
                first = false;
                menu
            };
            for group in groups.into_iter().filter(|group| !group.is_empty()) {
                menu = group.into_iter().fold(divide(menu), PopupMenu::item);
            }

            // Where it applies, as two submenus. *Change scope* only for a
            // plugin installed at one scope: installed at two, which of them
            // moves is a question the menu cannot ask.
            menu = divide(menu);
            if plugin.installed.len() == 1 {
                let (plugin, view, from) = (plugin.clone(), view.clone(), plugin.installed[0]);
                menu = menu.submenu("Change scope", window, cx, move |menu, _, _| {
                    Scope::ALL.into_iter().fold(menu, |menu, to| {
                        let item = menu_item(to.reach()).checked(to == from);
                        menu.item(if to == from {
                            item
                        } else {
                            item.on_click(act(
                                &view,
                                Change {
                                    id: plugin.id.clone(),
                                    scope: to,
                                    verb: Verb::Move(from),
                                },
                            ))
                        })
                    })
                });
            }
            let (on_for, view_on) = (plugin.clone(), view.clone());
            menu = menu.submenu("Turn on for", window, cx, move |menu, _, _| {
                Scope::ALL
                    .into_iter()
                    .filter(|scope| on_for.reaches(*scope))
                    .fold(menu, |menu, scope| {
                        let from = match on_for.source(scope) {
                            Some(source) if source != scope => {
                                format!(" · from {}", source.label())
                            }
                            _ => String::new(),
                        };
                        menu.item(
                            menu_item(format!("{}{from}", scope.reach()))
                                .checked(on_for.in_force(scope))
                                .on_click(act(&view_on, on_for.flip(scope))),
                        )
                    })
            });

            menu = files.into_iter().fold(divide(menu), PopupMenu::item);
            removes.into_iter().fold(divide(menu), PopupMenu::item)
        })
    }

    /// One plugin the marketplaces offer.
    ///
    /// **Its control follows what is already true of it here**: *Installed*
    /// where it is installed and on, *Enable* where it is installed and off —
    /// which turns it on on this machine, as the installed row's switch does —
    /// and *Install ▾* otherwise. The last is split: the press installs at the
    /// scope used last, the caret picks another and installs there, so the
    /// choice is made at the moment of installing rather than by a control
    /// above the list that has to be remembered.
    pub(super) fn available_row(
        &self,
        i: usize,
        plugin: &Available,
        catalog: &Catalog,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.busy.is_some();
        let installed = catalog.installed.iter().find(|p| p.id == plugin.id);
        let control = match installed {
            Some(have) if have.in_force(Scope::Local) => note("Installed", cx),
            Some(have) => {
                let enable = have.flip(Scope::Local);
                action(("plugin-enable", i))
                    .xsmall()
                    .outline()
                    .label("Enable")
                    .tooltip("Installed but off here — turn it on on this machine only")
                    .loading(self.running(&enable))
                    .disabled(busy)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.change(enable.clone(), cx)
                    }))
                    .into_any_element()
            }
            None => self.install_split(i, plugin, cx),
        };

        // Official and the count, and nothing else: which marketplace it is
        // from is the chip above the list.
        let meta: Vec<AnyElement> = [
            plugin.official().then(|| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_0p5()
                    .child(Icon::new(IconName::CircleCheck).xsmall())
                    .child("Official")
                    .into_any_element()
            }),
            plugin.install_count.map(|count| {
                div()
                    .child(format!("{} installs", compact(count)))
                    .into_any_element()
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        let meta = (!meta.is_empty()).then(|| {
            let mut line = div()
                .h_flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .text_color(muted);
            for (n, part) in meta.into_iter().enumerate() {
                if n > 0 {
                    line = line.child("·");
                }
                line = line.child(part);
            }
            line
        });

        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .px_3()
            .py_2p5()
            .gap_1()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(plugin.name.clone()),
                    )
                    .child(control),
            )
            // Two lines, then an ellipsis. Clipped as well as clamped: a
            // description can hold a word with nowhere to break — a URL — and
            // unclipped that ran past the panel's edge.
            .when(!plugin.description.is_empty(), |row| {
                row.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(muted)
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(plugin.description.clone()),
                )
            })
            .children(meta)
            .into_any_element()
    }

    /// *Install ▾*: the press installs at the scope used last, the caret
    /// installs at another and makes it the one used next.
    fn install_split(&self, i: usize, plugin: &Available, cx: &mut Context<Self>) -> AnyElement {
        let scope = self.install_scope;
        let busy = self.busy.is_some();
        let install = Change {
            id: plugin.id.clone(),
            scope,
            verb: Verb::Install,
        };
        let running = Scope::ALL.into_iter().any(|at| {
            self.running(&Change {
                id: plugin.id.clone(),
                scope: at,
                verb: Verb::Install,
            })
        });
        let main = action(("plugin-install", i))
            .xsmall()
            .outline()
            .label("Install")
            .tooltip(format!("Install for: {}", scope.meaning()))
            .loading(running)
            .disabled(busy)
            .on_click(
                cx.listener(move |view, _: &ClickEvent, _, cx| view.change(install.clone(), cx)),
            );
        let caret = action(SharedString::from(format!(
            "plugin-install-scope-{}",
            plugin.id
        )))
        .xsmall()
        .outline()
        .icon(Icon::new(IconName::ChevronDown))
        .disabled(busy)
        .tooltip("Install for…");
        let (id, view) = (plugin.id.clone(), cx.entity());
        let menu = menu_below(
            SharedString::from(format!("plugin-install-menu-{}", plugin.id)),
            caret,
            move |menu, _, _| {
                Scope::ALL
                    .into_iter()
                    .fold(menu.label("Install for"), |menu, at| {
                        let (id, view) = (id.clone(), view.clone());
                        menu.item(menu_item(at.reach()).checked(at == scope).on_click(
                            move |_, _, cx: &mut App| {
                                view.update(cx, |view, cx| {
                                    view.install_scope = at;
                                    view.change(
                                        Change {
                                            id: id.clone(),
                                            scope: at,
                                            verb: Verb::Install,
                                        },
                                        cx,
                                    )
                                })
                            },
                        ))
                    })
            },
        );
        div()
            .h_flex()
            .flex_none()
            .gap_0p5()
            .child(main)
            .child(menu)
            .into_any_element()
    }
}

/// A menu row's press that makes `change`.
fn act(
    view: &Entity<PluginsView>,
    change: Change,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, _, cx| view.update(cx, |view, cx| view.change(change.clone(), cx))
}

/// Ask before uninstalling, naming who loses it and what goes with it.
///
/// A dialog rather than an armed second press, for the reason the app's other
/// removal asks this way: an armed control looks like one that did nothing.
/// The plugin's saved data is kept (`--keep-data`), and it says so.
fn confirm_uninstall(
    view: &Entity<PluginsView>,
    plugin: &Plugin,
    scope: Scope,
    window: &mut Window,
    cx: &mut App,
) {
    let inventory = &plugin.inventory;
    let parts: Vec<String> = tally(inventory, "MCP server", "MCP servers")
        .into_iter()
        .chain(
            (!inventory.hooks.is_empty()).then(|| counted(inventory.hooks.len(), "hook", "hooks")),
        )
        .collect();
    // A folder past the walk's bound was not read to the end, so the counts
    // are a floor and are said as one.
    let at_least = if inventory.cut { "At least " } else { "" };
    let losing = if parts.is_empty() {
        String::new()
    } else {
        format!(
            " {at_least}{} will no longer be available.",
            parts.join(", ")
        )
    };
    let description = format!(
        "{} is removed from {} ({}).{losing} Its saved data is kept.",
        plugin.name(),
        scope.label(),
        scope.reach().to_lowercase(),
    );
    let title = format!("Uninstall {}?", plugin.name());
    let change = Change {
        id: plugin.id.clone(),
        scope,
        verb: Verb::Uninstall,
    };
    let view = view.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        // Cloned per build: a dialog's builder runs again on every frame it is
        // on screen, so nothing captured here can be consumed by one.
        let (view, change) = (view.clone(), change.clone());
        alert
            .title(title.clone())
            .description(description.clone())
            .footer(
                DialogFooter::new()
                    .child(DialogClose::new().child(action("plugin-keep").ghost().label("Cancel")))
                    .child(
                        action("plugin-confirm-uninstall")
                            .danger()
                            .label("Uninstall")
                            .on_click(move |_, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                                view.update(cx, |view, cx| view.change(change.clone(), cx));
                            }),
                    ),
            )
    });
}
