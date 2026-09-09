//! The Editor mode's own state: the open buffers per project root, and the
//! rules that keep a save from clobbering somebody else's write.

use crate::view::{RootBuffers, body, new_buffer, save_status, tab_strip};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    Styled, Window, div,
};
use gpui_component::StyledExt;
use gpui_component::input::InputEvent;
use onehand_core::editor::SaveOutcome;
use onehand_plugin_host::{hint, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// A close that would discard unsaved edits, armed for its second click.
    ///
    /// Same shape as the shell's guarded root removal, and for the same reason:
    /// the destructive half of a one-touch control needs a touch of its own.
    /// Any other action disarms it, so the confirmation always belongs to the
    /// control just clicked.
    pending_close: Option<PendingClose>,
}

/// A close waiting on its confirming click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingClose {
    Tab(usize),
    All,
}

impl EditorView {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            root: None,
            buffers: HashMap::new(),
            status: None,
            saving: HashMap::new(),
            pending_close: None,
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
            .map(|buffers| buffers.tabs.files.iter().filter(|f| f.dirty).count())
            .unwrap_or(0)
    }

    /// Where the caret goes, if there is a buffer to put it in.
    pub(crate) fn caret(&self, cx: &App) -> Option<gpui::FocusHandle> {
        let buffers = self.buffers.get(self.root.as_ref()?)?;
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
        self.pending_close = None;
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

    fn select_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.pending_close = None;
        if let Some(buffers) = self.root.as_ref().and_then(|r| self.buffers.get_mut(r))
            && idx < buffers.tabs.files.len()
        {
            buffers.tabs.active = idx;
        }
        cx.notify();
    }

    /// Close tab `idx`, asking twice when that would throw away edits.
    fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.buffers.get_mut(r)) else {
            return;
        };
        let Some(file) = buffers.tabs.files.get(idx) else {
            return;
        };
        if file.dirty && self.pending_close != Some(PendingClose::Tab(idx)) {
            let label = file.label.clone();
            self.pending_close = Some(PendingClose::Tab(idx));
            self.status = Some(format!(
                "{label} has unsaved edits — Ctrl+S to save, or click ✕ again to discard them."
            ));
            cx.notify();
            return;
        }

        let uid = file.uid;
        buffers.tabs.close(idx);
        buffers.forget(uid);
        self.pending_close = None;
        self.status = None;
        cx.notify();
    }

    /// Close every tab, asking twice when any of them has unsaved edits.
    fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.buffers.get_mut(r)) else {
            return;
        };
        if buffers.any_dirty() && self.pending_close != Some(PendingClose::All) {
            let n = buffers.tabs.files.iter().filter(|f| f.dirty).count();
            let s = if n == 1 { "file has" } else { "files have" };
            self.pending_close = Some(PendingClose::All);
            self.status = Some(format!(
                "{n} {s} unsaved edits — click ✕ again to discard them."
            ));
            cx.notify();
            return;
        }

        for file in std::mem::take(&mut buffers.tabs.files) {
            buffers.forget(file.uid);
        }
        buffers.tabs.close_all();
        self.pending_close = None;
        self.status = None;
        cx.notify();
    }
}

/// Process-wide buffer id salt, for the same reason core hands one out per tab.
fn next_buffer_uid() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Render for EditorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self
            .root
            .as_ref()
            .and_then(|r| self.buffers.get(r))
            .filter(|buffers| !buffers.tabs.files.is_empty());

        let body = match open {
            None => hint("Open a file from a tool card or the Files tab", cx),
            Some(buffers) => {
                let active = buffers.tabs.active_file().map(|f| f.uid);
                let buffer = active.and_then(|uid| buffers.buffer(uid)).cloned();
                div()
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .child(tab_strip(
                        buffers,
                        cx.listener(|view: &mut Self, idx: &usize, _, cx| {
                            view.select_tab(*idx, cx)
                        }),
                        cx.listener(|view: &mut Self, idx: &usize, _, cx| view.close_tab(*idx, cx)),
                        cx.listener(|view: &mut Self, _, _, cx| view.close_all_tabs(cx)),
                        cx,
                    ))
                    .children(buffer.map(|state| {
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(body(&state))
                            .into_any_element()
                    }))
                    .into_any_element()
            }
        };

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.status.clone().map(|status| status_line(status, cx)))
    }
}
