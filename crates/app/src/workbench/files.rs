//! Files mode: the active root's directory tree.
//!
//! The flatten, the sort, the caps and the `.git` skip are all core's
//! (`onehand_core::tree`); the git badges come from the same `GitStatus` the
//! rail reads. This module is the drawing and the click plumbing.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    Window, div, px,
};
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::gitstat::{FileChange, GitStatus};
use onehand_core::tree::FileTree;
use std::path::{Path, PathBuf};

/// The tint a change badge carries. Severity, not category: red is something
/// that lost content, amber is something that changed, green is something that
/// arrived.
fn change_color(change: FileChange, cx: &App) -> gpui::Hsla {
    let status = crate::theme::status_ink(cx);
    match change {
        FileChange::Modified => status.warning,
        FileChange::Added | FileChange::Untracked | FileChange::Renamed => status.success,
        FileChange::Deleted | FileChange::Conflicted => status.danger,
    }
}

/// Render the tree for `root`.
///
/// `git` is the root's status, if it is a repo at all — a non-repo root simply
/// draws no badges rather than an empty column.
pub fn tree(
    root: &Path,
    tree: &FileTree,
    git: Option<&GitStatus>,
    on_toggle: impl Fn(&PathBuf, &mut Window, &mut App) + 'static,
    on_open: impl Fn(&PathBuf, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let (rows, truncated) = tree.visible_rows(root);
    let on_toggle = std::rc::Rc::new(on_toggle);
    let on_open = std::rc::Rc::new(on_open);

    div()
        .id("file-tree")
        .v_flex()
        .size_full()
        .p_1()
        .overflow_y_scroll()
        .children(rows.into_iter().enumerate().map(|(i, row)| {
            let entry = row.entry;
            let (toggle, open) = (on_toggle.clone(), on_open.clone());
            let path = entry.path.clone();
            let expanded = tree.expanded.contains(&entry.path);

            // Badges are keyed by repo-relative path, which is the only form
            // `git status --porcelain` ever reports.
            let rel = entry.path.strip_prefix(root).unwrap_or(&entry.path);
            let change = git.and_then(|status| {
                if entry.is_dir {
                    status.dir_change(rel)
                } else {
                    status.file_change(rel)
                }
            });

            div()
                .id(("tree-row", i))
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .h_6()
                .px_1()
                .rounded(cx.theme().radius)
                .text_sm()
                .cursor_pointer()
                .hover(|r| r.bg(cx.theme().accent.opacity(0.5)))
                // Indent by depth, not by nested containers: a 600-row tree
                // (the core cap) would otherwise be 600 nested elements.
                .pl(px(4. + row.depth as f32 * 12.))
                .child(
                    Icon::new(if entry.is_dir {
                        if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        }
                    } else {
                        IconName::File
                    })
                    .size_3(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when_some(change, |name, change| {
                            name.text_color(change_color(change, cx))
                        })
                        .child(entry.name.clone()),
                )
                .when_some(change, |row, change| {
                    let color = change_color(change, cx);
                    // A directory carries a dot for "something under here
                    // changed"; only a file names *how*. The dot is drawn, not
                    // typed: a bullet character is an icon wearing text's
                    // clothes, and it sits on the text baseline instead of the
                    // row's centre.
                    row.child(if entry.is_dir {
                        div()
                            .flex_none()
                            .size(px(6.))
                            .rounded_full()
                            .bg(color)
                            .into_any_element()
                    } else {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(color)
                            .child(change.badge().to_string())
                            .into_any_element()
                    })
                })
                .on_click(move |_, window, cx: &mut App| {
                    if entry_is_dir(&path) {
                        toggle(&path, window, cx);
                    } else {
                        open(&path, window, cx);
                    }
                })
        }))
        .when(truncated, |tree| {
            // The cap is core's, and a tree that silently stops is a tree the
            // user thinks they have seen all of.
            tree.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("… more entries not shown"),
            )
        })
        .into_any_element()
}

/// Whether the click target is a directory.
///
/// Read back from the path rather than captured from the row: the closure
/// outlives this frame's borrow of the tree.
fn entry_is_dir(path: &Path) -> bool {
    path.is_dir()
}
