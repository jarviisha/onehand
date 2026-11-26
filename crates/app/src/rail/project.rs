use super::row::{
    MAX_BRANCH_W, ProjectDrag, covered_tip, covering, fade_band, faded, hover_fill, key_ring,
    labelled, menu_button, project_key, row_control, row_inks, row_surfaces,
};
use super::session::{signal_hint, status_mark};
use crate::chat::pane::{ProjectFacts, SessionSignal};
use crate::controls::Named as _;
use crate::drag::DragGhost;
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, rems,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_ui::mark_slot;

mod menu;
use menu::project_menu;

/// What a project row says on hover: the whole of everything the row cuts.
///
/// The full name first, then the branch, the counts in words, and the root's
/// path (`host:path` for a folder on a host) -- the last because the label is a
/// folder name and two projects can share one.
pub(super) fn project_hint(
    label: &str,
    git: Option<&GitLine>,
    path: &SharedString,
) -> Vec<SharedString> {
    let mut hint = vec![SharedString::from(label.to_string())];
    if let Some(git) = git {
        hint.push(SharedString::from(format!("Branch: {}", git.branch)));
        hint.extend(git.parts().into_iter().map(|(_, _, words)| words));
    }
    hint.push(path.clone());
    hint
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

/// One part of a project's git line: a mark and its count, in the line's
/// `ink`, named in full on hover and to assistive technology.
fn git_part(
    id: gpui::ElementId,
    part: GitPart,
    n: usize,
    words: SharedString,
    row: &SharedString,
    ink: gpui::Hsla,
) -> impl IntoElement + use<> {
    let mark = match part {
        GitPart::Changed => div()
            .size(CHANGE_DOT)
            .rounded_full()
            .bg(ink)
            .into_any_element(),
        GitPart::Ahead => Icon::new(IconName::ArrowUp).xsmall().into_any_element(),
        GitPart::Behind => Icon::new(IconName::ArrowDown).xsmall().into_any_element(),
    };
    div()
        .id(id)
        .h_flex()
        .items_center()
        .flex_none()
        .gap_1()
        .role(gpui::accesskit::Role::Image)
        .aria_label(words.clone())
        .tooltip(covered_tip(row, move |window, cx| {
            Tooltip::new(words.clone()).build(window, cx)
        }))
        .child(mark)
        .child(n.to_string())
}

/// How a project row stands in the list.
pub(super) struct Place {
    /// Where the row is drawn, which is what a drag hands back.
    pub(super) at: usize,
    pub(super) open: bool,
    /// Its own page is what shows: the chosen fill.
    pub(super) chosen: bool,
    /// The keyboard's row, while the list holds the keys and the keys put it
    /// there: the ring, and its actions as if it were hovered.
    pub(super) ring: bool,
    /// What its sessions roll up to, drawn only while folded.
    pub(super) badge: Option<SessionSignal>,
    /// Removing it asks first.
    pub(super) asks: bool,
}

/// A project: the fold chevron, its folder, its name fading where its room
/// ends, its git line and the pin, and while folded the most urgent state of
/// its sessions. Under the pointer, over the row's end: a new session here,
/// and `⋯`.
///
/// A click goes to the project: its page shows, over whatever sessions it has
/// open, and the keyboard's row moves there. Only the chevron, a button
/// of its own, folds it.
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
        chosen,
        ring,
        badge: folded,
        asks,
    } = place;
    let pinned = root.pinned;
    // A folder on a host has no problems here; what is filed under its path
    // is a local twin's. Its `git status` is read over its attach.
    let status = window_state.git_of(root);
    let mut facts = ProjectFacts::of(root, status.is_some());
    facts.board = crate::projects::board_here(root, cx).is_some();
    // *Run here again* only while this machine holds the code.
    let away = crate::offload::get(cx, &root.path).filter(|_| root.offload.is_some());
    if facts.offload == Some(true) && !away.is_some_and(crate::offload::Offload::here) {
        facts.offload = None;
    }
    let git = status.map(GitLine::of);
    // A folder on a host is named with the host, as ssh names one.
    let path = SharedString::from(onehand_core::workspace::host_path(
        root.on_host(),
        &root.path,
    ));
    let host = root
        .on_host()
        .map(|host| SharedString::from(host.to_string()));
    // Out of reach of its host: attached again by itself while lost, never
    // while another machine holds it. Nothing before a first attach.
    let unreached = root
        .on_host()
        .and_then(|host| match crate::hosts::state(cx, host) {
            crate::hosts::HostState::Lost(_) => Some(crate::hosts::not_connected(
                host,
                crate::hosts::retrying(cx, host),
            )),
            crate::hosts::HostState::Taken => {
                Some(format!("{host} was attached from another machine"))
            }
            crate::hosts::HostState::Unknown
            | crate::hosts::HostState::Attaching
            | crate::hosts::HostState::Live => None,
        });
    let unreached_ink = cx.theme().muted_foreground;
    // An offloaded project whose code its host holds: marked only then, and
    // in the warning ink when bringing it back stopped on a change here,
    // which waits on the person.
    let away = root
        .offload
        .as_ref()
        .and_then(|_| crate::offload::get(cx, &root.path))
        .filter(|o| o.locked());
    let held = away.map(|o| {
        let stopped = o.state.as_ref().is_some_and(|s| s.stopped.is_some());
        let tip = match stopped {
            true => format!(
                "Bringing the code back from {} stopped: something changed here meanwhile",
                o.host
            ),
            false => crate::offload::mark_line(&o.host),
        };
        (SharedString::from(tip), stopped)
    });
    // An offloaded project on a branch, while its host's branch has commits
    // this one does not: calm, since merging waits on nothing but the person.
    let to_merge = root
        .offload
        .as_ref()
        .and_then(|_| crate::offload::get(cx, &root.path))
        .and_then(|o| {
            let branch = o.branch.as_ref()?;
            let ahead = branch.ahead.filter(|n| *n > 0)?;
            Some(SharedString::from(crate::offload::merge_line(
                &branch.name,
                &o.host,
                ahead,
            )))
        });
    let mut hint = project_hint(&root.label, git.as_ref(), &path);
    if pinned {
        hint.insert(1, "Pinned to the top".into());
    }
    let key = project_key(&root.path);
    let (rest, hovered) = row_surfaces(chosen, cx);
    let (name_ink, meta_ink) = row_inks(chosen, cx);
    let (shell, hover) = (cx.entity().downgrade(), hover_fill(cx));
    let radius = cx.theme().radius;
    let warning = crate::theme::status_ink(cx).warning;
    let (fold_path, ghost) = (root.path.clone(), root.label.clone());
    let label = root.label.clone();

    div()
        .id(gpui::ElementId::Name(key.clone()))
        .debug_selector(move || format!("rail-project-{root_idx}"))
        .group(key.clone())
        .h_flex()
        .items_center()
        .relative()
        .h(crate::tokens::ROW_H)
        .px_2()
        .gap_2()
        .rounded(radius)
        .cursor_pointer()
        .when(chosen, |d| d.bg(super::row::chosen_fill(cx)))
        .when(!chosen, |d| d.hover(move |d| d.bg(hover)))
        .text_color(name_ink)
        .font_medium()
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.rail_click_project(root_idx, window, cx);
            }),
        )
        // Folding and going to a project are two intentions, so the fold is
        // a target of its own and the row's click never folds. Its icon sits
        // on the marks' line; the control round it spills past the slot.
        .child(mark_slot(labelled(
            ("project-fold-name", root_idx),
            "Fold or unfold this project",
            row_control(
                ("project-fold", root_idx),
                match open {
                    true => IconName::ChevronDown,
                    false => IconName::ChevronRight,
                },
                cx,
            )
            .named(match open {
                true => "Hide this project's sessions",
                false => "Show this project's sessions",
            })
            .on_click(cx.listener(move |shell: &mut Shell, _, _, cx| {
                shell.rail_fold(fold_path.clone(), open, cx);
            })),
        )))
        // One shape folded or not: the chevron beside it already says which.
        .child(mark_slot(Icon::new(IconName::Folder).small()))
        .child(
            faded(
                ("project-name", root_idx),
                label.clone(),
                key.clone(),
                rest,
                hovered,
            )
            .debug_selector(move || format!("rail-project-name-{root_idx}"))
            .min_w(super::row::NAME_MIN_W)
            .tooltip(covered_tip(&key, move |window, cx| {
                let hint = hint.clone();
                // The path, last, is machine text and stands in its face.
                Tooltip::element(move |_, cx: &mut App| {
                    let (path, words) = hint
                        .split_last()
                        .map_or((None, &[][..]), |(p, w)| (Some(p), w));
                    div()
                        .v_flex()
                        .gap_0p5()
                        .children(words.iter().cloned())
                        .children(path.map(|path| {
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(path.clone())
                        }))
                })
                .build(window, cx)
            })),
        )
        .child(
            // Shrinks, by its host and its branch alone: everything else on it
            // keeps its width, and those truncate once the name is down to its
            // minimum. A folder on a host names its host first, then its branch
            // once that is read over the host's attach.
            div()
                .debug_selector(move || format!("rail-project-tail-{root_idx}"))
                .h_flex()
                .items_center()
                .min_w_0()
                .gap_1()
                .text_xs()
                .font_normal()
                .text_color(meta_ink)
                .when_some(host, |row, host| {
                    let place = path.clone();
                    row.child(
                        div()
                            .id(("project-host", root_idx))
                            .min_w_0()
                            .max_w(MAX_BRANCH_W)
                            .truncate()
                            .tooltip(covered_tip(&key, move |window, cx| {
                                Tooltip::new(place.clone()).build(window, cx)
                            }))
                            .child(host),
                    )
                })
                .when_some(unreached, |row, said| {
                    let tip = SharedString::from(said);
                    row.child(
                        div()
                            .id(("project-unreached", root_idx))
                            .flex_none()
                            .role(gpui::accesskit::Role::Image)
                            .aria_label(tip.clone())
                            .tooltip(covered_tip(&key, move |window, cx| {
                                Tooltip::new(tip.clone()).build(window, cx)
                            }))
                            .child(
                                Icon::new(crate::icons::Icon::Unplug)
                                    .xsmall()
                                    .text_color(unreached_ink),
                            ),
                    )
                })
                .when_some(held, |row, (tip, stopped)| {
                    let ink = match stopped {
                        true => warning,
                        false => unreached_ink,
                    };
                    row.child(
                        div()
                            .id(("project-held", root_idx))
                            .flex_none()
                            .role(gpui::accesskit::Role::Image)
                            .aria_label(tip.clone())
                            .tooltip(covered_tip(&key, move |window, cx| {
                                Tooltip::new(tip.clone()).build(window, cx)
                            }))
                            .child(Icon::new(crate::icons::Icon::Lock).xsmall().text_color(ink)),
                    )
                })
                .when_some(to_merge, |row, tip| {
                    row.child(
                        div()
                            .id(("project-to-merge", root_idx))
                            .flex_none()
                            .role(gpui::accesskit::Role::Image)
                            .aria_label(tip.clone())
                            .tooltip(covered_tip(&key, move |window, cx| {
                                Tooltip::new(tip.clone()).build(window, cx)
                            }))
                            .child(
                                Icon::new(crate::icons::Icon::GitMerge)
                                    .xsmall()
                                    .text_color(unreached_ink),
                            ),
                    )
                })
                .when_some(git, |row, git| {
                    let branch = git.branch.clone();
                    row.child(
                        div()
                            .id(("project-branch", root_idx))
                            .min_w_0()
                            .max_w(MAX_BRANCH_W)
                            .truncate()
                            .tooltip(covered_tip(&key, move |window, cx| {
                                Tooltip::new(format!("Branch: {branch}")).build(window, cx)
                            }))
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(crate::tokens::CODE_TEXT)
                            .child(git.branch.clone()),
                    )
                    .children(git.parts().into_iter().map(
                        |(part, n, words)| {
                            let id = SharedString::from(format!("{key}-{part:?}"));
                            git_part(gpui::ElementId::Name(id), part, n, words, &key, meta_ink)
                        },
                    ))
                })
                // Position alone does not say a project is pinned: first in
                // the list is where a project can also be by accident.
                .when(pinned, |row| row.child(Icon::new(IconName::Star).xsmall())),
        )
        .children(folded.filter(|_| !open).map(|signal| {
            let label = format!("{}: {}", root.label, signal_hint(Some(signal)));
            status_mark(
                ("badge", root_idx).into(),
                Some(signal),
                label.into(),
                Some(&key),
                cx,
            )
        }))
        // Over the end of the row, in the place of its meta, on its fill, so
        // they take no room from the name while hidden: for the pointer, and
        // for the keyboard on its row.
        .child(
            covering(div().id(("project-actions", root_idx)), &key)
                .debug_selector(move || format!("rail-project-actions-{root_idx}"))
                .h_flex()
                .items_center()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .px_1()
                .rounded(radius)
                .bg(rest)
                .when(!ring, |d| d.invisible())
                .group_hover(key.clone(), move |s| s.visible().bg(hovered))
                // What of the meta they leave uncovered fades into them
                // rather than ending on a cut glyph, and still answers the
                // pointer.
                .child(fade_band(key.clone(), rest, hovered).right_full())
                .child(labelled(
                    ("project-new-name", root_idx),
                    "New session in this project",
                    row_control(("project-new", root_idx), IconName::Plus, cx)
                        .named("New session in this project")
                        .on_click(cx.listener(move |shell: &mut Shell, _, window, cx| {
                            shell.new_session_in(root_idx, window, cx);
                        })),
                ))
                .child(labelled(
                    ("project-menu-name", root_idx),
                    "Project actions",
                    menu_button(
                        row_control(("project-menu", root_idx), IconName::Ellipsis, cx),
                        "What can be done with this project",
                        project_menu(root_idx, facts, asks, shell.clone()),
                    ),
                )),
        )
        .when(ring, |row| row.child(key_ring(cx)))
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
            project_menu(root_idx, facts, asks, shell.clone())(menu, window, cx)
        })
        .into_any_element()
}
