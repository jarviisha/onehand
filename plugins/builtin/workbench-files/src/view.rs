//! Files mode: the active root's directory tree.
//!
//! The flatten, the sort, the caps and the `.git` skip are all core's
//! (`onehand_core::tree`); the git badges come from the same `GitStatus` the
//! rail reads. This module is the state behind the tree, the drawing and the
//! click plumbing.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::gitstat::{FileChange, GitStatus};
use onehand_core::tree::{self, FileTree};
use onehand_plugin_host::{Ask, Request, hint, status_ink};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The tree of every project root this window has visited, and the one on
/// screen.
///
/// Keyed by root the way everything else in the Workbench is, so switching
/// projects swaps the whole tree rather than mixing one project's folders with
/// another's.
pub(crate) struct FilesView {
    root: Option<PathBuf>,
    trees: HashMap<PathBuf, FileTree>,
    git: HashMap<PathBuf, GitStatus>,
    /// How a click on a file reaches the mode that edits files. Opening one is
    /// the quick editor's business, and this tree has no way to reach it.
    ask: Ask,
}

impl FilesView {
    pub(crate) fn new(ask: Ask, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            trees: HashMap::new(),
            git: HashMap::new(),
            ask,
        })
    }

    /// Point the tree at a project root, seeding it on first sight.
    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        if self.trees.contains_key(root) {
            // A root seen before: its cached listings are as old as the last
            // time it was on screen, and the agent has been working since.
            self.rescan(cx);
        } else {
            self.trees.insert(root.to_path_buf(), FileTree::default());
            self.scan(root.to_path_buf(), cx);
        }
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.trees.remove(root);
        self.git.remove(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    pub(crate) fn set_git(&mut self, git: &HashMap<PathBuf, GitStatus>, cx: &mut Context<Self>) {
        self.git = git.clone();
        cx.notify();
    }

    /// Re-read every directory currently on screen for the active root.
    ///
    /// The tree was seeded once per root and never refreshed, so a file the
    /// agent had just created did not appear until the user folded and unfolded
    /// its directory by hand. Called when a turn ends and when the window is
    /// activated — the two moments the tree is most likely to have moved under
    /// it.
    ///
    /// Bounded by what is *visible*: the root plus its expanded directories,
    /// which is exactly what `visible_rows` draws from. A collapsed subtree is
    /// rescanned when it is opened, as it always was.
    pub(crate) fn rescan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(tree) = self.trees.get(&root) else {
            return;
        };
        let dirs: Vec<PathBuf> = std::iter::once(root)
            .chain(tree.expanded.iter().cloned())
            .collect();
        for dir in dirs {
            self.scan(dir, cx);
        }
    }

    /// Read one directory off the UI loop and cache its listing.
    fn scan(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let listing = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { tree::read_dir_sorted(&dir) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                if let Some(tree) = view.trees.get_mut(&root) {
                    tree.listings.insert(dir, listing);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fold or unfold a directory. Expanding rescans — a cached listing can be
    /// minutes old, and the agent has been writing to this tree the whole time.
    fn toggle_dir(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let expanding = self
            .trees
            .get_mut(&root)
            .map(|tree| {
                if tree.expanded.remove(&dir) {
                    false
                } else {
                    tree.expanded.insert(dir.clone());
                    true
                }
            })
            .unwrap_or(false);
        if expanding {
            self.scan(dir, cx);
        }
        cx.notify();
    }
}

impl Render for FilesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx).into_any_element();
        };
        let Some(tree) = self.trees.get(&root) else {
            return hint("No project root", cx).into_any_element();
        };
        div()
            .flex_1()
            .min_h_0()
            .child(rows(
                &root,
                tree,
                self.git.get(&root),
                self.ask.clone(),
                cx.listener(|view: &mut Self, dir: &PathBuf, _, cx| {
                    view.toggle_dir(dir.clone(), cx)
                }),
                cx,
            ))
            .into_any_element()
    }
}

/// The tint a change badge carries. Severity, not category: red is something
/// that lost content, amber is something that changed, green is something that
/// arrived.
fn change_color(change: FileChange, cx: &App) -> gpui::Hsla {
    let status = status_ink(cx);
    match change {
        FileChange::Modified => status.warning,
        FileChange::Added | FileChange::Untracked | FileChange::Renamed => status.success,
        FileChange::Deleted | FileChange::Conflicted => status.danger,
    }
}

/// Draw the tree for `root`.
///
/// `git` is the root's status, if it is a repo at all — a non-repo root simply
/// draws no badges rather than an empty column.
fn rows(
    root: &Path,
    tree: &FileTree,
    git: Option<&GitStatus>,
    ask: Ask,
    on_toggle: impl Fn(&PathBuf, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let (rows, truncated) = tree.visible_rows(root);
    let on_toggle = std::rc::Rc::new(on_toggle);

    div()
        .id("file-tree")
        .v_flex()
        .size_full()
        .p_1()
        .overflow_y_scroll()
        .children(rows.into_iter().enumerate().map(|(i, row)| {
            let entry = row.entry;
            let (toggle, ask) = (on_toggle.clone(), ask.clone());
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
                        ask(&Request::OpenFile(&path), window, cx);
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
