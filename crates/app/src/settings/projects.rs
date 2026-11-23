//! The Projects page: one project of the window at a time, its check
//! command, the hosts it builds on and where its new worktrees go.
//!
//! **Here and not on the project's page.** The page is where work on a
//! project starts and is found again; what the project is *set to* changes
//! rarely, and kept beside the work it pushed the work down a screen. Here it
//! is one menu away from any project, whatever is on screen.

use super::{about, list_row, list_rows, page_head, section};
use crate::controls::Named as _;
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Entity, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, StyledExt};
use onehand_core::workspace::{BuildHost, ProjectRoot};
use onehand_ui::{Short as _, ShortIcon as _, Tall as _};
use std::path::{Path, PathBuf};

/// How many build hosts the page lists before it says how many it left out.
const BUILD_HOSTS_SHOWN: usize = 20;

/// The projects of `roots` this page is about, by folder and name: those
/// whose sessions run on this machine, a worker's row left out.
pub(crate) fn offered(roots: &[ProjectRoot]) -> Vec<(PathBuf, String)> {
    roots
        .iter()
        .filter(|root| !root.transient && root.runs_here())
        .filter_map(|root| Some((root.local_path()?.to_path_buf(), root.label.clone())))
        .collect()
}

/// The project shown: `picked` while it is offered, else the first offered.
pub(crate) fn shown(picked: Option<&Path>, offered: &[(PathBuf, String)]) -> Option<PathBuf> {
    picked
        .filter(|picked| offered.iter().any(|(path, _)| path == picked))
        .map(Path::to_path_buf)
        .or_else(|| offered.first().map(|(path, _)| path.clone()))
}

/// What a build host's row says under its name: the paths a build brings
/// back, and the folder they land under when it names one.
pub(super) fn build_host_line(entry: &BuildHost) -> String {
    if entry.artifacts.is_empty() {
        return "Brings nothing back".into();
    }
    let paths: Vec<String> = entry
        .artifacts
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    match &entry.into {
        Some(into) => format!("{}, into {}", paths.join(", "), into.display()),
        None => paths.join(", "),
    }
}

pub(super) fn projects_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let roots = &shell.workspace_window().workspace.roots;
    let offered = offered(roots);
    let root = shell.settings_project(cx);
    let head = page_head(
        "What each project is set to, kept in its project file",
        "Project",
        cx,
    );
    let page = div().v_flex().gap_6().w_full().child(head);
    let Some(root) = root else {
        return page
            .child(
                section(None, None, cx).child(
                    div()
                        .text_size(crate::tokens::CHROME_TEXT)
                        .text_color(cx.theme().muted_foreground)
                        .child("No project here runs on this machine"),
                ),
            )
            .into_any_element();
    };
    let at = roots.iter().position(|r| r.local_path() == Some(&root));
    let builds = at
        .map(|at| roots[at].build_hosts().to_vec())
        .unwrap_or_default();
    let check = shell.check_input(&root);
    let parent = crate::projects::worktree_parent(&root, cx);
    page.child(section(None, None, cx).child(onehand_ui::field(
        "Project",
        None,
        picker(handle, &offered, &root, cx),
        cx,
    )))
    .child(
        section(None, None, cx)
            .children(check.map(|input| {
                onehand_ui::field(
                    "Check command",
                    about("What Run check and a command step naming none run here"),
                    text_input(&input, cx).tall(),
                    cx,
                )
            }))
            .child(build_hosts(handle, &root, &builds, cx))
            .child(worktree_folder(handle, &root, parent.as_deref(), cx)),
    )
    .into_any_element()
}

/// The menu that picks which project the page shows, capped as Settings'
/// lists are.
fn picker(
    handle: &Entity<Shell>,
    offered: &[(PathBuf, String)],
    root: &Path,
    cx: &App,
) -> AnyElement {
    let label = offered
        .iter()
        .find(|(path, _)| path == root)
        .map_or_else(String::new, |(_, label)| label.clone());
    let more = offered.len().saturating_sub(super::pages::LIST_SHOWN);
    let choices: Vec<(PathBuf, String, bool)> = offered
        .iter()
        .take(super::pages::LIST_SHOWN)
        .map(|(path, label)| (path.clone(), label.clone(), path == root))
        .collect();
    let handle = handle.clone();
    crate::controls::menu_below(
        "settings-project",
        onehand_ui::picker(
            "settings-project-trigger",
            label,
            true,
            onehand_ui::ControlSize::Tall,
            cx,
        ),
        move |mut menu, _, _| {
            for (path, label, checked) in &choices {
                let (handle, path) = (handle.clone(), path.clone());
                menu = menu.item(
                    crate::controls::menu_item(label.clone())
                        .checked(*checked)
                        .on_click(move |_, window: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| {
                                shell.pick_settings_project(path.clone(), window, cx);
                            });
                        }),
                );
            }
            if more > 0 {
                menu = menu.label(SharedString::from(format!("{more} more not shown")));
            }
            menu
        },
    )
    .into_any_element()
}

/// One row per host the project builds on, then the way to add one.
fn build_hosts(handle: &Entity<Shell>, root: &Path, builds: &[BuildHost], cx: &App) -> AnyElement {
    let ink = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    let more = builds.len().saturating_sub(BUILD_HOSTS_SHOWN);
    let on = |act: fn(&mut Shell, PathBuf, &mut Window, &mut gpui::Context<Shell>)| {
        let (handle, root) = (handle.clone(), root.to_path_buf());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            handle.update(cx, |shell, cx| act(shell, root.clone(), window, cx));
        }
    };
    let rows: Vec<AnyElement> = builds
        .iter()
        .take(BUILD_HOSTS_SHOWN)
        .enumerate()
        .map(|(idx, entry)| {
            let (edit, remove) = (handle.clone(), handle.clone());
            let (edit_root, remove_root) = (root.to_path_buf(), root.to_path_buf());
            list_row(
                entry.host.to_string(),
                Some(build_host_line(entry).into_any_element()),
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        crate::controls::action(("edit-build-host", idx))
                            .short_icon()
                            .secondary()
                            .icon(Icon::new(crate::icons::Icon::SquarePen))
                            .named("Edit")
                            .on_click(move |_, window, cx| {
                                edit.update(cx, |shell, cx| {
                                    shell.open_build_host_form(
                                        edit_root.clone(),
                                        Some(idx),
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .child(
                        crate::controls::action(("remove-build-host", idx))
                            .short()
                            .secondary()
                            .text_color(ink.danger)
                            .label("Remove…")
                            .on_click(move |_, window, cx| {
                                remove.update(cx, |shell, cx| {
                                    shell.confirm_remove_build_host(
                                        remove_root.clone(),
                                        idx,
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    ),
                cx,
            )
            .into_any_element()
        })
        .collect();
    let said = |line: String| div().text_xs().text_color(muted).child(line);
    onehand_ui::field(
        "Build hosts",
        Some("Where this project builds, and what comes back".into_any_element()),
        div()
            .v_flex()
            .gap_2()
            .children((!rows.is_empty()).then(|| list_rows(rows, cx)))
            .children((more > 0).then(|| said(format!("{more} more not shown"))))
            .children(builds.is_empty().then(|| said("No build hosts".into())))
            .child(
                div().h_flex().child(
                    crate::controls::action("add-build-host")
                        .short()
                        .secondary()
                        .label("Add build host…")
                        .on_click(on(|shell, root, window, cx| {
                            shell.open_build_host_form(root, None, window, cx);
                        })),
                ),
            ),
        cx,
    )
    .into_any_element()
}

/// Where the project's new worktrees go, and the ways to change it.
fn worktree_folder(
    handle: &Entity<Shell>,
    root: &Path,
    parent: Option<&Path>,
    cx: &App,
) -> AnyElement {
    let place = match parent {
        Some(parent) => match std::env::home_dir().and_then(|home| {
            parent
                .strip_prefix(&home)
                .ok()
                .map(|rest| format!("~/{}", rest.display()))
        }) {
            Some(short) => short,
            None => parent.display().to_string(),
        },
        None => "Beside the repository".to_string(),
    };
    let (pick, clear) = (handle.clone(), handle.clone());
    let (pick_root, clear_root) = (root.to_path_buf(), root.to_path_buf());
    onehand_ui::field(
        "Worktree folder",
        Some("Where new worktrees of this project go".into_any_element()),
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(div().min_w_0().truncate().child(place))
            .child(
                crate::controls::action("worktree-folder")
                    .short()
                    .secondary()
                    .label("Change…")
                    .on_click(move |_, _, cx| {
                        pick.update(cx, |shell, cx| {
                            shell.pick_worktree_folder(pick_root.clone(), cx);
                        });
                    }),
            )
            .when(parent.is_some(), |row| {
                row.child(
                    crate::controls::action("worktree-folder-beside")
                        .short()
                        .secondary()
                        .label("Beside the repository")
                        .on_click(move |_, _, cx| {
                            clear.update(cx, |shell, cx| {
                                shell.clear_worktree_folder(clear_root.clone(), cx);
                            });
                        }),
                )
            }),
        cx,
    )
    .into_any_element()
}
