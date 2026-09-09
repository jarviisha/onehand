//! The Markdown mode's own state: which project it is looking at, what the
//! walk found, and what is being read.

use crate::document::{RootDocs, list, reader};
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};
use gpui_component::{ActiveTheme, StyledExt, h_resizable, resizable_panel};
use onehand_plugin_host::{Ask, Request, hint, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often the open document's file is asked whether it has changed.
///
/// One `stat` at this rate, and only while a document is actually open — the
/// re-read and the re-parse happen on the ticks where the answer moved, which
/// for a file nobody is writing is none of them. Fast enough that a document
/// the agent is editing reads as live rather than as stale.
const DOC_POLL: Duration = Duration::from_millis(750);

/// The document list's width before anybody drags it, and the range a drag may
/// take it through.
///
/// Pixels rather than rems because that is the only thing the split accepts,
/// the same as the rail's own range. Its own numbers and not the rail's: this
/// column holds file names inside a dock the user has already sized, so the
/// floor is what a name needs to be readable at and the ceiling is the point
/// past which the list is taking the room the document was opened for.
const DOC_LIST_W: f32 = 220.;
const DOC_LIST_MIN: f32 = 140.;
const DOC_LIST_MAX: f32 = 420.;

pub(crate) struct MarkdownView {
    root: Option<PathBuf>,
    /// The index and the document being read, per root.
    docs: HashMap<PathBuf, RootDocs>,
    /// Whether the document list is drawn beside the document.
    ///
    /// Per window rather than per root, unlike everything else here: how much
    /// room the reading gets is a preference about this panel, and one that
    /// reset itself on every project switch would be the odd one out. Not
    /// persisted either, for the reason the rail's own visibility is not: a
    /// panel that came back with its list gone reads as one that lost it.
    list_shown: bool,
    /// Where the drag between the list and the document sits.
    ///
    /// Held here rather than left to the element, for the reason the window's
    /// rail split is held by the shell: a width has to outlive the frames its
    /// panel is not drawn in, and this one is not drawn whenever the list is
    /// hidden. Kept out of the split entirely while the list is hidden, so the
    /// group never renders holding one panel — that truncates the state to one
    /// size and loses the width the user chose.
    split: Entity<gpui_component::ResizableState>,
    /// Whether the index needs walking again before it is next drawn.
    ///
    /// The walk is the whole project, so it is deliberately not run when the
    /// root changes or a turn ends — only when this mode is about to be *seen*
    /// after one of those. A workspace with a dozen roots must not walk a dozen
    /// projects for a mode nobody has opened, and the render is the one moment
    /// that is known not to be the case.
    stale: bool,
    /// The document walk in flight, held only so that starting another drops
    /// it.
    ///
    /// Underscored because nothing reads it and nothing should: what it is for
    /// is its `Drop`, which cancels a walk whose answer is already out of date.
    _scan: Option<gpui::Task<()>>,
    /// The `stat` loop behind the live reload.
    ///
    /// Held rather than detached, and that is what stops it: the task ends when
    /// this is cleared, which is how a document being closed or a root being
    /// removed takes its polling with it. One for the mode and not one per
    /// document, because only the active root's document is on screen.
    watch: Option<gpui::Task<()>>,
    /// A document that could not be read, shown as a line under the reading.
    ///
    /// **Deliberately not a notification**, unlike the rest of the app's
    /// transient status. A file that has outgrown the read's size bound or gone
    /// missing is a standing condition rather than news, and a toast that fades
    /// leaves the reader believing the document on screen is current. Cleared
    /// by a read that works.
    status: Option<String>,
    /// How the header's *Edit source* reaches the mode that edits files.
    ask: Ask,
}

impl MarkdownView {
    pub(crate) fn new(ask: Ask, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            root: None,
            docs: HashMap::new(),
            list_shown: true,
            split: cx.new(|_| gpui_component::ResizableState::default()),
            stale: false,
            _scan: None,
            watch: None,
            status: None,
            ask,
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        // The document being watched belonged to the root being left. Whether
        // the arriving one has one of its own is decided by the poll itself,
        // once its state is in hand.
        self.watch = None;
        self.watch_doc(cx);
        self.stale = true;
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.docs.remove(root);
        if self.root.as_deref() == Some(root) {
            // The document that was being watched belonged to this root, and
            // the poll would otherwise go on asking about a file in a project
            // no longer in the workspace.
            self.watch = None;
            self.root = None;
        }
        cx.notify();
    }

    /// Note that the index is out of date. A turn has ended and the agent has
    /// been writing since the walk, and a file it just wrote is exactly what
    /// somebody switches to this mode to read.
    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        cx.notify();
    }

    /// Walk the active root for markdown documents.
    ///
    /// Re-walked rather than merged into what is there: a document deleted
    /// since the last walk has to leave the list, and a walk that only ever
    /// added would keep a row that opens nothing.
    ///
    /// The state is made **here**, while the root is known to be active, and
    /// the walk that lands later only fills it in. An entry created on the way
    /// back would resurrect a root the workspace had removed in the meantime,
    /// and leave its index and its parsed document alive for as long as the
    /// window is.
    ///
    /// The task is **held**, so starting a walk drops whichever was still
    /// running. Two walks of one project cost twice for one answer, and the
    /// slower of them can land last and put a staler index on screen.
    fn index(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.docs.entry(root.clone()).or_default();
        self._scan = Some(cx.spawn(async move |view, cx| {
            let index = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { crate::index::scan_blocking(&root) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(docs) = view.docs.get_mut(&root) else {
                    return;
                };
                docs.index = Some(index);
                cx.notify();
            });
        }));
    }

    /// Show or hide the document list beside the document.
    fn toggle_list(&mut self, cx: &mut Context<Self>) {
        self.list_shown = !self.list_shown;
        cx.notify();
    }

    /// Fold or unfold a directory in the document list.
    fn toggle_dir(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if let Some(root) = self.root.clone()
            && let Some(docs) = self.docs.get_mut(&root)
        {
            docs.toggle(dir);
            cx.notify();
        }
    }

    /// Read a document and put it on screen.
    ///
    /// Through core's editor read, so this obeys the same size bound the quick
    /// editor does and comes back with the same mtime — which is what the live
    /// reload then compares against.
    fn open_doc(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let label = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();

        cx.spawn(async move |view, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::read_blocking(&path) }
                })
                .await;

            let _ = view.update(cx, |view: &mut Self, cx| {
                // The root can have left the workspace while the read was in
                // flight, and putting the entry back would leave a parsed
                // document alive under a project nothing can reach.
                if !view.docs.contains_key(&root) {
                    return;
                }
                match read {
                    Ok((text, mtime)) => {
                        // A read that worked answers whatever the last failure
                        // said, the same way opening a file in the editor does.
                        view.status = None;
                        if let Some(docs) = view.docs.get_mut(&root) {
                            docs.show(path, label, &text, mtime, cx);
                        }
                        view.watch_doc(cx);
                    }
                    Err(e) => view.status = Some(format!("{} — {e}", path.display())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Keep one poll running for as long as a document is open, and no longer.
    ///
    /// Dropping the task is what stops it, so this is called from both ends —
    /// after a document opens, and after the active root changes to one that
    /// may have none.
    fn watch_doc(&mut self, cx: &mut Context<Self>) {
        let open = self
            .root
            .as_ref()
            .and_then(|root| self.docs.get(root))
            .is_some_and(|docs| docs.open.is_some());
        if !open {
            self.watch = None;
            return;
        }
        if self.watch.is_some() {
            return;
        }

        self.watch = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(DOC_POLL).await;
                // The view is asked afresh every tick rather than closing over
                // a path: the reader can have moved to another document, or to
                // another project, since the last one.
                let Ok(open) = view.update(cx, |view: &mut Self, _| {
                    view.root
                        .as_ref()
                        .and_then(|root| view.docs.get(root))
                        .and_then(|docs| docs.open.as_ref())
                        .map(|doc| (doc.path.clone(), doc.mtime))
                }) else {
                    // The view is gone, and with it the window.
                    return;
                };
                let Some((path, seen)) = open else {
                    continue;
                };

                let fresh = cx
                    .background_executor()
                    .spawn({
                        let path = path.clone();
                        async move {
                            std::fs::metadata(&path)
                                .ok()
                                .and_then(|meta| meta.modified().ok())
                        }
                    })
                    .await;
                if fresh == seen {
                    continue;
                }

                let read = cx
                    .background_executor()
                    .spawn({
                        let path = path.clone();
                        async move { onehand_core::editor::read_blocking(&path) }
                    })
                    .await;

                let updated = view.update(cx, |view: &mut Self, cx| {
                    let Some(root) = view.root.clone() else {
                        return;
                    };
                    let Some(docs) = view.docs.get_mut(&root) else {
                        return;
                    };
                    let failed = match read {
                        Ok((text, mtime)) => {
                            docs.refresh(&path, &text, mtime, cx);
                            None
                        }
                        // A file that grew past the size bound, or that was
                        // deleted out from under the reader. The new stamp is
                        // recorded anyway, or this same failure would be
                        // reported again on every tick from here on — and it is
                        // said only while the stamp landed, since a failure
                        // about a document the reader has already moved off is a
                        // standing warning naming a file that is not on screen.
                        Err(e) => docs
                            .stamp(&path, fresh)
                            .then(|| format!("{} — {e}", path.display())),
                    };
                    if let Some(message) = failed {
                        view.status = Some(message);
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
        }));
    }
}

impl Render for MarkdownView {
    /// The document list beside the document, which is the whole mode.
    ///
    /// The list keeps a third of the panel and the document the rest: the rows
    /// are file names and the document is prose, so the two do not want the
    /// same share of a dock the user has already sized for reading.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Being drawn is what says the walk is worth its cost, so it is started
        // here rather than at the moment the index went stale.
        if self.stale {
            self.stale = false;
            self.index(cx);
        }

        let body = self.body(window, cx);
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.status.clone().map(|status| status_line(status, cx)))
    }
}

impl MarkdownView {
    fn body(&mut self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx);
        };
        let Some(docs) = self.docs.get(&root) else {
            // The walk was asked for and has not landed; the list says so for
            // itself once there is one.
            return hint("Looking for documents…", cx);
        };

        let reader = {
            let path = docs.open.as_ref().map(|doc| doc.path.clone());
            let ask = self.ask.clone();
            reader(
                docs.open.as_ref(),
                self.list_shown,
                // The rem base in force, which is the panel's zoom: this body
                // is drawn inside the override, so the window is already
                // answering with the zoomed value. The renderer sizes its
                // headings from an absolute pixel value, which is the one thing
                // that override cannot reach by itself.
                window.rem_size(),
                cx.listener(|view: &mut Self, _, _, cx| view.toggle_list(cx)),
                move |_, window, cx: &mut App| {
                    // Handing it to the editor switches the mode, which is the
                    // honest answer: this one does not edit.
                    if let Some(path) = path.clone() {
                        ask(&Request::OpenFile(&path), window, cx);
                    }
                },
                cx,
            )
        };

        if !self.list_shown {
            // No list, so no split: a group holding one panel draws a handle
            // against the panel's own edge that resizes nothing. The same
            // reason the window's rail split is skipped while the rail is
            // hidden.
            return div()
                .flex_1()
                .min_h_0()
                // A row, but deliberately not the shared `h_flex`: that one
                // centres its children, which leaves each column as tall as its
                // own content instead of as tall as the panel. The document's
                // body is sized by what is left over, so centred it is given
                // nothing and the header floats in the middle of an empty panel.
                .flex()
                .flex_row()
                .child(reader)
                .into_any_element();
        }

        let list = div()
            .size_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(list(
                &root,
                docs,
                cx.listener(|view: &mut Self, dir: &PathBuf, _, cx| view.toggle_dir(dir, cx)),
                cx.listener(|view: &mut Self, path: &PathBuf, _, cx| {
                    view.open_doc(path.clone(), cx)
                }),
                cx,
            ));

        div()
            .flex_1()
            .min_h_0()
            .child(
                h_resizable("markdown-split")
                    .with_state(&self.split)
                    .child(
                        // `flex_none`, as the rail's own panel is: the group
                        // sets `flex_grow: 1` on a panel, and a list that grows
                        // takes whatever the document is not using — which is
                        // most of the panel. The width here is only the one it
                        // starts at; once dragged, the split's own state is
                        // what answers.
                        resizable_panel()
                            .size(gpui::px(DOC_LIST_W))
                            .size_range(gpui::px(DOC_LIST_MIN)..gpui::px(DOC_LIST_MAX))
                            .flex_none()
                            .child(list),
                    )
                    .child(resizable_panel().child(reader)),
            )
            .into_any_element()
    }
}
