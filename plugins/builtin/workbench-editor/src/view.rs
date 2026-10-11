//! The Editor mode's own state: the open buffers per project root, and the
//! rules that keep a save from clobbering somebody else's write.

use crate::buffers::{Lead, RootBuffers, StripHandlers, body, new_buffer, save_status, tab_strip};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use gpui_component::input::InputEvent;
use gpui_component::{StyledExt, WindowExt as _};
use onehand_core::editor::SaveOutcome;
use onehand_plugin_host::{hint, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(crate) struct EditorView {
    root: Option<PathBuf>,
    buffers: HashMap<PathBuf, RootBuffers>,
    /// A save conflict or a read failure, shown as a line under the body.
    ///
    /// **Deliberately not a notification**, unlike the rest of the app's
    /// transient status. "Changed on disk — nothing was written" is not news
    /// that may be missed: it is a standing condition, and the next `Ctrl+S` is
    /// what resolves it. A toast that fades leaves the user believing the save
    /// went through. Cleared by whatever answers it — a successful save, or a
    /// successful open.
    status: Option<String>,
    /// Tabs with a write in flight, and whether another save was asked for
    /// while it ran.
    ///
    /// Two saves of one tab must not overlap: each captures `disk_mtime` at
    /// spawn, so the second carries the time from *before* the first wrote and
    /// comes back as a conflict against our own write. The second press is not
    /// dropped either — it is remembered here and re-run once the first lands,
    /// which is what makes it pick up the keystrokes that prompted it.
    saving: HashMap<u64, bool>,
    /// The width the strip leaves its tabs, in rems, as last laid out.
    /// Infinite until measured, so the first frame draws tabs, not a select.
    tabs_w: onehand_plugin_host::Measured,
    /// Whether the file tree beside the buffers is showing.
    ///
    /// Held here rather than on the split that draws the tree, because the
    /// control that flips it sits on this view's tab strip and has to show
    /// which way round it is; the split observes this view and reads it. One
    /// flag for every project, not per root, and not persisted — the same
    /// answer as the divider's position, for the same reason.
    tree_shown: bool,
    /// Whether the file shows rather than the tree, while the two are too
    /// narrow to sit side by side. Opening a file sets it and the way back
    /// clears it; what is open is kept either way.
    detail: bool,
    /// Whether the tree and the buffers are shown one at a time, as the split
    /// last measured: the strip leads with the way back to the tree then,
    /// rather than the toggle that hides it.
    alone: bool,
}

impl EditorView {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            buffers: HashMap::new(),
            status: None,
            saving: HashMap::new(),
            tabs_w: onehand_plugin_host::unmeasured(),
            tree_shown: true,
            detail: false,
            alone: false,
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        cx.notify();
    }

    /// Drop everything held for `root`.
    ///
    /// Called when a project root leaves the workspace. Buffers go with it: an
    /// unsaved edit in a project the user just removed has nowhere to be saved
    /// *to* — the tab strip it belonged to is gone.
    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.buffers.remove(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// How many of `root`'s open files have edits a removal would discard.
    ///
    /// Asked by the shell *before* it removes a root, because that is the one
    /// place with a control to guard.
    pub(crate) fn unsaved(&self, root: &Path) -> usize {
        self.buffers
            .get(root)
            .map(RootBuffers::dirty_count)
            .unwrap_or(0)
    }

    /// Where the caret goes, if there is a buffer to put it in.
    pub(crate) fn caret(&self, cx: &App) -> Option<gpui::FocusHandle> {
        let buffers = self.current()?;
        let uid = buffers.tabs.active_file()?.uid;
        Some(buffers.buffer(uid)?.focus_handle(cx))
    }

    /// Open `path`, and say whether this mode took it.
    ///
    /// An already-open file is focused, never reloaded: the second click on a
    /// path in a tool card must not discard unsaved edits (core's
    /// `RootEditors::open` decides that, not this).
    ///
    /// **No line number, and nothing to take one from.** ACP's diff payload is
    /// `{ path, old_text, new_text }` with no hunk offsets, so the tool card
    /// that links here has no line to send. The other half — `path:line:col`
    /// tokens written in an agent's prose — needs the transcript to *detect*
    /// them first, which it does not do, and there is no parser waiting in core
    /// for it either.
    pub(crate) fn open(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(root) = self.root.clone() else {
            return false;
        };
        self.detail = true;
        let path = path.to_path_buf();

        if let Some(buffers) = self.buffers.get(&root)
            && buffers.tabs.index_of(&path).is_some()
        {
            self.buffers
                .entry(root)
                .or_default()
                .tabs
                .open(path, None, 0);
            cx.notify();
            return true;
        }

        // `spawn_in`, not `spawn`: building an `EditorState` needs a `&mut
        // Window`, and the read has to happen off the UI loop first.
        cx.spawn_in(window, async move |view, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::read_blocking(&path) }
                })
                .await;

            let _ = view.update_in(cx, |view: &mut Self, window, cx| match read {
                Ok((text, mtime)) => {
                    // A read that worked answers whatever the last failure said.
                    // Left set, the message outlived its subject and read as a
                    // complaint about the file now on screen.
                    view.status = None;
                    let uid = next_buffer_uid();
                    let state = new_buffer(&path, &text, window, cx);
                    // Typing is what makes a buffer dirty, and nothing was
                    // listening for it. Subscribed *after* `new_buffer`, whose
                    // `set_value` seeds the text -- gpui-component's
                    // `set_value` does not emit `Change`, but ordering it this
                    // way means a tab is never born dirty even if that changes
                    // upstream.
                    let watch = cx.subscribe(&state, {
                        let root = root.clone();
                        move |view: &mut Self, _, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change) {
                                view.mark_dirty(&root, uid, cx);
                            }
                        }
                    });
                    let buffers = view.buffers.entry(root).or_default();
                    // Core decides whether this is a new tab; a buffer is only
                    // adopted when it is, so a re-open keeps its edits.
                    let (_, is_new) = buffers.tabs.open(path.clone(), mtime, uid);
                    if is_new {
                        buffers.insert(uid, state, watch);
                    }
                    cx.notify();
                }
                Err(e) => {
                    view.status = Some(format!("{} — {e}", path.display()));
                    cx.notify();
                }
            });
        })
        .detach();
        true
    }

    /// Note that a buffer has been typed into.
    ///
    /// The one place `dirty` is ever set true; before this the amber tab dot
    /// was unreachable and every close discarded edits in silence.
    fn mark_dirty(&mut self, root: &Path, uid: u64, cx: &mut Context<Self>) {
        if let Some(buffers) = self.buffers.get_mut(root)
            && let Some(file) = buffers.tabs.files.iter_mut().find(|f| f.uid == uid)
            && !file.dirty
        {
            file.dirty = true;
            cx.notify();
        }
    }

    /// Save the active tab, refusing to clobber a concurrent writer.
    pub(crate) fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(uid) = self
            .buffers
            .get(&root)
            .and_then(|buffers| buffers.tabs.active_file())
            .map(|file| file.uid)
        else {
            return;
        };
        self.save_tab(root, uid, cx);
    }

    /// Write one tab, keyed by `uid` rather than by tab position so a re-run
    /// after an in-flight save still targets the buffer that asked for it.
    fn save_tab(&mut self, root: PathBuf, uid: u64, cx: &mut Context<Self>) {
        // Already writing this tab: remember that another save was asked for
        // and let the one in flight land first (see `saving`).
        if let Some(again) = self.saving.get_mut(&uid) {
            *again = true;
            return;
        }
        let Some(buffers) = self.buffers.get(&root) else {
            return;
        };
        let Some(file) = buffers.tabs.files.iter().find(|f| f.uid == uid) else {
            return;
        };
        let Some(state) = buffers.buffer(uid) else {
            return;
        };

        let (path, label, mtime) = (file.path.clone(), file.label.clone(), file.disk_mtime);
        let text = state.read(cx).text().to_string();
        self.saving.insert(uid, false);

        cx.spawn(async move |view, cx| {
            let outcome = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::save_blocking(&path, text, mtime) }
                })
                .await;

            let _ = view.update(cx, |view: &mut Self, cx| {
                view.status = save_status(&outcome, &label);
                let again = view.saving.remove(&uid).unwrap_or(false);
                // Read the live buffer before taking the tab set mutably: what
                // the dot should say depends on whether the user kept typing
                // while the write was in flight.
                let current = view
                    .buffers
                    .get(&root)
                    .and_then(|buffers| buffers.buffer(uid))
                    .map(|state| state.read(cx).text().to_string());

                if let Some(buffers) = view.buffers.get_mut(&root)
                    && let Some(file) = buffers.tabs.files.iter_mut().find(|f| f.uid == uid)
                {
                    match &outcome {
                        SaveOutcome::Saved { mtime, text } => {
                            file.disk_mtime = *mtime;
                            // The snapshot core hands back is the whole point of
                            // it carrying one: keystrokes that arrived during
                            // the write are still unsaved, and clearing the dot
                            // for them would be a lie the next close acts on.
                            file.dirty = current.is_some_and(|live| live != **text);
                        }
                        // Record the *current* on-disk time so the next
                        // save is an explicit, informed overwrite.
                        SaveOutcome::Conflict { disk_mtime } => file.disk_mtime = *disk_mtime,
                        SaveOutcome::Failed(_) => {}
                    }
                }
                if again {
                    view.save_tab(root, uid, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The buffers of the project on screen, if it has any.
    fn current(&self) -> Option<&RootBuffers> {
        self.buffers.get(self.root.as_ref()?)
    }

    fn current_mut(&mut self) -> Option<&mut RootBuffers> {
        self.buffers.get_mut(self.root.as_ref()?)
    }

    pub(crate) fn tree_shown(&self) -> bool {
        self.tree_shown
    }

    /// Whether a file is what shows while the halves are one at a time: one
    /// asked for, and one there to show.
    pub(crate) fn showing_file(&self) -> bool {
        self.detail
            && self
                .current()
                .is_some_and(|buffers| buffers.tabs.active_file().is_some())
    }

    /// Say whether the halves are shown one at a time. Guarded, because the
    /// split says it on every frame.
    pub(crate) fn set_alone(&mut self, alone: bool, cx: &mut Context<Self>) {
        if self.alone != alone {
            self.alone = alone;
            cx.notify();
        }
    }

    fn back_to_tree(&mut self, cx: &mut Context<Self>) {
        self.detail = false;
        cx.notify();
    }

    fn toggle_tree(&mut self, cx: &mut Context<Self>) {
        self.tree_shown = !self.tree_shown;
        cx.notify();
    }

    fn select_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(buffers) = self.current_mut()
            && idx < buffers.tabs.files.len()
        {
            buffers.tabs.active = idx;
        }
        cx.notify();
    }

    /// Close tab `idx`, asking first when that would throw away edits.
    fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(file) = self
            .current()
            .and_then(|buffers| buffers.tabs.files.get(idx))
        else {
            return;
        };
        let close = Close::Tab(root, file.uid);
        if file.dirty {
            let label = file.label.clone();
            self.confirm_discard(
                "Discard unsaved edits?".into(),
                format!("“{label}” has edits that were never saved. Closing it throws them away."),
                close,
                window,
                cx,
            );
            return;
        }
        self.discard(close, cx);
    }

    /// Close every tab, asking first when any of them has unsaved edits.
    fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let n = self.current().map_or(0, RootBuffers::dirty_count);
        if n > 0 {
            let s = if n == 1 { "file has" } else { "files have" };
            self.confirm_discard(
                "Close all files?".into(),
                format!("{n} {s} edits that were never saved. Closing them throws those away."),
                Close::All(root),
                window,
                cx,
            );
            return;
        }
        self.discard(Close::All(root), cx);
    }

    /// Ask before a close throws edits away, in a dialog rather than by arming
    /// the ✕ for a second press.
    ///
    /// An armed control looks like one that did nothing, and the sentence
    /// explaining it was drawn at the foot of the panel, nowhere near the cross
    /// that was pressed. What to close is named by project, and a tab by `uid`
    /// rather than by position, because the question can be on screen while
    /// the strip changes or the window moves to another project — a remote
    /// `/open` does that — and *Discard* must still mean what it said.
    fn confirm_discard(
        &mut self,
        title: SharedString,
        description: String,
        close: Close,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            // Cloned per build: the builder runs again on every frame the
            // dialog is on screen, so nothing captured can be consumed by one.
            let (view, close) = (view.clone(), close.clone());
            alert
                .title(title.clone())
                .description(description.clone())
                // Our own pair rather than the library's OK/Cancel, which draw
                // the arrow cursor over controls that act. Keep is first and
                // plain, Discard last and in the danger tint.
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(
                                onehand_plugin_host::action("keep-edits")
                                    .ghost()
                                    .label("Keep"),
                            ),
                        )
                        .child(
                            onehand_plugin_host::action("discard-edits")
                                .danger()
                                .label("Discard")
                                .on_click(move |_, window: &mut Window, cx: &mut App| {
                                    window.close_dialog(cx);
                                    let close = close.clone();
                                    view.update(cx, |view: &mut Self, cx| view.discard(close, cx));
                                }),
                        ),
                )
        });
    }

    /// Close without asking; the question, if there was one, is answered.
    fn discard(&mut self, close: Close, cx: &mut Context<Self>) {
        let root = match &close {
            Close::Tab(root, _) | Close::All(root) => root.clone(),
        };
        let Some(buffers) = self.buffers.get_mut(&root) else {
            return;
        };
        match close {
            Close::Tab(_, uid) => {
                let Some(idx) = buffers.tabs.files.iter().position(|f| f.uid == uid) else {
                    return;
                };
                buffers.tabs.close(idx);
                buffers.forget(uid);
            }
            Close::All(_) => {
                for file in std::mem::take(&mut buffers.tabs.files) {
                    buffers.forget(file.uid);
                }
                buffers.tabs.close_all();
            }
        }
        // The status line is the view's, drawn over whichever project is on
        // screen: a discard answered after moving elsewhere must not wipe a
        // save conflict that belongs to the project now showing.
        if self.root.as_ref() == Some(&root) {
            self.status = None;
        }
        cx.notify();
    }
}

/// What a close is about to drop, and in which project.
#[derive(Debug, Clone)]
enum Close {
    Tab(PathBuf, u64),
    All(PathBuf),
}

/// Process-wide buffer id salt, for the same reason core hands one out per tab.
fn next_buffer_uid() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Render for EditorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(root) = self.root.as_ref() else {
            return div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .v_flex()
                .child(hint("No project root", cx));
        };
        // The strip is drawn with no tabs as well: the tree's toggle lives on
        // it, and a strip that went with the last tab would leave a hidden tree
        // with no way back.
        let empty = RootBuffers::default();
        let buffers = self.buffers.get(root).unwrap_or(&empty);
        let active = buffers.tabs.active_file().map(|f| f.uid);
        let buffer = active.and_then(|uid| buffers.buffer(uid)).cloned();
        let strip = tab_strip(
            root,
            buffers,
            &self.tabs_w,
            cx.entity().downgrade(),
            match self.alone {
                true => Lead::Back,
                false => Lead::Toggle(self.tree_shown),
            },
            StripHandlers {
                toggle_tree: Box::new(
                    cx.listener(|view: &mut Self, _, _, cx| view.toggle_tree(cx)),
                ),
                back: Box::new(cx.listener(|view: &mut Self, _, _, cx| view.back_to_tree(cx))),
                select: Rc::new(
                    cx.listener(|view: &mut Self, idx: &usize, _, cx| view.select_tab(*idx, cx)),
                ),
                close: Rc::new(cx.listener(|view: &mut Self, idx: &usize, window, cx| {
                    view.close_tab(*idx, window, cx)
                })),
                close_all: Box::new(
                    cx.listener(|view: &mut Self, _, window, cx| view.close_all_tabs(window, cx)),
                ),
            },
            cx,
        );
        let body = match buffer {
            None => hint("Open a file from the tree, or from a tool card", cx),
            Some(state) => div()
                .flex_1()
                .min_h_0()
                .child(body(&state))
                .into_any_element(),
        };

        // `min_w_0`: the resizable panel holding this is a flex *row*, and a flex
        // item's floor is otherwise its content's width, so the strip's measure
        // would follow its tabs rather than the room the panel has.
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .v_flex()
            .child(strip)
            .child(body)
            .children(self.status.clone().map(|status| status_line(status, cx)))
    }
}
