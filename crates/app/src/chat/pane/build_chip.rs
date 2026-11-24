//! The project page's *Builds* chip: where the project's routed commands
//! build, this machine or one of its build hosts, and the machine's switch
//! for builds on hosts, which the palette's command turns too.
//!
//! The pick is the projects' drive's; the switch and each host's standing
//! are the route drive's, read again on every render, since a routed run
//! refreshes the windows when it ends and when a host's standing changes.

use super::ChatPane;
use crate::state::Shared;
use gpui::{AnyElement, App, BorrowAppContext as _, Context, IntoElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::{ButtonCustomVariant, ButtonVariants as _};
use onehand_core::host::route::drive::{RouteEffect, route_drive, write_switch_blocking};
use onehand_core::host::route::{Builds, Machine, builds, routable_hosts};
use onehand_core::project::Project;
use onehand_core::workspace::HostName;

/// How many build hosts the menu lists before it says how many it left out,
/// as Settings lists them.
const HOSTS_SHOWN: usize = crate::settings::BUILD_HOSTS_SHOWN;

/// The chip for `project`'s head; none for a project with no build host a
/// command of it can be routed to.
pub(super) fn build_chip(project: &Project, cx: &mut Context<ChatPane>) -> Option<AnyElement> {
    let machine = Machine::this(&Shared::global(cx).settings.hosts);
    let hosts = routable_hosts(project, &machine);
    if hosts.is_empty() {
        return None;
    }
    let (on, now) = {
        let drive = route_drive();
        let now = builds(project, &drive, &machine);
        (drive.hosts_on(), now)
    };
    // The row checked is the pick itself, while the switch is off too.
    let picked = project.builds_on.clone().filter(|on| hosts.contains(on));
    let (label, warn) = chip_label(&now);
    let mut trigger = onehand_ui::picker(
        "page-builds-trigger",
        label,
        false,
        onehand_ui::ControlSize::Short,
        cx,
    );
    if warn {
        // The picker's own unset look, in the warning ink.
        trigger = trigger.custom(
            ButtonCustomVariant::new(cx)
                .foreground(crate::theme::status_ink(cx).warning)
                .hover(onehand_ui::control_fill(cx))
                .active(onehand_ui::control_fill(cx)),
        );
    }
    let id = project.id.clone();
    let more = hosts.len().saturating_sub(HOSTS_SHOWN);
    let rows: Vec<(Option<HostName>, String)> = std::iter::once((None, "This machine".into()))
        .chain(hosts.into_iter().take(HOSTS_SHOWN).map(|host| {
            let label = host.to_string();
            (Some(host), label)
        }))
        .collect();
    Some(
        crate::controls::menu_below("page-builds", trigger, move |mut menu, _, _| {
            menu = menu.item(crate::controls::menu_label("Builds"));
            for (host, label) in &rows {
                let (id, host) = (id.clone(), host.clone());
                menu = menu.item(
                    crate::controls::menu_item(label.clone())
                        .checked(host == picked)
                        .on_click(move |_, window, cx: &mut App| {
                            pick(&id, host.clone(), window, cx)
                        }),
                );
            }
            if more > 0 {
                menu = menu.item(crate::controls::menu_label(format!(
                    "{more} more not shown"
                )));
            }
            menu.separator().item(
                crate::controls::menu_item("Build on hosts")
                    .checked(on)
                    .on_click(move |_, window, cx: &mut App| set_host_builds(!on, window, cx)),
            )
        })
        .into_any_element(),
    )
}

/// What the chip reads, and whether in the warning ink.
fn chip_label(now: &Builds) -> (String, bool) {
    match now {
        Builds::Here => ("Builds: This machine".to_string(), false),
        Builds::On(host) => (format!("Builds: {host}"), false),
        Builds::HostsOff => ("Builds: This machine (hosts off)".to_string(), false),
        Builds::NotAnswering(host) => {
            (format!("Builds: This machine ({host} not answering)"), true)
        }
    }
}

/// Make `host` where project `id` builds, or this machine; a refusal said in
/// a toast.
fn pick(
    id: &onehand_core::project::ProjectId,
    host: Option<HostName>,
    window: &mut Window,
    cx: &mut App,
) {
    let picked =
        cx.update_global::<Shared, _>(|shared, _| shared.projects.pick_build_host(id, host));
    match picked {
        Ok(fx) => crate::projects::carry(fx, cx),
        Err(why) => window.push_notification(onehand_ui::toast::error(why), cx),
    }
    cx.refresh_windows();
}

/// Turn builds on hosts on or off for the whole machine: shown at once, the
/// flag file written off the UI thread, and a write that fails puts the
/// switch back and says why.
fn set_host_builds(on: bool, window: &mut Window, cx: &mut App) {
    let fx = route_drive().set_hosts_on(on);
    carry(fx, window, cx);
}

/// Carry what the route drive answered.
fn carry(fx: Vec<RouteEffect>, window: &mut Window, cx: &mut App) {
    cx.refresh_windows();
    for effect in fx {
        match effect {
            RouteEffect::WriteSwitch(on) => {
                let dir = onehand_core::config::config_dir();
                let written = cx
                    .background_executor()
                    .spawn(async move { write_switch_blocking(&dir, on) });
                window
                    .spawn(cx, async move |cx| {
                        let Err(why) = written.await else {
                            return;
                        };
                        let _ = cx.update(|window, cx| {
                            let fx = route_drive().write_failed(on);
                            carry(fx, window, cx);
                            window.push_notification(
                                onehand_ui::toast::failed("Changing builds on hosts", why),
                                cx,
                            );
                        });
                    })
                    .detach();
            }
        }
    }
}

/// The palette's command: the switch the other way.
pub(crate) fn toggle_host_builds(window: &mut Window, cx: &mut App) {
    let on = route_drive().hosts_on();
    set_host_builds(!on, window, cx);
}

#[cfg(test)]
mod tests;
