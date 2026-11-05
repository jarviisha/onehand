//! Files mode: the active root's directory tree.
//!
//! The flatten, the sort, the caps and the `.git` skip are all core's
//! (`onehand_core::tree`); the git badges come from the same `GitStatus` the
//! rail reads. This module is the state behind the tree, the drawing and the
//! click plumbing.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::gitstat::{FileChange, GitStatus};
use onehand_core::tree::{FileTree, TreeEntry};
use onehand_ui::{hint, mark_slot, menu_item, status_ink, status_line, tokens, tree_row};
use onehand_workbench::{Ask, Disk, Request};
use std::collections::{BTreeMap, HashMap};
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
    /// Where each root's files are read from.
    disks: HashMap<PathBuf, Disk>,
    /// The folders whose listing could not be read, by root, and why: a
    /// folder silently missing reads as a folder that is empty. Kept per
    /// folder because a rescan lists the root and every open folder at once,
    /// and the root listing must not answer a folder's failure. Cleared when
    /// that folder lists, or is folded away.
    failed: HashMap<PathBuf, BTreeMap<PathBuf, String>>,
    git: HashMap<PathBuf, GitStatus>,
    /// How a click on a file reaches the mode that edits files. Opening one is
    /// the quick editor's business, and this tree has no way to reach it.
    ask: Ask,
    /// The file open in the editor beside the tree: its row is the tree's
    /// selected one, as the open document is the Markdown list's.
    active: Option<PathBuf>,
    /// The offloaded roots, each with its host: a row of one offers to send
    /// itself there.
    offloaded: HashMap<PathBuf, String>,
}

impl FilesView {
    pub(crate) fn new(ask: Ask, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            trees: HashMap::new(),
            disks: HashMap::new(),
            failed: HashMap::new(),
            git: HashMap::new(),
            ask,
            active: None,
            offloaded: HashMap::new(),
        })
    }

    /// Mark `path` as the file open beside the tree.
    pub(crate) fn set_active(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active != path {
            self.active = path;
            cx.notify();
        }
    }

    /// Point the tree at a project root and the disk its files are on,
    /// seeding it on first sight.
    pub(crate) fn set_root(&mut self, root: &Path, disk: &Disk, cx: &mut Context<Self>) {
        // Kept even for the root already on screen: a host's attach can be
        // replaced while its project stays.
        self.disks.insert(root.to_path_buf(), disk.clone());
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
        self.disks.remove(root);
        self.git.remove(root);
        self.failed.remove(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    pub(crate) fn set_offloaded(
        &mut self,
        roots: &HashMap<PathBuf, String>,
        cx: &mut Context<Self>,
    ) {
        if &self.offloaded != roots {
            self.offloaded = roots.clone();
            cx.notify();
        }
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
        let Some(disk) = self.disks.get(&root).cloned() else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { disk.read_dir(&dir) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| view.listed(root, dir, read, cx));
        })
        .detach();
    }

    /// One folder's listing landed, or said why it could not.
    fn listed(
        &mut self,
        root: PathBuf,
        dir: PathBuf,
        read: Result<(Vec<TreeEntry>, usize), String>,
        cx: &mut Context<Self>,
    ) {
        match read {
            Ok((listing, cut)) => {
                self.unfail(&root, &dir);
                if let Some(tree) = self.trees.get_mut(&root) {
                    tree.cut.insert(dir.clone(), cut);
                    tree.listings.insert(dir, listing);
                }
            }
            Err(why) => {
                self.failed.entry(root).or_default().insert(dir, why);
            }
        }
        cx.notify();
    }

    fn unfail(&mut self, root: &Path, dir: &Path) {
        if let Some(failed) = self.failed.get_mut(root) {
            failed.remove(dir);
            if failed.is_empty() {
                self.failed.remove(root);
            }
        }
    }

    /// A row pressed: a folder folds or unfolds, a file opens beside the tree.
    ///
    /// By the kind the listing gave, not by asking the disk again: that would
    /// be a stat on the UI loop, and a host's path is not on this machine.
    fn click(&mut self, entry: &TreeEntry, window: &mut Window, cx: &mut Context<Self>) {
        if entry.is_dir {
            self.toggle_dir(entry.path.clone(), cx);
        } else {
            (self.ask)(&Request::OpenFile(&entry.path), window, cx);
        }
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
        } else {
            // Folded away, its failure has nothing left on screen to explain.
            self.unfail(&root, &dir);
        }
        cx.notify();
    }
}

impl Render for FilesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(root) = self.root.clone() else {
            return hint("No project selected", cx).into_any_element();
        };
        let Some(tree) = self.trees.get(&root) else {
            return hint("No project selected", cx).into_any_element();
        };
        // One line, the first failure's: a folded folder takes its failure
        // with it, so the next one shows once the first is answered.
        let failure = self
            .failed
            .get(&root)
            .and_then(|failed| failed.values().next())
            .map(|why| status_line(why.clone(), status_ink(cx).danger));
        // A root that never listed has no rows for the line to sit under.
        if !tree.listings.contains_key(&root)
            && let Some(failure) = failure
        {
            return div().flex_1().min_h_0().child(failure).into_any_element();
        }
        // Until the root's first listing lands there is nothing to draw, and a
        // blank panel reads as an empty project.
        if !tree.listings.contains_key(&root) {
            return hint("Reading the project's files…", cx).into_any_element();
        }
        // The failure stands under the rows rather than over them, so the
        // folder it names can still be folded away.
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(div().flex_1().min_h_0().child(rows(
                &root,
                tree,
                self.git.get(&root),
                self.active.as_deref(),
                cx.listener(|view: &mut Self, entry: &TreeEntry, window, cx| {
                    view.click(entry, window, cx)
                }),
                self.offloaded.get(&root).map(|host| Send {
                    host: host.clone(),
                    ask: self.ask.clone(),
                }),
                cx,
            )))
            .children(failure)
            .into_any_element()
    }
}

/// The ink a change's letter takes. Severity, not category: red is something
/// that lost content, amber is something that changed, green is something
/// that arrived.
fn change_color(change: FileChange, cx: &App) -> gpui::Hsla {
    let status = status_ink(cx);
    match change {
        FileChange::Modified => status.warning,
        FileChange::Added | FileChange::Untracked | FileChange::Renamed => status.success,
        FileChange::Deleted | FileChange::Conflicted => status.danger,
    }
}

/// A row's *Send to <host>*, on an offloaded root.
struct Send {
    host: String,
    ask: Ask,
}

/// The trailing slot a row's change mark stands in, held on every row so the
/// marks run down one column.
const MARK_W: gpui::Rems = gpui::rems(1.);

/// Draw the tree for `root`, the file open beside it, `active`, picked.
///
/// `git` is the root's status, if it is a repo at all — a non-repo root simply
/// draws no marks rather than an empty column.
fn rows(
    root: &Path,
    tree: &FileTree,
    git: Option<&GitStatus>,
    active: Option<&Path>,
    on_click: impl Fn(&TreeEntry, &mut Window, &mut App) + 'static,
    send: Option<Send>,
    cx: &App,
) -> gpui::AnyElement {
    let send = send.map(std::rc::Rc::new);
    let (rows, left_out) = tree.visible_rows(root);
    let on_click = std::rc::Rc::new(on_click);
    let muted = cx.theme().muted_foreground;

    div()
        .id("file-tree")
        .v_flex()
        .size_full()
        .py_2()
        .overflow_y_scroll()
        .children(rows.into_iter().enumerate().map(|(i, row)| {
            let entry = row.entry;
            let click = on_click.clone();
            let expanded = tree.expanded.contains(&entry.path);
            let picked = active == Some(entry.path.as_path());

            // Marks are keyed by repo-relative path, which is the only form
            // `git status --porcelain` ever reports.
            let rel = entry.path.strip_prefix(root).unwrap_or(&entry.path);
            let change = git.and_then(|status| {
                if entry.is_dir {
                    status.dir_change(rel)
                } else {
                    status.file_change(rel)
                }
            });
            let mark = Icon::new(match (entry.is_dir, expanded) {
                (true, true) => IconName::ChevronDown,
                (true, false) => IconName::ChevronRight,
                (false, _) => IconName::File,
            })
            .small()
            .when(!picked, |icon| icon.text_color(muted));

            // Indent by depth, not by nested containers: a 600-row tree (the
            // core cap) would otherwise be 600 nested elements.
            tree_row(("tree-row", i), row.depth as usize, picked, cx)
                .child(mark_slot(mark))
                // The name in the row's own ink, changed or not: the letter at
                // the end says how it changed.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(entry.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .w(MARK_W)
                        .h_flex()
                        .justify_center()
                        .when_some(change, |slot, change| {
                            let color = change_color(change, cx);
                            // A folder carries a dot for "something under here
                            // changed"; only a file names how. The dot is drawn,
                            // not typed, so it sits on the row's centre.
                            slot.child(if entry.is_dir {
                                div().size_1p5().rounded_full().bg(color).into_any_element()
                            } else {
                                div()
                                    .text_xs()
                                    .text_color(color)
                                    .child(change.badge().to_string())
                                    .into_any_element()
                            })
                        }),
                )
                .on_click({
                    let entry = entry.clone();
                    move |_, window, cx: &mut App| click(&entry, window, cx)
                })
                // Only on an offloaded root; the library opens no menu for
                // an empty list.
                .context_menu({
                    let (send, path) = (send.clone(), entry.path.clone());
                    move |menu, _, _| match &send {
                        Some(send) => {
                            let (ask, path) = (send.ask.clone(), path.clone());
                            menu.item(menu_item(format!("Send to {}", send.host)).on_click(
                                move |_, window, cx| ask(&Request::SendToHost(&path), window, cx),
                            ))
                        }
                        None => menu,
                    }
                })
        }))
        // The cap is core's, and a tree that silently stops is a tree the user
        // thinks they have seen all of.
        .when(left_out > 0, |tree| {
            tree.child(
                div()
                    .pl(tokens::ROW_INSET)
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{left_out} more not shown")),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests;
