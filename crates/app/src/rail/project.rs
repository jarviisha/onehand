use super::row::{
    DragGhost, MAX_BRANCH_W, ProjectDrag, faded, hover_fill, labelled, menu_button, project_key,
    rail_control, row_surfaces,
};
use super::session::{signal_hint, status_mark};
use crate::chat::pane::{ProjectFacts, SessionSignal};
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, rems,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

/// What a project row says on hover: the whole of everything the row cuts.
///
/// The full name first, then the branch, the counts in words, and the root's
/// path -- the last because the label is a folder name and two projects can
/// share one.
pub(super) fn project_hint(
    label: &str,
    git: Option<&GitLine>,
    auto: Option<&SharedString>,
    path: &SharedString,
) -> Vec<SharedString> {
    let mut hint = vec![SharedString::from(label.to_string())];
    if let Some(git) = git {
        hint.push(SharedString::from(format!("Branch: {}", git.branch)));
        hint.extend(git.parts().into_iter().map(|(_, _, words)| words));
    }
    if let Some(auto) = auto {
        hint.push(auto.clone());
    }
    hint.push(path.clone());
    hint
}

/// The entry that opens a project's open issues, to pick one and start on it
/// now — in both menus that carry it, built here once for the reason
/// [`unattended_item`] is.
///
/// Offered where the switch is — on a repository that is not a run's own
/// worktree — because it starts the same kind of run by a different road.
pub fn pick_item(click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> PopupMenuItem {
    crate::controls::menu_item("Work an issue…")
        .icon(Icon::new(IconName::Inbox))
        .on_click(click)
}

/// The entry that turns unattended runs on and off for a project, in both menus
/// that carry it — the rail's and the project page's — built here once so the
/// two cannot come to say it differently.
///
/// A check rather than an on/off pair of labels: the same switch is a switch in
/// Settings, and the check is what makes it read as one here.
pub fn unattended_item(
    on: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> PopupMenuItem {
    crate::controls::menu_item("Work labelled issues")
        .icon(Icon::new(IconName::Bot))
        .checked(on)
        .on_click(click)
}

/// What a project row says about unattended runs.
#[derive(Debug, PartialEq)]
pub(super) struct AutoStatus {
    /// The word in its pill.
    pub(super) badge: SharedString,
    /// The line its hover carries.
    pub(super) line: SharedString,
    /// The switch is on and nothing can come of it — the pill takes the
    /// warning ink, and the line says why.
    pub(super) stuck: bool,
}

/// The run a project's row names: one on that project's issues, as how its
/// issue is shown and whether it is waiting on a card, the working one ahead
/// of a waiting one. `runs` is each run's project, issue and whether it waits.
pub(super) fn run_on<'a>(
    runs: impl IntoIterator<Item = (&'a std::path::Path, &'a str, bool)>,
    root: &std::path::Path,
) -> Option<(&'a str, bool)> {
    runs.into_iter()
        .filter(|&(repo, _, _)| repo == root)
        .map(|(_, number, waiting)| (number, waiting))
        .min_by_key(|&(_, waiting)| waiting)
}

/// What a project row says about unattended runs, or `None` while the project
/// is not switched on.
///
/// A run can only be working on a project that is switched on, but it is read
/// independently so a project switched off mid-run still says the run is
/// there — switching off stops the next run, not the one already going.
///
/// **A switch that is on while nothing can happen is the one state that looks
/// exactly like working**, so every way it can be stuck is said on the row, as
/// `stuck`: a config that stops every run (no label, an interval that does not
/// parse, a mode the agent does not offer), or the last look at this project
/// failing — a remote that is not on GitHub, a `gh` that is missing or signed
/// out.
pub(super) fn auto_status(
    unattended: bool,
    run: Option<(&str, bool)>,
    label: &str,
    stuck: Option<String>,
) -> Option<AutoStatus> {
    let status = |badge: String, line: String, stuck: bool| AutoStatus {
        badge: badge.into(),
        line: line.into(),
        stuck,
    };
    match (unattended, run, stuck) {
        (_, Some((n, false)), _) => Some(status(
            format!("auto · {n}"),
            format!("Unattended run working on issue {n}"),
            false,
        )),
        // Said apart from working: a run standing still on a card is waiting
        // for the person reading this, and "working" would tell them there is
        // nothing to do.
        (_, Some((n, true)), _) => Some(status(
            format!("auto · {n} waiting"),
            format!("Unattended run on issue {n} is waiting for an answer"),
            false,
        )),
        (false, None, _) => None,
        (true, None, Some(why)) => Some(status(
            "auto".into(),
            format!("Unattended runs on, but nothing can be picked up here: {why}"),
            true,
        )),
        (true, None, None) => Some(status(
            "auto".into(),
            format!("Unattended runs on: issues you opened labelled `{label}` are picked up"),
            false,
        )),
    }
}

/// Everything a project row offers, behind one button.
///
/// **Not a ✕ any more.** A remove control sitting on the row that also *selects*
/// the project put the one irreversible action in the rail under its smallest
/// target, a few pixels from the thing users click most -- and next to a
/// session row's ✕, which closes one session, it read as "close this tab"
/// rather than "drop this project from the workspace". Behind a menu the
/// removal gets a full-width label that says what it removes, sits last, is
/// separated from the harmless entries above it, and is drawn in the danger
/// tint. The other four entries are things that were either buried or reachable
/// only by first selecting the project.
///
/// The button is offered on the **active** row only, for the same reason the ✕
/// was: a rail where every row carries a control is a rail of controls, and the
/// user selects a project to see what is in it before acting on it anyway.
/// Every other row still reaches the same entries by right-click, exactly as a
/// session row does — that parity is the point. While this menu existed only as
/// a dropdown, *Remove from workspace* and *New worktree…* could be reached on
/// one row in the rail and nowhere else, so acting on a project always meant
/// selecting it first and tearing down whatever was on screen on the way.
///
/// Written against `&mut App` rather than `&mut Context<PopupMenu>` for the
/// reason `session::session_menu` is: the dropdown host and the context-menu host
/// disagree about that argument, and `Context` derefs to `App`.
fn project_menu(
    root_idx: usize,
    ProjectFacts {
        pinned,
        is_repo,
        unattended,
        check: _,
    }: ProjectFacts,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, cx: &mut App| {
        let danger = crate::theme::status_ink(cx).danger;
        let (pin, auto, pick, start, split, terminal, copy, refresh, remove) = (
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
            shell.clone(),
        );
        menu.item(
            // The label is the state readout as well as the action: with no
            // pin marker of its own a row would otherwise only say it is
            // pinned by *where* it is, which reads as an accident.
            crate::controls::menu_item(if pinned { "Unpin" } else { "Pin to top" })
                .icon(Icon::new(IconName::Star))
                .on_click(move |_, window, cx: &mut App| {
                    pin.update(cx, |shell: &mut Shell, cx| {
                        shell.toggle_pin(root_idx, window, cx);
                    })
                    .ok();
                }),
        )
        .when_some(unattended, |menu, on| {
            menu.item(unattended_item(on, move |_, window, cx: &mut App| {
                auto.update(cx, |shell: &mut Shell, cx| {
                    shell.toggle_unattended(root_idx, window, cx);
                })
                .ok();
            }))
        })
        .when(is_repo && unattended.is_some(), |menu| {
            menu.item(pick_item(move |_, window, cx: &mut App| {
                pick.update(cx, |shell: &mut Shell, cx| {
                    shell.begin_pick(root_idx, None, window, cx)
                })
                .ok();
            }))
        })
        .item(
            crate::controls::menu_item("New session")
                .icon(Icon::new(IconName::Plus))
                .on_click(move |_, window, cx: &mut App| {
                    start
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.new_session_in(root_idx, window, cx);
                        })
                        .ok();
                }),
        )
        // Only where there is a repository to split. On a plain folder the
        // entry could not do anything but report that git said no, and an
        // entry whose whole job is to fail is one the eye has to learn to
        // skip.
        .when(is_repo, |menu| {
            menu.item(
                crate::controls::menu_item("New worktree…")
                    .icon(Icon::new(crate::icons::Icon::GitBranch))
                    .on_click(move |_, window, cx: &mut App| {
                        split
                            .update(cx, |shell: &mut Shell, cx| {
                                shell.begin_worktree(root_idx, window, cx);
                            })
                            .ok();
                    }),
            )
        })
        .item(
            crate::controls::menu_item("Open terminal")
                .icon(Icon::new(IconName::SquareTerminal))
                .on_click(move |_, window, cx: &mut App| {
                    terminal
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.open_terminal_in(root_idx, window, cx);
                        })
                        .ok();
                }),
        )
        .item(
            crate::controls::menu_item("Copy project path")
                .icon(Icon::new(IconName::Copy))
                .on_click(move |_, window, cx: &mut App| {
                    copy.update(cx, |shell: &mut Shell, cx| {
                        shell.copy_root_path(root_idx, window, cx);
                    })
                    .ok();
                }),
        )
        .item(
            crate::controls::menu_item("Refresh Git status")
                .icon(Icon::new(IconName::Redo))
                .on_click(move |_, _, cx: &mut App| {
                    refresh
                        .update(cx, |shell: &mut Shell, cx| shell.refresh_git(cx))
                        .ok();
                }),
        )
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| {
                div().text_color(danger).child("Remove from workspace")
            })
            .icon(Icon::new(IconName::Delete).text_color(danger))
            .on_click(move |_, window, cx: &mut App| {
                remove
                    .update(cx, |shell: &mut Shell, cx| {
                        shell.remove_root(root_idx, window, cx);
                    })
                    .ok();
            }),
        )
    }
}

/// A project's git state as its row says it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct GitLine {
    pub(super) branch: SharedString,
    pub(super) changed: usize,
    pub(super) ahead: usize,
    pub(super) behind: usize,
}

/// Which part of the git line a count is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum GitPart {
    Changed,
    Ahead,
    Behind,
}

impl GitLine {
    pub(super) fn of(status: &onehand_core::gitstat::GitStatus) -> Self {
        Self {
            branch: status.branch.clone().into(),
            changed: status.changed,
            ahead: status.ahead,
            behind: status.behind,
        }
    }

    /// The counts that are not zero, each with its full words.
    pub(super) fn parts(&self) -> Vec<(GitPart, usize, SharedString)> {
        fn count(n: usize, one: &str, many: &str) -> String {
            format!("{n} {}", if n == 1 { one } else { many })
        }
        [
            (
                GitPart::Changed,
                self.changed,
                count(self.changed, "uncommitted change", "uncommitted changes"),
            ),
            (
                GitPart::Ahead,
                self.ahead,
                count(self.ahead, "commit", "commits") + " ahead of the remote",
            ),
            (
                GitPart::Behind,
                self.behind,
                count(self.behind, "commit", "commits") + " behind the remote",
            ),
        ]
        .into_iter()
        .filter(|(_, n, _)| *n > 0)
        .map(|(part, n, words)| (part, n, words.into()))
        .collect()
    }
}

/// The dot before a project's change count: a count of what, said on hover.
const CHANGE_DOT: gpui::Rems = rems(0.375);

/// One part of a project's git line: a mark and its count, named in full on
/// hover and to assistive technology.
fn git_part(
    id: gpui::ElementId,
    part: GitPart,
    n: usize,
    words: SharedString,
    cx: &App,
) -> impl IntoElement + use<> {
    let mark = match part {
        GitPart::Changed => div()
            .size(CHANGE_DOT)
            .rounded_full()
            .bg(cx.theme().muted_foreground)
            .into_any_element(),
        GitPart::Ahead => Icon::new(IconName::ArrowUp).xsmall().into_any_element(),
        GitPart::Behind => Icon::new(IconName::ArrowDown).xsmall().into_any_element(),
    };
    div()
        .id(id)
        .h_flex()
        .items_center()
        .flex_none()
        .gap_0p5()
        .role(gpui::accesskit::Role::Image)
        .aria_label(words.clone())
        .tooltip(move |window, cx| Tooltip::new(words.clone()).build(window, cx))
        .child(mark)
        .child(n.to_string())
}

/// How a project row stands in the list.
pub(super) struct Place {
    /// Where the row is drawn, which is what a drag hands back.
    pub(super) at: usize,
    pub(super) open: bool,
    /// The keyboard's row, while the list has focus.
    pub(super) cursor: bool,
    /// What its sessions roll up to, drawn only while folded.
    pub(super) badge: Option<SessionSignal>,
}

/// A project: the fold chevron, its folder, its name fading where its room
/// ends, its git line, the pin and the unattended pill, and while folded the
/// most urgent state of its sessions. Under the pointer, over the row's end:
/// a new session here, and `⋯`.
///
/// A click puts the keyboard on it and folds or unfolds it; it does not
/// change the session on screen.
pub(super) fn project_row(
    window_state: &WorkspaceWindow,
    root_idx: usize,
    place: Place,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let root = &window_state.workspace.roots[root_idx];
    let Place {
        at,
        open,
        cursor,
        badge,
    } = place;
    let pinned = root.pinned;
    // The issue a run is working on in this project right now, if one is; the
    // working one ahead of a waiting one, since a project can hold both.
    let runs = crate::task::live_issues(cx);
    let run = run_on(
        runs.iter()
            .map(|run| (run.repo.as_path(), run.name.as_str(), run.waiting.is_some())),
        &root.path,
    );
    // The kept issue of the run the pill names, which pressing it opens on the
    // Issues page; a forge's own issue has no page to open on.
    let pill_issue = run.and_then(|(name, waiting)| {
        runs.iter()
            .find(|r| r.repo == root.path && r.name == name && r.waiting.is_some() == waiting)?
            .kept
    });
    let auto = auto_status(
        root.unattended,
        run,
        &crate::unattended::label(cx),
        // What stops every run outranks what stops this project's.
        crate::unattended::blocked(cx).or_else(|| crate::unattended::problem(&root.path, cx)),
    );
    let status = window_state.git.get(&root.path);
    let facts = ProjectFacts::of(root, status.is_some());
    let git = status.map(GitLine::of);
    let path = SharedString::from(root.path.display().to_string());
    let mut hint = project_hint(
        &root.label,
        git.as_ref(),
        auto.as_ref().map(|auto| &auto.line),
        &path,
    );
    if pinned {
        hint.insert(1, "Pinned to the top".into());
    }
    let key = project_key(&root.path);
    let (rest, hovered) = row_surfaces(cursor, cx);
    let (shell, hover) = (cx.entity().downgrade(), hover_fill(cx));
    let muted = cx.theme().muted_foreground;
    let (radius, chip, chip_ink) = (
        cx.theme().radius,
        cx.theme().secondary,
        cx.theme().secondary_foreground,
    );
    let warning = crate::theme::status_ink(cx).warning;
    let (fold_path, pill_root, ghost) = (root.path.clone(), root.path.clone(), root.label.clone());
    let label = root.label.clone();

    div()
        .id(gpui::ElementId::Name(key.clone()))
        .group(key.clone())
        .h_flex()
        .items_center()
        .relative()
        .h(super::ROW_H)
        .pl_1p5()
        .pr_1()
        .gap_1p5()
        .rounded(radius)
        .cursor_pointer()
        .when(cursor, |d| d.bg(cx.theme().sidebar_accent))
        .when(!cursor, |d| d.hover(move |d| d.bg(hover)))
        .text_color(cx.theme().foreground)
        .font_medium()
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.rail_click_project(fold_path.clone(), window, cx);
            }),
        )
        .child(
            Icon::new(match open {
                true => IconName::ChevronDown,
                false => IconName::ChevronRight,
            })
            .xsmall()
            .text_color(muted),
        )
        .child(
            Icon::new(match open {
                true => IconName::FolderOpen,
                false => IconName::Folder,
            })
            .small(),
        )
        .child(
            faded(
                ("project-name", root_idx),
                label.clone(),
                key.clone(),
                rest,
                hovered,
            )
            .tooltip(move |window, cx| {
                let hint = hint.clone();
                Tooltip::element(move |_, _| div().v_flex().gap_0p5().children(hint.clone()))
                    .build(window, cx)
            }),
        )
        .child(
            div()
                .h_flex()
                .items_center()
                .flex_none()
                .gap_1p5()
                .text_xs()
                .font_normal()
                .text_color(muted)
                .when_some(git, |row, git| {
                    row.child(
                        div()
                            .max_w(MAX_BRANCH_W)
                            .truncate()
                            .child(git.branch.clone()),
                    )
                    .children(git.parts().into_iter().map(
                        |(part, n, words)| {
                            let id = SharedString::from(format!("{key}-{part:?}"));
                            git_part(gpui::ElementId::Name(id), part, n, words, cx)
                        },
                    ))
                })
                // Position alone does not say a project is pinned: first in
                // the list is where a project can also be by accident.
                .when(pinned, |row| row.child(Icon::new(IconName::Star).xsmall()))
                // Whether its labelled issues are worked unattended, and the
                // issue a run is on. Stuck takes the warning ink; the hover
                // says why. A run's pill opens its issue on the Issues page.
                .when_some(auto, |row, auto| {
                    let target = shell.clone();
                    row.child(
                        div()
                            .id(("project-auto", root_idx))
                            .flex_none()
                            .px_1()
                            .rounded(radius)
                            .bg(chip)
                            .text_color(if auto.stuck { warning } else { chip_ink })
                            .child(auto.badge)
                            .when_some(pill_issue, |pill, number| {
                                pill.cursor_pointer()
                                    .on_mouse_up(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(move |_, window, cx: &mut App| {
                                        let root = pill_root.clone();
                                        target
                                            .update(cx, |shell: &mut Shell, cx| {
                                                shell.open_issue_on_page(&root, number, window, cx)
                                            })
                                            .ok();
                                    })
                            }),
                    )
                }),
        )
        .children(badge.filter(|_| !open).map(|signal| {
            let label = format!("{}: {}", root.label, signal_hint(Some(signal)));
            status_mark(("badge", root_idx).into(), Some(signal), label.into(), cx)
        }))
        // Over the end of the row, on its hover fill, so they take no room
        // from the name while hidden.
        .child(
            div()
                .h_flex()
                .items_center()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .px_1()
                .rounded(radius)
                .bg(hovered)
                .invisible()
                .group_hover(key.clone(), |s| s.visible())
                .child(labelled(
                    ("project-new-name", root_idx),
                    "New session in this project",
                    rail_control(("project-new", root_idx), IconName::Plus, cx)
                        .tooltip("New session in this project")
                        .on_click(cx.listener(move |shell: &mut Shell, _, window, cx| {
                            shell.new_session_in(root_idx, window, cx);
                        })),
                ))
                .child(labelled(
                    ("project-menu-name", root_idx),
                    "Project actions",
                    menu_button(
                        rail_control(("project-menu", root_idx), IconName::Ellipsis, cx),
                        "What can be done with this project",
                        project_menu(root_idx, facts, shell.clone()),
                    ),
                )),
        )
        // Dropped by display position: `Workspace::move_root` writes the
        // permutation back into the roots, and clamps a drop across the pin
        // line rather than refusing it.
        .on_drag(ProjectDrag { from: at }, move |_, _, _, cx| {
            let ghost = ghost.clone();
            cx.new(|_| DragGhost(SharedString::from(ghost)))
        })
        .drag_over::<ProjectDrag>(|style, _, _, cx| style.bg(hover_fill(cx)))
        .on_drop({
            let target = shell.clone();
            move |drag: &ProjectDrag, window, cx| {
                let from = drag.from;
                let _ = target.update(cx, |shell: &mut Shell, cx| {
                    shell.move_root(from, at, window, cx);
                });
            }
        })
        .context_menu(move |menu, window, cx| {
            project_menu(root_idx, facts, shell.clone())(menu, window, cx)
        })
        .into_any_element()
}
