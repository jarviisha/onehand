//! The Editor mode's view, and the carrier of its drive: the open buffers
//! per project root are `onehand_core::editor::drive::EditorDrive`'s, every
//! rule with them; what lives here is each tab's buffer, the reads and writes
//! carried off the UI loop, and the drawing.

use crate::buffers::{Lead, StripHandlers, body, new_buffer, standing_line, tab_strip};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, IntoElement,
    ParentElement, Render, SharedString, Styled, Window, div,
};
use gpui_component::WindowExt as _;
use gpui_component::input::{EditorState, InputEvent};
use gpui_component::{ActiveTheme as _, StyledExt};
use onehand_core::editor::RootEditors;
use onehand_core::editor::SaveOutcome;
use onehand_core::editor::drive::{Close, Closing, EditorDrive, Effect, Input, Signal};
use onehand_ui::{hint, status_ink, status_line};
use onehand_workbench::Disk;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(crate) struct EditorView {
    /// The project on screen.
    root: Option<PathBuf>,
    drive: EditorDrive,
    /// Each tab's buffer, by its `uid`, and what turns typing into it into
    /// the drive's `typed`. The subscription is held beside its buffer: one
    /// dropped stops delivering at once, which is indistinguishable from never
    /// having subscribed.
    buffers: HashMap<u64, (Entity<EditorState>, gpui::Subscription)>,
    /// The window this view is drawn in, kept from its first open or draw: a
    /// save is asked for without one, and what it comes to is said there as
    /// soon as it comes back, whether or not the Workbench is showing this mode.
    window: Option<AnyWindowHandle>,
    /// The width the strip leaves its tabs, in rems, as last laid out.
    /// Infinite until measured, so the first frame draws every tab.
    tabs_w: onehand_ui::Measured,
    /// Each tab's width as last drawn, so the strip knows how many fit.
    tab_widths: onehand_ui::TabWidths,
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
            drive: EditorDrive::default(),
            buffers: HashMap::new(),
            window: None,
            tabs_w: onehand_ui::unmeasured(),
            tab_widths: Default::default(),
            tree_shown: true,
            detail: false,
            alone: false,
        })
    }

    /// Which folders may not be written now, whole.
    pub(crate) fn set_locked(&mut self, locks: &HashMap<PathBuf, String>, cx: &mut Context<Self>) {
        self.drive.set_locked(locks);
        cx.notify();
    }

    pub(crate) fn set_root(&mut self, root: &Path, disk: &Disk, cx: &mut Context<Self>) {
        // Told even for the root already on screen: a host's attach can be
        // replaced while its project stays.
        self.drive.set_root(root, disk);
        if self.root.as_deref() != Some(root) {
            self.root = Some(root.to_path_buf());
            cx.notify();
        }
    }

    /// Drop everything held for `root`, which has left the workspace.
    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        let effects = self.drive.forget_root(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        self.perform(effects, None, cx);
    }

    /// How many of `root`'s open files have edits a removal would discard.
    pub(crate) fn unsaved(&self, root: &Path) -> usize {
        self.drive.unsaved(root)
    }

    /// The file open in the active tab, if any.
    pub(crate) fn active_path(&self) -> Option<PathBuf> {
        Some(self.tabs()?.active_file()?.path.clone())
    }

    /// Where the caret goes, if there is a buffer to put it in.
    pub(crate) fn caret(&self, cx: &App) -> Option<gpui::FocusHandle> {
        let uid = self.tabs()?.active_file()?.uid;
        Some(self.buffers.get(&uid)?.0.focus_handle(cx))
    }

    /// Open `path`, and say whether this mode took it.
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
        self.window = Some(window.window_handle());
        let (took, effects) = self.drive.open(&root, path);
        if took {
            self.detail = true;
        }
        self.perform(effects, Some(window), cx);
        took
    }

    /// Save the active tab, refusing to clobber a concurrent writer.
    pub(crate) fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let effects = self.drive.save_active(&root);
        self.perform(effects, None, cx);
    }

    /// Carry out the drive's effects, in order. A buffer to build or a thing
    /// to say needs the window, which a save or a read landing does not have:
    /// those go to the window this view is drawn in, after this update.
    fn perform(
        &mut self,
        effects: Vec<Effect>,
        mut window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        for effect in effects {
            match effect {
                Effect::Read { root, path, disk } => {
                    cx.spawn(async move |view, cx| {
                        let read = cx
                            .background_executor()
                            .spawn({
                                let path = path.clone();
                                async move { disk.read_file(&path) }
                            })
                            .await;
                        let _ = view.update(cx, |view: &mut Self, cx| {
                            view.landed(Input::Read { root, path, read }, cx)
                        });
                    })
                    .detach();
                }
                Effect::Write {
                    root,
                    uid,
                    path,
                    mtime,
                    disk,
                } => self.write(root, uid, path, mtime, disk, cx),
                Effect::Forget { uid } => {
                    self.buffers.remove(&uid);
                }
                Effect::Buffer {
                    root,
                    uid,
                    path,
                    text,
                } => match window.as_deref_mut() {
                    Some(window) => self.build(root, uid, &path, &text, window, cx),
                    None => self.in_window(cx, move |view, window, cx| {
                        view.build(root, uid, &path, &text, window, cx)
                    }),
                },
                Effect::Signal(signal) => {
                    let note = match &signal {
                        Signal::OpenFailed { path, why } => {
                            let name = path.file_name().map_or_else(
                                || path.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            onehand_ui::toast::failed(&format!("Opening {name}"), why)
                        }
                        Signal::SaveFailed { label, why } => {
                            onehand_ui::toast::failed(&format!("Saving {label}"), why)
                        }
                    };
                    match window.as_deref_mut() {
                        Some(window) => window.push_notification(note, cx),
                        None => self
                            .in_window(cx, move |_, window, cx| window.push_notification(note, cx)),
                    }
                }
            }
        }
        cx.notify();
    }

    /// Report what the world said, and carry out what the drive answers.
    fn landed(&mut self, input: Input, cx: &mut Context<Self>) {
        let effects = self.drive.handle(input);
        self.perform(effects, None, cx);
    }

    /// Write tab `uid`'s buffer as it stands now, off the UI loop.
    fn write(
        &mut self,
        root: PathBuf,
        uid: u64,
        path: PathBuf,
        mtime: Option<std::time::SystemTime>,
        disk: Disk,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = self
            .buffers
            .get(&uid)
            .map(|(state, _)| state.read(cx).text().to_string())
        else {
            // A tab whose buffer is gone: answered all the same, so the save
            // it held is let go.
            let outcome = SaveOutcome::Failed("the file is no longer open".into());
            let effects = self.drive.handle(Input::Saved {
                root,
                uid,
                outcome,
                still: false,
            });
            return self.perform(effects, None, cx);
        };
        cx.spawn(async move |view, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { disk.save_file(&path, text, mtime) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                // Whether the buffer still holds what landed: keystrokes that
                // came in while the write was on its way are still unsaved.
                let still = match &outcome {
                    SaveOutcome::Saved { text, .. } => {
                        let live = view
                            .buffers
                            .get(&uid)
                            .map(|(state, _)| state.read(cx).text().to_string());
                        live.is_some_and(|live| live == **text)
                    }
                    SaveOutcome::Conflict { .. } | SaveOutcome::Failed(_) => false,
                };
                view.landed(
                    Input::Saved {
                        root,
                        uid,
                        outcome,
                        still,
                    },
                    cx,
                )
            });
        })
        .detach();
    }

    /// Build tab `uid`'s buffer, and listen for typing into it.
    fn build(
        &mut self,
        root: PathBuf,
        uid: u64,
        path: &Path,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = new_buffer(path, text, window, cx);
        // Subscribed *after* `new_buffer`, whose `set_value` seeds the text:
        // gpui-component's `set_value` does not emit `Change`, but ordering it
        // this way means a tab is never born dirty even if that changes
        // upstream.
        let watch = cx.subscribe(&state, move |view: &mut Self, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                view.drive.typed(&root, uid);
                cx.notify();
            }
        });
        self.buffers.insert(uid, (state, watch));
        cx.notify();
    }

    /// Run `then` in the window this view is drawn in, after the update under
    /// way: it may be the window's own.
    fn in_window(
        &self,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(handle) = self.window else {
            return;
        };
        let view = cx.entity();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                view.update(cx, |view, cx| then(view, window, cx))
            });
        });
    }

    /// The tabs of the project on screen, if it has any.
    fn tabs(&self) -> Option<&RootEditors> {
        self.drive.tabs(self.root.as_ref()?)
    }

    pub(crate) fn tree_shown(&self) -> bool {
        self.tree_shown
    }

    /// Whether a file is what shows while the halves are one at a time: one
    /// asked for, and one there to show.
    pub(crate) fn showing_file(&self) -> bool {
        self.detail && self.tabs().is_some_and(|tabs| tabs.active_file().is_some())
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
        if let Some(root) = &self.root {
            self.drive.select(root, idx);
        }
        cx.notify();
    }

    /// Close tab `idx`, asking first when that would throw away edits.
    fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let closing = self.drive.close_tab(&root, idx);
        self.closing(closing, window, cx);
    }

    /// Close every tab, asking first when any of them has unsaved edits.
    fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let closing = self.drive.close_all(&root);
        self.closing(closing, window, cx);
    }

    /// Carry out a close, or ask first in a dialog rather than by arming the ✕
    /// for a second press: an armed control looks like one that did nothing.
    fn closing(&mut self, closing: Closing, window: &mut Window, cx: &mut Context<Self>) {
        let (close, unsaved, label) = match closing {
            Closing::Closed(effects) => return self.perform(effects, Some(window), cx),
            Closing::Asks {
                close,
                unsaved,
                label,
            } => (close, unsaved, label),
        };
        let (title, description): (SharedString, String) = match label {
            Some(label) => (
                format!("Close {label}?").into(),
                "It has edits that were never saved. Closing it throws them away.".into(),
            ),
            None => {
                let s = if unsaved == 1 {
                    "file has"
                } else {
                    "files have"
                };
                (
                    "Close all files?".into(),
                    format!(
                        "{unsaved} {s} edits that were never saved. Closing them throws those away."
                    ),
                )
            }
        };
        onehand_ui::confirm_on(
            cx.entity(),
            onehand_ui::Confirm {
                id: "discard-edits",
                title,
                description: description.into(),
                // The verb of the control that asked.
                act: "Close",
                ..Default::default()
            },
            window,
            cx,
            move |view, window, cx| view.discard(&close, window, cx),
        );
    }

    /// Close without asking; the question, if there was one, is answered.
    fn discard(&mut self, close: &Close, window: &mut Window, cx: &mut Context<Self>) {
        let effects = self.drive.discard(close);
        self.perform(effects, Some(window), cx);
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        let Some(root) = self.root.as_ref() else {
            return div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .v_flex()
                .child(hint("No project selected", cx));
        };
        // The strip is drawn with no tabs as well: the tree's toggle lives on
        // it, and a strip that went with the last tab would leave a hidden tree
        // with no way back.
        let empty = RootEditors::default();
        let tabs = self.drive.tabs(root).unwrap_or(&empty);
        let locked = self.drive.locked(root).map(str::to_string);
        let standing = self.drive.standing(root).map(standing_line);
        let active = tabs.active_file().map(|f| f.uid);
        let buffer = active
            .and_then(|uid| self.buffers.get(&uid))
            .map(|(state, _)| state.clone());
        let strip = tab_strip(
            root,
            tabs,
            (&self.tabs_w, &self.tab_widths),
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
            // The read's wait stands where the file will be, and only where
            // nothing is drawn yet: a buffer already open stays readable while
            // the next one is read.
            None if self.drive.opening().is_some() => hint("Opening the file…", cx),
            None => hint("Open a file from the tree, or from a tool card", cx),
            Some(state) => div()
                .flex_1()
                .min_h_0()
                .child(body(&state, locked.is_some()))
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
            .children(standing.map(|status| status_line(status, status_ink(cx).warning)))
            .children(locked.map(|why| status_line(why, cx.theme().muted_foreground)))
    }
}

#[cfg(test)]
mod tests;
