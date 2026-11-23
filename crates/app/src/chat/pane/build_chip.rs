//! The project page's *Builds* chip: where the project's routed commands
//! build, this machine or one of its build hosts, and the machine's switch
//! for builds on hosts, which the palette's command turns too.
//!
//! The pick is the projects' drive's; the switch and each host's standing
//! are the route drive's, read again on every render, since a routed run
//! refreshes the windows when it ends.

use super::ChatPane;
use crate::state::Shared;
use gpui::{AnyElement, App, BorrowAppContext as _, Context, IntoElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::{ButtonCustomVariant, ButtonVariants as _};
use onehand_core::config::HostOs;
use onehand_core::host::route::drive::{RouteEffect, Standing, route_drive, write_switch_blocking};
use onehand_core::project::Project;
use onehand_core::workspace::HostName;

/// The chip for `project`'s head; none for a project that builds nowhere
/// but here: one whose sessions run on a host, which builds there already,
/// or one with no build host of this machine's system.
pub(super) fn build_chip(project: &Project, cx: &mut Context<ChatPane>) -> Option<AnyElement> {
    if project.host.is_some() || project.offload.is_some() {
        return None;
    }
    let hosts = same_system(project, cx);
    if hosts.is_empty() {
        return None;
    }
    // A pick of a host that builds nothing from here reads as no pick, as
    // the route decision takes it.
    let picked = project.builds_on.clone().filter(|on| hosts.contains(on));
    let (on, standing) = {
        let drive = route_drive();
        (
            drive.hosts_on(),
            picked.as_ref().and_then(|host| drive.standing(host)),
        )
    };
    let (label, warn) = chip_label(picked.as_ref(), on, standing);
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
    let rows: Vec<(Option<HostName>, String)> = std::iter::once((None, "This machine".into()))
        .chain(hosts.into_iter().map(|host| {
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
            menu.separator().item(
                crate::controls::menu_item("Build on hosts")
                    .checked(on)
                    .on_click(move |_, window, cx: &mut App| set_host_builds(!on, window, cx)),
            )
        })
        .into_any_element(),
    )
}

/// What the chip reads, and whether in the warning ink: where builds go now,
/// which is here while the switch is off or the picked host did not answer.
fn chip_label(picked: Option<&HostName>, on: bool, standing: Option<Standing>) -> (String, bool) {
    match (picked, on, standing) {
        (None, _, _) => ("Builds: This machine".to_string(), false),
        (Some(_), false, _) => ("Builds: here (hosts off)".to_string(), false),
        (Some(host), true, Some(Standing::NotAnswering)) => {
            (format!("Builds: here ({host} not answering)"), true)
        }
        (Some(host), true, Some(Standing::Reached) | None) => (format!("Builds: {host}"), false),
    }
}

/// The build hosts of `project` a routed command can go to from here: Linux
/// ones, a host nobody declared taken for one, from a Linux machine alone.
fn same_system(project: &Project, cx: &App) -> Vec<HostName> {
    if std::env::consts::OS != "linux" {
        return Vec::new();
    }
    let declared = &Shared::global(cx).settings.hosts;
    project
        .builds
        .iter()
        .map(|entry| entry.host.clone())
        .filter(|host| {
            onehand_core::config::declared(declared, &host.0)
                .is_none_or(|spec| spec.os == HostOs::Linux)
        })
        .collect()
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
pub(crate) fn set_host_builds(on: bool, window: &mut Window, cx: &mut App) {
    let fx = route_drive().set_hosts_on(on);
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
                            // The file is as it was, so the drive goes back
                            // to it with nothing more to write.
                            let _ = route_drive().set_hosts_on(!on);
                            cx.refresh_windows();
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
