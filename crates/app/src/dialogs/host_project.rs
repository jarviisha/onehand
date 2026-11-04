//! The form that adds a project whose folder is on a host, already there or
//! cloned there, and the words of the question asked when that host runs
//! another build.

use super::{field, footer_row, title_row};
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::shell::{Browse, Shell, Way};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, WeakEntity, Window, div,
};
use gpui_component::button::ButtonVariants;
use gpui_component::dialog::Dialog;
use gpui_component::{ActiveTheme as _, IconName, StyledExt as _};
use onehand_core::host::wire::{HostSession, clone_name};
use onehand_core::tree::TreeEntry;
use onehand_core::workspace::HostName;
use onehand_ui::Tall as _;
use onehand_ui::TallIcon as _;
use std::path::{Path, PathBuf};

/// Why *Add* refuses while the host attaches and answers.
const ATTACHING: &str = "Attaching to the host";
/// Why *Add* refuses while the host's busy sessions are waited on.
const WAITING: &str = "Waiting for the host's sessions to finish";

/// How many of a busy host's sessions the restart question names.
const BUSY_NAMED: usize = 5;

/// Why *Add* cannot go yet, from what the two fields hold.
///
/// The folder has to start at `/`: the host reads the path as written, with
/// no shell to expand `~` and no folder of the person's to start from.
pub(crate) fn add_refusal(host: &str, folder: &str) -> Option<&'static str> {
    let folder = folder.trim();
    if host.trim().is_empty() {
        Some("Name the host")
    } else if folder.is_empty() {
        Some("Name the folder")
    } else if !folder.starts_with('/') {
        Some("The folder starts at /")
    } else {
        None
    }
}

/// Why *Clone* cannot go yet, from what the three fields hold: the host,
/// a repository whose name says what folder it makes, and the folder to
/// make it in, from `/` for *Add*'s reason.
pub(crate) fn clone_refusal(host: &str, repo: &str, into: &str) -> Option<&'static str> {
    let (repo, into) = (repo.trim(), into.trim());
    if host.trim().is_empty() {
        Some("Name the host")
    } else if repo.is_empty() {
        Some("Name the repository")
    } else if clone_name(repo).is_none() {
        Some("Cannot tell the repository's name from this URL")
    } else if into.is_empty() {
        Some("Name the folder to clone into")
    } else if !into.starts_with('/') {
        Some("The folder starts at /")
    } else {
        None
    }
}

/// The folder a clone of `repo` into `into` makes, as git names it: `None`
/// when the URL says no name.
pub(crate) fn clone_target(repo: &str, into: &Path) -> Option<PathBuf> {
    Some(into.join(clone_name(repo.trim())?))
}

/// How many hosts the menu beside *Host* offers.
pub(crate) const HOST_CHOICES_CAP: usize = 50;

/// How many folders the list under *Folder* draws.
pub(crate) const BROWSE_CAP: usize = 200;

/// How tall the folder list stands before it scrolls, so *Add* stays in
/// reach under it.
const BROWSE_H: gpui::Rems = gpui::rems(12.);

/// The hosts offered beside *Host*: those `used` by the workspace first, in
/// its order, then the SSH config's `aliases`, each once; at most
/// [`HOST_CHOICES_CAP`], and how many more there were.
pub(crate) fn host_choices(aliases: Vec<String>, used: Vec<HostName>) -> (Vec<String>, usize) {
    let mut seen = std::collections::HashSet::new();
    let all: Vec<String> = used
        .into_iter()
        .map(|host| host.0)
        .chain(aliases)
        .filter(|host| seen.insert(host.clone()))
        .collect();
    let more = all.len().saturating_sub(HOST_CHOICES_CAP);
    (all.into_iter().take(HOST_CHOICES_CAP).collect(), more)
}

/// The folders of `at`'s listing, in the host's order, at most
/// [`BROWSE_CAP`]; and how many it leaves out, with the `cut` the host's own
/// bound made.
pub(crate) fn browse_rows(at: &Path, entries: &[TreeEntry], cut: usize) -> (Vec<PathBuf>, usize) {
    let dirs: Vec<PathBuf> = entries
        .iter()
        .filter(|entry| entry.is_dir)
        .map(|entry| at.join(&entry.name))
        .collect();
    let more = dirs.len().saturating_sub(BROWSE_CAP) + cut;
    (dirs.into_iter().take(BROWSE_CAP).collect(), more)
}

/// Why *Browse*, or a folder pressed in its list, cannot go yet: the host
/// is unnamed, or *Add*'s attach or a busy host's wait holds the form.
pub(crate) fn browse_refusal(host: &str, busy: bool, waiting: bool) -> Option<&'static str> {
    if waiting {
        Some(WAITING)
    } else if busy {
        Some(ATTACHING)
    } else if host.trim().is_empty() {
        Some("Name the host")
    } else {
        None
    }
}

/// What the restart question says: the host, and its mid-turn sessions by
/// when they started (as of `now`, in unix seconds), five of them and then
/// how many more.
pub(crate) fn busy_body(host: &HostName, busy: &[HostSession], now: u64) -> String {
    let mut names = busy
        .iter()
        .take(BUSY_NAMED)
        .map(|id| {
            let since = onehand_core::rel_time(now, id.started().0);
            format!("a session started {since}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let more = busy.len().saturating_sub(BUSY_NAMED);
    if more > 0 {
        names.push_str(&format!(" and {more} more"));
    }
    format!(
        "{host} runs another build of the host service, and these sessions are mid-turn: \
         {names}. Restarting stops them."
    )
}

/// The add-a-project-on-a-host form.
///
/// **No trigger**, for the rename's reason: the rail's rows are rebuilt every
/// frame, and the shell decides whether the form exists.
///
/// Two ways, *Open a folder* and *Clone a repository*, over two or three
/// fields, so it stands at a form's width with a form's controls. The ways
/// share *Host* and the folder field, which a clone calls *Into*, so a
/// switch keeps what was typed.
pub fn add_host_project(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let Some(draft) = shell.host_project() else {
        return Dialog::new(cx);
    };
    let (host, folder, repo) = (draft.host.clone(), draft.folder.clone(), draft.repo.clone());
    let (error, busy, way) = (draft.error.clone(), draft.busy(), draft.way);
    let waiting = draft.waiting.clone();
    let refusal = match (&waiting, busy, way) {
        (Some(_), _, _) => Some(WAITING),
        (None, true, _) => Some(ATTACHING),
        (None, false, Way::Folder) => add_refusal(&host.read(cx).value(), &folder.read(cx).value()),
        (None, false, Way::Clone) => clone_refusal(
            &host.read(cx).value(),
            &repo.read(cx).value(),
            &folder.read(cx).value(),
        ),
    };
    let (folder_label, folder_about) = match way {
        Way::Folder => ("Folder", "The project's folder on that host, from /"),
        Way::Clone => ("Into", "The folder on that host to clone into, from /"),
    };
    // A clone says what it is doing while it runs, as it can run minutes.
    let act = match way {
        Way::Folder => "Add",
        Way::Clone if busy && waiting.is_none() => "Cloning…",
        Way::Clone => "Clone",
    };
    let danger = crate::theme::status_ink(cx).danger;
    let muted = cx.theme().muted_foreground;
    // What it waits on, said where its failures are, with what ends it.
    // Only while the host is waited on for some: once the wait ends, the
    // form is attaching again.
    let waiting = waiting.and_then(|on| {
        let n = crate::hosts::busy(cx, &on).len();
        (n > 0).then(|| (crate::hosts::waiting_line(&on, n), on))
    });
    let (choices, more_hosts) = host_choices(draft.aliases.clone(), shell.used_hosts());
    let shell = cx.entity().downgrade();
    let waits = waiting.is_some();
    let browse_refusal = browse_refusal(&host.read(cx).value(), busy, draft.waiting.is_some());
    let browsing = draft.browse.as_ref().is_some_and(|browse| browse.loading);
    let browse = draft.browse.clone();

    Dialog::new(cx)
        .w(super::form_width(window))
        .close_button(false)
        .content(move |content, _, cx: &mut App| {
            let ways = onehand_ui::switch(
                "host-project-way",
                &["Open a folder".into(), "Clone a repository".into()],
                usize::from(way == Way::Clone),
                onehand_ui::ControlSize::Tall,
                {
                    let shell = shell.clone();
                    move |at: &usize, _, cx: &mut App| {
                        let way = if *at == 0 { Way::Folder } else { Way::Clone };
                        let _ = shell.update(cx, |shell, cx| shell.pick_way(way, cx));
                    }
                },
                cx,
            );
            content
                .child(title_row("Add a project on a host", cx))
                .child(ways)
                .child(field(
                    "Host",
                    crate::settings::about("An alias from your SSH config, or user@machine"),
                    div()
                        .h_flex()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(text_input(&host, cx).tall()))
                        .when(!choices.is_empty(), |row| {
                            row.child(host_menu(choices.clone(), more_hosts, &shell, cx))
                        }),
                    cx,
                ))
                .when(way == Way::Clone, |form| {
                    form.child(field(
                        "Repository",
                        crate::settings::about(
                            "A git URL the host can clone with its own credentials",
                        ),
                        text_input(&repo, cx).tall(),
                        cx,
                    ))
                })
                .child(field(
                    folder_label,
                    crate::settings::about(folder_about),
                    div()
                        .h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(text_input(&folder, cx).tall()),
                        )
                        .child(
                            crate::controls::action("host-project-browse")
                                .tall()
                                .secondary()
                                .label("Browse")
                                .loading(browsing && !waits)
                                .refuses(browse_refusal)
                                .on_click({
                                    let shell = shell.clone();
                                    move |_: &ClickEvent, _, cx| {
                                        let _ = shell.update(cx, |shell, cx| {
                                            shell.browse_host_folders(None, cx)
                                        });
                                    }
                                }),
                        ),
                    cx,
                ))
                .children(
                    browse
                        .as_ref()
                        .map(|browse| folder_list(browse, waits, &shell, cx)),
                )
                // The host's own words, or that the folder is not there: shown
                // against the names that caused them, which is why the form
                // is still up.
                .when_some(error.clone(), |form, why| {
                    form.child(div().text_xs().text_color(danger).child(why))
                })
                .when_some(waiting.clone(), |form, (line, on)| {
                    let shell = shell.clone();
                    let restart = crate::controls::action("host-project-restart")
                        .short()
                        .secondary()
                        .label("Restart now")
                        .on_click(move |_: &ClickEvent, window, cx| {
                            let on = on.clone();
                            let _ = shell
                                .update(cx, |shell, cx| shell.ask_restart_host(on, window, cx));
                        });
                    form.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(onehand_ui::status_line(line, muted)),
                            )
                            .child(restart),
                    )
                })
        })
        .footer(
            footer_row()
                .child(div().flex_1())
                .child(
                    crate::controls::action("cancel-host-project")
                        .tall()
                        .quiet(cx)
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_host_project(cx);
                        })),
                )
                .child(
                    crate::controls::action("add-host-project")
                        .tall()
                        .primary()
                        .label(act)
                        .loading(busy && !waits)
                        .refuses(refusal)
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.commit_host_project(window, cx);
                            }),
                        ),
                ),
        )
        // Esc and the ✕ clear the state putting this on screen, or the next
        // frame renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_host_project(cx);
        }))
}

/// The menu beside *Host*: the hosts to pick from, and how many it leaves
/// out.
fn host_menu(
    choices: Vec<String>,
    more: usize,
    shell: &WeakEntity<Shell>,
    cx: &App,
) -> impl IntoElement {
    let shell = shell.clone();
    crate::controls::menu_below(
        "host-choices",
        crate::controls::action("host-choices-trigger")
            .tall_icon()
            .quiet_icon(false, cx)
            .icon(IconName::ChevronDown)
            .named("Hosts from your SSH config"),
        move |mut menu, _, _| {
            for host in &choices {
                let (host, shell) = (host.clone(), shell.clone());
                menu = menu.item(crate::controls::menu_item(host.clone()).on_click(
                    move |_, window, cx: &mut App| {
                        let host = host.clone();
                        let _ = shell.update(cx, |shell, cx| shell.pick_host(host, window, cx));
                    },
                ));
            }
            if more > 0 {
                menu = menu.item(crate::controls::menu_label(format!(
                    "{more} more hosts not shown"
                )));
            }
            menu
        },
    )
}

/// The folders *Browse* listed, under *Folder*: a press on one makes it
/// *Folder* and lists it.
fn folder_list(browse: &Browse, waits: bool, shell: &WeakEntity<Shell>, cx: &App) -> AnyElement {
    let danger = crate::theme::status_ink(cx).danger;
    let muted = cx.theme().muted_foreground;
    let meta = |line: String| div().text_xs().text_color(muted).truncate().child(line);
    let listed = !browse.at.as_os_str().is_empty();
    let row = |id: usize, name: String, to: PathBuf| {
        let shell = shell.clone();
        onehand_ui::row(false, cx)
            .id(("host-browse-row", id))
            .h_flex()
            .items_center()
            .text_size(onehand_ui::tokens::CHROME_TEXT)
            .child(div().min_w_0().truncate().child(name))
            .on_click(move |_: &ClickEvent, window, cx| {
                let to = to.clone();
                let _ = shell.update(cx, |shell, cx| shell.browse_to(to, window, cx));
            })
    };
    let up = browse
        .at
        .parent()
        .map(|parent| row(0, "..".into(), parent.to_path_buf()));
    let rows = browse.dirs.iter().enumerate().map(|(i, dir)| {
        let name = dir.file_name().map_or_else(
            || dir.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        row(i + 1, name, dir.clone())
    });
    div()
        .v_flex()
        .gap_1()
        .min_w_0()
        // Content on its way, until the first listing lands; a busy host's
        // wait says itself in its own line.
        .when(!listed && browse.loading && !waits, |list| {
            list.child(meta(format!("Reading the folders on {}…", browse.host)))
        })
        .when(listed, |list| {
            list.child(meta(format!(
                "Folders in {} on {}",
                browse.at.display(),
                browse.host
            )))
            .child(
                div()
                    .id("host-browse")
                    .v_flex()
                    .max_h(BROWSE_H)
                    .overflow_y_scroll()
                    .children(up)
                    .children(rows),
            )
            .when(browse.dirs.is_empty(), |list| {
                list.child(meta("No folders here".into()))
            })
            .when(browse.cut > 0, |list| {
                list.child(meta(format!("{} more folders not shown", browse.cut)))
            })
        })
        .when_some(browse.error.clone(), |list, why| {
            list.child(div().text_xs().text_color(danger).child(why))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests;
