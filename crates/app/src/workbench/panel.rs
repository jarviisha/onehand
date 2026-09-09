//! The Workbench dock panel: Editor, Files, Markdown and Neovim, one at a time.
//!
//! State is **per project root** — `RootBuffers`, `FileTree` and the Neovim PTY
//! are all keyed by root path, so switching roots swaps the whole panel rather
//! than mixing one project's tabs with another's tree.
//!
//! **Neovim is a mode here rather than a tab of the terminal**, because this is
//! the panel about files: a tab called `nvim` sitting between two called `zsh`
//! says the editor is a kind of shell. Two consequences the other modes do not
//! have, both because this one is a live PTY rather than an element tree.
//! Its zoom is a font size and not the rem scale wrapped around the body below,
//! since the grid is *measured* from a shaped glyph — scaling the box around it
//! leaves every column landing past its own character. And the panel takes the
//! terminal's key context while it is showing, which is what gives `Ctrl+S` back
//! to `:w`: that binding is `Shell && !Terminal` precisely so a program in a PTY
//! keeps it, and a grid mounted somewhere with no such context would have the
//! save of the quick editor fire over the top of it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::input::InputEvent;
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_core::editor::SaveOutcome;
use onehand_core::gitstat::GitStatus;
use onehand_plugin_api::PluginId;
use onehand_plugin_host::{Ask, Request, WorkbenchHost, WorkbenchMode};
use onehand_workbench_editor::{self as editor, RootBuffers};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const EDITOR_MODE: PluginId = onehand_workbench_editor::MODE_ID;
pub const FILES_MODE: PluginId = onehand_workbench_files::MODE_ID;
pub const MARKDOWN_MODE: PluginId = onehand_workbench_markdown::MODE_ID;
pub const NEOVIM_MODE: PluginId = onehand_workbench_neovim::MODE_ID;

pub struct Workbench {
    host: WorkbenchHost,
    /// The modes that own their own state and draw their own body.
    ///
    /// Each is asked in the panel's order, and a mode that has an answer to a
    /// request says so. The `if` chain in `render` is what is left of the modes
    /// that have not moved here yet.
    modes: Vec<Box<dyn WorkbenchMode>>,
    /// The root everything below is keyed by. `None` before any root is active.
    root: Option<PathBuf>,
    editors: HashMap<PathBuf, RootBuffers>,
    /// A save conflict, a read failure, or a Neovim that would not start, shown
    /// as a line under the body.
    ///
    /// **Deliberately not a notification**, unlike the rest of the app's
    /// transient status. "Changed on disk — nothing was written" is not news
    /// that may be missed: it is a standing condition, and the next Ctrl+S is
    /// what resolves it. A toast that fades leaves the user believing the save
    /// went through. Cleared by whatever answers it — a successful save, or a
    /// successful open.
    pub status: Option<String>,
    /// Tabs with a write in flight, and whether another save was asked for
    /// while it ran.
    ///
    /// Two saves of one tab must not overlap: each captures `disk_mtime` at
    /// spawn, so the second carries the time from *before* the first wrote and
    /// comes back as a conflict against our own write. The
    /// second press is not dropped either -- it is remembered here and re-run
    /// once the first lands, which is what makes it pick up the keystrokes that
    /// prompted it.
    saving: HashMap<u64, bool>,
    /// A close that would discard unsaved edits, armed for its second click.
    ///
    /// Same shape as the shell's guarded root removal, and for the same reason:
    /// the destructive half of a one-touch control needs a touch of its own.
    /// Any other action disarms it, so the confirmation always belongs to the
    /// control just clicked.
    pending_close: Option<PendingClose>,
    /// Reading size for the body. Per panel, not per root: the dock's own
    /// sizes are per window here, and a zoom that reset itself on every root
    /// switch would be the odd one out.
    zoom: crate::zoom::Zoom,
}

/// A close waiting on its confirming click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingClose {
    Tab(usize),
    All,
}

impl Workbench {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let modes = crate::state::Shared::global(cx)
            .plugins
            .workbench_modes()
            .to_vec();
        let host = WorkbenchHost::new(modes, cx);
        cx.new(|cx| {
            // Handed to the modes as they are built rather than set afterwards,
            // so none of them is ever on screen holding nothing to ask through.
            let panel = cx.weak_entity();
            let ask: Ask = Rc::new(move |request, window, cx| {
                let _ = panel.update(cx, |panel: &mut Self, cx| {
                    panel.answer(request, window, cx);
                });
            });
            Self {
                modes: crate::plugins::workbench_modes(
                    ask,
                    crate::zoom::term_font_size(crate::zoom::Zoom::default()),
                    cx,
                ),
                host,
                zoom: crate::zoom::Zoom::default(),
                root: None,
                editors: HashMap::new(),
                status: None,
                saving: HashMap::new(),
                pending_close: None,
            }
        })
    }

    /// Answer a request a mode raised, which is the panel's half of
    /// [`onehand_plugin_host::Ask`].
    ///
    /// The vocabulary is the same in both directions, so what arrives here is
    /// either something only the panel can do — opening a file is the quick
    /// editor's business and the file tree cannot reach it — or something to
    /// pass on to whichever mode owns it.
    fn answer(&mut self, request: &Request<'_>, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            Request::OpenFile(path) => self.open_file(path.to_path_buf(), window, cx),
            // The caret is the panel's half of reaping: a view dropped while it
            // holds focus leaves the window pointing at an element no frame
            // contains, and GPUI resolves a key along the path down to the
            // focused node — so every shortcut stops working, including the one
            // that would reopen this panel. Asked *before* the drop, since a
            // handle no longer drawn cannot answer. Only moved when focus was
            // inside this panel already: a child exiting in the background must
            // not take the caret from what the user is doing.
            Request::Reap => {
                let held = self.host.focus_handle().contains_focused(window, cx);
                if self.broadcast(&Request::Reap, cx) && held {
                    self.focus_active(window, cx);
                }
            }
            other => {
                self.broadcast(other, cx);
            }
        }
    }

    /// Put a request to every mode in the panel's order, and say whether one of
    /// them took it.
    fn broadcast(&mut self, request: &Request<'_>, cx: &mut Context<Self>) -> bool {
        let mut taken = false;
        for mode in &mut self.modes {
            taken |= mode.handle(request, cx);
        }
        taken
    }

    pub fn mode(&self) -> PluginId {
        self.host.active()
    }

    /// Drop everything this panel holds for `root`.
    ///
    /// Called when a project root leaves the workspace. Buffers are dropped
    /// with it: an unsaved edit in a project the user just removed has nowhere
    /// to be saved *to* -- the tab strip it belonged to is gone.
    /// How many of `root`'s open files have edits a removal would discard.
    ///
    /// Asked by the shell *before* it removes a root, because that is the one
    /// place with a control to guard. Until `dirty` was wired up to typing,
    /// this question had no answer worth asking.
    pub fn unsaved_in(&self, root: &Path) -> usize {
        self.editors
            .get(root)
            .map(|buffers| buffers.tabs.files.iter().filter(|f| f.dirty).count())
            .unwrap_or(0)
    }

    pub fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        for mode in &mut self.modes {
            mode.forget_root(root, cx);
        }
        self.editors.remove(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// Start whatever child process a mode is a front end for.
    ///
    /// Separate from switching to that mode on purpose: a mode change is a view
    /// change and must not launch a process, or the strip becomes a row of
    /// buttons one of which spawns something. The key does this first and then
    /// switches; the mode's own empty state is the other way in.
    pub fn start_child(&mut self, cx: &mut Context<Self>) {
        self.broadcast(&Request::Start, cx);
    }

    /// Step this panel's zoom.
    ///
    /// Handed the whole value rather than a `&mut` to the field, because a step
    /// here is not only a number: a live grid has to be re-measured at the new
    /// font size, and a caller holding the field directly would set it and leave
    /// Neovim drawn at the old one.
    pub fn set_zoom(&mut self, zoom: crate::zoom::Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        self.broadcast(&Request::SetFontSize(crate::zoom::term_font_size(zoom)), cx);
        cx.notify();
    }

    /// This panel's zoom, for the status bar to report.
    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    /// Put focus where this mode's work happens: the open buffer in Editor
    /// mode, the grid in Neovim, the panel itself in Files, which is clicked
    /// rather than typed into. A panel shortcut that opens a dock without moving
    /// focus makes the user reach for the mouse to use what they just opened.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        // The showing mode first: it is the only one whose body is on screen,
        // and a mode that is clicked rather than typed into refuses, which is
        // what leaves the caret on the panel itself further down.
        if let Some(showing) = self.modes.iter().find(|item| item.spec().id == self.mode())
            && showing.focus(window, cx)
        {
            return;
        }
        if self.mode() == EDITOR_MODE {
            let buffer = self
                .root
                .as_ref()
                .and_then(|root| self.editors.get(root))
                .and_then(|buffers| {
                    let uid = buffers.tabs.active_file()?.uid;
                    buffers.buffer(uid)
                });
            if let Some(state) = buffer {
                state.focus_handle(cx).focus(window, cx);
                return;
            }
        }
        self.host.focus_handle().focus(window, cx);
    }

    pub fn set_mode(&mut self, mode: PluginId, cx: &mut Context<Self>) {
        if self.host.select(mode) {
            // To the arriving mode alone, and not broadcast: it is the one
            // request whose answer depends on being the mode about to be seen.
            // A listing that costs a walk of the whole project is refreshed
            // here rather than at boot or on a project switch behind another
            // mode's back.
            if let Some(arriving) = self.modes.iter_mut().find(|item| item.spec().id == mode) {
                arriving.handle(&Request::Shown, cx);
            }
            cx.notify();
        }
    }

    /// Point the panel at a project root, seeding its tree on first sight.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.root.as_ref() == Some(&root) {
            return;
        }
        self.root = Some(root.clone());
        for mode in &mut self.modes {
            mode.set_root(&root, cx);
        }
        cx.notify();
    }

    pub fn set_git(&mut self, git: HashMap<PathBuf, GitStatus>, cx: &mut Context<Self>) {
        for mode in &mut self.modes {
            mode.handle(&Request::SetGit(&git), cx);
        }
        cx.notify();
    }

    /// Re-read every directory currently on screen for the active root.
    ///
    /// The tree was seeded once per root and never refreshed, so a file the
    /// agent had just created did not appear until the user folded and unfolded
    /// its directory by hand. Called when a turn ends and when
    /// the window is activated — the two moments the tree is most likely to
    /// have moved under it.
    ///
    /// Bounded by what is *visible*: the root plus its expanded directories,
    /// which is exactly what `visible_rows` draws from. A collapsed subtree is
    /// rescanned when it is opened, as it always was.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        for mode in &mut self.modes {
            mode.handle(&Request::Rescan, cx);
        }
    }

    /// Open `path` in the editor.
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
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.host.select(EDITOR_MODE);

        if let Some(buffers) = self.editors.get(&root)
            && buffers.tabs.index_of(&path).is_some()
        {
            self.editors
                .entry(root)
                .or_default()
                .tabs
                .open(path, None, 0);
            cx.notify();
            return;
        }

        // `spawn_in`, not `spawn`: building an `EditorState` needs a `&mut
        // Window`, and the read has to happen off the UI loop first.
        cx.spawn_in(window, async move |panel, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::read_blocking(&path) }
                })
                .await;

            let _ = panel.update_in(cx, |panel: &mut Self, window, cx| match read {
                Ok((text, mtime)) => {
                    // A read that worked answers whatever the last failure said.
                    // Left set, the message outlived its subject and read as a
                    // complaint about the file now on screen.
                    panel.status = None;
                    let uid = next_buffer_uid();
                    let state = editor::new_buffer(&path, &text, window, cx);
                    // Typing is what makes a buffer dirty, and nothing was
                    // listening for it. Subscribed *after*
                    // `new_buffer`, whose `set_value` seeds the text --
                    // gpui-component's `set_value` does not emit `Change`, but
                    // ordering it this way means a tab is never born dirty even
                    // if that changes upstream.
                    let watch = cx.subscribe(&state, {
                        let root = root.clone();
                        move |panel: &mut Self, _, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change) {
                                panel.mark_dirty(&root, uid, cx);
                            }
                        }
                    });
                    let buffers = panel.editors.entry(root).or_default();
                    // Core decides whether this is a new tab; a buffer is only
                    // adopted when it is, so a re-open keeps its edits.
                    let (_, is_new) = buffers.tabs.open(path.clone(), mtime, uid);
                    if is_new {
                        buffers.insert(uid, state, watch);
                    }
                    cx.notify();
                }
                Err(e) => {
                    panel.status = Some(format!("{} — {e}", path.display()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Note that a buffer has been typed into.
    ///
    /// The one place `dirty` is ever set true; before this the amber tab dot
    /// was unreachable and every close discarded edits in silence.
    fn mark_dirty(&mut self, root: &Path, uid: u64, cx: &mut Context<Self>) {
        if let Some(buffers) = self.editors.get_mut(root)
            && let Some(file) = buffers.tabs.files.iter_mut().find(|f| f.uid == uid)
            && !file.dirty
        {
            file.dirty = true;
            cx.notify();
        }
    }

    /// Save the active tab, refusing to clobber a concurrent writer.
    pub fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(uid) = self
            .editors
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
        let Some(buffers) = self.editors.get(&root) else {
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

        cx.spawn(async move |panel, cx| {
            let outcome = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::save_blocking(&path, text, mtime) }
                })
                .await;

            let _ = panel.update(cx, |panel: &mut Self, cx| {
                panel.status = editor::save_status(&outcome, &label);
                let again = panel.saving.remove(&uid).unwrap_or(false);
                // Read the live buffer before taking the tab set mutably: what
                // the dot should say depends on whether the user kept typing
                // while the write was in flight.
                let current = panel
                    .editors
                    .get(&root)
                    .and_then(|buffers| buffers.buffer(uid))
                    .map(|state| state.read(cx).text().to_string());

                if let Some(buffers) = panel.editors.get_mut(&root)
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
                    panel.save_tab(root, uid, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn select_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.pending_close = None;
        if let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get_mut(r))
            && idx < buffers.tabs.files.len()
        {
            buffers.tabs.active = idx;
        }
        cx.notify();
    }

    /// Close tab `idx`, asking twice when that would throw away edits.
    pub fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get_mut(r)) else {
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
    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get_mut(r)) else {
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

impl Panel for Workbench {
    fn panel_name(&self) -> &'static str {
        "Workbench"
    }

    /// The content-only maximize: the panel fills the frame right of the rail,
    /// which stays. Put in the
    /// toolbar rather than the overflow menu -- it is the direction reached
    /// for most often, and the app direction already has a key.
    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        Some(PanelControl::Toolbar)
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Workbench")
    }
}

impl EventEmitter<PanelEvent> for Workbench {}

impl Focusable for Workbench {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.host.focus_handle()
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.mode();
        let contributions = self.host.contributions().to_vec();
        let modes = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(
                contributions
                    .into_iter()
                    .map(|item| mode_tab(item.label, item.id, mode, cx)),
            );

        // The two facts below are read off the showing mode's own declaration
        // rather than worked out from its ID. A mode nothing registered has
        // neither, and falls back to the panel's own context and scale so the
        // strip that would switch away from it still answers the keyboard.
        let showing = self.host.active_contribution().copied();
        let key_context = showing
            .map(|item| item.key_context)
            .unwrap_or(onehand_plugin_api::WORKBENCH_KEY_CONTEXT);
        let rem_zoom = showing.is_none_or(|item| item.rem_zoom);

        // The mode strip is chrome and keeps its size; only the work below it
        // scales. A zoomed-in editor whose own tab bar grew with it wastes the
        // room the zoom was asking for.
        let zoom = self.zoom;
        // A mode that owns its own body draws it; the chain below is what is
        // left of the modes still drawn from here.
        let owned = self
            .modes
            .iter()
            .find(|item| item.spec().id == mode)
            .map(|item| item.view());
        let body = if let Some(view) = owned {
            view.into_any_element()
        } else if mode == EDITOR_MODE {
            self.editor_body(cx)
        } else {
            hint("This Workbench contribution is unavailable", cx)
        };
        // A measured glyph grid is sized by the font it was configured with and
        // not by the rem base around it, so a mode that says so is left alone:
        // wrapping it in the scale would stretch the box while the cell stayed
        // put, leaving every column landing past its own character. Such a mode
        // takes its reading size as a font size instead, through `set_zoom`.
        let body = if rem_zoom {
            zoom.scale(window, body).into_any_element()
        } else {
            body
        };
        div()
            .size_full()
            .v_flex()
            // A mode hosting a PTY takes the *terminal's* context while it is
            // showing, and it has to be that name and not one of its own:
            // `Ctrl+S` is bound `Shell && !Terminal` so that a program in a PTY
            // keeps it, and a predicate that had to learn a second name for the
            // same fact is one that gets updated in one place and not the other.
            // Under any other mode this is the Workbench, which is what the save
            // is *for*.
            .key_context(key_context)
            .child(modes)
            .child(body)
            .when_some(self.status.clone(), |panel, status| {
                panel.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).warning)
                        .child(status),
                )
            })
    }
}

impl Workbench {
    fn editor_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get(r)) else {
            return hint("Open a file from a tool card or the Files tab", cx);
        };
        if buffers.tabs.files.is_empty() {
            return hint("Open a file from a tool card or the Files tab", cx);
        }
        let active = buffers.tabs.active_file().map(|f| f.uid);
        let body = active.and_then(|uid| buffers.buffer(uid)).cloned();

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(editor::tab_strip(
                buffers,
                cx.listener(|panel: &mut Self, idx: &usize, _, cx| panel.select_tab(*idx, cx)),
                cx.listener(|panel: &mut Self, idx: &usize, _, cx| panel.close_tab(*idx, cx)),
                cx.listener(|panel: &mut Self, _, _, cx| panel.close_all_tabs(cx)),
                cx,
            ))
            .children(body.map(|state| {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(editor::body(&state))
                    .into_any_element()
            }))
            .into_any_element()
    }
}

fn mode_tab(
    label: &'static str,
    which: PluginId,
    active: PluginId,
    cx: &mut Context<Workbench>,
) -> impl IntoElement + use<> {
    crate::controls::action(label)
        .xsmall()
        .map(|b| {
            if which == active {
                b.primary()
            } else {
                b.ghost()
            }
        })
        .label(label)
        .on_click(cx.listener(move |panel: &mut Workbench, _, _, cx| {
            panel.set_mode(which, cx);
        }))
}

fn hint(text: &'static str, cx: &App) -> gpui::AnyElement {
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
